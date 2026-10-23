//! The dialog for scanning a Windows (SMB) share.

use super::app::{App, Modal, Target};
use super::modals::{close_button, modal_frame, title, wrapped};
use super::theme::{lerp_color, with_alpha};
use super::widgets::{self, bold, cr, font, BtnStyle, Icon};
use crate::i18n::tr;
use crate::smb::{self, Version};
use eframe::egui::{self, Align2, Color32, Id, Pos2, Rect, Sense, Vec2};

#[derive(Clone, Default)]
pub struct SmbForm {
    /// What was typed: `\\server\share\folder`, `server:port/share`, an smb:// URL.
    pub server: String,
    pub user: String,
    /// Kept in memory only (see [`smb::set_password`]).
    pub password: String,
    pub version: Version,
    pub encrypt: bool,
    /// Why the last try failed.
    pub error: Option<String>,
    /// A sign-in that was turned down: the password field asks again.
    pub retry: bool,
}

impl SmbForm {
    /// The form again for a share, after `error`.
    pub fn again(url: &str, error: Option<String>, retry: bool) -> SmbForm {
        match smb::Spec::parse(url) {
            Some(s) => SmbForm { server: s.display(), user: s.user.clone(), version: s.version, encrypt: s.encrypt, error, retry, ..Default::default() },
            None => SmbForm { server: url.into(), error, retry, ..Default::default() },
        }
    }

    fn spec(&self) -> Option<smb::Spec> {
        let mut s = smb::Spec::parse(&self.server)?;
        if !self.user.trim().is_empty() {
            s.user = self.user.trim().to_string();
        }
        if self.version != Version::Auto {
            s.version = self.version;
        }
        s.encrypt = (self.encrypt || s.encrypt) && matches!(s.version, Version::Auto | Version::V3);
        Some(s)
    }
}

impl App {
    pub(super) fn smb_modal(&mut self, ctx: &egui::Context, mut f: Box<SmbForm>) -> Option<Modal> {
        let theme = self.theme.clone();
        let mut close = false;
        let mut go = false;
        let history: Vec<String> = self.settings.smb_history.clone();
        // side by side, or one under the other on a phone
        let narrow = ctx.content_rect().width().min(560.0 + 32.0) - 32.0 - 56.0 < 400.0;
        let height = 470.0 + if history.is_empty() { 0.0 } else { 36.0 } + if narrow { 64.0 } else { 0.0 } + if f.error.is_some() { 20.0 } else { 0.0 };
        let bd = modal_frame(ctx, self.modal_opened, Vec2::new(560.0, height), &theme, |ui, card| {
            title(ui, card, tr("Scan a Windows share"), Some(tr("Over SMB, the way Windows, NAS boxes and Samba share folders.")), &theme);
            close = close_button(ui, card, &theme);
            let x0 = card.left() + 28.0;
            let w = card.width() - 56.0;
            let mut y = card.top() + 96.0;
            let label = |ui: &egui::Ui, x: f32, y: f32, s: &str| {
                ui.painter().text(Pos2::new(x, y), Align2::LEFT_TOP, s, bold(11.0), theme.text_faint);
            };
            label(ui, x0, y, tr("SHARE"));
            y += 18.0;
            let r = widgets::text_input(ui, Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, 38.0)), Id::new("smb-server"), &mut f.server, "\\\\nas\\share\\folder", &theme);
            if r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            y += 46.0;
            // shares scanned before
            let mut x = x0;
            for u in history.iter().take(8) {
                let Some(s) = smb::Spec::parse(u) else { continue };
                let shown = s.display();
                let wch = ui.painter().layout_no_wrap(shown.clone(), font(12.0), Color32::WHITE).size().x + 20.0;
                if x + wch > x0 + w {
                    break;
                }
                let cr_ = Rect::from_min_size(Pos2::new(x, y), Vec2::new(wch, 26.0));
                let id = Id::new(("smb-chip", u));
                let resp = ui.interact(cr_, id, Sense::click());
                let hh = ui.ctx().animate_bool_with_time(id, resp.hovered() || f.server == shown, 0.1);
                ui.painter().rect_filled(cr_, cr(6.0), lerp_color(theme.surface_hi, with_alpha(theme.accent, 0.5), hh));
                ui.painter().text(cr_.center(), Align2::CENTER_CENTER, &shown, font(12.0), theme.text);
                if resp.clicked() {
                    *f = SmbForm { password: std::mem::take(&mut f.password), ..SmbForm::again(u, None, false) };
                }
                x += wch + 6.0;
            }
            if !history.is_empty() {
                y += 36.0;
            }
            // user and password
            let (fw, px, py) = if narrow { (w, x0, y + 64.0) } else { ((w - 12.0) / 2.0, x0 + (w + 12.0) / 2.0, y) };
            label(ui, x0, y, tr("USER"));
            widgets::text_input(ui, Rect::from_min_size(Pos2::new(x0, y + 18.0), Vec2::new(fw, 38.0)), Id::new("smb-user"), &mut f.user, tr("empty for a guest"), &theme);
            label(ui, px, py, tr("PASSWORD"));
            let pr = widgets::secret_input(ui, Rect::from_min_size(Pos2::new(px, py + 18.0), Vec2::new(fw, 38.0)), Id::new("smb-password"), &mut f.password, f.retry, &theme);
            if pr.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                go = true;
            }
            y = py + 64.0;
            // version chips
            label(ui, x0, y, tr("VERSION"));
            y += 18.0;
            let mut x = x0;
            for (v, name) in [(Version::Auto, tr("Newest")), (Version::V1, "SMB 1"), (Version::V2, "SMB 2"), (Version::V3, "SMB 3")] {
                let wch = ui.painter().layout_no_wrap(name.to_string(), font(12.5), Color32::WHITE).size().x + 26.0;
                let r = Rect::from_min_size(Pos2::new(x, y), Vec2::new(wch, 28.0));
                let id = Id::new(("smb-ver", name));
                let resp = ui.interact(r, id, Sense::click());
                let on = ui.ctx().animate_bool_with_time(id, f.version == v, 0.12);
                let hov = if resp.hovered() { 0.25 } else { 0.0 };
                ui.painter().rect_filled(r, cr(6.0), lerp_color(theme.surface_hi, with_alpha(theme.accent, 0.55), on.max(hov)));
                ui.painter().text(r.center(), Align2::CENTER_CENTER, name, font(12.5), theme.text);
                if resp.clicked() {
                    f.version = v;
                }
                x += wch + 6.0;
            }
            y += 40.0;
            let can_encrypt = matches!(f.version, Version::Auto | Version::V3);
            let mut enc = f.encrypt && can_encrypt;
            let er = Rect::from_min_size(Pos2::new(x0, y), Vec2::new(w, 22.0));
            if can_encrypt {
                widgets::checkbox(ui, er, Id::new("smb-encrypt"), &mut enc, tr("Require encryption (SMB 3)"), &theme);
                f.encrypt = enc;
            } else {
                ui.painter().text(Pos2::new(x0, er.center().y), Align2::LEFT_CENTER, tr("Encryption needs SMB 3"), font(13.0), theme.text_faint);
            }
            y += 34.0;
            if let Some(e) = &f.error {
                let msg = if f.retry { tr("That didn't work, please try again.").to_string() } else { e.clone() };
                y += wrapped(ui, Pos2::new(x0, y), w, &msg, font(12.0), theme.danger) + 6.0;
            }
            let note = if f.version == Version::V1 {
                tr("SMB 1 can't encrypt: anyone on the network can read what's sent. The password is kept in memory for this session only.")
            } else {
                tr("The password is kept in memory for this session only, never saved to disk.")
            };
            wrapped(ui, Pos2::new(x0, y), w, note, font(11.5), theme.text_faint);
            let ok = f.spec().is_some();
            let b = Rect::from_min_size(Pos2::new(card.right() - 150.0, card.bottom() - 60.0), Vec2::new(122.0, 36.0));
            go |= widgets::button_ex(ui, b, Id::new("smb-go"), tr("Connect"), Some(Icon::Bolt), BtnStyle::Primary, &theme, ok).clicked();
            let c = Rect::from_min_size(Pos2::new(card.right() - 260.0, card.bottom() - 60.0), Vec2::new(100.0, 36.0));
            close |= widgets::button(ui, c, Id::new("smb-cancel"), tr("Cancel"), None, BtnStyle::Ghost, &theme).clicked();
        });
        if go {
            if let Some(spec) = f.spec() {
                smb::set_password(&spec, std::mem::take(&mut f.password));
                let url = spec.url();
                let top = spec.at("").url();
                self.settings.smb_history.retain(|x| x != &top);
                self.settings.smb_history.insert(0, top);
                self.settings.smb_history.truncate(10);
                self.settings.save();
                self.modal = None;
                self.start_session(Target::Smb(url));
                return None;
            }
        }
        if close || bd { None } else { Some(Modal::Smb(f)) }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_builds_the_share() {
        let f = SmbForm { server: "\\\\nas:4451\\media\\films".into(), user: "WORK\\ann".into(), version: Version::V3, encrypt: true, ..Default::default() };
        let s = f.spec().unwrap();
        assert_eq!((s.host.as_str(), s.port, s.share.as_str(), s.path.as_str(), s.user.as_str()), ("nas", 4451, "media", "films", "WORK\\ann"));
        assert!(s.encrypt);
        // SMB 2 can't encrypt
        let f = SmbForm { version: Version::V2, ..f };
        assert!(!f.spec().unwrap().encrypt);
        // and the form comes back the same
        let again = SmbForm::again(&s.url(), None, false);
        assert_eq!((again.server.as_str(), again.user.as_str(), again.version, again.encrypt), ("\\\\nas:4451\\media\\films", "WORK\\ann", Version::V3, true));
        assert!(SmbForm { server: "nas".into(), ..Default::default() }.spec().is_none());
    }
}
