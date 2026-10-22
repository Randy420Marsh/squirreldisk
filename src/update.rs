//! Update notes and version helpers.
//!
//! This build makes no network requests: it never checks for, downloads or
//! installs updates. Get new versions by building from source or downloading a
//! release yourself.

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Release {
    pub version: String,
    /// The release's description on GitHub, as Markdown.
    pub notes: String,
    pub asset_url: Option<String>,
}

#[derive(Clone, Debug)]
pub enum State {
    Idle,
    Available(Release),
    Downloading,
    Ready(String),
}

pub struct Updater {
    pub state: Arc<Mutex<State>>,
    pub downloaded: Arc<AtomicU64>,
    pub total: Arc<AtomicU64>,
    /// The newer release last found, kept while it downloads and installs.
    pub latest: Arc<Mutex<Option<Release>>>,
}

/// A line of release notes, ready to show.
#[derive(Debug, PartialEq)]
pub enum Note {
    Heading(String),
    /// A list item: its opening words in bold, if any, and the rest.
    Bullet(String, String),
    Text(String),
}

/// Release notes as plain lines: what is new, without the Markdown marks and
/// without the sections on how to download and first open the app.
pub fn notes(markdown: &str) -> Vec<Note> {
    // `[text](url)` becomes text; emphasis and code marks go
    fn plain(s: &str) -> String {
        let mut out = String::new();
        let mut rest = s;
        while let Some(open) = rest.find('[') {
            let link = rest[open..].find("](").and_then(|mid| rest[open + mid..].find(')').map(|end| (open + mid, open + mid + end)));
            match link {
                Some((mid, end)) => {
                    out.push_str(&rest[..open]);
                    out.push_str(&rest[open + 1..mid]);
                    rest = &rest[end + 1..];
                }
                None => break,
            }
        }
        out.push_str(rest);
        // (the bundled fonts have no arrow)
        out.replace("**", "").replace("__", "").replace('`', "").replace('→', ">").trim().to_string()
    }
    let mut lines = Vec::new();
    for line in markdown.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with("```") || line.starts_with("![") || line.starts_with("<!--") {
            continue;
        }
        if let Some(h) = line.strip_prefix('#') {
            let h = plain(h.trim_start_matches('#'));
            if matches!(h.to_lowercase().as_str(), "downloads" | "download" | "first launch" | "install" | "installation") {
                break;
            }
            lines.push(Note::Heading(h));
        } else if let Some(item) = ["- ", "* ", "+ "].iter().find_map(|m| line.strip_prefix(m)) {
            let lead = item.strip_prefix("**").and_then(|r| r.split_once("**"));
            lines.push(match lead {
                Some((lead, rest)) => Note::Bullet(plain(lead), plain(rest)),
                None => Note::Bullet(String::new(), plain(item)),
            });
        } else if !line.to_lowercase().contains("full changelog") {
            lines.push(Note::Text(plain(line)));
        }
    }
    lines
}

/// When running from an AppImage, the path of the .AppImage file (set by the
/// AppImage runtime). The executable itself lives on a read-only mount, so
/// updates replace this file instead.
pub fn appimage_path() -> Option<std::path::PathBuf> {
    std::env::var_os("APPIMAGE").map(std::path::PathBuf::from).filter(|p| p.is_file())
}

impl Updater {
    pub fn new() -> Self {
        Updater {
            state: Arc::new(Mutex::new(State::Idle)),
            downloaded: Arc::new(AtomicU64::new(0)),
            total: Arc::new(AtomicU64::new(0)),
            latest: Arc::new(Mutex::new(None)),
        }
    }

    pub fn state(&self) -> State {
        self.state.lock().unwrap().clone()
    }

    /// Updates are disabled in this build: nothing is downloaded, so the
    /// (debug-only) offer is just dismissed.
    pub fn install(&self, _rel: Release, ctx: eframe::egui::Context) {
        *self.state.lock().unwrap() = State::Idle;
        ctx.request_repaint();
    }
}

/// Relaunches the (updated) executable and exits.
pub fn restart() {
    if let Some(appimage) = appimage_path() {
        let _ = std::process::Command::new(appimage).spawn();
        std::process::exit(0);
    }
    if let Ok(exe) = std::env::current_exe() {
        // Inside a macOS bundle, relaunch the .app so it keeps its Dock identity.
        let bundle = exe.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")).map(|p| p.to_path_buf());
        match bundle {
            Some(app) if cfg!(target_os = "macos") => {
                let _ = std::process::Command::new("open").arg("-n").arg(app).spawn();
            }
            _ => {
                let _ = std::process::Command::new(exe).spawn();
            }
        }
    }
    std::process::exit(0);
}

#[cfg(test)]
mod tests {
    #[test]
    fn release_notes_become_plain_lines() {
        use super::{notes, Note};
        let md = "## App 2.6\n\n- **Faster scans.** Now with `--fresh`, see [the docs](https://x.y/z) → here.\n* plain item\n\nA [link](https://a.b) and **bold**.\n**Full Changelog**: https://github.com/a/b/compare/1...2\n\n### Downloads\n- **macOS:** the dmg\n";
        assert_eq!(
            notes(md),
            [
                Note::Heading("App 2.6".into()),
                Note::Bullet("Faster scans.".into(), "Now with --fresh, see the docs > here.".into()),
                Note::Bullet(String::new(), "plain item".into()),
                Note::Text("A link and bold.".into()),
            ]
        );
        assert!(notes("").is_empty());
    }
}
