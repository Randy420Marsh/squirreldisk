//! Previews of pictures, videos and text files, inside the app. What a file
//! is comes from its first bytes, never its name: a program renamed
//! `photo.jpg` gets no preview. Pictures are decoded by the image crate's
//! own Rust code, under size limits; a video shows one frame, made by the
//! system (Android's media framework, Quick Look on the Mac, ffmpeg when it
//! is installed elsewhere); text is shown as text, only when it is text.

use eframe::egui;
use std::io::Read;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Image,
    Video,
    Text,
}

/// Names that say "text"; the bytes must agree (see [`looks_like_text`]).
const TEXT_EXT: &[&str] = &["txt", "text", "md", "log", "csv", "tsv", "json", "xml", "yaml", "yml", "toml", "ini", "cfg", "conf", "srt"];

/// The longest side of a picture as shown.
const MAX_SIDE: u32 = 1600;
/// How much of a text file is shown.
const TEXT_BYTES: usize = 64 * 1024;

/// What preview `path` can have, from its first bytes (and, for text, its
/// name too).
pub fn sniff(path: &Path) -> Option<Kind> {
    let mut head = Vec::with_capacity(8192);
    std::fs::File::open(path).ok()?.take(8192).read_to_end(&mut head).ok()?;
    kind_of(&head, path.extension().and_then(|e| e.to_str()).unwrap_or(""))
}

fn kind_of(head: &[u8], ext: &str) -> Option<Kind> {
    use image::ImageFormat as F;
    if let Ok(F::Png | F::Jpeg | F::Gif | F::WebP | F::Bmp) = image::guess_format(head) {
        return Some(Kind::Image);
    }
    if head.get(4..8) == Some(b"ftyp") {
        let brand = head.get(8..12)?;
        return match brand {
            // HEIC and AVIF photos: only the system can read them
            b"heic" | b"heix" | b"hevc" | b"heim" | b"heis" | b"mif1" | b"msf1" | b"avif" => {
                (cfg!(target_os = "android") || cfg!(target_os = "macos")).then_some(Kind::Image)
            }
            b"M4A " | b"M4B " | b"M4P " => None,
            _ => Some(Kind::Video),
        };
    }
    // Matroska / WebM, AVI
    if head.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) || (head.starts_with(b"RIFF") && head.get(8..12) == Some(b"AVI ")) {
        return Some(Kind::Video);
    }
    if TEXT_EXT.contains(&ext.to_ascii_lowercase().as_str()) && looks_like_text(head) {
        return Some(Kind::Text);
    }
    None
}

/// UTF-8 (perhaps cut off mid-letter at the end) with no control
/// characters but tabs, line breaks and form feeds.
fn looks_like_text(bytes: &[u8]) -> bool {
    let text = match std::str::from_utf8(bytes) {
        Ok(t) => t,
        Err(e) if e.error_len().is_none() => std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap_or(""),
        Err(_) => return false,
    };
    !text.is_empty() && !text.chars().any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\r' | '\x0c'))
}

enum Loaded {
    Image(egui::ColorImage),
    Text(String),
}

fn load(path: &Path, kind: Kind) -> Result<Loaded, String> {
    match kind {
        Kind::Image => picture(path).map(Loaded::Image),
        Kind::Video => video_frame(path).map(Loaded::Image).ok_or_else(|| crate::i18n::tr("No preview for this video").to_string()),
        Kind::Text => {
            let mut bytes = Vec::new();
            std::fs::File::open(path).and_then(|f| f.take(TEXT_BYTES as u64).read_to_end(&mut bytes)).map_err(|e| e.to_string())?;
            if !looks_like_text(&bytes) {
                return Err(crate::i18n::tr("This isn't plain text").into());
            }
            Ok(Loaded::Text(String::from_utf8_lossy(&bytes).replace("\r\n", "\n")))
        }
    }
}

/// A picture, made smaller to fit [`MAX_SIDE`].
pub fn picture(path: &Path) -> Result<egui::ColorImage, String> {
    #[cfg(any(target_os = "android", target_os = "macos"))]
    {
        let mut head = [0u8; 12];
        let _ = std::fs::File::open(path).and_then(|mut f| f.read(&mut head));
        if &head[4..8] == b"ftyp" {
            return system_picture(path).ok_or_else(|| crate::i18n::tr("No preview for this picture").to_string());
        }
    }
    let mut reader = image::ImageReader::open(path).map_err(|e| e.to_string())?.with_guessed_format().map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(20_000);
    limits.max_image_height = Some(20_000);
    limits.max_alloc = Some(512 << 20);
    reader.limits(limits);
    let img = reader.decode().map_err(|e| e.to_string())?;
    let img = if img.width() > MAX_SIDE || img.height() > MAX_SIDE { img.thumbnail(MAX_SIDE, MAX_SIDE) } else { img };
    let rgba = img.to_rgba8();
    Ok(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()))
}

#[cfg(target_os = "android")]
fn system_picture(path: &Path) -> Option<egui::ColorImage> {
    crate::android::decode_picture(path, MAX_SIDE)
}

#[cfg(target_os = "macos")]
fn system_picture(path: &Path) -> Option<egui::ColorImage> {
    super::dupes::quicklook(&path.to_string_lossy(), 0)
}

#[cfg(target_os = "android")]
pub fn video_frame(path: &Path) -> Option<egui::ColorImage> {
    crate::android::video_frame(path, MAX_SIDE)
}

#[cfg(target_os = "macos")]
pub fn video_frame(path: &Path) -> Option<egui::ColorImage> {
    super::dupes::quicklook(&path.to_string_lossy(), 0)
}

/// A frame a second in (or the first, for a shorter video), by ffmpeg.
#[cfg(not(any(target_os = "android", target_os = "macos")))]
pub fn video_frame(path: &Path) -> Option<egui::ColorImage> {
    for seek in ["1", "0"] {
        let mut c = std::process::Command::new("ffmpeg");
        c.args(["-nostdin", "-v", "error", "-ss", seek, "-i"]).arg(path);
        c.args(["-frames:v", "1", "-vf", &format!("scale='min({MAX_SIDE},iw)':-2"), "-f", "image2pipe", "-c:v", "png", "-"]);
        c.stdin(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        crate::scan::remote::hide_console(&mut c);
        let Ok(out) = c.output() else { return None };
        if let Ok(img) = image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Png) {
            let rgba = img.to_rgba8();
            return Some(egui::ColorImage::from_rgba_unmultiplied([rgba.width() as usize, rgba.height() as usize], rgba.as_raw()));
        }
    }
    None
}

pub enum Shown {
    Loading,
    Image(egui::TextureHandle),
    Text(String),
    Failed(String),
}

/// The file in the preview dialog, loaded in the background.
pub struct Preview {
    pub path: String,
    pub kind: Kind,
    pub scroll: f32,
    slot: Arc<Mutex<Option<Result<Loaded, String>>>>,
    pub shown: Shown,
}

impl Preview {
    pub fn start(ctx: &egui::Context, path: String, kind: Kind) -> Self {
        let slot = Arc::new(Mutex::new(None));
        let (s, p, ctx2) = (slot.clone(), path.clone(), ctx.clone());
        std::thread::spawn(move || {
            let r = load(Path::new(&p), kind);
            *s.lock().unwrap() = Some(r);
            ctx2.request_repaint();
        });
        Preview { path, kind, scroll: 0.0, slot, shown: Shown::Loading }
    }

    /// Picks up the loaded file once it is ready.
    pub fn poll(&mut self, ctx: &egui::Context) {
        if !matches!(self.shown, Shown::Loading) {
            return;
        }
        let Some(r) = self.slot.lock().unwrap().take() else { return };
        self.shown = match r {
            Ok(Loaded::Image(img)) => Shown::Image(ctx.load_texture("preview", img, egui::TextureOptions::LINEAR)),
            Ok(Loaded::Text(t)) => Shown::Text(t),
            Err(e) => Shown::Failed(e),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_file_is_comes_from_its_bytes() {
        let png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR";
        assert_eq!(kind_of(png, "png"), Some(Kind::Image));
        // a picture with the wrong name is still a picture
        assert_eq!(kind_of(png, "txt"), Some(Kind::Image));
        assert_eq!(kind_of(b"\xff\xd8\xff\xe0\0\x10JFIF", "jpg"), Some(Kind::Image));
        assert_eq!(kind_of(b"\0\0\0\x20ftypisom\0\0\x02\0", "mp4"), Some(Kind::Video));
        assert_eq!(kind_of(b"\0\0\0\x20ftypM4A \0\0\x02\0", "m4a"), None);
        assert_eq!(kind_of(b"\x1a\x45\xdf\xa3\x01\0\0\0", "webm"), Some(Kind::Video));
        assert_eq!(kind_of(b"RIFF\0\0\0\0AVI LIST", "avi"), Some(Kind::Video));
        // a program named like a picture or a text file gets nothing
        assert_eq!(kind_of(b"MZ\x90\0\x03\0\0\0\x04\0", "jpg"), None);
        assert_eq!(kind_of(b"\x7fELF\x02\x01\x01\0\0\0", "txt"), None);
        assert_eq!(kind_of(b"PK\x03\x04\x14\0", "txt"), None);
    }

    #[test]
    fn text_only_when_named_and_made_of_text() {
        assert_eq!(kind_of("Grüße\r\n\tline two\n".as_bytes(), "txt"), Some(Kind::Text));
        assert_eq!(kind_of(b"{\"a\": 1}", "JSON"), Some(Kind::Text));
        // text, but not named as text: could be a script
        assert_eq!(kind_of(b"#!/bin/sh\necho hi\n", "sh"), None);
        assert_eq!(kind_of(b"echo hi", ""), None);
        assert_eq!(kind_of(b"ab\0cd", "txt"), None);
        assert_eq!(kind_of(b"\x1b[31mred", "log"), None);
        assert_eq!(kind_of(b"\xff\xfe\xfd", "txt"), None);
        assert_eq!(kind_of(b"", "txt"), None);
        // cut off in the middle of a letter at the end of what was read
        assert_eq!(kind_of(&"aé".as_bytes()[..2], "txt"), Some(Kind::Text));
    }

    #[test]
    fn pictures_decode_smaller() {
        let dir = std::env::temp_dir().join(format!("sqd-preview-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("big.png");
        image::RgbaImage::from_pixel(3200, 800, image::Rgba([10, 20, 30, 255])).save(&p).unwrap();
        assert_eq!(sniff(&p), Some(Kind::Image));
        let img = picture(&p).unwrap();
        assert_eq!(img.size, [1600, 400]);
        let t = dir.join("notes.txt");
        std::fs::write(&t, "hello\n").unwrap();
        assert_eq!(sniff(&t), Some(Kind::Text));
        let fake = dir.join("fake.txt");
        std::fs::write(&fake, b"\x7fELF\x02\x01\x01\0").unwrap();
        assert_eq!(sniff(&fake), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
