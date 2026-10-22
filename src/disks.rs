//! Lists the mounted volumes worth scanning.

use sysinfo::Disks;

#[derive(Clone, Debug, PartialEq)]
pub struct Volume {
    pub name: String,
    pub mount: String,
    pub total: u64,
    pub available: u64,
    pub removable: bool,
    pub fs: String,
    pub is_boot: bool,
    /// A mounted network share (SMB 1/2/3, NFS…).
    pub network: bool,
}

impl Volume {
    pub fn used(&self) -> u64 {
        self.total.saturating_sub(self.available)
    }
    pub fn used_frac(&self) -> f32 {
        if self.total == 0 { 0.0 } else { self.used() as f32 / self.total as f32 }
    }
}

const PSEUDO_FS: &[&str] = &[
    "tmpfs", "devtmpfs", "devfs", "proc", "sysfs", "overlay", "squashfs", "autofs", "cgroup", "cgroup2",
    "efivarfs", "fuse.portal", "nullfs", "ramfs", "securityfs", "tracefs", "debugfs", "binfmt_misc",
];

pub fn list() -> Vec<Volume> {
    if crate::scan::demo::enabled() {
        return crate::scan::demo::volumes()
            .into_iter()
            .map(|(name, mount, total, available, removable)| Volume {
                name: name.into(),
                mount: mount.into(),
                total,
                available,
                removable,
                fs: "apfs".into(),
                is_boot: mount == "/",
                network: false,
            })
            .collect();
    }
    let disks = Disks::new_with_refreshed_list();
    let mut out: Vec<Volume> = Vec::new();
    for d in disks.list() {
        let mount = d.mount_point().to_string_lossy().into_owned();
        let fs = d.file_system().to_string_lossy().into_owned();
        if PSEUDO_FS.contains(&fs.as_str()) || d.total_space() == 0 {
            continue;
        }
        if cfg!(target_os = "macos") && mount.starts_with("/System/Volumes/") {
            continue; // parts of the startup disk (Data, VM, Preboot…)
        }
        if cfg!(target_os = "linux")
            && (mount.starts_with("/snap/") || mount.starts_with("/boot") || mount.starts_with("/run/") || mount.starts_with("/var/lib/docker"))
        {
            continue;
        }
        if out.iter().any(|v| v.mount == mount) {
            continue;
        }
        let is_boot = mount == "/" || mount.eq_ignore_ascii_case("C:\\");
        let mut name = d.name().to_string_lossy().into_owned();
        if name.is_empty() || name.starts_with("/dev/") {
            name = if is_boot {
                "System".to_string()
            } else {
                std::path::Path::new(&mount).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or(mount.clone())
            };
        }
        if cfg!(windows) {
            name = if name.is_empty() { mount.clone() } else { format!("{name} ({})", mount.trim_end_matches('\\')) };
        }
        let (mut total, mut available) = (d.total_space(), d.available_space());
        // On macOS the real usage of the startup disk is on the APFS container.
        if let Some((t, a)) = crate::scan::platform::volume_space(d.mount_point()) {
            total = t;
            available = a.max(available.min(t));
        }
        out.push(Volume { name, mount, total, available, removable: d.is_removable(), fs, is_boot, network: false });
    }
    for (name, mount, fs) in network_shares() {
        if out.iter().any(|v| v.mount == mount) {
            continue;
        }
        let (total, available) = share_space(&mount).unwrap_or((0, 0));
        out.push(Volume { name, mount, total, available, removable: false, fs, is_boot: false, network: true });
    }
    out.sort_by_key(|v| (!v.is_boot, v.network, v.removable, v.mount.clone()));
    out
}

/// Network file systems shown as shares. The operating system's own client
/// talks to the server, so any SMB version it was mounted with (1, 2 or 3)
/// works; SquirrelDisk only reads the files like any other folder.
#[cfg(any(target_os = "linux", target_os = "macos"))]
const NET_FS: &[&str] = &["cifs", "smb3", "smbfs", "nfs", "nfs4", "afpfs", "webdav"];

/// Space of a share, without ever blocking: asking a server that went away
/// can hang (hard mounts), so the answer comes from a background thread and
/// the last known value is used meanwhile; one question per share at a time.
fn share_space(mount: &str) -> Option<(u64, u64)> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    // mount → (last known space, a question is under way)
    static KNOWN: Mutex<Option<HashMap<String, (Option<(u64, u64)>, bool)>>> = Mutex::new(None);
    let mut known = KNOWN.lock().unwrap();
    let entry = known.get_or_insert_with(HashMap::new).entry(mount.to_string()).or_insert((None, false));
    if !entry.1 {
        entry.1 = true;
        let mount = mount.to_string();
        std::thread::spawn(move || {
            let space = crate::scan::platform::volume_space(std::path::Path::new(&mount));
            if let Some(e) = KNOWN.lock().unwrap().as_mut().and_then(|k| k.get_mut(&mount)) {
                *e = (space.or(e.0), false);
            }
        });
    }
    entry.0
}

/// `//user@server/share`, `\\server\share` or `server:/export` → `server/share`.
#[cfg_attr(windows, allow(dead_code))]
fn share_name(source: &str) -> String {
    let s = source.trim_start_matches(['/', '\\']);
    // the user and any password, up to the last `@` before the share
    let host_end = s.find(['/', '\\']).unwrap_or(s.len());
    let s = s[..host_end].rfind('@').map_or(s, |at| &s[at + 1..]);
    s.replace('\\', "/").replace(":/", "/").replace("._smb._tcp.local", "").trim_end_matches('/').to_string()
}

/// macOS mount sources and gvfs folder names are URL-encoded: `My%20Photos`.
#[cfg(any(target_os = "linux", target_os = "macos"))]
fn percent_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16);
        match (b[i], b.get(i + 1).copied().and_then(hex), b.get(i + 2).copied().and_then(hex)) {
            (b'%', Some(h), Some(l)) => {
                out.push((h * 16 + l) as u8);
                i += 3;
            }
            (c, ..) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `/proc/self/mounts` escapes spaces and such as `\040`.
#[cfg(target_os = "linux")]
fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..i + 4].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push(u8::from_str_radix(&s[i + 1..i + 4], 8).unwrap_or(b'?'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// (name, mount point, file system) of the mounted network shares: kernel
/// mounts (`mount -t cifs`, fstab) and shares opened in the GNOME file
/// manager (gvfs, under /run/user/<uid>/gvfs).
#[cfg(target_os = "linux")]
fn network_shares() -> Vec<(String, String, String)> {
    let mut out: Vec<(String, String, String)> = std::fs::read_to_string("/proc/self/mounts")
        .unwrap_or_default()
        .lines()
        .filter_map(|l| {
            let mut f = l.split_whitespace();
            let (source, mount, fs) = (f.next()?, f.next()?, f.next()?);
            NET_FS.contains(&fs).then(|| (share_name(&unescape(source)), unescape(mount), fs.to_string()))
        })
        .collect();
    let gvfs = format!("/run/user/{}/gvfs", unsafe { libc::getuid() });
    for e in std::fs::read_dir(&gvfs).into_iter().flatten().flatten() {
        let dir = e.file_name().to_string_lossy().into_owned();
        // smb-share:server=nas,share=media,user=me
        let Some(args) = dir.strip_prefix("smb-share:") else { continue };
        let arg = |k: &str| args.split(',').find_map(|a| a.strip_prefix(k)?.strip_prefix('=')).unwrap_or("");
        out.push((percent_decode(&format!("{}/{}", arg("server"), arg("share"))), e.path().to_string_lossy().into_owned(), "smb (gvfs)".into()));
    }
    out
}

#[cfg(target_os = "macos")]
fn network_shares() -> Vec<(String, String, String)> {
    use std::ffi::CStr;
    let text = |b: &[libc::c_char]| unsafe { CStr::from_ptr(b.as_ptr()) }.to_string_lossy().into_owned();
    let mut mounts: *mut libc::statfs = std::ptr::null_mut();
    let n = unsafe { libc::getmntinfo(&mut mounts, libc::MNT_NOWAIT) };
    // SAFETY: getmntinfo returns `n` entries that stay valid for this thread's next call.
    let mounts = if n > 0 && !mounts.is_null() { unsafe { std::slice::from_raw_parts(mounts, n as usize) } } else { &[] };
    mounts
        .iter()
        .filter(|m| NET_FS.contains(&text(&m.f_fstypename).as_str()))
        .map(|m| (percent_decode(&share_name(&text(&m.f_mntfromname))), text(&m.f_mntonname), text(&m.f_fstypename)))
        .collect()
}

/// Mapped network drives (`net use Z: \\server\share`, "Map network drive").
#[cfg(windows)]
fn network_shares() -> Vec<(String, String, String)> {
    use windows_sys::Win32::NetworkManagement::WNet::WNetGetConnectionW;
    use windows_sys::Win32::Storage::FileSystem::{GetDriveTypeW, GetLogicalDrives};
    const DRIVE_REMOTE: u32 = 4;
    let drives = unsafe { GetLogicalDrives() };
    (0..26u8)
        .filter(|i| drives & (1 << i) != 0)
        .filter_map(|i| {
            let letter = format!("{}:", (b'A' + i) as char);
            let root: Vec<u16> = format!("{letter}\\").encode_utf16().chain([0]).collect();
            if unsafe { GetDriveTypeW(root.as_ptr()) } != DRIVE_REMOTE {
                return None;
            }
            let local: Vec<u16> = letter.encode_utf16().chain([0]).collect();
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            let remote = if unsafe { WNetGetConnectionW(local.as_ptr(), buf.as_mut_ptr(), &mut len) } == 0 {
                String::from_utf16_lossy(&buf[..buf.iter().position(|&c| c == 0).unwrap_or(0)])
            } else {
                String::new()
            };
            let name = if remote.is_empty() { letter.clone() } else { format!("{remote} ({letter})") };
            Some((name, format!("{letter}\\"), "SMB".into()))
        })
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
fn network_shares() -> Vec<(String, String, String)> {
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_names() {
        assert_eq!(share_name("//nas/media"), "nas/media");
        assert_eq!(share_name("//me@nas.local/Photos/"), "nas.local/Photos");
        assert_eq!(share_name(r"\\server\share"), "server/share");
        assert_eq!(share_name("nas:/export/home"), "nas/export/home");
        // no user or password on screen, even with an `@` in the password
        assert_eq!(share_name("//me:secret@nas/share"), "nas/share");
        assert_eq!(share_name("//me:p@ss@nas/share"), "nas/share");
        assert_eq!(share_name("//nas/share@2024"), "nas/share@2024");
        assert_eq!(share_name("//GUEST:@nas._smb._tcp.local/Media"), "nas/Media");
        assert_eq!(share_name(r"\\server\share\sub folder\"), "server/share/sub folder");
        assert_eq!(share_name("//nas"), "nas");
        assert_eq!(share_name(""), "");
    }

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    #[test]
    fn decodes_url_encoded_shares() {
        assert_eq!(percent_decode("nas/My%20Photos"), "nas/My Photos");
        assert_eq!(percent_decode("nas/K%C3%A4ytt%C3%A4j%C3%A4t"), "nas/Käyttäjät");
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("a%2"), "a%2");
        assert_eq!(percent_decode("a%zz"), "a%zz");
        assert_eq!(percent_decode("%FF"), "\u{FFFD}");
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn unescapes_mounts() {
        assert_eq!(unescape(r"/mnt/my\040share"), "/mnt/my share");
        assert_eq!(unescape(r"/mnt/plain"), "/mnt/plain");
        assert_eq!(unescape(r"/mnt/end\040"), "/mnt/end ");
        assert_eq!(unescape(r"/mnt/tab\011and\134slash"), "/mnt/tab\tand\\slash");
        assert_eq!(unescape(r"/mnt/short\04"), r"/mnt/short\04");
        assert_eq!(unescape(r"/mnt/K\303\244yt"), "/mnt/Käyt");
    }
}
