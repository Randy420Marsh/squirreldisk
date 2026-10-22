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
}

const SCENES: &[Scene] = &[Scene::Home, Scene::Session, Scene::Delete, Scene::Settings, Scene::Ssh, Scene::Cloud, Scene::Dupes, Scene::DupesPreview];

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
    fn frame(&mut self, (w, h, ppp): (f32, f32, f32)) -> egui::FullOutput {
        self.time += 0.05;
        self.app.last_frame = Instant::now() - Duration::from_millis(50);
        let mut raw = egui::RawInput { screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(w, h))), time: Some(self.time), predicted_dt: 0.05, ..Default::default() };
        raw.viewports.entry(egui::ViewportId::ROOT).or_default().native_pixels_per_point = Some(ppp);
        let app = &mut self.app;
        self.ctx.run_ui(raw, |ui| app.frame_ui(ui))
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
