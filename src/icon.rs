//! The app icon: an oak with a spectrum of acorns in a gold-rimmed disc.
//! It is a painting, so the master is a 1024 px PNG with transparent corners
//! (`assets/icon/icon-1024.png`); every other icon file is scaled from it by
//! [`write_assets`]. The binary carries the 512 px copy for the window icon,
//! the home screen and the animated macOS Dock tile, where a scan's progress
//! runs round the gold rim.
//!
//! Below 64 px the painting is only a coloured blob, so small icons are a
//! drawn version instead ([`small`]): the same disc and rim with the oak as
//! a few bold shapes, its crown and acorns growing as the icon shrinks so
//! that they still read at 16 px. `--render-icons` also writes it as
//! `icon-small.svg`.
#![cfg_attr(not(target_os = "macos"), allow(dead_code))]

use tiny_skia::{Color, FilterQuality, LineCap, Paint, PathBuilder, Pixmap, PixmapPaint, Stroke, Transform};

/// The 512 px icon, written by [`write_assets`] from the master.
const ART: &[u8] = include_bytes!("../assets/icon/icon-512.png");

/// What the rim shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Ring {
    /// Default look: the plain gold rim.
    Full,
    /// Scan progress, 0.0 – 1.0, filling the rim clockwise from the top.
    Progress(f32),
    /// Progress unknown (folder scans): a lit arc running round the rim.
    Spinner(f32),
}

#[derive(Clone, Copy, Debug)]
pub struct Frame {
    pub ring: Ring,
    /// Sway of the disc in degrees, about its centre.
    pub wobble: f32,
    /// Vertical hop in canvas units of 100 (negative = up).
    pub hop: f32,
}

impl Frame {
    pub const IDLE: Frame = Frame { ring: Ring::Full, wobble: 0.0, hop: 0.0 };
}

/// The middle of the gold rim and its width, in canvas units of 100.
const RIM: (f32, f32) = (45.0, 3.2);
const LIT: [u8; 3] = [0x9c, 0xff, 0xc4];

fn art() -> &'static Pixmap {
    static ART_PM: std::sync::OnceLock<Pixmap> = std::sync::OnceLock::new();
    ART_PM.get_or_init(|| Pixmap::decode_png(ART).expect("embedded icon"))
}

/// `src` scaled to `size`: halved with a 2×2 box filter while it is at least
/// twice too big (a mipmap, so fine detail averages instead of aliasing),
/// then drawn bicubic for the rest.
fn scaled(src: &Pixmap, size: u32) -> Pixmap {
    let mut cur = src.clone();
    while cur.width() >= size * 2 {
        let (w, h) = (cur.width() / 2, cur.height() / 2);
        let mut half = Pixmap::new(w, h).expect("icon size");
        let s = cur.pixels();
        let sw = cur.width() as usize;
        for (i, out) in half.pixels_mut().iter_mut().enumerate() {
            let (x, y) = (i % w as usize * 2, i / w as usize * 2);
            let q = [s[y * sw + x], s[y * sw + x + 1], s[(y + 1) * sw + x], s[(y + 1) * sw + x + 1]];
            let avg = |f: fn(&tiny_skia::PremultipliedColorU8) -> u8| ((q.iter().map(|p| f(p) as u32).sum::<u32>() + 2) / 4) as u8;
            *out = tiny_skia::PremultipliedColorU8::from_rgba(avg(|p| p.red()), avg(|p| p.green()), avg(|p| p.blue()), avg(|p| p.alpha())).expect("premultiplied average");
        }
        cur = half;
    }
    if cur.width() == size {
        return cur;
    }
    let mut out = Pixmap::new(size, size).expect("icon size");
    let k = size as f32 / cur.width() as f32;
    out.draw_pixmap(0, 0, cur.as_ref(), &PixmapPaint { quality: FilterQuality::Bicubic, ..PixmapPaint::default() }, Transform::from_scale(k, k), None);
    out
}

/// The drawn icon is used below this size.
const SMALL_BELOW: u32 = 64;

/// A filled circle: centre, radius (canvas units of 100) and colour.
type Dot = (f32, f32, f32, [u8; 3]);

/// The drawn icon's shapes, painted in order, for an icon `size` px wide:
/// the smaller the icon, the thicker the rim and the bigger the crown and
/// acorns.
fn small_shapes(size: u32) -> Vec<Dot> {
    // 0 at 16 px, 1 at 64 px
    let g = ((size as f32 - 16.0) / 48.0).clamp(0.0, 1.0);
    let rim = 7.0 - 3.0 * g;
    let crown = 38.5 - 5.5 * g - rim / 2.0;
    let mut d: Vec<Dot> = vec![
        (50.0, 50.0, 50.0, [0x2a, 0x15, 0x4f]),
        (50.0, 50.0, 49.0 - 1.0 * g, [0xf0, 0xb0, 0x42]),
        (50.0, 50.0, 49.0 - rim, [0x3d, 0x22, 0x75]),
    ];
    // the crown: lobes round a middle, outlined in dark green, lit from the top left
    let lobes = |grow: f32, dx: f32, dy: f32, colour: [u8; 3], d: &mut Vec<Dot>| {
        let r = crown * 0.62;
        d.push((50.0 + dx, 50.0 + dy, r + grow, colour));
        for k in 0..8 {
            let a = (k as f32 + 0.5) * std::f32::consts::TAU / 8.0;
            d.push((50.0 + dx + crown * 0.6 * a.sin(), 50.0 + dy - crown * 0.6 * a.cos(), crown * 0.4 + grow, colour));
        }
    };
    lobes(2.2, 0.0, 0.0, [0x10, 0x5a, 0x3c], &mut d);
    lobes(0.0, 0.0, 0.0, [0x22, 0xa8, 0x55], &mut d);
    lobes(-crown * 0.2, -crown * 0.08, -crown * 0.1, [0x45, 0xcf, 0x6a], &mut d);
    // acorns: three big ones on the smallest icons, six from 32 px
    let acorns: &[(f32, f32, [u8; 3])] = if size < 32 {
        &[(-0.38, -0.3, [0xff, 0x4d, 0x5e]), (0.4, -0.12, [0xff, 0xd2, 0x3b]), (-0.05, 0.45, [0x3d, 0x9b, 0xff])]
    } else {
        &[
            (-0.42, -0.3, [0xff, 0x4d, 0x5e]),
            (0.2, -0.5, [0xff, 0x9a, 0x2e]),
            (0.5, 0.05, [0xff, 0xd2, 0x3b]),
            (0.15, 0.48, [0x3d, 0x9b, 0xff]),
            (-0.4, 0.32, [0xb4, 0x5c, 0xff]),
            (0.0, -0.02, [0x2e, 0xe0, 0xd8]),
        ]
    };
    let r = if size < 32 { 7.5 } else { 7.0 - 1.5 * g };
    for &(x, y, c) in acorns {
        let (x, y) = (50.0 + x * crown, 50.0 + y * crown);
        if size < 40 {
            d.push((x, y, r, c));
        } else {
            // the nut, then its brown cap over the top
            d.push((x, y, r + 1.2, [0x10, 0x3a, 0x2c]));
            d.push((x, y + r * 0.15, r, c));
            d.push((x, y - r * 0.45, r * 0.72, [0x7a, 0x45, 0x22]));
        }
    }
    d
}

/// The drawn icon at `size` px: painted four times as big and scaled down,
/// so its edges are smooth even at 16 px.
fn small(size: u32) -> Pixmap {
    let big = size * 4;
    let mut pm = Pixmap::new(big, big).expect("icon size");
    let k = big as f32 / 100.0;
    for (x, y, r, c) in small_shapes(size) {
        let mut paint = Paint::default();
        paint.set_color_rgba8(c[0], c[1], c[2], 255);
        paint.anti_alias = true;
        if let Some(p) = PathBuilder::from_circle(x, y, r) {
            pm.fill_path(&p, &paint, tiny_skia::FillRule::Winding, Transform::from_scale(k, k), None);
        }
    }
    scaled(&pm, size)
}

/// The drawn icon as an SVG, as it looks at `size` px.
fn small_svg(size: u32) -> String {
    let mut s = String::from("<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 100 100\">\n");
    for (x, y, r, c) in small_shapes(size) {
        s += &format!("  <circle cx=\"{x:.2}\" cy=\"{y:.2}\" r=\"{r:.2}\" fill=\"#{:02x}{:02x}{:02x}\"/>\n", c[0], c[1], c[2]);
    }
    s + "</svg>\n"
}

/// The icon without its frame at `size` px: the drawn one when small,
/// otherwise the painting from `master`.
fn disc(master: &Pixmap, size: u32) -> Pixmap {
    if size < SMALL_BELOW { small(size) } else { scaled(master, size) }
}

/// An arc of the rim from `start` to `end`, in turns clockwise from the top.
fn arc(start: f32, end: f32) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    let steps = ((end - start).abs() * 96.0).ceil().max(2.0) as usize;
    for i in 0..=steps {
        let a = (start + (end - start) * i as f32 / steps as f32) * std::f32::consts::TAU;
        let (x, y) = (50.0 + RIM.0 * a.sin(), 50.0 - RIM.0 * a.cos());
        if i == 0 { pb.move_to(x, y) } else { pb.line_to(x, y) }
    }
    pb.finish()
}

/// Renders the icon. `inset` shrinks it inside the canvas (macOS icons use
/// a 824/1024 body with transparent margins; window icons use the full size).
pub fn render(size: u32, frame: Frame, inset: bool) -> Pixmap {
    let body = if inset { (size as f32 * 824.0 / 1024.0).round() as u32 } else { size };
    let disc = disc(art(), body);
    let mut pm = Pixmap::new(size, size).expect("icon size");
    let off = (size - body) as f32 / 2.0;
    let c = size as f32 / 2.0;
    let unit = body as f32 / 100.0;
    let at = Transform::from_translate(off, off + frame.hop * unit).post_concat(Transform::from_rotate_at(frame.wobble, c, c));
    let quality = if frame.wobble == 0.0 { FilterQuality::Nearest } else { FilterQuality::Bilinear };
    pm.draw_pixmap(0, 0, disc.as_ref(), &PixmapPaint { quality, ..PixmapPaint::default() }, at, None);
    let lit = match frame.ring {
        Ring::Full => None,
        Ring::Progress(f) => Some((0.0, f.clamp(0.0, 1.0))),
        Ring::Spinner(t) => Some((t.fract(), t.fract() + 0.22)),
    };
    if let Some(p) = lit.filter(|(a, b)| b > a).and_then(|(a, b)| arc(a, b)) {
        let mut paint = Paint::default();
        paint.set_color(Color::from_rgba8(LIT[0], LIT[1], LIT[2], 235));
        paint.anti_alias = true;
        let stroke = Stroke { width: RIM.1, line_cap: LineCap::Round, ..Stroke::default() };
        pm.stroke_path(&p, &paint, &stroke, Transform::from_scale(unit, unit).post_concat(at), None);
    }
    pm
}

/// Straight (non-premultiplied) RGBA bytes, as window-icon APIs expect.
pub fn rgba(pm: &Pixmap) -> Vec<u8> {
    pm.pixels()
        .iter()
        .flat_map(|p| {
            let c = p.demultiply();
            [c.red(), c.green(), c.blue(), c.alpha()]
        })
        .collect()
}

pub fn png(size: u32, frame: Frame, inset: bool) -> Vec<u8> {
    render(size, frame, inset).encode_png().unwrap_or_default()
}

/// Animation frame for a scan in progress (`fraction` is `None` when unknown).
pub fn scanning_frame(fraction: Option<f32>, time: f32) -> Frame {
    Frame {
        ring: match fraction {
            Some(f) => Ring::Progress(f),
            None => Ring::Spinner(time * 0.6),
        },
        wobble: (time * 2.5).sin() * 2.5,
        hop: 0.0,
    }
}

/// Little hop when a scan completes (`t` seconds since completion, ~1.2 s long).
pub fn done_frame(t: f32) -> Frame {
    let k = (1.0 - t / 1.2).max(0.0);
    Frame { ring: Ring::Full, wobble: 0.0, hop: -((t * 9.0).sin().abs() * 3.0 * k) }
}

/// macOS `.icns`: PNG images under their type codes (read since macOS
/// 10.7), so it needs no `iconutil` and any platform can write it.
fn icns(master: &Pixmap) -> Vec<u8> {
    let entries: [(&[u8; 4], u32); 11] = [
        (b"icp4", 16),
        (b"icp5", 32),
        (b"ic11", 32),
        (b"icp6", 64),
        (b"ic12", 64),
        (b"ic07", 128),
        (b"ic08", 256),
        (b"ic13", 256),
        (b"ic09", 512),
        (b"ic14", 512),
        (b"ic10", 1024),
    ];
    let mut body = Vec::new();
    for (code, size) in entries {
        let img = inset(master, size).encode_png().unwrap_or_default();
        body.extend_from_slice(code);
        body.extend_from_slice(&(img.len() as u32 + 8).to_be_bytes());
        body.extend_from_slice(&img);
    }
    let mut out = b"icns".to_vec();
    out.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    out.extend_from_slice(&body);
    out
}

/// The master at `size` inside the macOS 824/1024 body.
fn inset(master: &Pixmap, size: u32) -> Pixmap {
    let body = (size as f32 * 824.0 / 1024.0).round() as u32;
    let mut pm = Pixmap::new(size, size).expect("icon size");
    let off = ((size - body) / 2) as i32;
    pm.draw_pixmap(off, off, disc(master, body).as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    pm
}

/// Writes every icon file into `dir` from its master `icon-1024.png`: the
/// smaller PNGs, a Windows `.ico` and a macOS `.icns`.
pub fn write_assets(dir: &std::path::Path) -> std::io::Result<()> {
    let bad = |e: &dyn std::fmt::Display| std::io::Error::new(std::io::ErrorKind::InvalidData, format!("icon-1024.png: {e}"));
    let master = Pixmap::decode_png(&std::fs::read(dir.join("icon-1024.png"))?).map_err(|e| bad(&e))?;
    if master.width() != 1024 || master.height() != 1024 {
        return Err(bad(&"must be 1024×1024"));
    }
    let png_at = |s: u32| disc(&master, s).encode_png().map_err(|e| bad(&e));
    for s in [16u32, 32, 48, 64, 128, 256, 512] {
        std::fs::write(dir.join(format!("icon-{s}.png")), png_at(s)?)?;
    }
    // .ico with embedded PNG images (supported since Windows Vista)
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let images = sizes.iter().map(|s| png_at(*s)).collect::<std::io::Result<Vec<_>>>()?;
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, img) in sizes.iter().zip(&images) {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend_from_slice(&[b, b, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(img.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in &images {
        ico.extend_from_slice(img);
    }
    std::fs::write(dir.join("SquirrelDisk.ico"), ico)?;
    std::fs::write(dir.join("SquirrelDisk.icns"), icns(&master))?;
    std::fs::write(dir.join("icon-small.svg"), small_svg(32))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_states() {
        for f in [Frame::IDLE, scanning_frame(Some(0.4), 1.0), scanning_frame(None, 2.0), done_frame(0.3)] {
            for inset in [true, false] {
                let pm = render(64, f, inset);
                assert_eq!(pm.width(), 64);
                // the disc is opaque, the corner outside it transparent
                assert_eq!(pm.pixel(32, 32).unwrap().alpha(), 255);
                assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
            }
        }
    }

    #[test]
    fn progress_lights_the_rim() {
        // the top of the rim lights up once progress starts, the bottom only past half
        let px = |f: Frame, y: u32| render(200, f, false).pixel(100, y).unwrap().demultiply();
        let (top, bottom) = (200 / 2 - 90, 200 / 2 + 90);
        assert_ne!(px(scanning_frame(Some(0.3), 0.0), top), px(Frame::IDLE, top));
        assert_eq!(px(scanning_frame(Some(0.3), 0.0), bottom), px(Frame::IDLE, bottom));
        assert_ne!(px(scanning_frame(Some(0.6), 0.0), bottom), px(Frame::IDLE, bottom));
    }

    #[test]
    fn scaling_keeps_the_corners_clear() {
        for s in [16, 32, 48, 184, 256] {
            let pm = scaled(art(), s);
            assert_eq!((pm.width(), pm.height()), (s, s));
            assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
            assert_eq!(pm.pixel(s / 2, s / 2).unwrap().alpha(), 255);
        }
    }

    #[test]
    fn small_icons_are_drawn() {
        for s in [16, 24, 32, 48] {
            let pm = disc(art(), s);
            assert_eq!(pm.pixel(0, 0).unwrap().alpha(), 0);
            assert_eq!(pm.pixel(s / 2, s / 2).unwrap().alpha(), 255);
        }
        assert!(small_svg(32).matches("<circle").count() > 30);
    }

    #[test]
    fn icns_holds_every_size() {
        let i = icns(art());
        assert_eq!(&i[..4], b"icns");
        assert_eq!(u32::from_be_bytes(i[4..8].try_into().unwrap()) as usize, i.len());
        assert!(i.windows(4).any(|w| w == b"ic10"));
    }
}
