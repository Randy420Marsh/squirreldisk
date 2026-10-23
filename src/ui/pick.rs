//! Folder dialogs. Android has none that hands back a file path, so there
//! the app has its own: a list of folders to go into, and a button to take
//! the one shown (see [`FolderPick`]).

use super::app::{App, Modal, Target};
use super::modals::{close_button, mark, modal_frame, title};
use super::theme::{lerp_color, with_alpha};
use super::widgets::{self, cr, font, BtnStyle, Icon};
use crate::i18n::{tr, trf};
use crate::sound::Sfx;
use eframe::egui::{self, Align2, Id, Pos2, Rect, Sense, Vec2};
use std::path::PathBuf;

#[cfg(not(target_os = "android"))]
pub fn folder(title: &str) -> Option<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folder()
}

#[cfg(not(target_os = "android"))]
pub fn folders(title: &str) -> Vec<PathBuf> {
    rfd::FileDialog::new().set_title(title).pick_folders().unwrap_or_default()
}

#[cfg(target_os = "android")]
pub fn folder(_title: &str) -> Option<PathBuf> {
    Some(crate::android::SHARED_STORAGE.into())
}

/// The folder browser in the app, and what the chosen folder is for.
pub struct FolderPick {
    pub path: PathBuf,
    /// The folders in `path`, or why they couldn't be read.
    pub dirs: Result<Vec<String>, String>,
    /// For the duplicates screen (else a scan).
    pub dupes: bool,
}

impl FolderPick {
    pub fn at(path: PathBuf, dupes: bool) -> Self {
        let mut f = FolderPick { path, dirs: Ok(Vec::new()), dupes };
        f.list();
        f
    }

    fn list(&mut self) {
        self.dirs = std::fs::read_dir(&self.path)
            .map(|rd| {
                // links aren't followed, as in a scan
                let mut v: Vec<String> =
                    rd.flatten().filter(|e| e.file_type().is_ok_and(|t| t.is_dir())).map(|e| e.file_name().to_string_lossy().into_owned()).collect();
                v.sort_by_key(|n| (n.starts_with('.'), n.to_lowercase()));
                v
            })
            .map_err(|e| e.to_string());
    }

    pub fn go(&mut self, path: PathBuf) {
        self.path = path;
        self.list();
    }

    /// The folder above, if it can be gone into: on a phone nothing above
    /// shared storage can be read.
    fn parent(&self) -> Option<PathBuf> {
        #[cfg(target_os = "android")]
        if self.path == std::path::Path::new(crate::android::SHARED_STORAGE) {
            return None;
        }
        self.path.parent().map(PathBuf::from)
    }

    /// One folder up; false at the top.
    pub fn up(&mut self) -> bool {
        let Some(p) = self.parent() else { return false };
        self.go(p);
        true
    }

    /// What the button calls the folder shown.
    fn name(&self) -> String {
        #[cfg(target_os = "android")]
        if self.path == std::path::Path::new(crate::android::SHARED_STORAGE) {
            return tr("Phone storage").into();
        }
        self.path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| self.path.to_string_lossy().into_owned())
    }
}

/// `text` with its start cut off to fit `width`, so the end of a path shows.
fn tail(p: &egui::Painter, text: &str, f: &egui::FontId, width: f32) -> String {
    let fits = |s: &str| p.layout_no_wrap(s.to_string(), f.clone(), egui::Color32::WHITE).size().x <= width;
    if fits(text) {
        return text.to_string();
    }
    let mut cut = text.char_indices().map(|(i, _)| i);
    loop {
        match cut.next() {
            Some(i) if !fits(&format!("…{}", &text[i..])) => continue,
            Some(i) => return format!("…{}", &text[i..]),
            None => return "…".into(),
        }
    }
}

impl App {
    /// Asks for a folder to scan, or to compare on the duplicates screen.
    /// SQD_IN_APP_PICKER=1 uses the app's own browser on the desktop too
    /// (to try it out).
    pub fn choose_folder(&mut self, dupes: bool) {
        if cfg!(target_os = "android") || std::env::var_os("SQD_IN_APP_PICKER").is_some() {
            #[cfg(target_os = "android")]
            let top = PathBuf::from(crate::android::SHARED_STORAGE);
            #[cfg(not(target_os = "android"))]
            let top = crate::settings::home_dir().unwrap_or_else(|| "/".into());
            let start = self.settings.last_folder.clone().map(PathBuf::from).filter(|p| p.is_dir()).unwrap_or(top);
            self.open_modal(Modal::Folder(Box::new(FolderPick::at(start, dupes))));
        } else {
            #[cfg(not(target_os = "android"))]
            if dupes {
                if let Some(d) = self.dupes.as_mut() {
                    for p in folders(tr("Choose folders to compare")) {
                        d.add_place(super::dupes::place_for(&Target::Folder(p)));
                    }
                }
            } else if let Some(p) = folder(tr("Choose a folder to scan")) {
                self.start_session(Target::Folder(p));
            }
        }
    }

    pub(super) fn folder_modal(&mut self, ctx: &egui::Context, mut f: Box<FolderPick>) -> Option<Modal> {
        let theme = self.theme.clone();
        let mut close = false;
        let mut take = false;
        let mut into: Option<PathBuf> = None;
        let mut up = false;
        let screen = ctx.content_rect();
        let bd = modal_frame(ctx, self.modal_opened, Vec2::new(520.0, (screen.height() - 32.0).min(640.0)), &theme, |ui, card| {
            title(ui, card, if f.dupes { tr("Choose a folder to compare") } else { tr("Choose a folder to scan") }, None, &theme);
            close = close_button(ui, card, &theme);
            let x0 = card.left() + 28.0;
            let w = card.width() - 56.0;
            let path = f.path.to_string_lossy().into_owned();
            let shown = tail(ui.painter(), &path, &font(12.5), w);
            ui.painter().text(Pos2::new(x0, card.top() + 58.0), Align2::LEFT_TOP, shown, font(12.5), theme.text_dim);
            let list = Rect::from_min_max(Pos2::new(card.left() + 16.0, card.top() + 86.0), Pos2::new(card.right() - 16.0, card.bottom() - 72.0));
            ui.painter().rect_filled(list, cr(10.0), with_alpha(theme.bg_bottom, 0.5));
            let parent = f.parent().is_some();
            ui.scope_builder(egui::UiBuilder::new().max_rect(list.shrink(4.0)), |ui| {
                egui::ScrollArea::vertical().id_salt(("folders", &path)).auto_shrink([false, false]).show(ui, |ui| {
                    let rw = ui.available_width();
                    let row = |ui: &mut egui::Ui, k: usize, icon: Icon, label: &str, dim: bool| {
                        let (r, resp) = ui.allocate_exact_size(Vec2::new(rw, 40.0), Sense::click());
                        let id = Id::new(("folder-row", k));
                        let h = ui.ctx().animate_bool_with_time(id, resp.hovered(), 0.08);
                        let p = ui.painter();
                        p.rect_filled(r, cr(8.0), with_alpha(theme.surface_hi, 0.9 * h));
                        let c = if dim { theme.text_dim } else { theme.text };
                        widgets::draw_icon(p, icon, Rect::from_center_size(Pos2::new(r.left() + 20.0, r.center().y), Vec2::splat(16.0)), lerp_color(theme.text_dim, theme.accent, h));
                        let name = widgets::truncate(p, label, &font(13.5), r.width() - 48.0);
                        p.text(Pos2::new(r.left() + 40.0, r.center().y), Align2::LEFT_CENTER, name, font(13.5), c);
                        if resp.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if resp.clicked() {
                            widgets::click(ui);
                        }
                        resp.clicked()
                    };
                    if parent && row(ui, 0, Icon::Back, tr("Up one folder"), true) {
                        up = true;
                    }
                    match &f.dirs {
                        Ok(dirs) => {
                            for (k, d) in dirs.iter().enumerate() {
                                if row(ui, k + 1, Icon::Folder, d, d.starts_with('.')) {
                                    into = Some(f.path.join(d));
                                }
                            }
                            if dirs.is_empty() {
                                ui.add_space(14.0);
                                ui.vertical_centered(|ui| ui.label(egui::RichText::new(tr("No folders in here")).font(font(12.5)).color(theme.text_faint)));
                            }
                        }
                        Err(_) => {
                            ui.add_space(14.0);
                            ui.vertical_centered(|ui| ui.label(egui::RichText::new(tr("This folder can't be read")).font(font(12.5)).color(theme.warn)));
                        }
                    }
                });
            });
            let bw = (w * 0.62).min(240.0);
            let b = Rect::from_min_size(Pos2::new(card.right() - 28.0 - bw, card.bottom() - 58.0), Vec2::new(bw, 38.0));
            mark("folder-take", b);
            let name = f.name();
            let label = if f.dupes { trf("Compare {name}", &[("name", &name)]) } else { trf("Scan {name}", &[("name", &name)]) };
            take = widgets::button_ex(ui, b, Id::new("folder-take"), &label, Some(if f.dupes { Icon::Copies } else { Icon::Search }), BtnStyle::Primary, &theme, f.dirs.is_ok()).clicked();
            let cw = (w - bw - 10.0).min(100.0);
            if cw >= 60.0 {
                let c = Rect::from_min_size(Pos2::new(b.left() - 10.0 - cw, b.top()), Vec2::new(cw, 38.0));
                close |= widgets::button(ui, c, Id::new("folder-cancel"), tr("Cancel"), None, BtnStyle::Ghost, &theme).clicked();
            }
        });
        if let Some(p) = into {
            f.go(p);
            self.sfx(Sfx::Blip);
        } else if up {
            f.up();
            self.sfx(Sfx::BlipDown);
        }
        if take {
            let path = f.path.clone();
            self.settings.last_folder = Some(path.to_string_lossy().into_owned());
            self.settings.save();
            if f.dupes {
                if let Some(d) = self.dupes.as_mut() {
                    d.add_place(super::dupes::place_for(&Target::Folder(path)));
                }
            } else {
                self.start_session(Target::Folder(path));
            }
            return None;
        }
        if close || bd { None } else { Some(Modal::Folder(f)) }
    }
}
