//! rclone: locating the binary, installing it into our data folder, and
//! managing its accounts ("remotes") through `rclone config`.

use crate::scan::remote::hide_console;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::Mutex;

static BIN: Mutex<Option<PathBuf>> = Mutex::new(None);

fn exe_name() -> &'static str {
    if cfg!(windows) { "rclone.exe" } else { "rclone" }
}

/// Where SquirrelDisk keeps its own copy of rclone.
pub fn managed_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("SQD_RCLONE_HOME") {
        return Some(PathBuf::from(d));
    }
    crate::settings::config_dir().map(|d| d.join("bin"))
}

/// Folders searched besides `PATH`: apps started from the Finder or a desktop
/// launcher often get a minimal `PATH` without Homebrew, Scoop and friends.
fn extra_dirs() -> Vec<PathBuf> {
    let home = crate::settings::home_dir().unwrap_or_default();
    let mut v = Vec::new();
    #[cfg(windows)]
    {
        if let Some(l) = std::env::var_os("LOCALAPPDATA") {
            v.push(PathBuf::from(l).join("Microsoft\\WinGet\\Links"));
        }
        v.push(home.join("scoop\\shims"));
        v.push(PathBuf::from("C:\\ProgramData\\chocolatey\\bin"));
    }
    #[cfg(not(windows))]
    {
        for d in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin", "/snap/bin", "/opt/local/bin"] {
            v.push(PathBuf::from(d));
        }
        v.push(home.join(".local/bin"));
        v.push(home.join("bin"));
    }
    v
}

/// In portable mode rclone's accounts live in our folder too, unless the
/// user already points rclone somewhere with `RCLONE_CONFIG`.
fn config_file() -> Option<PathBuf> {
    if std::env::var_os("RCLONE_CONFIG").is_some() {
        return None;
    }
    crate::settings::portable_dir().map(|d| d.join("rclone.conf"))
}

fn find() -> Option<PathBuf> {
    let managed = || managed_dir().map(|d| d.join(exe_name())).filter(|f| f.is_file());
    // a portable install prefers the copy it carries along
    if crate::settings::portable_dir().is_some() {
        if let Some(f) = managed() {
            return Some(f);
        }
    }
    // SQD_NO_SYSTEM_RCLONE=1 pretends rclone isn't installed (testing the installer)
    if std::env::var_os("SQD_NO_SYSTEM_RCLONE").is_none() {
        let path = std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect::<Vec<_>>()).unwrap_or_default();
        for d in path.into_iter().chain(extra_dirs()) {
            let f = d.join(exe_name());
            if f.is_file() {
                return Some(f);
            }
        }
    }
    managed()
}

/// The rclone executable, if there is one.
pub fn bin() -> Option<PathBuf> {
    let mut b = BIN.lock().unwrap();
    if b.as_ref().is_none_or(|p| !p.is_file()) {
        *b = find();
    }
    b.clone()
}

/// Forget the cached location (after installing).
pub fn rescan() {
    *BIN.lock().unwrap() = None;
}

/// `rclone` ready to run (falls back to the bare name, which fails cleanly).
pub fn command() -> Command {
    let mut c = Command::new(bin().unwrap_or_else(|| PathBuf::from("rclone")));
    if let Some(f) = config_file() {
        c.env("RCLONE_CONFIG", f);
    }
    hide_console(&mut c);
    c
}

pub fn available() -> bool {
    bin().is_some() && command().arg("version").stdout(Stdio::null()).stderr(Stdio::null()).status().map(|s| s.success()).unwrap_or(false)
}

// ---------------------------------------------------------------------------
// accounts

#[derive(Clone, Debug, Default)]
pub struct Remote {
    pub name: String,
    pub kind: String,
    pub params: Vec<(String, String)>,
}

impl Remote {
    pub fn param(&self, k: &str) -> Option<&str> {
        self.params.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str())
    }
}

/// All configured remotes, from `rclone config dump`.
pub fn remotes() -> Vec<Remote> {
    let Ok(o) = command().args(["config", "dump"]).stdin(Stdio::null()).stderr(Stdio::null()).output() else {
        return Vec::new();
    };
    let Ok(serde_json::Value::Object(map)) = serde_json::from_slice::<serde_json::Value>(&o.stdout) else {
        return Vec::new();
    };
    let mut v: Vec<Remote> = map
        .into_iter()
        .map(|(name, cfg)| {
            let mut r = Remote { name, ..Default::default() };
            if let serde_json::Value::Object(cfg) = cfg {
                for (k, val) in cfg {
                    let s = match val {
                        serde_json::Value::String(s) => s,
                        other => other.to_string(),
                    };
                    if k == "type" {
                        r.kind = s;
                    } else {
                        r.params.push((k, s));
                    }
                }
            }
            r
        })
        .collect();
    v.sort_by_key(|r| r.name.to_lowercase());
    v
}

pub fn valid_name(n: &str) -> bool {
    !n.is_empty() && !n.starts_with('-') && !n.starts_with(' ') && n.chars().all(|c| c.is_alphanumeric() || "_-. @".contains(c)) && !n.ends_with(' ')
}

/// Opens a terminal running the interactive `rclone config`, for providers
/// and options the built-in forms don't cover.
pub fn open_config_terminal() -> Result<(), String> {
    let bin = bin().ok_or("rclone is not installed")?;
    let conf: Vec<String> = config_file().map(|f| vec!["--config".into(), f.to_string_lossy().into_owned()]).unwrap_or_default();
    #[cfg(target_os = "macos")]
    {
        // a .command file opens in Terminal without needing Automation permission
        let f = std::env::temp_dir().join("squirreldisk-rclone-config.command");
        let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
        let extra: String = conf.iter().map(|a| format!(" {}", q(a))).collect();
        std::fs::write(&f, format!("#!/bin/sh\nclear\n{} config{extra}\n", q(&bin.to_string_lossy()))).map_err(|e| e.to_string())?;
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o755));
        Command::new("open").arg(&f).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(windows)]
    {
        Command::new("cmd").args(["/c", "start", "rclone config"]).arg(&bin).arg("config").args(&conf).spawn().map(|_| ()).map_err(|e| e.to_string())
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let b = bin.to_string_lossy().to_string();
        let tries: [(&str, &[&str]); 7] = [
            ("x-terminal-emulator", &["-e"]),
            ("gnome-terminal", &["--"]),
            ("konsole", &["-e"]),
            ("xfce4-terminal", &["-x"]),
            ("kitty", &[]),
            ("alacritty", &["-e"]),
            ("xterm", &["-e"]),
        ];
        for (term, pre) in tries {
            if Command::new(term).args(pre).arg(&b).arg("config").args(&conf).spawn().is_ok() {
                return Ok(());
            }
        }
        Err("no terminal emulator found — run `rclone config` in a terminal".into())
    }
}

// ---------------------------------------------------------------------------
// background tasks (config create / update / reconnect)

#[cfg(feature = "gui")]
pub use tasks::*;

#[cfg(feature = "gui")]
mod tasks {
    use super::*;
    use std::io::{Read, Write};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    #[derive(Default)]
    pub struct Task {
        pub done: AtomicBool,
        pub error: Mutex<Option<String>>,
        /// Sign-in URL printed by rclone during an OAuth flow.
        pub link: Mutex<Option<String>>,
        cancel: AtomicBool,
        child: Mutex<Option<std::process::Child>>,
    }

    impl Task {
        pub fn finished(&self) -> Option<Result<(), String>> {
            if !self.done.load(Ordering::Acquire) {
                return None;
            }
            Some(match self.error.lock().unwrap().clone() {
                Some(e) => Err(e),
                None => Ok(()),
            })
        }
        pub fn cancel(&self) {
            self.cancel.store(true, Ordering::Release);
            if let Some(c) = self.child.lock().unwrap().as_mut() {
                let _ = c.kill();
            }
        }
        fn finish(&self, r: Result<(), String>, ctx: &eframe::egui::Context) {
            if let Err(e) = r {
                *self.error.lock().unwrap() = Some(e);
            }
            self.done.store(true, Ordering::Release);
            ctx.request_repaint();
        }
    }

    /// Runs `rclone <args>` in the background. OAuth providers open the
    /// browser and block until the user has signed in.
    pub fn run(args: Vec<String>, ctx: eframe::egui::Context) -> Arc<Task> {
        let task = Arc::new(Task::default());
        let t = task.clone();
        std::thread::spawn(move || {
            let r = run_blocking(&t, &args);
            t.finish(r, &ctx);
        });
        task
    }

    /// Several rclone commands in a row, stopping at the first failure.
    pub fn run_all(steps: Vec<Vec<String>>, ctx: eframe::egui::Context) -> Arc<Task> {
        let task = Arc::new(Task::default());
        let t = task.clone();
        std::thread::spawn(move || {
            let mut r = Ok(());
            for s in &steps {
                r = run_blocking(&t, s);
                if r.is_err() {
                    break;
                }
            }
            t.finish(r, &ctx);
        });
        task
    }

    fn run_blocking(t: &Task, args: &[String]) -> Result<(), String> {
        let mut c = command();
        c.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        let mut child = c.spawn().map_err(|e| format!("cannot run rclone: {e}"))?;
        // any question left gets its default answer
        if let Some(mut si) = child.stdin.take() {
            std::thread::spawn(move || {
                for _ in 0..32 {
                    if si.write_all(b"\n").is_err() {
                        break;
                    }
                }
            });
        }
        let mut out = child.stdout.take();
        let mut err = child.stderr.take();
        let h1 = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(o) = out.as_mut() {
                let _ = o.read_to_string(&mut s);
            }
            s
        });
        let link = Arc::new(Mutex::new(None::<String>));
        let l2 = link.clone();
        let h2 = std::thread::spawn(move || {
            let mut s = String::new();
            if let Some(e) = err.take() {
                use std::io::BufRead;
                for line in std::io::BufReader::new(e).lines().map_while(Result::ok) {
                    if let Some(i) = line.find("http://127.0.0.1:") {
                        let url = line[i..].split_whitespace().next().unwrap_or("").to_string();
                        *l2.lock().unwrap() = Some(url);
                    }
                    s.push_str(&line);
                    s.push('\n');
                }
            }
            s
        });
        *t.child.lock().unwrap() = Some(child);
        let status = loop {
            if let Some(l) = link.lock().unwrap().clone() {
                t.link.lock().unwrap().get_or_insert(l);
            }
            if let Some(c) = t.child.lock().unwrap().as_mut() {
                if let Ok(Some(st)) = c.try_wait() {
                    break st;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(80));
        };
        t.child.lock().unwrap().take();
        let so = h1.join().unwrap_or_default();
        let se = h2.join().unwrap_or_default();
        if t.cancel.load(Ordering::Acquire) {
            return Err("cancelled".into());
        }
        if status.success() {
            Ok(())
        } else {
            Err(last_error(&se, &so))
        }
    }

    /// The most useful line of rclone's output, without log noise.
    pub(crate) fn last_error(stderr: &str, stdout: &str) -> String {
        let clean = |l: &str| -> String {
            let mut l = l.trim();
            // "2026/09/28 12:14:28 "
            if l.len() > 20 && l.as_bytes()[4] == b'/' && l.as_bytes()[10] == b' ' {
                l = &l[20..];
            }
            for lv in ["CRITICAL: ", "ERROR : ", "NOTICE: ", "INFO  : "] {
                l = l.strip_prefix(lv).unwrap_or(l);
            }
            for cut in ["last error was: ", "api error "] {
                if let Some(i) = l.rfind(cut) {
                    l = &l[i + cut.len()..];
                }
            }
            l.trim_start_matches("Failed to ").to_string()
        };
        let lines: Vec<&str> = stderr.lines().chain(stdout.lines()).filter(|l| !l.trim().is_empty()).collect();
        let rank = |l: &str| {
            if l.contains("CRITICAL") {
                3
            } else if l.contains("ERROR") {
                2
            } else if l.contains("Failed") || l.contains("error") {
                1
            } else {
                0
            }
        };
        let best = lines.iter().enumerate().max_by_key(|(i, l)| (rank(l), *i)).map(|(_, l)| clean(l));
        let mut e = best.unwrap_or_else(|| "rclone failed".into());
        if e.chars().count() > 240 {
            e = e.chars().take(240).collect::<String>() + "…";
        }
        e
    }
}

#[cfg(all(test, feature = "gui"))]
mod tests {
    #[test]
    fn rclone_errors_are_readable() {
        let s3 = "2026/09/28 12:14:28 ERROR : error listing: operation error S3: ListBuckets, https response error StatusCode: 403, RequestID: 2, api error InvalidAccessKeyId: The AWS Access Key Id you provided does not exist in our records.\n2026/09/28 12:14:28 NOTICE: Failed to lsd with 2 errors: last error was: operation error S3: ListBuckets, StatusCode: 403, api error InvalidAccessKeyId: The AWS Access Key Id you provided does not exist in our records.";
        assert_eq!(super::last_error(s3, ""), "InvalidAccessKeyId: The AWS Access Key Id you provided does not exist in our records.");
        let sftp = "2026/09/28 12:14:28 NOTICE: t1: No host key validation is being performed.\n2026/09/28 12:14:28 CRITICAL: Failed to create file system for \"t1:\": NewFs: couldn't connect SSH: dial tcp: lookup example.invalid: no such host";
        assert_eq!(super::last_error(sftp, ""), "create file system for \"t1:\": NewFs: couldn't connect SSH: dial tcp: lookup example.invalid: no such host");
    }
}

#[cfg(test)]
mod portable_tests {
    use std::path::PathBuf;

    fn rclone_config(c: &std::process::Command) -> Option<PathBuf> {
        c.get_envs().find(|(k, _)| *k == "RCLONE_CONFIG").and_then(|(_, v)| v).map(PathBuf::from)
    }

    /// One test, since both cases change process-wide state.
    #[test]
    fn portable_config_dir() {
        let exe_dir = std::env::current_exe().unwrap().parent().unwrap().to_path_buf();
        let portable = exe_dir.join("portable");
        std::fs::create_dir_all(&portable).unwrap();
        assert_eq!(crate::settings::config_dir(), Some(portable.clone()));
        assert_eq!(super::managed_dir(), Some(portable.join("bin")));
        assert_eq!(rclone_config(&super::command()), Some(portable.join("rclone.conf")));

        let custom = std::env::temp_dir().join("sqd-portable-test");
        std::env::set_var("SQUIRRELDISK_CONFIG_DIR", &custom);
        assert_eq!(crate::settings::config_dir(), Some(custom.clone()));
        assert_eq!(rclone_config(&super::command()), Some(custom.join("rclone.conf")));
        std::env::remove_var("SQUIRRELDISK_CONFIG_DIR");

        std::fs::remove_dir(&portable).unwrap();
        assert_ne!(crate::settings::config_dir(), Some(portable));
        assert_eq!(rclone_config(&super::command()), None);
    }
}
