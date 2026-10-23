//! SMB 2 and 3 (MS-SMB2): sign-in, folders, reading and deleting, with
//! signing (HMAC-SHA256 for 2.x, AES-CMAC for 3.x) and SMB 3 encryption
//! (AES-128-CCM or -GCM). One request at a time per connection.

use super::ntlm::{self, Creds};
use super::{dir_entries, Entry, Error, Fid, Wire};
use aes::Aes128;
use aes_gcm::aead::consts::{U11, U16};
use aes_gcm::{AeadInOut, Aes128Gcm, KeyInit};
use cmac::{Cmac, Mac};
use hmac::Hmac;
use sha2::{Digest, Sha256, Sha512};

type Aes128Ccm = ccm::Ccm<Aes128, U16, U11>;

const NEGOTIATE: u16 = 0;
const SESSION_SETUP: u16 = 1;
const TREE_CONNECT: u16 = 3;
const CREATE: u16 = 5;
const CLOSE: u16 = 6;
const READ: u16 = 8;
const QUERY_DIRECTORY: u16 = 0x0e;

const FLAG_RESPONSE: u32 = 0x1;
const FLAG_ASYNC: u32 = 0x2;
const FLAG_SIGNED: u32 = 0x8;

pub const MORE_PROCESSING: u32 = 0xC000_0016;
const PENDING: u32 = 0x0000_0103;
const NO_MORE_FILES: u32 = 0x8000_0006;
const NO_SUCH_FILE: u32 = 0xC000_000F;
const END_OF_FILE: u32 = 0xC000_0011;

pub const SMB202: u16 = 0x0202;
pub const SMB210: u16 = 0x0210;
pub const SMB300: u16 = 0x0300;
pub const SMB302: u16 = 0x0302;
pub const SMB311: u16 = 0x0311;

/// What a dialect is called.
pub fn dialect_name(d: u16) -> &'static str {
    match d {
        SMB202 => "SMB 2.0.2",
        SMB210 => "SMB 2.1",
        SMB300 => "SMB 3.0",
        SMB302 => "SMB 3.0.2",
        _ => "SMB 3.1.1",
    }
}

enum Signer {
    Sha256([u8; 16]),
    Cmac([u8; 16]),
}

impl Signer {
    fn sign(&self, msg: &[u8]) -> [u8; 16] {
        let mut m = msg.to_vec();
        m[48..64].fill(0);
        match self {
            Signer::Sha256(k) => {
                let mut h = <Hmac<Sha256> as KeyInit>::new_from_slice(k).unwrap();
                h.update(&m);
                h.finalize().into_bytes()[..16].try_into().unwrap()
            }
            Signer::Cmac(k) => {
                let mut c = <Cmac<Aes128> as KeyInit>::new_from_slice(k).unwrap();
                c.update(&m);
                c.finalize().into_bytes().into()
            }
        }
    }
}

#[derive(Clone, Copy)]
enum Cipher {
    Ccm,
    Gcm,
}

struct Sealer {
    cipher: Cipher,
    enc: [u8; 16],
    dec: [u8; 16],
}

impl Sealer {
    /// Wraps a message in a transform header, encrypted.
    fn seal(&self, msg: &[u8], session: u64) -> Result<Vec<u8>, Error> {
        let mut out = vec![0u8; 52];
        out[..4].copy_from_slice(b"\xfdSMB");
        let n = match self.cipher {
            Cipher::Ccm => 11,
            Cipher::Gcm => 12,
        };
        getrandom::fill(&mut out[20..20 + n]).map_err(|e| Error::Other(e.to_string()))?;
        out[36..40].copy_from_slice(&(msg.len() as u32).to_le_bytes());
        out[42..44].copy_from_slice(&1u16.to_le_bytes());
        out[44..52].copy_from_slice(&session.to_le_bytes());
        let mut body = msg.to_vec();
        let (nonce, aad) = (out[20..20 + n].to_vec(), out[20..52].to_vec());
        let tag: [u8; 16] = match self.cipher {
            Cipher::Ccm => Aes128Ccm::new_from_slice(&self.enc)
                .unwrap()
                .encrypt_inout_detached(nonce[..].try_into().unwrap(), &aad, body.as_mut_slice().into())
                .map(Into::into),
            Cipher::Gcm => Aes128Gcm::new_from_slice(&self.enc)
                .unwrap()
                .encrypt_inout_detached(nonce[..].try_into().unwrap(), &aad, body.as_mut_slice().into())
                .map(Into::into),
        }
        .map_err(|_| Error::Other("encryption failed".into()))?;
        out[4..20].copy_from_slice(&tag);
        out.extend(body);
        Ok(out)
    }

    fn open(&self, msg: &[u8]) -> Result<Vec<u8>, Error> {
        if msg.len() < 52 {
            return Err(Error::Gone("a short encrypted message".into()));
        }
        let n = match self.cipher {
            Cipher::Ccm => 11,
            Cipher::Gcm => 12,
        };
        let tag: [u8; 16] = msg[4..20].try_into().unwrap();
        let (nonce, aad) = (&msg[20..20 + n], &msg[20..52]);
        let mut body = msg[52..].to_vec();
        match self.cipher {
            Cipher::Ccm => Aes128Ccm::new_from_slice(&self.dec).unwrap().decrypt_inout_detached(nonce.try_into().unwrap(), aad, body.as_mut_slice().into(), (&tag).into()),
            Cipher::Gcm => Aes128Gcm::new_from_slice(&self.dec).unwrap().decrypt_inout_detached(nonce.try_into().unwrap(), aad, body.as_mut_slice().into(), (&tag).into()),
        }
        .map_err(|_| Error::Gone("a message from the server failed its integrity check".into()))?;
        Ok(body)
    }
}

/// SP800-108 in counter mode with HMAC-SHA256, as SMB 3 derives its keys.
fn kdf(key: &[u8; 16], label: &[u8], context: &[u8]) -> [u8; 16] {
    let mut h = <Hmac<Sha256> as KeyInit>::new_from_slice(key).unwrap();
    h.update(&1u32.to_be_bytes());
    h.update(label);
    h.update(&[0]);
    h.update(context);
    h.update(&128u32.to_be_bytes());
    h.finalize().into_bytes()[..16].try_into().unwrap()
}

fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |s| u16::from_le_bytes(s.try_into().unwrap()))
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}
fn u64_at(b: &[u8], at: usize) -> u64 {
    b.get(at..at + 8).map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()))
}

/// A response: the whole message, header first (offsets in bodies count
/// from the header).
struct Resp(Vec<u8>);

impl Resp {
    fn status(&self) -> u32 {
        u32_at(&self.0, 8)
    }
    /// A byte of the body.
    fn b(&self, at: usize) -> u8 {
        self.0.get(64 + at).copied().unwrap_or(0)
    }
    fn u16(&self, at: usize) -> u16 {
        u16_at(&self.0, 64 + at)
    }
    fn u32(&self, at: usize) -> u32 {
        u32_at(&self.0, 64 + at)
    }
    fn u64(&self, at: usize) -> u64 {
        u64_at(&self.0, 64 + at)
    }
    /// The buffer at a header-relative offset.
    fn slice(&self, off: usize, len: usize) -> &[u8] {
        self.0.get(off..off + len).unwrap_or(&[])
    }
    fn ok(self) -> Result<Resp, Error> {
        match self.status() {
            0 => Ok(self),
            s => Err(Error::Status(s)),
        }
    }
}

pub struct Conn {
    wire: Wire,
    pub dialect: u16,
    msg_id: u64,
    session: u64,
    tree: u32,
    signer: Option<Signer>,
    sealer: Option<Sealer>,
    encrypt: bool,
    max_read: u32,
    /// 3.1.1's running hash of the sign-in messages.
    preauth: [u8; 64],
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
}

impl Conn {
    /// How messages are protected, if they are.
    pub fn protection(&self) -> Option<&'static str> {
        if self.encrypt {
            Some("encrypted")
        } else if self.signer.is_some() {
            Some("signed")
        } else {
            None
        }
    }

    /// Signs in and opens the share. `dialects` are the versions to offer;
    /// `encrypt` insists on SMB 3 encryption.
    pub fn connect(wire: Wire, host: &str, share: &str, creds: &Creds, dialects: &[u16], encrypt: bool) -> Result<Conn, Error> {
        let mut c = Conn { wire, dialect: 0, msg_id: 0, session: 0, tree: 0, signer: None, sealer: None, encrypt: false, max_read: 65536, preauth: [0; 64] };
        let cipher = c.negotiate(dialects)?;
        if encrypt && cipher.is_none() {
            return Err(Error::Other("This server doesn't offer SMB 3 encryption".into()));
        }
        let flags = c.session_setup(creds, cipher)?;
        c.encrypt = encrypt || flags & 0x4 != 0;
        if c.encrypt && c.sealer.is_none() {
            return Err(Error::Other("This server wants encryption, which needs a signed-in user".into()));
        }
        let path = utf16(&format!("\\\\{host}\\{share}"));
        let mut body = Vec::with_capacity(8 + path.len());
        body.extend(9u16.to_le_bytes());
        body.extend(0u16.to_le_bytes());
        body.extend(72u16.to_le_bytes());
        body.extend((path.len() as u16).to_le_bytes());
        body.extend(path);
        let r = c.request(TREE_CONNECT, &body)?.ok()?;
        c.tree = u32_at(&r.0, 36);
        if r.u32(4) & 0x8 != 0 {
            if c.sealer.is_none() {
                return Err(Error::Other("This share wants encryption, which needs a signed-in user".into()));
            }
            c.encrypt = true;
        }
        Ok(c)
    }

    /// Picks a version; returns the cipher it offers, if any.
    fn negotiate(&mut self, dialects: &[u16]) -> Result<Option<Cipher>, Error> {
        let wants311 = dialects.contains(&SMB311);
        let mut b = Vec::new();
        b.extend(36u16.to_le_bytes());
        b.extend((dialects.len() as u16).to_le_bytes());
        b.extend(1u16.to_le_bytes()); // signing enabled
        b.extend(0u16.to_le_bytes());
        b.extend(0x40u32.to_le_bytes()); // encryption
        let mut guid = [0u8; 16];
        getrandom::fill(&mut guid).map_err(|e| Error::Other(e.to_string()))?;
        b.extend(guid);
        b.extend([0u8; 8]); // negotiate contexts, filled in below
        for d in dialects {
            b.extend(d.to_le_bytes());
        }
        if wants311 {
            while (64 + b.len()) % 8 != 0 {
                b.push(0);
            }
            let off = 64 + b.len();
            b[28..32].copy_from_slice(&(off as u32).to_le_bytes());
            b[32..34].copy_from_slice(&2u16.to_le_bytes());
            let mut salt = [0u8; 32];
            getrandom::fill(&mut salt).map_err(|e| Error::Other(e.to_string()))?;
            // preauth integrity: SHA-512
            b.extend([1, 0, 38, 0, 0, 0, 0, 0, 1, 0, 32, 0, 1, 0]);
            b.extend(salt);
            while b.len() % 8 != 0 {
                b.push(0);
            }
            // encryption: AES-128-GCM, then AES-128-CCM
            b.extend([2, 0, 6, 0, 0, 0, 0, 0, 2, 0, 2, 0, 1, 0]);
        }
        let r = self.request(NEGOTIATE, &b)?.ok()?;
        self.dialect = r.u16(4);
        if !dialects.contains(&self.dialect) {
            return Err(Error::Other(format!("the server answered with an unknown SMB version ({:#06x})", self.dialect)));
        }
        self.max_read = r.u32(32).clamp(4096, 65536);
        let caps = r.u32(24);
        Ok(match self.dialect {
            SMB311 => {
                let mut cipher = None;
                let (mut off, n) = (r.u32(60) as usize, r.u16(6));
                for _ in 0..n {
                    let ty = u16_at(&r.0, off);
                    let len = u16_at(&r.0, off + 2) as usize;
                    if ty == 2 && u16_at(&r.0, off + 8) >= 1 {
                        cipher = match u16_at(&r.0, off + 10) {
                            1 => Some(Cipher::Ccm),
                            2 => Some(Cipher::Gcm),
                            _ => None,
                        };
                    }
                    off = (off + 8 + len).next_multiple_of(8);
                }
                cipher
            }
            SMB300 | SMB302 if caps & 0x40 != 0 => Some(Cipher::Ccm),
            _ => None,
        })
    }

    /// Signs in; returns the session flags.
    fn session_setup(&mut self, creds: &Creds, cipher: Option<Cipher>) -> Result<u16, Error> {
        let setup = |c: &mut Conn, blob: &[u8]| {
            let mut b = Vec::with_capacity(24 + blob.len());
            b.extend(25u16.to_le_bytes());
            b.push(0);
            b.push(1); // signing enabled
            b.extend(0u32.to_le_bytes());
            b.extend(0u32.to_le_bytes());
            b.extend(88u16.to_le_bytes());
            b.extend((blob.len() as u16).to_le_bytes());
            b.extend(0u64.to_le_bytes());
            b.extend_from_slice(blob);
            c.request(SESSION_SETUP, &b)
        };
        let r = setup(self, &ntlm::negotiate())?;
        if r.status() != MORE_PROCESSING {
            return Err(Error::Status(r.status()));
        }
        self.session = u64_at(&r.0, 40);
        let challenge = r.slice(r.u16(4) as usize, r.u16(6) as usize).to_vec();
        let auth = ntlm::authenticate(&challenge, creds).map_err(Error::Other)?;
        let r = setup(self, &auth.blob)?;
        if r.status() != 0 {
            return Err(Error::Status(r.status()));
        }
        let flags = r.u16(2);
        // a guest or anonymous session has no key to sign with
        if let (Some(key), 0) = (auth.key, flags & 0x3) {
            if self.dialect >= SMB300 {
                let (sl, sc, el, ec, dl, dc): (&[u8], &[u8], &[u8], &[u8], &[u8], &[u8]) = if self.dialect == SMB311 {
                    (b"SMBSigningKey\0", &self.preauth, b"SMBC2SCipherKey\0", &self.preauth, b"SMBS2CCipherKey\0", &self.preauth)
                } else {
                    (b"SMB2AESCMAC\0", b"SmbSign\0", b"SMB2AESCCM\0", b"ServerIn \0", b"SMB2AESCCM\0", b"ServerOut\0")
                };
                self.signer = Some(Signer::Cmac(kdf(&key, sl, sc)));
                self.sealer = cipher.map(|cipher| Sealer { cipher, enc: kdf(&key, el, ec), dec: kdf(&key, dl, dc) });
            } else {
                self.signer = Some(Signer::Sha256(key));
            }
            // the last sign-in answer is signed with the new key
            if let Some(s) = &self.signer {
                if u32_at(&r.0, 16) & FLAG_SIGNED == 0 || s.sign(&r.0) != r.0[48..64] {
                    return Err(Error::Other("the server's sign-in answer isn't signed correctly".into()));
                }
            }
        }
        Ok(flags)
    }

    /// Sends one request and waits for its answer.
    fn request(&mut self, cmd: u16, body: &[u8]) -> Result<Resp, Error> {
        let id = self.msg_id;
        self.msg_id += 1;
        let mut m = Vec::with_capacity(64 + body.len());
        m.extend(b"\xfeSMB");
        m.extend(64u16.to_le_bytes());
        let charge: u16 = if cmd == NEGOTIATE || self.dialect == SMB202 { 0 } else { 1 };
        m.extend(charge.to_le_bytes());
        m.extend(0u32.to_le_bytes());
        m.extend(cmd.to_le_bytes());
        m.extend(64u16.to_le_bytes()); // credits asked for
        let signed = self.signer.is_some() && !self.encrypt;
        m.extend((if signed { FLAG_SIGNED } else { 0 }).to_le_bytes());
        m.extend(0u32.to_le_bytes());
        m.extend(id.to_le_bytes());
        m.extend(0xfeffu32.to_le_bytes());
        m.extend(self.tree.to_le_bytes());
        m.extend(self.session.to_le_bytes());
        m.extend([0u8; 16]);
        m.extend_from_slice(body);
        if let (true, Some(s)) = (signed, &self.signer) {
            let sig = s.sign(&m);
            m[48..64].copy_from_slice(&sig);
        }
        // the negotiate is hashed before the dialect is known
        if cmd == NEGOTIATE || (self.dialect == SMB311 && cmd == SESSION_SETUP) {
            self.preauth = Sha512::new().chain_update(self.preauth).chain_update(&m).finalize().into();
        }
        match (&self.sealer, self.encrypt) {
            (Some(s), true) => {
                let sealed = s.seal(&m, self.session)?;
                self.wire.send(&sealed)?;
            }
            _ => self.wire.send(&m)?,
        }
        loop {
            let mut r = self.wire.recv()?;
            let sealed = r.starts_with(b"\xfdSMB");
            if sealed {
                r = self.sealer.as_ref().ok_or_else(|| Error::Gone("an encrypted answer to an unencrypted request".into()))?.open(&r)?;
            } else if self.encrypt {
                return Err(Error::Gone("the server answered without encryption".into()));
            }
            if r.starts_with(b"\xffSMB") {
                return Err(Error::Smb1Only);
            }
            if r.len() < 64 || !r.starts_with(b"\xfeSMB") {
                return Err(Error::Gone("not an SMB answer".into()));
            }
            let (flags, status) = (u32_at(&r, 16), u32_at(&r, 8));
            if flags & FLAG_RESPONSE == 0 || u64_at(&r, 24) != id {
                continue;
            }
            if flags & FLAG_ASYNC != 0 && status == PENDING {
                continue;
            }
            if cmd == NEGOTIATE || (self.dialect == SMB311 && cmd == SESSION_SETUP && status == MORE_PROCESSING) {
                self.preauth = Sha512::new().chain_update(self.preauth).chain_update(&r).finalize().into();
            }
            // answers to signed requests come back signed (encrypted ones
            // are already checked)
            if signed && !sealed {
                let s = self.signer.as_ref().unwrap();
                if flags & FLAG_SIGNED == 0 || s.sign(&r) != r[48..64] {
                    return Err(Error::Gone("an answer from the server isn't signed correctly".into()));
                }
            }
            return Ok(Resp(r));
        }
    }

    fn create(&mut self, path: &str, access: u32, options: u32) -> Result<(Fid, Resp), Error> {
        let name = utf16(&path.replace('/', "\\"));
        let mut b = Vec::with_capacity(56 + name.len().max(1));
        b.extend(57u16.to_le_bytes());
        b.push(0);
        b.push(0); // no oplock
        b.extend(2u32.to_le_bytes()); // impersonation
        b.extend([0u8; 16]);
        b.extend(access.to_le_bytes());
        b.extend(0u32.to_le_bytes());
        b.extend(7u32.to_le_bytes()); // share read, write and delete
        b.extend(1u32.to_le_bytes()); // open an existing one
        b.extend(options.to_le_bytes());
        b.extend(120u16.to_le_bytes());
        b.extend((name.len() as u16).to_le_bytes());
        b.extend([0u8; 8]);
        if name.is_empty() {
            b.push(0);
        }
        b.extend(name);
        let r = self.request(CREATE, &b)?.ok()?;
        let fid: Fid = r.0[64 + 64..64 + 80].try_into().map_err(|_| Error::Gone("a short answer".into()))?;
        Ok((fid, r))
    }

    pub fn close(&mut self, fid: &Fid) -> Result<(), Error> {
        let mut b = Vec::with_capacity(24);
        b.extend(24u16.to_le_bytes());
        b.extend([0u8; 6]);
        b.extend(fid);
        self.request(CLOSE, &b)?.ok().map(|_| ())
    }

    /// The folder's entries (`path` is '/'-separated, from the share's top).
    pub fn list(&mut self, path: &str) -> Result<Vec<Entry>, Error> {
        let (fid, _) = self.create(path, 0x0010_0081, 0x1)?;
        let res = self.query_dir(&fid);
        let _ = self.close(&fid);
        res
    }

    fn query_dir(&mut self, fid: &Fid) -> Result<Vec<Entry>, Error> {
        let mut out = Vec::new();
        let pattern = utf16("*");
        for first in std::iter::once(true).chain(std::iter::repeat(false)) {
            let mut b = Vec::with_capacity(32 + pattern.len());
            b.extend(33u16.to_le_bytes());
            b.push(0x01); // FileDirectoryInformation
            b.push(if first { 0x01 } else { 0 }); // restart
            b.extend(0u32.to_le_bytes());
            b.extend(fid);
            b.extend(96u16.to_le_bytes());
            b.extend((pattern.len() as u16).to_le_bytes());
            b.extend(65536u32.to_le_bytes());
            b.extend(&pattern);
            let r = self.request(QUERY_DIRECTORY, &b)?;
            match r.status() {
                0 => {}
                NO_MORE_FILES => break,
                NO_SUCH_FILE if first => break,
                s => return Err(Error::Status(s)),
            }
            let buf = r.slice(r.u16(2) as usize, r.u32(4) as usize);
            if buf.is_empty() {
                break;
            }
            out.extend(dir_entries(buf));
        }
        Ok(out)
    }

    /// Opens a file to read; returns it and its size.
    pub fn open(&mut self, path: &str) -> Result<(Fid, u64), Error> {
        let (fid, r) = self.create(path, 0x0012_0089, 0x40)?;
        Ok((fid, r.u64(48)))
    }

    pub fn read(&mut self, fid: &Fid, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let len = buf.len().min(self.max_read as usize);
        let mut b = Vec::with_capacity(49);
        b.extend(49u16.to_le_bytes());
        b.push(0x50);
        b.push(0);
        b.extend((len as u32).to_le_bytes());
        b.extend(offset.to_le_bytes());
        b.extend(fid);
        b.extend([0u8; 17]);
        let r = self.request(READ, &b)?;
        match r.status() {
            0 => {}
            END_OF_FILE => return Ok(0),
            s => return Err(Error::Status(s)),
        }
        let data = r.slice(r.b(2) as usize, r.u32(4) as usize);
        let n = data.len().min(len);
        buf[..n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    /// Deletes a file, or an empty folder.
    pub fn delete(&mut self, path: &str, dir: bool) -> Result<(), Error> {
        // DELETE and read attributes; delete on close
        let (fid, _) = self.create(path, 0x0001_0080, 0x1000 | if dir { 0x1 } else { 0x40 })?;
        self.close(&fid)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmac_rfc4493() {
        let k = [0x2b, 0x7e, 0x15, 0x16, 0x28, 0xae, 0xd2, 0xa6, 0xab, 0xf7, 0x15, 0x88, 0x09, 0xcf, 0x4f, 0x3c];
        let mut msg = [0u8; 64];
        msg[..16].copy_from_slice(&[0x6b, 0xc1, 0xbe, 0xe2, 0x2e, 0x40, 0x9f, 0x96, 0xe9, 0x3d, 0x7e, 0x11, 0x73, 0x93, 0x17, 0x2a]);
        // sign() zeroes 48..64, so check a 16-byte message through the mac directly
        let mut c = <Cmac<Aes128> as KeyInit>::new_from_slice(&k).unwrap();
        c.update(&msg[..16]);
        let t: [u8; 16] = c.finalize().into_bytes().into();
        assert_eq!(t, [0x07, 0x0a, 0x16, 0xb4, 0x6b, 0x4d, 0x41, 0x44, 0xf7, 0x9b, 0xdd, 0x9d, 0xd0, 0x4a, 0x28, 0x7c]);
    }

    #[test]
    fn kdf_counter_mode() {
        let key = [0u8; 16];
        let a = kdf(&key, b"SMB2AESCCM\0", b"ServerIn \0");
        let b = kdf(&key, b"SMB2AESCCM\0", b"ServerOut\0");
        assert_ne!(a, b);
        assert_eq!(a, kdf(&key, b"SMB2AESCCM\0", b"ServerIn \0"));
    }

    #[test]
    fn seal_roundtrip() {
        for cipher in [Cipher::Ccm, Cipher::Gcm] {
            let s = Sealer { cipher, enc: [7; 16], dec: [7; 16] };
            let msg = b"\xfeSMB and the rest of a message".to_vec();
            let sealed = s.seal(&msg, 42).unwrap();
            assert_ne!(&sealed[52..], &msg[..]);
            assert_eq!(s.open(&sealed).unwrap(), msg);
            let mut bad = sealed.clone();
            bad[60] ^= 1;
            assert!(s.open(&bad).is_err());
        }
    }
}
