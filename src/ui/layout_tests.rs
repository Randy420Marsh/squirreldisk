//! Every screen and dialog, landscape and portrait, from a phone held upright
//! to a 1080p monitor turned on its side: no text may run off the side of
//! the window or over other text. Runs the real `App` headless (no window,
//! no OpenGL), with the demo's made-up disks and scan.

use super::*;
use eframe::epaint::{ClippedShape, Shape, TextShape};

/// (width, height, pixels per point), landscape first
const SIZES: &[(f32, f32, f32)] = &[
    (792.0, 360.0, 3.0), // phone, 1080×2376
    (800.0, 480.0, 1.0), // Raspberry Pi HyperPixel 4.0
    (800.0, 444.0, 1.0), // … less a 36-pixel panel
    (640.0, 480.0, 1.0), // CRT
    (640.0, 420.0, 1.0),
    (1024.0, 600.0, 1.0),
    (1280.0, 720.0, 1.0),
    (1920.0, 1080.0, 1.0),
];

#[derive(Clone, Copy, Debug)]
enum Scene {
    Home,
    Session,
    Delete,
    Settings,
    Ssh,
    Cloud,
    Dupes,
    DupesPreview,
    Folder,
}

const SCENES: &[Scene] = &[Scene::Home, Scene::Session, Scene::Delete, Scene::Settings, Scene::Ssh, Scene::Cloud, Scene::Dupes, Scene::DupesPreview, Scene::Folder];

/// A folder with folders in it, one with a long name, for the folder browser.
fn folders() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sqd-folders-{}", std::process::id()));
    for n in ["Camera", "Documents", "Download", "Music", ".thumbnails", "A folder with a name far too long to fit on a phone held upright"] {
        let _ = std::fs::create_dir_all(dir.join(n));
    }
    dir
}

struct Harness {
    ctx: egui::Context,
    app: App,
    time: f64,
}

impl Harness {
    fn new(lang: &str) -> Self {
        scan::demo::force_on_this_thread();
        let ctx = egui::Context::default();
        let settings = Settings { language: lang.into(), ..Default::default() };
        let app = App::bare(&ctx, settings, None);
        Harness { ctx, app, time: 0.0 }
    }

    /// One frame at `size` points; animations step 50 ms per frame.
    fn frame(&mut self, size: (f32, f32, f32)) -> egui::FullOutput {
        self.frame_with(size, Vec::new())
    }

    /// One frame in which `key` is pressed and let go.
    fn press(&mut self, key: egui::Key) {
        let ev = |pressed| egui::Event::Key { key, physical_key: None, pressed, repeat: false, modifiers: Default::default() };
        self.frame_with((792.0, 360.0, 3.0), vec![ev(true), ev(false)]);
    }

    fn frame_with(&mut self, (w, h, ppp): (f32, f32, f32), events: Vec<egui::Event>) -> egui::FullOutput {
        self.time += 0.05;
        self.app.last_frame = Instant::now() - Duration::from_millis(50);
        let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))), time: Some(self.time), predicted_dt: 0.05, events, ..Default::default() };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let app = &mut self.app;
        let mut out = self.ctx.run_ui(raw, |ui| app.frame_ui(ui));
        // nothing draws here, so new glyphs need no upload (epaint panics on
        // dropped texture changes otherwise)
        out.textures_delta.clear();
        out
    }

    fn set_scene(&mut self, scene: Scene) {
        let a = &mut self.app;
        a.modal = None;
        a.ctx_menu = None;
        a.screen = Screen::Home;
        match scene {
            Scene::Home => {}
            Scene::Session => a.screen = Screen::Session(0),
            Scene::Delete => {
                a.screen = Screen::Session(0);
                a.open_delete_confirm(0);
            }
            Scene::Settings => a.open_modal(Modal::Settings),
            Scene::Folder => a.open_modal(Modal::Folder(Box::new(crate::ui::pick::FolderPick::at(folders(), false)))),
            Scene::Ssh => a.open_modal(Modal::Ssh { host: String::new(), path: "/".into() }),
            Scene::Cloud => a.open_modal(Modal::Rclone { path: String::new(), confirm_remove: None }),
            Scene::Dupes | Scene::DupesPreview => {
                a.open_dupes();
                let d = a.dupes.as_mut().unwrap();
                d.selected = None;
                if matches!(scene, Scene::DupesPreview) {
                    d.debug_pick();
                }
            }
        }
        // skip the dialogs' opening animation
        a.modal_opened = Instant::now() - Duration::from_secs(1);
    }

    /// Scans the first made-up disk and puts its two biggest folders in
    /// the collector, as for the delete dialog.
    fn scan(&mut self) {
        let v = self.app.volumes[0].clone();
        self.app.start_session(Target::Volume(v));
        let until = Instant::now() + Duration::from_secs(60);
        while !self.app.sessions[0].progress.is_done() {
            assert!(Instant::now() < until, "the demo scan didn't finish");
            self.frame((1280.0, 720.0, 1.0));
            std::thread::sleep(Duration::from_millis(20));
        }
        self.frame((1280.0, 720.0, 1.0));
        let s = &mut self.app.sessions[0];
        let kids: Vec<u32> = s.tree.read().unwrap().sorted_children(0).into_iter().take(2).collect();
        for k in kids {
            s.collect(k);
        }
    }
}

struct Text {
    rect: Rect,
    text: String,
}

fn flatten<'a>(shape: &'a Shape, clip: Rect, out: &mut Vec<(Rect, &'a Shape)>) {
    match shape {
        Shape::Vec(v) => v.iter().for_each(|s| flatten(s, clip, out)),
        s => out.push((clip, s)),
    }
}

fn visible(t: &TextShape) -> bool {
    let alpha = t.override_text_color.map(|c| c.a()).unwrap_or_else(|| {
        t.galley.job.sections.iter().map(|s| if s.format.color == Color32::PLACEHOLDER { t.fallback_color.a() } else { s.format.color.a() }).max().unwrap_or(0)
    });
    !t.galley.text().trim().is_empty() && alpha as f32 * t.opacity_factor > 60.0
}

/// The text the user can see: on top of the last full-window backdrop (an
/// open dialog), and inside its clip rect and the window.
fn texts(shapes: &[ClippedShape], screen: Rect) -> (Vec<Text>, Vec<String>) {
    let mut flat = Vec::new();
    for c in shapes {
        flatten(&c.shape, c.clip_rect, &mut flat);
    }
    let covers = |r: Rect| r.min.x <= screen.min.x + 1.0 && r.min.y <= screen.min.y + 1.0 && r.max.x >= screen.max.x - 1.0 && r.max.y >= screen.max.y - 1.0;
    let top = flat.iter().rposition(|(_, s)| matches!(s, Shape::Rect(r) if covers(r.rect) && r.fill.a() > 0)).unwrap_or(0);
    let mut seen = Vec::new();
    let mut off = Vec::new();
    for (clip, s) in &flat[top..] {
        let Shape::Text(t) = s else { continue };
        if !visible(t) {
            continue;
        }
        let rect = t.visual_bounding_rect();
        let shown = rect.intersect(*clip).intersect(screen);
        if shown.width() <= 0.5 || shown.height() <= 0.5 {
            continue;
        }
        let text = t.galley.text().to_string();
        // cut off at the side of the window or of its own box
        let room = clip.intersect(screen);
        if rect.left() < room.left() - 1.0 || rect.right() > room.right() + 1.0 {
            off.push(format!("{text:?} at x {:.0}..{:.0} runs out of {:.0}..{:.0}", rect.left(), rect.right(), room.left(), room.right()));
        }
        seen.push(Text { rect: shown, text });
    }
    (seen, off)
}

fn overlaps(texts: &[Text]) -> Vec<String> {
    let mut out = Vec::new();
    for (i, a) in texts.iter().enumerate() {
        for b in &texts[i + 1..] {
            let o = a.rect.intersect(b.rect);
            if o.width() > 2.0 && o.height() > 2.0 && a.text != b.text {
                out.push(format!("{:?} overlaps {:?}", a.text, b.text));
            }
        }
    }
    out
}

/// Every scene at every size, both ways up; returns what is wrong.
fn check(lang: &str) -> Vec<String> {
    let mut h = Harness::new(lang);
    h.frame((1280.0, 720.0, 1.0)); // fonts apply from the second frame
    h.scan();
    let mut problems = Vec::new();
    for &scene in SCENES {
        for &(w, ht, ppp) in SIZES {
            for size in [(w, ht, ppp), (ht, w, ppp)] {
                h.set_scene(scene);
                let mut out = h.frame(size);
                for _ in 0..12 {
                    out = h.frame(size);
                }
                let screen = h.ctx.content_rect();
                let (seen, off) = texts(&out.shapes, screen);
                let at = format!("[{lang}] {scene:?} at {}x{} ({:.0}x{:.0} points)", size.0, size.1, screen.width(), screen.height());
                problems.extend(off.into_iter().chain(overlaps(&seen)).map(|p| format!("{at}: {p}")));
            }
        }
    }
    problems
}

#[test]
fn every_screen_fits_landscape_and_portrait() {
    let problems = check("en");
    assert!(problems.is_empty(), "{} layout problems:\n{}", problems.len(), problems.join("\n"));
}

/// Android's Back (BrowserBack) does what Esc does: closes the dialog that
/// a failed SSH sign-in opens, then goes home from the session.
#[test]
fn back_closes_dialogs_and_leaves_a_failed_session() {
    for key in [egui::Key::BrowserBack, egui::Key::Escape] {
        let mut h = Harness::new("en");
        h.app.start_session(Target::Ssh { host: "pi@10.0.0.7".into(), path: "/".into() });
        h.app.open_modal(Modal::SshAuth { host: "pi@10.0.0.7".into(), path: "/".into(), secret: String::new(), retry: false });
        h.frame((792.0, 360.0, 3.0));
        h.press(key);
        assert!(h.app.modal.is_none(), "{key:?} left the sign-in dialog open");
        assert!(matches!(h.app.screen, Screen::Session(_)));
        h.press(key);
        assert!(h.app.screen == Screen::Home, "{key:?} didn't go home from the session");
        h.app.open_modal(Modal::Ssh { host: String::new(), path: "/".into() });
        h.press(key);
        assert!(h.app.modal.is_none(), "{key:?} left the SSH dialog open");
    }
}

/// Runs `f` on a bare 400×300 window at `time` with these pointer events.
fn widget_frame(ctx: &egui::Context, time: f64, events: Vec<egui::Event>, f: impl FnMut(&mut egui::Ui)) {
    let raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(400.0, 300.0))), time: Some(time), events, ..Default::default() };
    // nothing draws it, so the font atlas is never uploaded
    ctx.run_ui(raw, f).textures_delta.clear();
}

fn pointer(at: Pos2, pressed: Option<bool>) -> Vec<egui::Event> {
    let mut v = vec![egui::Event::PointerMoved(at)];
    if let Some(pressed) = pressed {
        v.push(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: Default::default() });
    }
    v
}

/// A toggle tells the app it was clicked, so every control makes the
/// click sound, not just the ones that play their own.
#[test]
fn toggles_report_their_clicks() {
    let ctx = egui::Context::default();
    let theme = super::theme::all().remove(0);
    let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), egui::vec2(60.0, 30.0));
    let mut on = false;
    let at = rect.center();
    for (t, ev) in [(0.0, pointer(at, None)), (0.1, pointer(at, Some(true))), (0.2, pointer(at, Some(false)))] {
        widget_frame(&ctx, t, ev, |ui| {
            widgets::toggle(ui, rect, egui::Id::new("t"), &mut on, &theme);
        });
    }
    assert!(on);
    assert!(widgets::take_click(&ctx));
    assert!(!widgets::take_click(&ctx), "one click, one sound");
}

/// Picking something up takes a still hold; a quick swipe picks up nothing.
#[test]
fn picking_up_takes_a_hold() {
    let theme = super::theme::all().remove(0);
    let rect = Rect::from_min_size(Pos2::new(20.0, 20.0), egui::vec2(300.0, 200.0));
    let run = |moves: &[(f64, Pos2)]| {
        let ctx = egui::Context::default();
        let mut got = None;
        let start = Pos2::new(100.0, 100.0);
        let mut frames = vec![(0.0, pointer(start, None)), (0.05, pointer(start, Some(true)))];
        frames.extend(moves.iter().map(|&(t, p)| (t, pointer(p, None))));
        for (t, ev) in frames {
            widget_frame(&ctx, t, ev, |ui| {
                let resp = ui.interact(rect, egui::Id::new("row"), egui::Sense::click_and_drag());
                if let Some(p) = widgets::held(ui, &resp, &theme) {
                    got = Some((t, p));
                }
            });
        }
        got
    };
    let still: Vec<(f64, Pos2)> = (2..12).map(|k| (k as f64 * 0.05, Pos2::new(101.0, 100.0))).collect();
    let (t, at) = run(&still).expect("a still hold picks up");
    assert!(t >= 0.05 + widgets::HOLD as f64 - 1e-6 && at == Pos2::new(100.0, 100.0));
    let swipe: Vec<(f64, Pos2)> = (2..12).map(|k| (k as f64 * 0.05, Pos2::new(100.0 + 15.0 * k as f32, 100.0))).collect();
    assert_eq!(run(&swipe), None, "a swipe picked something up");
}
