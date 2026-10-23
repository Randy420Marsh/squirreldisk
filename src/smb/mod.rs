//! SMB file shares (Windows shares, Samba, most NAS boxes): the app's own
//! client for SMB 1 (v1.rs) and SMB 2 and 3 (v2.rs), signing in with NTLMv2
//! (ntlm.rs). It connects only where the user pointed it: no DFS referrals,
//! no multichannel, no browsing for servers.
//!
//! A share is named by a URL, `smb://user@host:port/share/path?v=3&enc=1`;
//! passwords stay in memory only.

pub mod ntlm;
pub mod v1;
pub mod v2;

use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Mutex;
use std::time::{Duration, Instant};

pub const PORT: u16 = 445;

/// Which SMB versions to try.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Version {
    /// The newest both ends speak, down to SMB 1.
    #[default]
    Auto,
    V1,
    V2,
    V3,
}

impl Version {
    fn tag(self) -> Option<&'static str> {
        match self {
            Version::Auto => None,
            Version::V1 => Some("1"),
            Version::V2 => Some("2"),
            Version::V3 => Some("3"),
        }
    }
}

/// A folder on a share.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Spec {
    pub host: String,
    pub port: u16,
    pub share: String,
    /// '/'-separated, from the top of the share; empty for the top.
    pub path: String,
    /// "user" or "DOMAIN\user"; empty to sign in anonymously.
    pub user: String,
    pub version: Version,
    /// Insist on SMB 3 encryption.
    pub encrypt: bool,
}

fn pct_encode(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '%' | '/' | '?' | '#' | '@' | '\\' | ':' => format!("%{:02X}", c as u8),
            c if c.is_control() => c.to_string().bytes().map(|b| format!("%{b:02X}")).collect(),
            c => c.to_string(),
        })
        .collect()
}

fn pct_decode(s: &str) -> String {
    let b = s.as_bytes();
    let mut v = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'%' && i + 3 <= b.len() {
            if let Ok(h) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                v.push(h);
                i += 3;
                continue;
            }
        }
        v.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&v).into_owned()
}

impl Spec {
    /// Reads `smb://[user@]host[:port]/share[/path][?v=1|2|3&enc=1]`, or
    /// what people type: `\\host\share\path`, `host/share`.
    pub fn parse(typed: &str) -> Option<Spec> {
        let t = typed.trim();
        let (t, query) = match t.split_once('?') {
            Some((a, q)) => (a, q),
            None => (t, ""),
        };
        let t = t.strip_prefix("smb://").or_else(|| t.strip_prefix("cifs://")).unwrap_or(t);
        let t = t.trim_start_matches(['\\', '/']).replace('\\', "/");
        let (auth, rest) = match t.split_once('/') {
            Some((a, r)) => (a.to_string(), r.to_string()),
            None => (t.clone(), String::new()),
        };
        let (user, hostport) = match auth.rsplit_once('@') {
            Some((u, h)) => (pct_decode(u), h.to_string()),
            None => (String::new(), auth),
        };
        let (host, port) = match hostport.rsplit_once(':') {
            Some((h, p)) if !h.contains(':') || h.ends_with(']') => (h.to_string(), p.parse().ok()?),
            _ => (hostport, PORT),
        };
        let host = host.trim_start_matches('[').trim_end_matches(']').to_string();
        let mut parts = rest.split('/').filter(|s| !s.is_empty()).map(pct_decode);
        let share = parts.next()?;
        let path = parts.collect::<Vec<_>>().join("/");
        if host.is_empty() || share.is_empty() {
            return None;
        }
        let mut s = Spec { host, port, share, path, user, version: Version::Auto, encrypt: false };
        for kv in query.split('&') {
            match kv.split_once('=') {
                Some(("v", "1")) => s.version = Version::V1,
                Some(("v", "2")) => s.version = Version::V2,
                Some(("v", "3")) => s.version = Version::V3,
                Some(("enc", "1")) => s.encrypt = true,
                _ => {}
            }
        }
        Some(s)
    }

    fn host_port(&self) -> String {
        let h = if self.host.contains(':') { format!("[{}]", self.host) } else { self.host.clone() };
        if self.port == PORT { h } else { format!("{h}:{}", self.port) }
    }

    pub fn url(&self) -> String {
        let mut u = String::from("smb://");
        if !self.user.is_empty() {
            u += &pct_encode(&self.user);
            u.push('@');
        }
        u += &self.host_port();
        for p in std::iter::once(self.share.as_str()).chain(self.path.split('/').filter(|s| !s.is_empty())) {
            u.push('/');
            u += &pct_encode(p);
        }
        let mut q = Vec::new();
        if let Some(v) = self.version.tag() {
            q.push(format!("v={v}"));
        }
        if self.encrypt {
            q.push("enc=1".into());
        }
        if !q.is_empty() {
            u.push('?');
            u += &q.join("&");
        }
        u
    }

    /// How a folder is shown: `\\host\share\path`.
    pub fn display(&self) -> String {
        let mut d = format!("\\\\{}\\{}", self.host_port(), self.share);
        if !self.path.is_empty() {
            d.push('\\');
            d += &self.path.replace('/', "\\");
        }
        d
    }

    /// Who signs in where (the password's key).
    pub fn account(&self) -> String {
        format!("{}@{}", self.user, self.host_port())
    }

    /// Which connections can be shared.
    fn conn_key(&self) -> String {
        format!("{}/{}?{:?}{}", self.account(), self.share.to_lowercase(), self.version, self.encrypt)
    }

    /// The same share, at another folder.
    pub fn at(&self, path: &str) -> Spec {
        Spec { path: path.trim_matches('/').to_string(), ..self.clone() }
    }
}

// ---------------------------------------------------------------------------
// errors

/// Starts every message about a refused sign-in, so the dialog can ask again.
pub const SIGN_IN_FAILED: &str = "Sign-in failed";

#[derive(Debug)]
pub enum Error {
    /// An NT status from the server.
    Status(u32),
    /// The connection is unusable.
    Gone(String),
    Other(String),
    /// The server only speaks SMB 1.
    Smb1Only,
}

impl Error {
    pub fn is_auth(&self) -> bool {
        matches!(self, Error::Status(0xC000_006D | 0xC000_006E | 0xC000_0064 | 0xC000_0070 | 0xC000_0071 | 0xC000_0072 | 0xC000_015B | 0xC000_0193 | 0xC000_0224 | 0xC000_0234))
    }
    /// The file or folder isn't there.
    #[cfg(test)]
    pub fn is_missing(&self) -> bool {
        matches!(self, Error::Status(0xC000_0034 | 0xC000_003A | 0xC000_000F))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        if self.is_auth() {
            return write!(f, "{SIGN_IN_FAILED}: the server refused this user name or password");
        }
        match self {
            Error::Status(s) => {
                let what = match *s {
                    0xC000_0022 => "access denied",
                    0xC000_0034 | 0xC000_000F => "no such file or folder",
                    0xC000_003A => "no such folder",
                    0xC000_0043 => "the file is in use",
                    0xC000_00CC => "no such share on this server",
                    0xC000_0101 => "the folder isn't empty",
                    0xC000_0121 => "it can't be deleted",
                    0xC000_00BA => "it's a folder",
                    0xC000_0103 => "it's not a folder",
                    0xC000_00BB => "the server doesn't support this",
                    0xC000_0203 | 0xC000_035C => "the server ended the session",
                    0xC000_A100 => "the server wants signing, which needs a signed-in user",
                    _ => return write!(f, "the server said {s:#010x}"),
                };
                f.write_str(what)
            }
            Error::Gone(e) => write!(f, "connection lost: {e}"),
            Error::Other(e) => f.write_str(e),
            Error::Smb1Only => f.write_str("the server only speaks SMB 1"),
        }
    }
}

pub fn is_auth_error(e: &str) -> bool {
    e.starts_with(SIGN_IN_FAILED)
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Gone(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// the connection

/// Direct TCP: each message after a 4-byte length.
pub struct Wire {
    s: TcpStream,
}

impl Wire {
    pub fn open(host: &str, port: u16) -> Result<Wire, Error> {
        let addrs = (host, port).to_socket_addrs().map_err(|e| Error::Other(format!("can't find {host}: {e}")))?;
        let mut last = None;
        for a in addrs {
            match TcpStream::connect_timeout(&a, Duration::from_secs(10)) {
                Ok(s) => {
                    s.set_read_timeout(Some(Duration::from_secs(60)))?;
                    s.set_write_timeout(Some(Duration::from_secs(60)))?;
                    let _ = s.set_nodelay(true);
                    return Ok(Wire { s });
                }
                Err(e) => last = Some(e),
            }
        }
        Err(Error::Other(match last {
            Some(e) => format!("can't connect to {host}:{port}: {e}"),
            None => format!("can't find {host}"),
        }))
    }

    pub fn send(&mut self, msg: &[u8]) -> Result<(), Error> {
        let n = msg.len();
        let mut out = Vec::with_capacity(4 + n);
        out.extend([0, (n >> 16) as u8, (n >> 8) as u8, n as u8]);
        out.extend_from_slice(msg);
        Ok(self.s.write_all(&out)?)
    }

    pub fn recv(&mut self) -> Result<Vec<u8>, Error> {
        loop {
            let mut h = [0u8; 4];
            self.s.read_exact(&mut h)?;
            let n = (h[1] as usize) << 16 | (h[2] as usize) << 8 | h[3] as usize;
            let mut m = vec![0u8; n];
            self.s.read_exact(&mut m)?;
            // 0x85: a NetBIOS keep-alive
            if h[0] == 0 {
                return Ok(m);
            }
        }
    }
}

/// A file handle on the server.
pub type Fid = [u8; 16];

/// A folder entry.
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: String,
    pub dir: bool,
    pub size: u64,
    /// Seconds since 1970.
    pub mtime: i64,
    /// A link or junction (not followed, as in a local scan).
    pub link: bool,
}

/// FILE_DIRECTORY_INFORMATION records, as both SMB 1 and SMB 2 list folders.
pub(crate) fn dir_entries(buf: &[u8]) -> Vec<Entry> {
    let u32_at = |at: usize| buf.get(at..at + 4).map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()));
    let u64_at = |at: usize| buf.get(at..at + 8).map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()));
    let mut out = Vec::new();
    let mut at = 0usize;
    loop {
        let next = u32_at(at) as usize;
        let len = u32_at(at + 60) as usize;
        let Some(name) = buf.get(at + 64..at + 64 + len) else { break };
        let name = String::from_utf16_lossy(&name.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>());
        let attrs = u32_at(at + 56);
        if name != "." && name != ".." {
            let ft = u64_at(at + 24);
            out.push(Entry {
                name,
                dir: attrs & 0x10 != 0,
                size: u64_at(at + 40),
                mtime: (ft / 10_000_000) as i64 - 11_644_473_600,
                link: attrs & 0x400 != 0,
            });
        }
        if next == 0 {
            break;
        }
        at += next;
    }
    out
}

pub enum Conn {
    V1(v1::Conn),
    V2(v2::Conn),
}

impl Conn {
    pub fn connect(spec: &Spec, password: &str) -> Result<Conn, Error> {
        let creds = ntlm::Creds::new(&spec.user, password);
        if spec.encrypt && matches!(spec.version, Version::V1 | Version::V2) {
            return Err(Error::Other(match spec.version {
                Version::V1 => "SMB 1 has no encryption; choose SMB 3 to encrypt".into(),
                _ => "SMB 2 has no encryption; choose SMB 3 to encrypt".into(),
            }));
        }
        let v1 = || -> Result<Conn, Error> { Ok(Conn::V1(v1::Conn::connect(Wire::open(&spec.host, spec.port)?, &spec.host, &spec.share, &creds)?)) };
        use v2::{SMB202, SMB210, SMB300, SMB302, SMB311};
        let dialects: &[u16] = match (spec.version, spec.encrypt) {
            (Version::V1, _) => return v1(),
            (Version::V2, _) => &[SMB202, SMB210],
            (Version::V3, _) | (Version::Auto, true) => &[SMB300, SMB302, SMB311],
            (Version::Auto, false) => &[SMB202, SMB210, SMB300, SMB302, SMB311],
        };
        match v2::Conn::connect(Wire::open(&spec.host, spec.port)?, &spec.host, &spec.share, &creds, dialects, spec.encrypt) {
            Ok(c) => Ok(Conn::V2(c)),
            Err(Error::Smb1Only) if spec.version == Version::Auto && !spec.encrypt => v1(),
            // an SMB 1-only server may just hang up on an SMB 2 hello
            Err(Error::Gone(e)) if spec.version == Version::Auto && !spec.encrypt => v1().map_err(|e1| match e1 {
                Error::Gone(_) => Error::Gone(e),
                e1 => e1,
            }),
            Err(Error::Smb1Only) if spec.encrypt => Err(Error::Other("the server only speaks SMB 1, which has no encryption".into())),
            Err(e) => Err(e),
        }
    }

    /// "SMB 3.1.1, encrypted"
    pub fn describe(&self) -> String {
        match self {
            Conn::V1(c) => if c.signing() { "SMB 1, signed".into() } else { "SMB 1".into() },
            Conn::V2(c) => {
                let name = v2::dialect_name(c.dialect);
                match c.protection() {
                    Some(p) => format!("{name}, {p}"),
                    None => name.into(),
                }
            }
        }
    }

    pub fn list(&mut self, path: &str) -> Result<Vec<Entry>, Error> {
        match self {
            Conn::V1(c) => c.list(path),
            Conn::V2(c) => c.list(path),
        }
    }

    pub fn open(&mut self, path: &str) -> Result<(Fid, u64), Error> {
        match self {
            Conn::V1(c) => c.open(path),
            Conn::V2(c) => c.open(path),
        }
    }

    pub fn read(&mut self, fid: &Fid, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        match self {
            Conn::V1(c) => c.read(fid, offset, buf),
            Conn::V2(c) => c.read(fid, offset, buf),
        }
    }

    pub fn close(&mut self, fid: &Fid) -> Result<(), Error> {
        match self {
            Conn::V1(c) => c.close(fid),
            Conn::V2(c) => c.close(fid),
        }
    }

    /// Deletes a file, or an empty folder.
    pub fn delete(&mut self, path: &str, dir: bool) -> Result<(), Error> {
        match self {
            Conn::V1(c) => c.delete(path, dir),
            Conn::V2(c) => c.delete(path, dir),
        }
    }

    /// Deletes a file, or a folder and everything in it.
    pub fn delete_all(&mut self, path: &str, dir: bool) -> Result<(), Error> {
        if dir {
            for e in self.list(path)? {
                let p = if path.is_empty() { e.name.clone() } else { format!("{path}/{}", e.name) };
                // a link is deleted itself, never what it points to
                self.delete_all(&p, e.dir && !e.link)?;
            }
        }
        self.delete(path, dir)
    }
}

// ---------------------------------------------------------------------------
// passwords and shared connections

fn passwords() -> &'static Mutex<std::collections::HashMap<String, String>> {
    static P: std::sync::OnceLock<Mutex<std::collections::HashMap<String, String>>> = std::sync::OnceLock::new();
    P.get_or_init(Default::default)
}

/// Remembers a password for this run of the app (never saved).
pub fn set_password(spec: &Spec, password: String) {
    passwords().lock().unwrap().insert(spec.account(), password);
    // connections signed in with the old one are no use
    pool().lock().unwrap().retain(|i| !i.key.starts_with(&spec.account()));
}

fn password(spec: &Spec) -> String {
    passwords().lock().unwrap().get(&spec.account()).cloned().unwrap_or_default()
}

struct Idle {
    key: String,
    conn: Conn,
    since: Instant,
}

fn pool() -> &'static Mutex<Vec<Idle>> {
    static P: std::sync::OnceLock<Mutex<Vec<Idle>>> = std::sync::OnceLock::new();
    P.get_or_init(Default::default)
}

/// A connection borrowed from the pool; it goes back when dropped, unless
/// it broke.
pub struct Lease {
    key: String,
    conn: Option<Conn>,
    reused: bool,
}

impl std::ops::Deref for Lease {
    type Target = Conn;
    fn deref(&self) -> &Conn {
        self.conn.as_ref().unwrap()
    }
}

impl std::ops::DerefMut for Lease {
    fn deref_mut(&mut self) -> &mut Conn {
        self.conn.as_mut().unwrap()
    }
}

impl Drop for Lease {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let mut p = pool().lock().unwrap();
            if p.iter().filter(|i| i.key == self.key).count() < 8 {
                p.push(Idle { key: std::mem::take(&mut self.key), conn, since: Instant::now() });
            }
        }
    }
}

/// A connection to the share: an idle one, or a new one.
pub fn lease(spec: &Spec) -> Result<Lease, Error> {
    let key = spec.conn_key();
    {
        let mut p = pool().lock().unwrap();
        // servers drop idle sessions; don't hand out stale ones
        p.retain(|i| i.since.elapsed() < Duration::from_secs(120));
        if let Some(at) = p.iter().rposition(|i| i.key == key) {
            let conn = p.remove(at).conn;
            return Ok(Lease { key, conn: Some(conn), reused: true });
        }
    }
    let conn = Conn::connect(spec, &password(spec))?;
    Ok(Lease { key, conn: Some(conn), reused: false })
}

/// Runs `f` on a connection to the share; a pooled connection that turns
/// out dead is replaced once.
pub fn with<T>(spec: &Spec, mut f: impl FnMut(&mut Conn) -> Result<T, Error>) -> Result<T, Error> {
    let mut l = lease(spec)?;
    match f(&mut l) {
        Err(Error::Gone(_)) if l.reused => {
            l.conn = None;
            let mut l = lease(spec)?;
            let r = f(&mut l);
            if matches!(r, Err(Error::Gone(_))) {
                l.conn = None;
            }
            r
        }
        Err(e @ Error::Gone(_)) => {
            l.conn = None;
            Err(e)
        }
        r => r,
    }
}

/// Signs in to check the share is there (and how it's protected).
pub fn check(spec: &Spec) -> Result<String, Error> {
    with(spec, |c| {
        c.list(&spec.path)?;
        Ok(c.describe())
    })
}

/// A file on a share, open for reading.
pub struct File {
    lease: Lease,
    fid: Fid,
    size: u64,
}

impl File {
    pub fn open(spec: &Spec, path: &str) -> Result<File, Error> {
        let mut lease = lease(spec)?;
        let opened = lease.open(path);
        let opened = match opened {
            Err(Error::Gone(_)) if lease.reused => {
                lease.conn = None;
                lease = self::lease(spec)?;
                lease.open(path)
            }
            r => r,
        };
        match opened {
            Ok((fid, size)) => Ok(File { lease, fid, size }),
            Err(e) => {
                if matches!(e, Error::Gone(_)) {
                    lease.conn = None;
                }
                Err(e)
            }
        }
    }

    pub fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, Error> {
        let r = self.lease.read(&self.fid, offset, buf);
        if matches!(r, Err(Error::Gone(_))) {
            self.lease.conn = None;
        }
        r
    }

    /// Reads it all into `out` (a backup copy).
    pub fn copy_to(&mut self, out: &mut impl Write, cancel: &dyn Fn() -> bool) -> Result<u64, String> {
        let mut buf = vec![0u8; 65536];
        let mut at = 0u64;
        loop {
            if cancel() {
                return Err("cancelled".into());
            }
            let n = self.read_at(&mut buf, at).map_err(|e| e.to_string())?;
            if n == 0 {
                return Ok(at);
            }
            out.write_all(&buf[..n]).map_err(|e| e.to_string())?;
            at += n as u64;
        }
    }
}

impl Drop for File {
    fn drop(&mut self) {
        if self.lease.conn.is_some() && self.lease.close(&self.fid).is_err() {
            self.lease.conn = None;
        }
    }
}

impl crate::dupes::hash::ReadAt for File {
    fn len(&mut self) -> Result<u64, String> {
        Ok(self.size)
    }
    fn read_at(&mut self, buf: &mut [u8], offset: u64) -> Result<usize, String> {
        File::read_at(self, buf, offset).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls() {
        let s = Spec::parse("smb://WORK%5Cann@nas:4451/media/photos/2024?v=3&enc=1").unwrap();
        assert_eq!((s.user.as_str(), s.host.as_str(), s.port), ("WORK\\ann", "nas", 4451));
        assert_eq!((s.share.as_str(), s.path.as_str()), ("media", "photos/2024"));
        assert_eq!((s.version, s.encrypt), (Version::V3, true));
        assert_eq!(Spec::parse(&s.url()).unwrap(), s);
        assert_eq!(s.display(), "\\\\nas:4451\\media\\photos\\2024");
        let t = Spec::parse("\\\\server\\share\\with space\\ä").unwrap();
        assert_eq!((t.host.as_str(), t.port, t.share.as_str(), t.path.as_str()), ("server", 445, "share", "with space/ä"));
        assert_eq!(Spec::parse(&t.url()).unwrap(), t);
        assert_eq!(t.url(), "smb://server/share/with space/ä");
        let u = Spec::parse("a@b@host/s%2Fx").unwrap();
        assert_eq!((u.user.as_str(), u.share.as_str()), ("a@b", "s/x"));
        assert_eq!(Spec::parse(&u.url()).unwrap(), u);
        assert!(Spec::parse("host").is_none());
        assert!(Spec::parse("smb:///share").is_none());
        let v6 = Spec::parse("smb://[::1]:4452/s").unwrap();
        assert_eq!((v6.host.as_str(), v6.port), ("::1", 4452));
        assert_eq!(Spec::parse(&v6.url()).unwrap(), v6);
    }

    #[test]
    fn directory_records() {
        let mut b = Vec::new();
        for (i, (name, attrs, size)) in [(".", 0x10u32, 0u64), ("a.txt", 0x20, 5), ("sub", 0x10, 0)].iter().enumerate() {
            let n: Vec<u8> = name.encode_utf16().flat_map(|c| c.to_le_bytes()).collect();
            let mut r = vec![0u8; 64];
            let len = (64 + n.len()).next_multiple_of(8);
            if i < 2 {
                r[0..4].copy_from_slice(&(len as u32).to_le_bytes());
            }
            r[24..32].copy_from_slice(&(116_444_736_000_000_000u64 + 10_000_000).to_le_bytes());
            r[40..48].copy_from_slice(&size.to_le_bytes());
            r[56..60].copy_from_slice(&attrs.to_le_bytes());
            r[60..64].copy_from_slice(&(n.len() as u32).to_le_bytes());
            r.extend(n);
            r.resize(len, 0);
            b.extend(r);
        }
        let e = dir_entries(&b);
        assert_eq!(e.len(), 2);
        assert_eq!((e[0].name.as_str(), e[0].dir, e[0].size, e[0].mtime), ("a.txt", false, 5, 1));
        assert!(e[1].dir);
    }

    /// The servers' account: whoever runs scripts/smb-test-servers.sh.
    fn test_user() -> String {
        std::env::var("USER").unwrap()
    }

    /// Against the test servers in DEVELOPMENT.md ("SMB test servers"):
    /// cargo test smb_servers -- --ignored
    #[test]
    #[ignore]
    fn smb_servers() {
        let host = std::env::var("SQD_SMB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        for (port, v, enc, want) in [
            (4451, Version::Auto, false, "SMB 1"),
            (4451, Version::V1, false, "SMB 1"),
            (4452, Version::Auto, false, "SMB 2.1"),
            (4452, Version::V2, false, "SMB 2.1"),
            (4453, Version::Auto, false, "SMB 3.1.1, encrypted"),
            (4453, Version::V3, true, "SMB 3.1.1, encrypted"),
            (4454, Version::V1, false, "SMB 1, signed"),
        ] {
            let spec = Spec { host: host.clone(), port, share: "test".into(), path: String::new(), user: test_user(), version: v, encrypt: enc };
            set_password(&spec, "sqdtest".into());
            let mut c = Conn::connect(&spec, "sqdtest").unwrap_or_else(|e| panic!("{port} {v:?}: {e}"));
            assert!(c.describe().starts_with(want), "{port} {v:?}: {}", c.describe());
            let mut names: Vec<String> = c.list("").unwrap().into_iter().map(|e| e.name).collect();
            names.sort();
            assert_eq!(names, ["docs", "photos", "with space"], "{port}");
            let docs = c.list("docs").unwrap();
            let big = docs.iter().find(|e| e.name == "big.bin").unwrap();
            assert_eq!(big.size, 1_200_000);
            let (fid, size) = c.open("docs/big.bin").unwrap();
            assert_eq!(size, 1_200_000);
            let mut all = Vec::new();
            let mut buf = vec![0u8; 65536];
            loop {
                let n = c.read(&fid, all.len() as u64, &mut buf).unwrap();
                if n == 0 {
                    break;
                }
                all.extend_from_slice(&buf[..n]);
            }
            c.close(&fid).unwrap();
            assert_eq!(all, std::fs::read(format!("{}/docs/big.bin", std::env::var("SQD_SMB_SHARE").unwrap())).unwrap(), "{port}");
            assert_eq!(c.list("with space").unwrap()[0].name, "note.txt");
            let bad = Conn::connect(&spec, "wrong").err().unwrap();
            assert!(bad.is_auth(), "{port}: {bad}");
        }
        let v1enc = Spec { host: host.clone(), port: 4451, share: "test".into(), path: String::new(), user: test_user(), version: Version::V1, encrypt: true };
        assert!(Conn::connect(&v1enc, "sqdtest").err().unwrap().to_string().contains("no encryption"));
        let on2 = Spec { port: 4452, version: Version::Auto, ..v1enc.clone() };
        assert!(Conn::connect(&on2, "sqdtest").is_err());
    }

    /// Deleting on the writable share `rw` of the same servers, whose folders
    /// are rw1..rw4 next to SQD_SMB_SHARE.
    #[test]
    #[ignore]
    fn smb_deletes() {
        let host = std::env::var("SQD_SMB_HOST").unwrap_or_else(|_| "127.0.0.1".into());
        let share = std::path::PathBuf::from(std::env::var("SQD_SMB_SHARE").unwrap());
        for (n, v) in [(1, Version::V1), (2, Version::V2), (3, Version::V3), (4, Version::V1)] {
            let dir = share.parent().unwrap().join(format!("rw{n}"));
            std::fs::create_dir_all(dir.join("old/deep")).unwrap();
            std::fs::write(dir.join("old/deep/a.txt"), "a").unwrap();
            std::fs::write(dir.join("old/b.txt"), "b").unwrap();
            std::fs::write(dir.join("keep.txt"), "k").unwrap();
            std::fs::write(dir.join("gone.txt"), "g").unwrap();
            let spec = Spec { host: host.clone(), port: 4450 + n, share: "rw".into(), path: String::new(), user: test_user(), version: v, encrypt: v == Version::V3 };
            let mut c = Conn::connect(&spec, "sqdtest").unwrap_or_else(|e| panic!("{n}: {e}"));
            c.delete_all("gone.txt", false).unwrap();
            c.delete_all("old", true).unwrap();
            assert!(c.delete_all("not-there.txt", false).unwrap_err().is_missing(), "{n}");
            let left: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
            assert_eq!(left, ["keep.txt"], "{n}");
            std::fs::remove_file(dir.join("keep.txt")).unwrap();
        }
    }
}
