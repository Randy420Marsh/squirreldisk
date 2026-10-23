//! Starting the app on Android, where NativeActivity calls `android_main`
//! instead of `main`.

use eframe::egui;
use winit::platform::android::activity::AndroidApp;

/// The phone's shared storage (Downloads, DCIM, Music…): what a folder
/// dialog picks on Android (see ui::pick). Seeing all of it needs the "All
/// files access" permission.
pub const SHARED_STORAGE: &str = "/storage/emulated/0";

static APP: std::sync::OnceLock<AndroidApp> = std::sync::OnceLock::new();

/// The part of `screen` not under the status and navigation bars: Android
/// draws apps edge to edge, under both. Asked again when the window changes
/// size (it turned, or the bars moved).
pub fn safe_area(ctx: &egui::Context, screen: egui::Rect) -> egui::Rect {
    static INSETS: std::sync::Mutex<Option<(egui::Vec2, [f32; 4])>> = std::sync::Mutex::new(None);
    let Some(app) = APP.get() else { return screen };
    let mut cached = INSETS.lock().unwrap_or_else(|e| e.into_inner());
    let [left, top, right, bottom] = match *cached {
        Some((size, insets)) if size == screen.size() => insets,
        _ => {
            let ppp = ctx.pixels_per_point();
            let insets = bar_insets(app).map(|i| i.map(|px| px.max(0) as f32 / ppp)).unwrap_or_else(|e| {
                log(&format!("reading the system bar sizes: {e}"));
                [0.0; 4]
            });
            *cached = Some((screen.size(), insets));
            insets
        }
    };
    if left + right >= screen.width() || top + bottom >= screen.height() {
        return screen;
    }
    egui::Rect::from_min_max(egui::pos2(screen.left() + left, screen.top() + top), egui::pos2(screen.right() - right, screen.bottom() - bottom))
}

/// Runs `f` with the app's Java side, from any thread. Its Java objects are
/// let go after, and a Java exception becomes the error.
fn with_java<T>(app: &AndroidApp, f: impl FnOnce(&mut jni::JNIEnv, &jni::objects::JObject) -> jni::errors::Result<T>) -> jni::errors::Result<T> {
    let vm = unsafe { jni::JavaVM::from_raw(app.vm_as_ptr().cast()) }?;
    let activity = unsafe { jni::objects::JObject::from_raw(app.activity_as_ptr().cast()) };
    let mut env = vm.attach_current_thread_permanently()?;
    let r = env.with_local_frame(64, |env| f(env, &activity));
    if env.exception_check().unwrap_or(false) {
        let _ = env.exception_clear();
    }
    r
}

/// The pixels of an android.graphics.Bitmap.
fn bitmap_pixels(env: &mut jni::JNIEnv, bitmap: &jni::objects::JObject) -> jni::errors::Result<egui::ColorImage> {
    // ARGB_8888 is RGBA, premultiplied, in memory
    let config = env.get_static_field("android/graphics/Bitmap$Config", "ARGB_8888", "Landroid/graphics/Bitmap$Config;")?.l()?;
    let bitmap = env.call_method(bitmap, "copy", "(Landroid/graphics/Bitmap$Config;Z)Landroid/graphics/Bitmap;", &[(&config).into(), false.into()])?.l()?;
    let w = env.call_method(&bitmap, "getWidth", "()I", &[])?.i()?.max(0) as usize;
    let h = env.call_method(&bitmap, "getHeight", "()I", &[])?.i()?.max(0) as usize;
    let mut pixels = vec![0u8; w * h * 4];
    // SAFETY: `pixels` outlives the buffer, which is gone with the local frame
    let buffer = unsafe { env.new_direct_byte_buffer(pixels.as_mut_ptr(), pixels.len()) }?;
    env.call_method(&bitmap, "copyPixelsToBuffer", "(Ljava/nio/Buffer;)V", &[(&buffer).into()])?;
    env.call_method(&bitmap, "recycle", "()V", &[])?;
    Ok(egui::ColorImage::from_rgba_premultiplied([w, h], &pixels))
}

/// A picture the system reads (HEIC, AVIF), at most `max` pixels a side.
pub fn decode_picture(path: &std::path::Path, max: u32) -> Option<egui::ColorImage> {
    let app = APP.get()?;
    let r = with_java(app, |env, _| {
        let path = env.new_string(path.to_string_lossy())?;
        let opts = env.new_object("android/graphics/BitmapFactory$Options", "()V", &[])?;
        env.set_field(&opts, "inJustDecodeBounds", "Z", true.into())?;
        let decode = "(Ljava/lang/String;Landroid/graphics/BitmapFactory$Options;)Landroid/graphics/Bitmap;";
        env.call_static_method("android/graphics/BitmapFactory", "decodeFile", decode, &[(&path).into(), (&opts).into()])?;
        let w = env.get_field(&opts, "outWidth", "I")?.i()?;
        let h = env.get_field(&opts, "outHeight", "I")?.i()?;
        let mut sample = 1;
        while w.max(h) / sample > max as i32 {
            sample *= 2;
        }
        env.set_field(&opts, "inJustDecodeBounds", "Z", false.into())?;
        env.set_field(&opts, "inSampleSize", "I", sample.into())?;
        let bitmap = env.call_static_method("android/graphics/BitmapFactory", "decodeFile", decode, &[(&path).into(), (&opts).into()])?.l()?;
        if bitmap.is_null() {
            return Ok(None);
        }
        bitmap_pixels(env, &bitmap).map(Some)
    });
    r.unwrap_or_else(|e| {
        log(&format!("reading a picture: {e}"));
        None
    })
}

/// A frame from a video (a second in, or its cover), at most `max` pixels a
/// side.
pub fn video_frame(path: &std::path::Path, max: u32) -> Option<egui::ColorImage> {
    const OPTION_CLOSEST_SYNC: i32 = 2;
    let app = APP.get()?;
    let r = with_java(app, |env, _| {
        let retriever = env.new_object("android/media/MediaMetadataRetriever", "()V", &[])?;
        let path = env.new_string(path.to_string_lossy())?;
        let frame = env.call_method(&retriever, "setDataSource", "(Ljava/lang/String;)V", &[(&path).into()]).and_then(|_| {
            let size = (max as i32).into();
            env.call_method(&retriever, "getScaledFrameAtTime", "(JIII)Landroid/graphics/Bitmap;", &[1_000_000i64.into(), OPTION_CLOSEST_SYNC.into(), size, size])?.l()
        });
        if env.exception_check()? {
            env.exception_clear()?;
        }
        let _ = env.call_method(&retriever, "release", "()V", &[]);
        let frame = frame?;
        if frame.is_null() {
            return Ok(None);
        }
        bitmap_pixels(env, &frame).map(Some)
    });
    r.unwrap_or_else(|e| {
        log(&format!("reading a video frame: {e}"));
        None
    })
}

/// The system bars' and camera cutout's size on each side, in pixels: left,
/// top, right, bottom.
fn bar_insets(app: &AndroidApp) -> jni::errors::Result<[i32; 4]> {
    with_java(app, |env, activity| {
        let window = env.call_method(activity, "getWindow", "()Landroid/view/Window;", &[])?.l()?;
        let view = env.call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?.l()?;
        let insets = env.call_method(&view, "getRootWindowInsets", "()Landroid/view/WindowInsets;", &[])?.l()?;
        if insets.is_null() {
            return Ok([0; 4]);
        }
        let bars = env.call_static_method("android/view/WindowInsets$Type", "systemBars", "()I", &[])?.i()?;
        let cutout = env.call_static_method("android/view/WindowInsets$Type", "displayCutout", "()I", &[])?.i()?;
        let sides = env.call_method(&insets, "getInsets", "(I)Landroid/graphics/Insets;", &[(bars | cutout).into()])?.l()?;
        let mut side = |name| env.get_field(&sides, name, "I").and_then(|v| v.i());
        Ok([side("left")?, side("top")?, side("right")?, side("bottom")?])
    })
}

/// What Back does on the home screen: puts the app away, like the home
/// button, rather than closing it.
pub fn to_background() {
    let Some(app) = APP.get() else { return };
    if let Err(e) = with_java(app, |env, activity| env.call_method(activity, "moveTaskToBack", "(Z)Z", &[true.into()]).map(|_| ())) {
        log(&format!("going to the background: {e}"));
    }
}

/// Opens a file in the app Android picks for its type or, with `reveal`, its
/// folder in the Files app (the folder itself, for a folder).
pub fn open(path: &str, reveal: bool) {
    let Some(app) = APP.get() else { return };
    let p = std::path::Path::new(path);
    let result = if reveal || p.is_dir() {
        let dir = if p.is_dir() { p } else { p.parent().unwrap_or(p) };
        view_folder(app, dir)
    } else {
        view_file(app, p)
    };
    if let Err(e) = result {
        log(&format!("opening {path}: {e}"));
    }
}

/// The Files app's document id of a path in shared storage or on a card:
/// `primary:Download/a.zip`, `1A2B-3C4D:Music`.
fn document_id(path: &std::path::Path) -> Option<String> {
    let rest = path.strip_prefix("/storage").ok()?;
    let mut parts = rest.iter();
    let volume = parts.next()?.to_str()?;
    let volume = if volume == "emulated" {
        parts.next()?;
        "primary"
    } else {
        volume
    };
    let rel: std::path::PathBuf = parts.collect();
    Some(format!("{volume}:{}", rel.to_string_lossy()))
}

fn start(env: &mut jni::JNIEnv, activity: &jni::objects::JObject, intent: &jni::objects::JObject) -> jni::errors::Result<()> {
    // with no app for it, ActivityNotFoundException (cleared by with_java)
    env.call_method(activity, "startActivity", "(Landroid/content/Intent;)V", &[intent.into()]).map(|_| ())
}

fn view_intent<'a>(env: &mut jni::JNIEnv<'a>, uri: &str, mime: &str) -> jni::errors::Result<jni::objects::JObject<'a>> {
    const NEW_TASK_AND_READ: i32 = 0x1000_0000 | 0x0000_0001;
    let action = env.new_string("android.intent.action.VIEW")?;
    let intent = env.new_object("android/content/Intent", "(Ljava/lang/String;)V", &[(&action).into()])?;
    let uri = env.new_string(uri)?;
    let uri = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[(&uri).into()])?.l()?;
    let mime = env.new_string(mime)?;
    env.call_method(&intent, "setDataAndType", "(Landroid/net/Uri;Ljava/lang/String;)Landroid/content/Intent;", &[(&uri).into(), (&mime).into()])?;
    env.call_method(&intent, "addFlags", "(I)Landroid/content/Intent;", &[NEW_TASK_AND_READ.into()])?;
    Ok(intent)
}

fn view_folder(app: &AndroidApp, dir: &std::path::Path) -> jni::errors::Result<()> {
    let id = document_id(dir).unwrap_or_else(|| "primary:".into());
    with_java(app, |env, activity| {
        let id = env.new_string(id)?;
        let encoded = env.call_static_method("android/net/Uri", "encode", "(Ljava/lang/String;)Ljava/lang/String;", &[(&id).into()])?.l()?;
        let encoded = env.get_string((&encoded).into())?.to_string_lossy().into_owned();
        let uri = format!("content://com.android.externalstorage.documents/document/{encoded}");
        let intent = view_intent(env, &uri, "vnd.android.document/directory")?;
        start(env, activity, &intent)
    })
}

fn view_file(app: &AndroidApp, file: &std::path::Path) -> jni::errors::Result<()> {
    with_java(app, |env, activity| {
        // a file:// link to shared storage, which every app with storage
        // access can read; Android refuses to pass those on unless the app's
        // StrictMode policy allows it, and this app has no files of its own
        // to share another way
        let policy = env.new_object("android/os/StrictMode$VmPolicy$Builder", "()V", &[])?;
        let policy = env.call_method(&policy, "build", "()Landroid/os/StrictMode$VmPolicy;", &[])?.l()?;
        env.call_static_method("android/os/StrictMode", "setVmPolicy", "(Landroid/os/StrictMode$VmPolicy;)V", &[(&policy).into()])?;
        let ext = file.extension().and_then(|e| e.to_str()).unwrap_or("").to_lowercase();
        let map = env.call_static_method("android/webkit/MimeTypeMap", "getSingleton", "()Landroid/webkit/MimeTypeMap;", &[])?.l()?;
        let ext = env.new_string(ext)?;
        let mime = env.call_method(&map, "getMimeTypeFromExtension", "(Ljava/lang/String;)Ljava/lang/String;", &[(&ext).into()])?.l()?;
        let mime = if mime.is_null() { "*/*".to_string() } else { env.get_string((&mime).into())?.to_string_lossy().into_owned() };
        let uri = env.new_string(file.to_string_lossy())?;
        let f = env.new_object("java/io/File", "(Ljava/lang/String;)V", &[(&uri).into()])?;
        let uri = env.call_static_method("android/net/Uri", "fromFile", "(Ljava/io/File;)Landroid/net/Uri;", &[(&f).into()])?.l()?;
        let uri = env.call_method(&uri, "toString", "()Ljava/lang/String;", &[])?.l()?;
        let uri = env.get_string((&uri).into())?.to_string_lossy().into_owned();
        let intent = view_intent(env, &uri, &mime)?;
        start(env, activity, &intent)
    })
}

/// Without "All files access" the app sees only its own folders: if it is not
/// granted, opens the page in Settings where it is (Android can't ask for
/// it in a dialog).
fn ask_for_all_files(app: &AndroidApp) -> jni::errors::Result<()> {
    with_java(app, |env, activity| {
        if env.call_static_method("android/os/Environment", "isExternalStorageManager", "()Z", &[])?.z()? {
            return Ok(());
        }
        let action = env.new_string("android.settings.MANAGE_APP_ALL_FILES_ACCESS_PERMISSION")?;
        let package = env.call_method(activity, "getPackageName", "()Ljava/lang/String;", &[])?.l()?;
        let package = env.get_string((&package).into())?.to_string_lossy().into_owned();
        let uri = env.new_string(format!("package:{package}"))?;
        let uri = env.call_static_method("android/net/Uri", "parse", "(Ljava/lang/String;)Landroid/net/Uri;", &[(&uri).into()])?.l()?;
        let intent = env.new_object("android/content/Intent", "(Ljava/lang/String;Landroid/net/Uri;)V", &[(&action).into(), (&uri).into()])?;
        env.call_method(activity, "startActivity", "(Landroid/content/Intent;)V", &[(&intent).into()])?;
        Ok(())
    })
}

/// The headless build, which is the SSH client on Android (src/sshc.rs).
/// Named like a library so that Android unpacks it with the others, into
/// the one folder apps may run programs from.
const SSH_CLIENT: &str = "libsquirreldisk_ssh.so";

/// Where Android unpacked the app's libraries: the folder of this one.
fn library_dir() -> Option<std::path::PathBuf> {
    let maps = std::fs::read_to_string("/proc/self/maps").ok()?;
    let lib = maps.lines().filter_map(|l| l.split_whitespace().nth(5)).find(|p| p.ends_with("/libsquirreldisk_android.so"))?;
    std::path::Path::new(lib).parent().map(|d| d.to_path_buf())
}

#[link(name = "log")]
extern "C" {
    fn __android_log_write(prio: i32, tag: *const std::ffi::c_char, text: *const std::ffi::c_char) -> i32;
}

/// To logcat (`adb logcat -s SquirrelDisk`): stderr goes nowhere on Android.
pub fn log(text: &str) {
    const ERROR: i32 = 6;
    let text = std::ffi::CString::new(text.replace('\0', " ")).unwrap_or_default();
    // SAFETY: both strings are NUL-terminated and outlive the call.
    unsafe { __android_log_write(ERROR, c"SquirrelDisk".as_ptr(), text.as_ptr()) };
}

#[no_mangle]
fn android_main(app: AndroidApp) {
    std::panic::set_hook(Box::new(|info| log(&info.to_string())));
    // settings, caches and the duplicate finder's hashes go in the app's
    // private folder, the only one it can always write to
    if let Some(dir) = app.internal_data_path() {
        std::env::set_var("HOME", &dir);
        std::env::set_var("XDG_CONFIG_HOME", &dir);
    }
    if let Err(e) = ask_for_all_files(&app) {
        log(&format!("asking for All files access: {e}"));
    }
    if let Some(dir) = library_dir() {
        std::env::set_var("SQUIRRELDISK_SSH", dir.join(SSH_CLIENT));
    }
    let _ = APP.set(app.clone());
    let options = eframe::NativeOptions {
        android_app: Some(app.clone()),
        viewport: egui::ViewportBuilder::default().with_title("SquirrelDisk"),
        multisampling: 0,
        ..Default::default()
    };
    if let Err(e) = eframe::run_native("SquirrelDisk", options, Box::new(|cc| Ok(Box::new(crate::ui::app::App::new(cc))))) {
        log(&format!("SquirrelDisk failed to start: {e}"));
    }
}
