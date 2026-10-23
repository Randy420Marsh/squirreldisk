// The app is a GUI program on Windows; the headless build stays a console
// one (it becomes `squirreldisk.com`, see `console_shim`).
#![cfg_attr(all(windows, feature = "gui", not(debug_assertions)), windows_subsystem = "windows")]
#![cfg_attr(not(feature = "gui"), allow(dead_code))]

mod agent;
#[cfg(all(target_os = "android", feature = "gui"))]
mod android;
#[cfg(target_os = "android")]
mod sshc;
mod cli;
mod delete;
#[cfg(unix)]
mod ipc;
mod rclone;
mod safety;
mod scan;
mod settings;
mod smb;
mod tree;

#[cfg(feature = "gui")]
mod disks;

/// Where the source of this modified version is (AGPL-3.0 §6 and §13).
pub const SOURCE_URL: &str = "github.com/Randy420Marsh/squirreldisk";
mod dupes;
#[cfg(feature = "gui")]
mod i18n;
#[cfg(feature = "gui")]
mod icon;
#[cfg(feature = "gui")]
mod sound;
#[cfg(feature = "gui")]
#[cfg(feature = "gui")]
mod ui;
#[cfg(feature = "gui")]
mod update;
#[cfg(feature = "gui")]
mod watch;

/// The AppImage's AppRun may add its bundled libraries to LD_LIBRARY_PATH.
/// The loader has already read it by now, so put the original back for the
/// programs the app starts: a file manager or ssh must not load our copies.
fn restore_library_path() {
    if let Some(orig) = std::env::var_os("SQUIRRELDISK_LD_LIBRARY_PATH") {
        if orig.is_empty() {
            std::env::remove_var("LD_LIBRARY_PATH");
        } else {
            std::env::set_var("LD_LIBRARY_PATH", orig);
        }
        std::env::remove_var("SQUIRRELDISK_LD_LIBRARY_PATH");
    }
}

fn main() {
    restore_library_path();
    // Android: the app's SSH client (it gets the secret below too)
    #[cfg(target_os = "android")]
    if std::env::args().nth(1).is_some_and(|a| a == "--ssh") {
        let args: Vec<String> = std::env::args().skip(2).collect();
        std::process::exit(sshc::main(&args));
    }
    // Invoked by ssh as SSH_ASKPASS helper: answer the prompt and leave.
    if let Ok(secret) = std::env::var(scan::remote::ASKPASS_ENV) {
        println!("{secret}");
        return;
    }
    let args: Vec<String> = std::env::args().skip(1).collect();
    #[cfg(windows)]
    if let Some(code) = console_shim(&args) {
        std::process::exit(code);
    }
    if args.first().map(|a| a == "--agent").unwrap_or(false) {
        std::process::exit(agent::main(&args[1..]));
    }
    #[cfg(feature = "gui")]
    if args.first().map(|a| a == "--render-icons").unwrap_or(false) {
        let dir = std::path::PathBuf::from(args.get(1).map(|s| s.as_str()).unwrap_or("assets/icon"));
        match icon::write_assets(&dir) {
            Ok(()) => println!("icons written to {}", dir.display()),
            Err(e) => {
                eprintln!("could not write icons: {e}");
                std::process::exit(1);
            }
        }
        return;
    }
    if cli::handles(&args) {
        std::process::exit(cli::main(&args));
    }
    // Anything else opens the app, scanning the folder given if any (from a
    // terminal, the file manager's menu or `open --args`; macOS may add a
    // `-psn_…` argument, hence skipping flags).
    let folder = args.iter().find(|a| !a.starts_with('-')).map(|a| cli::absolute(std::path::Path::new(a)));
    if let Some(f) = folder.as_ref().filter(|f| !f.is_dir()) {
        eprintln!("squirreldisk: {}: not a folder\n\n{}", f.display(), cli::USAGE);
        std::process::exit(2);
    }
    #[cfg(feature = "gui")]
    {
        #[cfg(unix)]
        detach_from_terminal(&args);
        run_gui(folder);
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = folder;
        eprintln!("This is the headless SquirrelDisk build: it has no window.\n\n{}", cli::USAGE);
        std::process::exit(2);
    }
}

/// `squirreldisk.com`, installed next to `squirreldisk.exe` and on the PATH:
/// typed in a terminal, `squirreldisk` finds it first (PATHEXT lists .COM
/// before .EXE). The .exe is a GUI program, which terminals neither wait for
/// nor show the output of; this console build runs it with the terminal's
/// handles instead, waiting for it only for command-line work. All the logic
/// stays in the .exe, which the updater keeps current.
#[cfg(windows)]
fn console_shim(args: &[String]) -> Option<i32> {
    use std::process::{Command, Stdio};
    let me = std::env::current_exe().ok()?;
    if !me.extension()?.eq_ignore_ascii_case("com") {
        return None;
    }
    let exe = me.with_extension("exe");
    let mut cmd = Command::new(&exe);
    cmd.args(args);
    let fail = |e: std::io::Error| {
        eprintln!("squirreldisk: can't run {}: {e}", exe.display());
        1
    };
    Some(if cli::handles(args) || args.first().is_some_and(|a| a == "--agent") {
        cmd.status().map(|s| s.code().unwrap_or(1)).unwrap_or_else(fail)
    } else {
        // opening the app: leave the terminal free
        cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null());
        cmd.spawn().map(|_| 0).unwrap_or_else(fail)
    })
}

/// `squirreldisk [folder]` typed in a terminal: start the app on its own
/// and give the terminal back, as other GUI apps do.
#[cfg(all(unix, feature = "gui"))]
fn detach_from_terminal(args: &[String]) {
    use std::io::IsTerminal;
    use std::os::unix::process::CommandExt;
    const DETACHED: &str = "SQUIRRELDISK_DETACHED";
    if !std::io::stdin().is_terminal() || std::env::var_os(DETACHED).is_some() {
        return;
    }
    // an AppImage runs from a mount that goes away when this process ends,
    // so start the .AppImage file again rather than the executable inside
    let me = match update::appimage_path() {
        Some(appimage) => appimage,
        None => match std::env::current_exe().and_then(std::fs::canonicalize) {
            Ok(me) => me,
            Err(_) => return,
        },
    };
    // macOS: through Launch Services, so it starts as the app it is (Dock
    // icon, focus) even when typed via the /usr/local/bin link
    #[cfg(target_os = "macos")]
    if let Some(app) = me.ancestors().find(|p| p.extension().is_some_and(|e| e == "app")) {
        let status = std::process::Command::new("open").arg("-n").arg("-a").arg(app).arg("--args").args(args).status();
        if status.is_ok_and(|s| s.success()) {
            std::process::exit(0);
        }
    }
    let mut cmd = std::process::Command::new(me);
    cmd.args(args).env(DETACHED, "1").stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    // SAFETY: setsid and close_range are async-signal-safe, as required
    // between fork and exec.
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            // nothing open here goes along: an AppImage's runtime keeps its
            // mount until every copy of its pipe is closed, so it would stay
            // for as long as the new copy runs (an old kernel without
            // close_range just keeps them)
            #[cfg(target_os = "linux")]
            libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, libc::CLOSE_RANGE_CLOEXEC);
            Ok(())
        });
    }
    if cmd.spawn().is_ok() {
        std::process::exit(0);
    }
}

#[cfg(feature = "gui")]
fn run_gui(folder: Option<std::path::PathBuf>) {
    use eframe::egui;
    let icon = app_icon();
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("SquirrelDisk")
        // the Wayland app_id and X11 WM_CLASS: docks and task bars find the
        // icon and name through the desktop entry of the same name
        .with_app_id("squirreldisk")
        .with_inner_size([1180.0, 760.0])
        .with_min_inner_size([320.0, 320.0])
        .with_drag_and_drop(true)
        .with_icon(icon);
    // SQD_WINDOW=WxH (development: check layouts at a given size)
    if let Some((w, h)) = std::env::var("SQD_WINDOW").ok().and_then(|v| {
        let (w, h) = v.split_once('x')?;
        Some((w.parse::<f32>().ok()?, h.parse::<f32>().ok()?))
    }) {
        viewport = viewport.with_inner_size([w, h]);
    }
    if cfg!(target_os = "macos") {
        viewport = viewport.with_fullsize_content_view(true).with_titlebar_shown(false).with_title_shown(false);
    }
    // Linux: eframe centres on the desktop, not the monitor, so the app
    // places the window itself (see App::place_window)
    let options = eframe::NativeOptions { viewport, multisampling: 0, centered: cfg!(any(target_os = "macos", windows)), ..Default::default() };
    if let Err(e) = eframe::run_native("SquirrelDisk", options, Box::new(|cc| {
        let mut app = ui::app::App::new(cc);
        app.open_at_start = folder;
        Ok(Box::new(app))
    })) {
        eprintln!("SquirrelDisk failed to start: {e}");
        std::process::exit(1);
    }
}

/// Window icon, rendered from the same code as every other icon.
#[cfg(feature = "gui")]
fn app_icon() -> eframe::egui::IconData {
    let pm = icon::render(256, icon::Frame::IDLE, false);
    eframe::egui::IconData { rgba: icon::rgba(&pm), width: 256, height: 256 }
}
