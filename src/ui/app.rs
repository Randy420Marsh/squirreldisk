use super::fx::{Drag, Particles, Toasts};
use super::shader::SunburstGl;
use super::sunburst::{Animator, View};
use super::theme::{self, Theme};
use super::widgets;
use crate::delete::{self, DeleteProgress};
use crate::disks::{self, Volume};
use crate::safety::Verdict;
use crate::scan::{self, Progress, SharedTree};
use crate::settings::Settings;
use crate::sound::{Sfx, Sounds};
use crate::tree::{Kind, Source};
use crate::update::Updater;
use crate::watch::FsWatch;
use eframe::egui::{self, Color32, FontData, FontDefinitions, FontFamily, Pos2, Rect};
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Target {
    Volume(Volume),
    Folder(PathBuf),
    Ssh { host: String, path: String },
    Rclone(String),
}

impl Target {
    pub fn title(&self) -> String {
        match self {
            Target::Volume(v) => v.name.clone(),
            Target::Folder(p) => p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| p.to_string_lossy().into_owned()),
            Target::Ssh { host, path } => format!("{host}:{path}"),
            Target::Rclone(r) => r.clone(),
        }
    }
    pub fn same_place(&self, other: &Target) -> bool {
        match (self, other) {
            (Target::Volume(a), Target::Volume(b)) => a.mount == b.mount,
            _ => self == other,
        }
    }
}

pub struct Session {
    pub target: Target,
    pub title: String,
    pub tree: SharedTree,
    pub progress: Arc<Progress>,
    pub view: View,
    pub back: Vec<View>,
    pub fwd: Vec<View>,
    pub anim: Animator,
    pub treemap: super::treemap::TreemapState,
    pub layout_sig: (u64, View, usize, i32, String),
    pub last_layout: Instant,
    pub collector: Vec<u32>,
    /// Node whose content is shown in the side panel.
    pub panel_view: View,
    pub hovered_key: Option<u64>,
    pub list_hover: Option<u32>,
    pub selected: Option<u32>,
    pub watch: Option<FsWatch>,
    pub pending: HashSet<PathBuf>,
    pub last_event: Instant,
    pub refreshing: Arc<AtomicBool>,
    pub delete: Option<Arc<DeleteProgress>>,
    pub intro: bool,
    pub was_done: bool,
    pub verts: Vec<f32>,
    pub idx: Vec<u32>,
    pub watch_focus: Option<View>,
    pub expanding: Arc<AtomicBool>,
    /// Last chart geometry (used by the demo director to aim at slices).
    pub geo: Option<super::sunburst::Geometry>,
    /// Free bytes on the scanned volume (disk scans only), refreshed periodically.
    pub free_space: u64,
    pub free_at: Instant,
    /// Nodes whose names have been checked for the fonts they need, and the
    /// scripts found in them (see [`crate::i18n::scripts_of`]).
    pub names_checked: usize,
    pub name_scripts: u32,
}

impl Session {
    pub fn new(target: Target) -> Session {
        let handle = match &target {
            _ if scan::demo::enabled() => match &target {
                Target::Volume(v) => scan::demo::start(v.name.clone(), v.mount.clone()),
                other => scan::demo::start(other.title(), "/Volumes/Demo".into()),
            },
            Target::Volume(v) => scan::local::start(PathBuf::from(&v.mount), v.name.clone()),
            Target::Folder(p) => scan::local::start(p.clone(), target.title()),
            Target::Ssh { host, path } => scan::remote::start_ssh(host.clone(), path.clone()),
            Target::Rclone(r) => scan::remote::start_rclone(r.clone()),
        };
        let root = View::node(0);
        Session {
            title: target.title(),
            target,
            tree: handle.tree,
            progress: handle.progress,
            view: root,
            back: Vec::new(),
            fwd: Vec::new(),
            anim: Animator::default(),
            treemap: Default::default(),
            layout_sig: (0, root, 0, 0, String::new()),
            last_layout: Instant::now() - Duration::from_secs(1),
            collector: Vec::new(),
            panel_view: root,
            hovered_key: None,
            list_hover: None,
            selected: None,
            watch: None,
            pending: HashSet::new(),
            last_event: Instant::now(),
            refreshing: Arc::new(AtomicBool::new(false)),
            delete: None,
            intro: true,
            was_done: false,
            verts: Vec::new(),
            idx: Vec::new(),
            watch_focus: None,
            expanding: Arc::new(AtomicBool::new(false)),
            geo: None,
            free_space: 0,
            free_at: Instant::now() - Duration::from_secs(60),
            names_checked: 0,
            name_scripts: 0,
        }
    }

    pub fn is_scanning(&self) -> bool {
        !self.progress.is_done()
    }

    pub fn source(&self) -> Source {
        self.tree.read().unwrap().source.clone()
    }

    /// Can the view go into `node`? (see [`crate::tree::Tree::can_enter`])
    pub fn can_enter(&self, node: u32) -> bool {
        let t = self.tree.read().unwrap();
        t.is_alive(node) && t.can_enter(node)
    }

    pub fn navigate(&mut self, v: View) {
        if v != self.view {
            self.back.push(self.view);
            self.fwd.clear();
            self.view = v;
            self.panel_view = v;
        }
    }

    pub fn go_back(&mut self) -> bool {
        if let Some(v) = self.back.pop() {
            self.fwd.push(self.view);
            self.view = v;
            self.panel_view = v;
            true
        } else {
            false
        }
    }

    pub fn go_forward(&mut self) -> bool {
        if let Some(v) = self.fwd.pop() {
            self.back.push(self.view);
            self.view = v;
            self.panel_view = v;
            true
        } else {
            false
        }
    }

    pub fn go_up(&mut self) -> bool {
        let t = self.tree.read().unwrap();
        let parent = if self.view.skip > 0 { self.view.node } else { t.get(self.view.node).parent };
        drop(t);
        if parent == crate::tree::NONE {
            return false;
        }
        self.navigate(View::node(parent));
        true
    }

    /// Adds a node to the collector, keeping the set minimal.
    /// Returns false if already covered.
    pub fn collect(&mut self, node: u32) -> bool {
        let t = self.tree.read().unwrap();
        if !t.is_alive(node) || node == t.root {
            return false;
        }
        if matches!(t.get(node).kind, Kind::Hidden | Kind::Mount) {
            return false;
        }
        if self.collector.iter().any(|c| *c == node || t.is_ancestor(*c, node)) {
            return false;
        }
        let keep: Vec<u32> = self.collector.iter().copied().filter(|c| !t.is_ancestor(node, *c)).collect();
        drop(t);
        self.collector = keep;
        self.collector.push(node);
        true
    }

    pub fn collected_size(&self) -> u64 {
        let t = self.tree.read().unwrap();
        self.collector.iter().filter(|c| t.is_alive(**c)).map(|c| t.get(*c).size).sum()
    }

    pub fn is_collected(&self, t: &crate::tree::Tree, node: u32) -> bool {
        self.collector.iter().any(|c| *c == node || t.is_ancestor(*c, node))
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Screen {
    Home,
    Session(usize),
    Dupes,
}

pub struct DeleteItem {
    pub node: u32,
    pub path: String,
    pub name: String,
    pub size: u64,
    pub verdict: Verdict,
}

#[derive(Clone)]
pub enum RcloneState {
    Looking,
    Missing,
    Ready(Vec<crate::rclone::Remote>),
}

/// Looks for rclone and lists its remotes in the background.
pub fn refresh_rclone(state: &Arc<Mutex<RcloneState>>, ctx: &egui::Context) {
    let r = state.clone();
    let ctx = ctx.clone();
    std::thread::spawn(move || {
        crate::rclone::rescan();
        let v = if crate::rclone::available() { RcloneState::Ready(crate::rclone::remotes()) } else { RcloneState::Missing };
        *r.lock().unwrap() = v;
        ctx.request_repaint();
    });
}

pub enum Modal {
    Settings,
    Ssh { host: String, path: String },
    /// Cloud storage: pick a remote to scan, manage accounts, install rclone.
    Rclone { path: String, confirm_remove: Option<String> },
    /// Add or edit an rclone account.
    CloudAccount(Box<super::cloud::AccountForm>),
    ConfirmDelete { session: usize, items: Vec<DeleteItem>, mode: usize, backup_folder: String, backup_remote: String, ack: bool, secure: bool },
    Deleting { session: usize, finished_at: Option<Instant> },
    Error { title: String, message: String },
    /// ssh needs a key passphrase or a password.
    SshAuth { host: String, path: String, secret: String, retry: bool },
    /// A newer version: what is new, update or restart now, or later.
    Update { scroll: f32, content_h: f32 },
    /// Duplicates: confirm removing the selected copies, then follow it.
    DupesConfirm { ack: bool, permanent: bool },
    DupesRemoving { finished_at: Option<Instant> },
}

/// Drives the animated Dock tile on macOS.
#[derive(Default)]
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub struct DockState {
    shown: bool,
    scanning: bool,
    last: Option<Instant>,
    done_at: Option<Instant>,
}

pub struct CtxMenu {
    pub session: usize,
    pub node: u32,
    pub pos: Pos2,
    pub opened: Instant,
}

pub struct App {
    pub settings: Settings,
    pub theme: Theme,
    pub themes: Vec<Theme>,
    pub sounds: Option<Sounds>,
    pub gl: Option<Arc<Mutex<SunburstGl>>>,
    pub screen: Screen,
    pub sessions: Vec<Session>,
    pub volumes: Vec<Volume>,
    pub volumes_at: Instant,
    pub updater: Updater,
    pub modal: Option<Modal>,
    pub modal_opened: Instant,
    pub toasts: Toasts,
    pub drag: Option<Drag>,
    pub particles: Particles,
    pub time: f64,
    pub dt: f32,
    pub last_frame: Instant,
    pub rclone: Arc<Mutex<RcloneState>>,
    /// rclone download in progress (Cloud storage dialog).
    pub ssh_hosts: Vec<String>,
    pub ctx_menu: Option<CtxMenu>,
    pub bin_rect: Rect,
    pub bin_open: bool,
    pub bin_bounce: f32,
    pub applied_theme: String,
    pub gl_error: Option<String>,
    pub debug: super::debug::Debug,
    /// (vertical center, right edge) of the macOS traffic lights, in points.
    pub chrome: (f32, f32),
    /// Hosts for which the user already typed a secret (to flag wrong ones).
    pub ssh_tried: HashSet<String>,
    /// Language picker popover open in Settings.
    pub lang_picker: bool,
    /// Settings modal scroll offset and measured content height.
    pub settings_scroll: f32,
    pub settings_content_h: f32,
    /// Home page scroll offset and measured content height.
    pub home_scroll: f32,
    pub home_content_h: f32,
    /// Version the update window was last shown for: once per version and launch.
    pub update_prompted: Option<String>,
    /// "Update now" was chosen: restart as soon as the update is installed.
    pub update_restart: bool,
    /// App icon as a texture, for the home header.
    pub logo: Option<egui::TextureHandle>,
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    pub dock: DockState,
    pub director: Option<super::director::Director>,
    /// Named rects of clickable things, for the demo director.
    pub marks: std::collections::HashMap<String, Rect>,
    /// Scripts of file names that the installed fonts cover.
    pub font_scripts: u32,
    /// Frames spent placing the window on the screen (done at 255).
    pub placing: u8,
    /// Folder to scan once the window is up (`squirreldisk <folder>`).
    pub open_at_start: Option<PathBuf>,
    /// Installing the `squirreldisk` command from Settings: the outcome.
    pub cli_job: Option<Arc<Mutex<Option<Result<(), String>>>>>,
    /// The finished scans `squirreldisk scan` can read (see [`crate::ipc`]).
    #[cfg(unix)]
    pub scans: Arc<Mutex<crate::ipc::Scans>>,
    /// The duplicate finder, once opened.
    pub dupes: Option<super::dupes::DupesUi>,
}

const INTER: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");

/// Where a font's baseline sits below the middle of its line box, in ems.
/// Read the way egui reads it, so the two agree.
fn baseline_drop(bytes: &[u8], index: u32) -> Option<f32> {
    use skrifa::MetadataProvider;
    let font = skrifa::FontRef::from_index(bytes, index).ok()?;
    let m = font.metrics(skrifa::instance::Size::unscaled(), skrifa::instance::LocationRef::default());
    if m.units_per_em == 0 {
        return None;
    }
    Some((m.ascent + m.descent - m.leading) / 2.0 / m.units_per_em as f32)
}

/// How far down to draw a system font's glyphs, as a fraction of the font
/// size, so they share Inter's baseline. egui lines a fallback font up by the
/// middle of its line box, and system fonts split theirs differently from
/// Inter (a tall line gap, a deep descent), which left their text a few
/// pixels too high or too low next to Latin.
fn baseline_shift(bytes: &[u8], index: u32) -> f32 {
    match (baseline_drop(INTER, 0), baseline_drop(bytes, index)) {
        (Some(inter), Some(font)) => inter - font,
        _ => 0.0,
    }
}

/// The system font for `code`'s script. Fonts still installed are reused
/// rather than read again from disk (installs happen on the UI thread, e.g.
/// each time a scan meets a new script); the cache holds weak references so
/// a font is freed once no installed font set uses it.
fn system_font_data(code: &str) -> Option<Arc<FontData>> {
    static CACHE: Mutex<Vec<(String, std::sync::Weak<FontData>)>> = Mutex::new(Vec::new());
    let mut cache = CACHE.lock().unwrap();
    cache.retain(|(_, w)| w.strong_count() > 0);
    if let Some(fd) = cache.iter().find(|(c, _)| c == code).and_then(|(_, w)| w.upgrade()) {
        return Some(fd);
    }
    let (bytes, index) = crate::i18n::system_font(code)?;
    let shift = baseline_shift(&bytes, index);
    let mut fd = FontData::from_owned(bytes);
    fd.index = index;
    fd.tweak.y_offset_factor = shift;
    let fd = Arc::new(fd);
    cache.push((code.to_string(), Arc::downgrade(&fd)));
    Some(fd)
}

/// Installs the bundled fonts plus, for non-Latin languages, a system font
/// that covers the script (loaded at runtime to keep the app small). The
/// scripts used by file names get a font too, whatever the UI language.
pub fn install_fonts(ctx: &egui::Context, lang: &str, all_scripts: bool) {
    let mut fonts = FontDefinitions::default();
    // The current language's script first, then the ones file names use;
    // with `all_scripts` (language picker open) one font per script so every
    // native name renders.
    let name_scripts = crate::i18n::name_scripts();
    let mut codes: Vec<&str> = vec![lang];
    let mut seen = vec![crate::i18n::lang(lang).script];
    for l in crate::i18n::LANGS.iter() {
        let windows_ja = all_scripts && cfg!(windows) && l.code == "ja";
        if (all_scripts || name_scripts & l.script.bit() != 0) && (!seen.contains(&l.script) || windows_ja) {
            seen.push(l.script);
            codes.push(l.code);
        }
    }
    let mut script_names: Vec<String> = Vec::new();
    for (k, code) in codes.iter().enumerate() {
        if let Some(fd) = system_font_data(code) {
            let name = format!("system-script-{k}");
            fonts.font_data.insert(name.clone(), fd);
            script_names.push(name);
        }
    }
    fonts.font_data.insert("inter".into(), Arc::new(FontData::from_static(INTER)));
    fonts.font_data.insert("inter-bold".into(), Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Inter-SemiBold.ttf"))));
    fonts.font_data.insert("fredoka".into(), Arc::new(FontData::from_static(include_bytes!("../../assets/fonts/Fredoka-SemiBold.ttf"))));
    let mut fallback: Vec<String> = fonts.families.get(&FontFamily::Proportional).cloned().unwrap_or_default();
    // after egui's defaults: they cover Cyrillic/Greek with proper metrics,
    // the system fonts then fill in CJK, Arabic, Indic scripts and Thai
    fallback.extend(script_names);
    let mut prop = vec!["inter".to_string()];
    prop.extend(fallback.iter().cloned());
    fonts.families.insert(FontFamily::Proportional, prop);
    let mut b = vec!["inter-bold".to_string()];
    b.extend(fallback.iter().cloned());
    fonts.families.insert(FontFamily::Name("bold".into()), b);
    let mut d = vec!["inter-bold".to_string()];
    d.extend(fallback.iter().cloned());
    fonts.families.insert(FontFamily::Name("display".into()), d);
    let mut br = vec!["fredoka".to_string(), "inter-bold".to_string()];
    br.extend(fallback.iter().cloned());
    fonts.families.insert(FontFamily::Name("brand".into()), br);
    ctx.set_fonts(fonts);
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut settings = Settings::load();
        if let Ok(style) = std::env::var("SQUIRRELDISK_CHART") {
            settings.chart_style = style;
        }
        let lang = crate::i18n::resolve(&std::env::var("SQUIRRELDISK_LANG").unwrap_or_else(|_| settings.language.clone()));
        crate::i18n::set_language(lang);
        install_fonts(&cc.egui_ctx, lang, false);
        // Ctrl +/−/0 drive `ui_scale` instead (see `auto_scale`)
        cc.egui_ctx.options_mut(|o| o.zoom_with_keyboard = false);
        let theme = theme::by_name(&settings.theme);
        let (gl, gl_error) = match cc.gl.as_ref().map(|gl| SunburstGl::new(gl)) {
            Some(Ok(r)) => (Some(Arc::new(Mutex::new(r))), None),
            Some(Err(e)) => (None, Some(e)),
            None => (None, Some("no OpenGL context".into())),
        };
        if let Some(e) = &gl_error {
            eprintln!("SquirrelDisk: falling back to CPU rendering: {e}");
        }
        let rclone = Arc::new(Mutex::new(RcloneState::Looking));
        refresh_rclone(&rclone, &cc.egui_ctx);
        // No-network build: never asks for updates.
        let updater = Updater::new();
        // system hooks: keep the folder menu pointing at this copy of the
        // app, and set up the `squirreldisk` command once
        if !scan::demo::enabled() {
            if settings.folder_menu {
                let _ = super::integration::set_folder_menu(true);
            }
            if !settings.cli_offered {
                settings.cli_offered = true;
                settings.save();
                super::integration::install_cli_first_launch();
            }
        }
        // `squirreldisk scan` asks this window before it scans by itself
        // (never the made-up disks of the demo)
        #[cfg(unix)]
        let scans = Arc::new(Mutex::new(crate::ipc::Scans::default()));
        #[cfg(unix)]
        if let Some(socket) = crate::ipc::socket_path().filter(|_| !scan::demo::enabled()) {
            let s = scans.clone();
            let _ = crate::ipc::serve(socket, move |request| crate::cli::answer(&s, request));
        }
        App {
            themes: theme::all(),
            theme,
            sounds: Some(Sounds::new()),
            gl,
            screen: Screen::Home,
            sessions: Vec::new(),
            volumes: disks::list(),
            volumes_at: Instant::now(),
            updater,
            modal: None,
            modal_opened: Instant::now(),
            toasts: Toasts::default(),
            drag: None,
            particles: Particles::default(),
            time: 0.0,
            dt: 0.016,
            last_frame: Instant::now(),
            rclone,
            ssh_hosts: scan::remote::ssh_config_hosts(),
            ctx_menu: None,
            bin_rect: Rect::NOTHING,
            bin_open: false,
            bin_bounce: 0.0,
            applied_theme: String::new(),
            gl_error,
            debug: super::debug::Debug::from_env(),
            ssh_tried: HashSet::new(),
            lang_picker: false,
            settings_scroll: 0.0,
            settings_content_h: 640.0,
            home_scroll: 0.0,
            home_content_h: 0.0,
            update_prompted: None,
            update_restart: false,
            logo: None,
            dock: DockState::default(),
            director: super::director::Director::from_env(),
            marks: Default::default(),
            open_at_start: None,
            cli_job: None,
            placing: if std::env::var("SQD_WINDOW").is_ok() { u8::MAX } else { 0 },
            font_scripts: 0,
            chrome: if cfg!(target_os = "macos") { (15.0, 68.0) } else { (22.0, 0.0) },
            #[cfg(unix)]
            scans,
            dupes: None,
            settings,
        }
    }

    /// Gives `squirreldisk scan` the finished scans of local folders.
    #[cfg(unix)]
    fn publish_scans(&self) {
        let done = self.sessions.iter().filter(|s| s.was_done && s.progress.error.lock().unwrap().is_none() && s.source().is_local());
        self.scans.lock().unwrap().set(done.map(|s| (s.tree.clone(), s.watch.is_some())));
    }

    /// Mirrors scan progress in the Dock icon: the ring fills up and the acorn
    /// wobbles while scanning, then it hops once and returns to the full icon.
    #[allow(unused_variables)]
    fn update_dock(&mut self, ctx: &egui::Context) {
        #[cfg(target_os = "macos")]
        {
            use crate::icon;
            let now = Instant::now();
            let due = |every: u64, d: &DockState| d.last.is_none_or(|l| now.duration_since(l).as_millis() as u64 >= every);
            let focus = match self.screen {
                Screen::Session(i) => self.sessions.get(i).filter(|s| s.is_scanning()),
                Screen::Home | Screen::Dupes => None,
            };
            let scanning = focus.or_else(|| self.sessions.iter().find(|s| s.is_scanning()));
            let frame = if let Some(s) = scanning {
                self.dock.scanning = true;
                self.dock.done_at = None;
                due(80, &self.dock).then(|| icon::scanning_frame(s.progress.fraction(), self.time as f32))
            } else {
                if self.dock.scanning {
                    self.dock.scanning = false;
                    self.dock.done_at = Some(now);
                }
                match self.dock.done_at {
                    Some(d) if d.elapsed().as_secs_f32() < 1.2 => {
                        ctx.request_repaint();
                        due(40, &self.dock).then(|| icon::done_frame(d.elapsed().as_secs_f32()))
                    }
                    Some(_) => {
                        self.dock.done_at = None;
                        Some(icon::Frame::IDLE)
                    }
                    None if !self.dock.shown => Some(icon::Frame::IDLE),
                    None => None,
                }
            };
            if let Some(f) = frame {
                super::macos::set_dock_icon(&icon::png(256, f, true));
                self.dock.shown = true;
                self.dock.last = Some(now);
            }
        }
    }

    /// Registers a clickable rect (only needed by the demo director).
    pub fn mark(&mut self, name: impl Into<String>, rect: Rect) {
        if self.director.is_some() {
            self.marks.insert(name.into(), rect);
        }
    }

    /// Switches the UI language ("auto" follows the system).
    pub fn set_language(&mut self, ctx: &egui::Context, setting: &str) {
        self.settings.language = setting.to_string();
        let code = crate::i18n::resolve(setting);
        crate::i18n::set_language(code);
        install_fonts(ctx, code, false);
        self.settings.save();
    }

    /// File and volume names can be in any script: loads fonts for the ones
    /// they use, else e.g. Chinese folder names show as boxes in English.
    fn fonts_for_names(&mut self, ctx: &egui::Context) {
        use crate::i18n::scripts_of;
        let mut scripts = self.volumes.iter().fold(0, |bits, v| bits | scripts_of(&v.name));
        for s in self.sessions.iter_mut() {
            let t = s.tree.read().unwrap();
            // nodes are only ever appended: each name is checked once
            let new = t.nodes.get(s.names_checked..).unwrap_or_default();
            s.name_scripts = new.iter().fold(s.name_scripts | scripts_of(&t.root_path), |bits, n| bits | scripts_of(&n.name));
            s.names_checked = t.nodes.len();
            scripts |= s.name_scripts;
        }
        // recomputed from what is open, so closing a session frees its fonts
        if scripts != self.font_scripts {
            self.font_scripts = scripts;
            crate::i18n::set_name_scripts(scripts);
            install_fonts(ctx, crate::i18n::current(), self.lang_picker);
            ctx.request_repaint(); // the new fonts apply from the next frame
        }
    }

    /// Scales the whole interface with the window: at the usual size and
    /// below, one point stays one point; a bigger window draws everything
    /// bigger, and a window smaller than the layout's minimum (600×360
    /// points, either way up: a phone held upright is 360 wide) shrinks it
    /// to fit. `ui_scale` multiplies the result.
    fn auto_scale(&mut self, ctx: &egui::Context) {
        use egui::gui_zoom::kb_shortcuts::{ZOOM_IN, ZOOM_IN_SECONDARY, ZOOM_OUT, ZOOM_RESET};
        let step = ctx.input_mut(|i| {
            if i.consume_shortcut(&ZOOM_RESET) {
                Some(0.0)
            } else if i.consume_shortcut(&ZOOM_IN) || i.consume_shortcut(&ZOOM_IN_SECONDARY) {
                Some(0.1)
            } else if i.consume_shortcut(&ZOOM_OUT) {
                Some(-0.1)
            } else {
                None
            }
        });
        if let Some(step) = step {
            self.settings.ui_scale = stepped_scale(self.settings.ui_scale, step);
            self.settings.save();
        }
        // window size in points at zoom 1
        let z = zoom_for(ctx.content_rect().size() * ctx.zoom_factor(), self.settings.ui_scale);
        if (z - ctx.zoom_factor()).abs() > 0.01 {
            ctx.set_zoom_factor(z);
        }
    }

    /// Opens the window centred on the screen the user is working on and
    /// sized to it (see [`super::placement`]). The screen is only known once
    /// the window exists, hence here, on the first frames.
    fn place_window(&mut self, ctx: &egui::Context, frame: &eframe::Frame) {
        use super::placement::{fit, on_active_screen, Placed};
        if self.placing == u8::MAX {
            return;
        }
        self.placing = self.placing.saturating_add(1);
        ctx.request_repaint();
        match on_active_screen(frame) {
            Placed::Done => self.placing = u8::MAX,
            Placed::Again if self.placing < 5 => {}
            Placed::Area(ax, ay, aw, ah) => {
                self.placing = u8::MAX;
                let native = ctx.native_pixels_per_point().unwrap_or(1.0) as f64;
                let (w, h) = fit((aw, ah), native);
                if w >= aw || h >= ah {
                    // a small screen (an old monitor, a Raspberry Pi panel)
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                    return;
                }
                // egui takes points at the current zoom
                let ppp = ctx.pixels_per_point() as f64;
                let (inner, outer) = ctx.input(|i| (i.viewport().inner_rect, i.viewport().outer_rect));
                let frame = match (inner, outer) {
                    (Some(i), Some(o)) => (o.size() - i.size()) * ppp as f32, // title bar and borders
                    _ => egui::Vec2::ZERO,
                };
                let x = ax + (aw - w - frame.x as f64) / 2.0;
                let y = ay + (ah - h - frame.y as f64) / 2.0;
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2((w / ppp) as f32, (h / ppp) as f32)));
                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(egui::pos2((x / ppp) as f32, (y / ppp) as f32)));
            }
            _ => {
                // fallback (Wayland): only the size of the screen egui
                // reports, in points; where the window goes is up to the
                // compositor
                let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) else {
                    // not reported yet: try again for a few frames, then keep the default
                    if self.placing > 30 {
                        self.placing = u8::MAX;
                    }
                    return;
                };
                self.placing = u8::MAX;
                if monitor.x < 1.0 || monitor.y < 1.0 {
                    return;
                }
                let (w, h) = fit((monitor.x as f64, monitor.y as f64), 1.0);
                let size = egui::vec2(w as f32, h as f32);
                if size.x >= monitor.x || size.y >= monitor.y {
                    // a small screen: the whole monitor would overlap the
                    // panels and docks, which only the compositor knows about
                    ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(true));
                } else {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(size));
                }
            }
        }
    }

    /// No-network build: updates are never checked for or downloaded, so
    /// this only shows the update window when the debug hooks fake one.
    fn auto_update(&mut self) {
        if scan::demo::enabled() {
            return;
        }
        let state = self.updater.state();
        self.offer_update(&state);
    }

    /// Opens the update window for a version that is installed and waits for
    /// a restart, or that waits to be installed. Once per version, and never
    /// over a scan or another window.
    pub fn offer_update(&mut self, state: &crate::update::State) {
        use crate::update::State;
        let version = match state {
            State::Ready(v) => v,
            State::Available(r) if r.asset_url.is_some() => &r.version,
            _ => return,
        };
        let busy = self.modal.is_some() || self.ctx_menu.is_some() || self.drag.is_some() || self.director.is_some() || self.sessions.iter().any(|s| s.is_scanning());
        if busy || self.update_prompted.as_ref() == Some(version) {
            return;
        }
        self.update_prompted = Some(version.clone());
        self.open_modal(Modal::Update { scroll: 0.0, content_h: 300.0 });
    }

    pub fn sfx(&mut self, s: Sfx) {
        if self.settings.sound {
            let vol = self.settings.volume;
            if let Some(snd) = self.sounds.as_mut() {
                snd.play(s, vol);
            }
        }
    }

    pub fn open_modal(&mut self, m: Modal) {
        self.modal = Some(m);
        self.modal_opened = Instant::now();
    }

    pub fn start_session(&mut self, target: Target) {
        // on the duplicates screen, what is picked becomes a place to compare
        if self.screen == Screen::Dupes {
            if let Some(d) = self.dupes.as_mut() {
                d.add_place(super::dupes::place_for(&target));
                self.sfx(Sfx::Blip);
                return;
            }
        }
        if let Some(i) = self.sessions.iter().position(|s| s.target.same_place(&target)) {
            if self.sessions[i].is_scanning() {
                self.screen = Screen::Session(i);
                return;
            }
            self.sessions[i] = Session::new(target);
            self.screen = Screen::Session(i);
        } else {
            self.sessions.push(Session::new(target));
            self.screen = Screen::Session(self.sessions.len() - 1);
        }
        self.sfx(Sfx::Blip);
    }

    pub fn cancel_session(&mut self, i: usize) {
        if let Some(s) = self.sessions.get(i) {
            s.progress.cancel.store(true, Ordering::Relaxed);
        }
        self.sessions.remove(i);
        self.screen = Screen::Home;
    }

    fn apply_theme(&mut self, ctx: &egui::Context) {
        if self.applied_theme == self.theme.name {
            return;
        }
        self.applied_theme = self.theme.name.to_string();
        let t = &self.theme;
        let mut v = if t.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        v.selection.bg_fill = theme::with_alpha(t.accent, 0.45);
        v.selection.stroke = egui::Stroke::new(1.0, t.text);
        v.text_cursor.stroke = egui::Stroke::new(2.0, t.accent);
        v.extreme_bg_color = t.bg_bottom;
        v.panel_fill = t.bg_bottom;
        v.window_fill = t.surface;
        v.override_text_color = Some(t.text);
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.interaction.selectable_labels = false;
            s.spacing.item_spacing = egui::vec2(0.0, 0.0);
        });
    }

    /// Per-frame housekeeping for all sessions: watcher events, deletions, scan completion.
    fn poll_sessions(&mut self) {
        let mut sounds: Vec<Sfx> = Vec::new();
        let mut toasts: Vec<(String, Color32)> = Vec::new();
        let mut auth_needed: Option<(String, String)> = None;
        let watch_enabled = self.settings.watch_fs;
        for s in self.sessions.iter_mut() {
            // scan finished?
            if !s.was_done && s.progress.is_done() {
                s.was_done = true;
                if let Some(e) = s.progress.error.lock().unwrap().clone() {
                    if let (Target::Ssh { host, path }, true) = (&s.target, scan::remote::is_auth_error(&e)) {
                        auth_needed = Some((host.clone(), path.clone()));
                    } else if e != "cancelled" {
                        toasts.push((format!("{}: {e}", s.title), self.theme.danger));
                        sounds.push(Sfx::Error);
                    }
                } else {
                    // A finished scan speaks for itself: no toast, just the (optional) sound.
                    sounds.push(Sfx::Success);
                    if watch_enabled && s.source().is_local() {
                        let root = s.tree.read().unwrap().root_path.clone();
                        s.watch = FsWatch::new(std::path::Path::new(&root));
                    }
                }
            }
            // filesystem events
            if let Some(w) = &s.watch {
                let root = s.tree.read().unwrap().root_path.clone();
                let mut got = false;
                while let Ok(p) = w.rx.try_recv() {
                    if s.pending.len() < 4096 {
                        s.pending.insert(crate::watch::normalize_event_path(p, &root));
                    }
                    got = true;
                }
                if got {
                    s.last_event = Instant::now();
                }
            }
            if !s.pending.is_empty() && s.last_event.elapsed() > Duration::from_millis(600) && !s.refreshing.load(Ordering::Relaxed) && s.delete.is_none() {
                let paths: Vec<PathBuf> = s.pending.drain().collect();
                let tree = s.tree.clone();
                let flag = s.refreshing.clone();
                flag.store(true, Ordering::Relaxed);
                std::thread::spawn(move || {
                    apply_fs_changes(&tree, paths);
                    flag.store(false, Ordering::Relaxed);
                });
            }
            // deletion progress
            if let Some(d) = &s.delete {
                let done: Vec<u32> = std::mem::take(&mut *d.completed.lock().unwrap());
                if !done.is_empty() {
                    let mut t = s.tree.write().unwrap();
                    for n in &done {
                        t.remove(*n);
                    }
                    drop(t);
                    s.collector.retain(|c| !done.contains(c));
                    // once per job: completions arrive frame after frame
                    if !d.sounded.swap(true, Ordering::Relaxed) {
                        sounds.push(Sfx::Crunch);
                    }
                }
                if d.finished.load(Ordering::Relaxed) && d.completed.lock().unwrap().is_empty() {
                    // keep the job around for the modal until it is closed
                }
            }
        }
        for (t, c) in toasts {
            self.toasts.push(t, c);
        }
        if let Some((host, path)) = auth_needed {
            let retry = self.ssh_tried.contains(&host);
            self.ssh_tried.insert(host.clone());
            self.open_modal(Modal::SshAuth { host, path, secret: String::new(), retry });
        }
        for snd in sounds {
            self.sfx(snd);
        }
    }

    pub fn start_delete(&mut self, session: usize, items: Vec<DeleteItem>, mode: delete::Mode) {
        let Some(s) = self.sessions.get_mut(session) else { return };
        let (source, root) = {
            let t = s.tree.read().unwrap();
            (t.source.clone(), t.root_path.clone())
        };
        let list: Vec<delete::Item> = items
            .into_iter()
            .filter(|i| !i.verdict.is_forbidden())
            .map(|i| delete::Item { node: i.node, path: i.path, size: i.size })
            .collect();
        s.delete = Some(delete::start(list, mode, source, root));
        self.open_modal(Modal::Deleting { session, finished_at: None });
    }
}

/// Maps a batch of changed paths to tree updates.
fn apply_fs_changes(tree: &SharedTree, paths: Vec<PathBuf>) {
    let mut dirs: HashSet<u32> = HashSet::new();
    let mut removed: Vec<u32> = Vec::new();
    {
        let t = tree.read().unwrap();
        for p in paths {
            let ps = p.to_string_lossy();
            let (id, exact) = t.find_path(&ps);
            if id == crate::tree::NONE {
                continue;
            }
            if exact && !p.exists() && id != t.root {
                removed.push(id);
                continue;
            }
            let dir = if exact && t.get(id).kind == Kind::Dir && !(p.exists() && !p.is_dir()) { id } else if exact { t.get(id).parent } else { id };
            if dir != crate::tree::NONE && t.get(dir).kind == Kind::Dir {
                dirs.insert(dir);
            }
        }
    }
    if !removed.is_empty() {
        let mut t = tree.write().unwrap();
        for r in removed {
            t.remove(r);
        }
    }
    for d in dirs.into_iter().take(64) {
        scan::local::refresh_dir(tree, d);
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        #[cfg(target_os = "macos")]
        match super::macos::traffic_lights(frame) {
            Some(Some(c)) => self.chrome = c,
            // full screen: same bar, starting at the window's edge
            Some(None) => self.chrome.1 = 0.0,
            None => {}
        }
        let _ = &frame;
        self.place_window(&ctx, frame);
        self.auto_scale(&ctx);
        if let Some(folder) = self.open_at_start.take() {
            let mount = folder.to_string_lossy();
            match self.volumes.iter().find(|v| v.mount == mount).cloned() {
                Some(v) => self.start_session(Target::Volume(v)),
                None => self.start_session(Target::Folder(folder)),
            }
        }
        let now = Instant::now();
        self.dt = now.duration_since(self.last_frame).as_secs_f32().clamp(0.001, 0.1);
        self.last_frame = now;
        self.time += self.dt as f64;
        self.apply_theme(&ctx);
        self.direct(&ctx);
        self.marks.clear();
        self.debug_hooks(&ctx);
        self.poll_sessions();
        #[cfg(unix)]
        self.publish_scans();
        self.fonts_for_names(&ctx);
        self.update_dock(&ctx);
        self.auto_update();
        if self.volumes_at.elapsed() > Duration::from_secs(5) && self.screen == Screen::Home {
            self.volumes = disks::list();
            self.volumes_at = Instant::now();
        }

        // dropped folders → scan
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().map(|f| f.path().to_path_buf()).collect());
        if let Some(p) = dropped.into_iter().find(|p| p.is_dir()) {
            self.start_session(Target::Folder(p));
        }

        let screen = ctx.content_rect();
        widgets::vgradient(ui.painter(), screen, self.theme.bg_top, self.theme.bg_bottom);

        match self.screen {
            Screen::Home => self.home_ui(ui, screen),
            Screen::Dupes => self.dupes_ui(ui, screen),
            Screen::Session(i) => {
                if i < self.sessions.len() {
                    self.session_ui(ui, screen, i)
                } else {
                    self.screen = Screen::Home;
                }
            }
        }

        self.context_menu_ui(&ctx);
        self.modal_ui(&ctx);
        self.drag_overlay(&ctx);
        let theme = self.theme.clone();
        self.particles.step_and_draw(&ctx, self.dt, &theme);
        self.toasts.draw(&ctx, &theme, self.dt);

        // keyboard shortcuts
        // ⌘, (Ctrl+, elsewhere) opens Settings from anywhere, like every Mac app.
        let open_settings = ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::Comma));
        if open_settings && !matches!(self.modal, Some(Modal::Deleting { .. } | Modal::DupesRemoving { .. }) | Some(Modal::Settings)) {
            self.ctx_menu = None;
            self.open_modal(Modal::Settings);
        }
        let (esc, back) = ctx.input(|i| (i.key_pressed(egui::Key::Escape), i.key_pressed(egui::Key::Backspace)));
        if esc {
            if self.ctx_menu.is_some() {
                self.ctx_menu = None;
            } else if self.modal.as_ref().is_some_and(|m| !matches!(m, Modal::Deleting { .. } | Modal::DupesRemoving { .. })) {
                self.modal = None;
            } else if self.drag.is_some() {
                self.drag = None;
            } else if self.screen == Screen::Dupes {
                self.dupes_escape();
            } else if let Screen::Session(i) = self.screen {
                if !self.sessions[i].go_up() {
                    self.screen = Screen::Home;
                } else {
                    self.sfx(Sfx::BlipDown);
                }
            }
        }
        if back && self.modal.is_none() && !ctx.egui_wants_keyboard_input() {
            if let Screen::Session(i) = self.screen {
                if self.sessions[i].go_up() {
                    self.sfx(Sfx::BlipDown);
                }
            }
        }

        // repaint policy: animate when something moves, poll slowly while scanning
        let scanning = self.sessions.iter().any(|s| s.is_scanning() || s.delete.is_some());
        let animating = self.drag.is_some()
            || !self.particles.is_empty()
            || !self.toasts.is_empty()
            || match self.screen {
                Screen::Session(i) => self.sessions.get(i).is_some_and(|s| if self.settings.chart_style == "treemap" { !s.treemap.settled } else { !s.anim.settled }),
                Screen::Home => false,
                Screen::Dupes => self.dupes.as_ref().is_some_and(|d| d.is_scanning()),
            };
        if animating || (self.settings.shader_fx && matches!(self.screen, Screen::Session(_))) {
            ctx.request_repaint();
        } else if scanning {
            ctx.request_repaint_after(Duration::from_millis(80));
        } else {
            ctx.request_repaint_after(Duration::from_millis(500));
        }
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(d) = self.director.as_mut() {
            d.inject(raw);
        }
    }

    fn on_exit(&mut self, gl: Option<&eframe::glow::Context>) {
        self.settings.save();
        if let (Some(gl), Some(r)) = (gl, &self.gl) {
            if let Ok(r) = r.lock() {
                r.destroy(gl);
            }
        }
        for s in &self.sessions {
            s.progress.cancel.store(true, Ordering::Relaxed);
        }
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        let c = self.theme.bg_bottom;
        [c.r() as f32 / 255.0, c.g() as f32 / 255.0, c.b() as f32 / 255.0, 1.0]
    }
}

/// Extensions the OS runs, installs or follows instead of just showing:
/// programs, scripts, shortcuts, installers, app bundles. Compared lowercase.
const LAUNCHABLE: &[&str] = &[
    // Windows
    "exe", "com", "bat", "cmd", "ps1", "psm1", "psd1", "vbs", "vbe", "js", "jse", "wsf", "wsh", "msi", "msp", "mst", "scr", "pif", "lnk", "url",
    "hta", "cpl", "reg", "inf", "scf", "msc", "gadget", "application", "appref-ms", "settingcontent-ms", "appx", "appxbundle", "msix",
    "msixbundle", "library-ms", "search-ms", "diagcab", "chm", "xll",
    // Linux, macOS and anywhere with an interpreter
    "sh", "bash", "zsh", "ksh", "csh", "fish", "command", "tool", "desktop", "appimage", "run", "bin", "py", "pyw", "pyz", "pl", "rb", "jar",
    "terminal", "scpt", "scptd", "applescript", "workflow", "action", "app", "prefpane", "saver", "pkg", "mpkg", "deb", "rpm", "flatpakref",
    "flatpak", "snap", "webloc", "inetloc",
];

/// True for files that opening would run rather than show. A scanned disk can
/// hold anything, so a double-click must never start a program someone left
/// there: we reveal these in the file manager instead.
pub fn is_launchable(path: &std::path::Path) -> bool {
    fn ext_hit(p: &std::path::Path) -> bool {
        // Windows drops trailing dots and spaces, so "x.exe." runs x.exe
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { return false };
        let name = name.trim_end_matches(['.', ' ']);
        name.rsplit_once('.').is_some_and(|(stem, ext)| !stem.is_empty() && LAUNCHABLE.contains(&ext.to_ascii_lowercase().as_str()))
    }
    // a symlink opens its target, so check both names
    if ext_hit(path) || std::fs::canonicalize(path).is_ok_and(|t| ext_hit(&t)) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;
        let Ok(md) = std::fs::metadata(path) else { return false };
        if !md.is_file() || md.permissions().mode() & 0o111 == 0 {
            return false;
        }
        // macOS runs any extensionless executable in Terminal
        if path.extension().is_none() {
            return true;
        }
        // FAT/NTFS mounts mark every file executable, so for the rest only
        // real programs count: ELF, Mach-O, or a #! script
        let mut head = [0u8; 4];
        let n = std::fs::File::open(path).and_then(|mut f| f.read(&mut head)).unwrap_or(0);
        let head = &head[..n];
        head.starts_with(b"#!")
            || head == b"\x7fELF"
            || matches!(head, [0xfe, 0xed, 0xfa, 0xce | 0xcf] | [0xce | 0xcf, 0xfa, 0xed, 0xfe] | [0xca, 0xfe, 0xba, 0xbe])
    }
    #[cfg(not(unix))]
    false
}

/// Opens a scanned file with its default app, or reveals it in the file
/// manager. Programs and scripts are always revealed (see [`is_launchable`]);
/// returns true when that happened, so the caller can say why.
pub fn os_open(path: &str, reveal: bool) -> bool {
    let guarded = !reveal && is_launchable(std::path::Path::new(path));
    os_launch(path, reveal || guarded);
    guarded
}

/// Opens a web page in the browser. Only http(s), so it can't run anything.
pub fn open_url(url: &str) {
    if url.starts_with("https://") || url.starts_with("http://") {
        os_launch(url, false);
    }
}

fn os_launch(path: &str, reveal: bool) {
    let mut cmd;
    #[cfg(target_os = "macos")]
    {
        cmd = std::process::Command::new("open");
        if reveal {
            cmd.arg("-R");
        }
        cmd.arg(path);
    }
    #[cfg(windows)]
    {
        cmd = std::process::Command::new("explorer");
        if reveal {
            cmd.arg(format!("/select,{path}"));
        } else {
            cmd.arg(path);
        }
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        cmd = std::process::Command::new("xdg-open");
        let p = std::path::Path::new(path);
        if reveal {
            cmd.arg(p.parent().unwrap_or(p));
        } else {
            cmd.arg(p);
        }
    }
    let _ = cmd.spawn();
}

/// `ui_scale` after Ctrl + (`step` 0.1), Ctrl − (−0.1) or Ctrl 0 (0): in
/// tenths, from 0.5 to 2.
pub fn stepped_scale(ui_scale: f32, step: f32) -> f32 {
    let s = if step == 0.0 || !ui_scale.is_finite() { 1.0 } else { ui_scale + step };
    ((s * 10.0).round() / 10.0).clamp(0.5, 2.0)
}

/// How far to scroll a page this frame: the wheel or touchpad, or a finger
/// dragging on a touch screen (egui only turns that into scrolling inside
/// its own scroll areas, which these pages don't use).
pub fn scroll_delta(ui: &egui::Ui) -> f32 {
    ui.input(|i| {
        let touch = if i.any_touches() && i.pointer.is_decidedly_dragging() { i.pointer.delta().y } else { 0.0 };
        i.smooth_scroll_delta.y + touch
    })
}

/// The smallest window, in points, the layout is made for: the short side
/// and the long side, whichever way up.
pub const MIN_SHORT: f32 = 360.0;
pub const MIN_LONG: f32 = 600.0;

/// The zoom for a window of `base` points (at zoom 1), as in
/// [`App::auto_scale`].
fn zoom_for(base: egui::Vec2, ui_scale: f32) -> f32 {
    let ui_scale = if ui_scale.is_finite() { ui_scale.clamp(0.5, 2.0) } else { 1.0 };
    let fit = (base.x / 1080.0).min(base.y / 720.0).clamp(1.0, 2.5) * ui_scale;
    let (short, long) = (base.x.min(base.y), base.x.max(base.y));
    fit.min(short / MIN_SHORT).min(long / MIN_LONG).max(0.5)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interface_scales_with_the_window() {
        let z = |w, h, s| zoom_for(egui::vec2(w, h), s);
        // the usual size and the layout's minimum, either way up, draw at 1
        assert_eq!(z(1080.0, 720.0, 1.0), 1.0);
        assert_eq!(z(600.0, 360.0, 1.0), 1.0);
        assert_eq!(z(360.0, 600.0, 1.0), 1.0);
        // small screens: an 800×480 panel, an old 640×480 monitor, a phone
        assert_eq!(z(800.0, 440.0, 1.0), 1.0);
        assert_eq!(z(640.0, 420.0, 1.0), 1.0);
        assert_eq!(z(360.0, 792.0, 1.0), 1.0);
        // bigger windows grow by the tighter side, up to 2.5
        assert_eq!(z(2160.0, 1440.0, 1.0), 2.0);
        assert_eq!(z(2160.0, 720.0, 1.0), 1.0);
        assert_eq!(z(7680.0, 4320.0, 1.0), 2.5);
        // a window below the minimum shrinks the layout to fit, down to 0.5
        assert_eq!(z(300.0, 600.0, 1.0), 300.0 / 360.0);
        assert_eq!(z(450.0, 270.0, 1.0), 0.75);
        assert_eq!(z(100.0, 100.0, 1.0), 0.5);
        // the user's scale multiplies, but never past what fits
        assert_eq!(z(1080.0, 720.0, 1.2), 1.2);
        assert_eq!(z(1080.0, 720.0, 2.0), 1080.0 / 600.0);
        assert_eq!(z(2160.0, 1440.0, 1.5), 3.0);
        assert_eq!(z(1080.0, 720.0, 0.5), 0.5);
        assert_eq!(z(800.0, 440.0, 1.3), 440.0 / 360.0);
        // out-of-range values (a hand-edited settings file, SQD_ZOOM) are held in
        assert_eq!(z(1080.0, 720.0, 100.0), 1080.0 / 600.0);
        assert_eq!(z(1080.0, 720.0, -3.0), 0.5);
        assert_eq!(z(1080.0, 720.0, f32::NAN), 1.0);
        assert_eq!(z(1080.0, 720.0, f32::INFINITY), 1.0);
        assert_eq!(z(0.0, 0.0, 1.0), 0.5);
    }

    #[test]
    fn zoom_keys_step_in_tenths() {
        assert_eq!(stepped_scale(1.0, 0.1), 1.1);
        assert_eq!(stepped_scale(1.0, -0.1), 0.9);
        assert_eq!(stepped_scale(1.7, 0.0), 1.0);
        assert_eq!(stepped_scale(2.0, 0.1), 2.0);
        assert_eq!(stepped_scale(0.5, -0.1), 0.5);
        assert_eq!(stepped_scale(f32::NAN, 0.1), 1.0);
        // ten presses up and back land on 1 exactly, not 0.99999
        let up = (0..10).fold(1.0, |s, _| stepped_scale(s, 0.1));
        assert_eq!(up, 2.0);
        assert_eq!((0..10).fold(up, |s, _| stepped_scale(s, -0.1)), 1.0);
    }

    #[test]
    fn programs_are_revealed_not_run() {
        let dir = std::env::temp_dir().join(format!("sqd-test-launch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Tool.app")).unwrap();
        let file = |name: &str, body: &[u8]| {
            let p = dir.join(name);
            std::fs::write(&p, body).unwrap();
            p
        };
        for name in ["setup.exe", "RUN.BAT", "x.ps1", "Shortcut.lnk", "a.desktop", "go.sh", "evil.exe.", "evil.exe ", "f.AppImage", "s.command"] {
            assert!(is_launchable(&file(name, b"")), "{name}");
        }
        assert!(is_launchable(&dir.join("Tool.app")));
        for name in ["photo.jpg", "notes.txt", "song.mp3", "archive.zip", ".exe", "exe", "readme"] {
            assert!(!is_launchable(&file(name, b"hello")), "{name}");
        }
        assert!(!is_launchable(&dir.join("missing.txt")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exec = |p: &std::path::Path| std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
            // an executable with no extension, or a real program with any name
            for (name, body) in [("tool", &b"data"[..]), ("x.txt", b"#!/bin/sh\n"), ("y.dat", b"\x7fELF\x02"), ("z.so", &[0xcf, 0xfa, 0xed, 0xfe][..])] {
                let p = file(name, body);
                exec(&p);
                assert!(is_launchable(&p), "{name}");
            }
            // FAT/NTFS mounts mark everything executable; a photo is still a photo
            let p = file("pic.jpg", &[0xff, 0xd8, 0xff, 0xe0]);
            exec(&p);
            assert!(!is_launchable(&p));
            // a harmless name pointing at a program opens the program
            let link = dir.join("innocent.txt");
            std::os::unix::fs::symlink(dir.join("setup.exe"), &link).unwrap();
            assert!(is_launchable(&link));
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn system_fonts_share_inters_baseline() {
        assert_eq!(baseline_shift(INTER, 0), 0.0);
        assert_eq!(baseline_shift(b"not a font", 0), 0.0);
        // every script's font on this machine: a shift is a fraction of an em
        for l in crate::i18n::LANGS.iter() {
            if let Some((bytes, index)) = crate::i18n::system_font(l.code) {
                let shift = baseline_shift(&bytes, index);
                assert!(shift.abs() < 0.5, "{}: {shift}", l.code);
            }
        }
    }
}
