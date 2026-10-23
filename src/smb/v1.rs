//! SMB 1 (MS-CIFS, MS-SMB), the "NT LM 0.12" dialect: for old NAS boxes and
//! Windows XP-era servers. Sign-in is NTLMv2 with extended security, and
//! messages are signed when the server signs. SMB 1 has no encryption.

use super::ntlm::{self, Creds};
use super::{dir_entries, Entry, Error, Fid, Wire};
use md5::{Digest, Md5};

const CLOSE: u8 = 0x04;
const TRANS2: u8 = 0x32;
const FIND_CLOSE2: u8 = 0x34;
const NEGOTIATE: u8 = 0x72;
const SESSION_SETUP: u8 = 0x73;
const TREE_CONNECT: u8 = 0x75;
const NT_CREATE: u8 = 0xa2;
const READ: u8 = 0x2e;

const FLAGS2: u16 = 0x8000 | 0x4000 | 0x0800 | 0x0001; // unicode, NT status, extended security, long names
const SIGNED: u16 = 0x0004;
const NO_SUCH_FILE: u32 = 0xC000_000F;
const NO_MORE_FILES: u32 = 0x8000_0006;
const END_OF_FILE: u32 = 0xC000_0011;

fn u16_at(b: &[u8], at: usize) -> u16 {
    b.get(at..at + 2).map_or(0, |s| u16::from_le_bytes(s.try_into().unwrap()))
}
fn u32_at(b: &[u8], at: usize) -> u32 {
    b.get(at..at + 4).map_or(0, |s| u32::from_le_bytes(s.try_into().unwrap()))
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain([0]).flat_map(|c| c.to_le_bytes()).collect()
}

/// A path on the share as SMB 1 writes it: `\a\b`.
fn smb_path(path: &str) -> String {
    format!("\\{}", path.trim_matches('/').replace('/', "\\"))
}

/// A response, header first.
struct Resp(Vec<u8>);

impl Resp {
    fn status(&self) -> u32 {
        u32_at(&self.0, 5)
    }
    fn wc(&self) -> usize {
        self.0.get(32).copied().unwrap_or(0) as usize
    }
    /// A byte of the parameter words.
    fn w8(&self, at: usize) -> u8 {
        self.0.get(33 + at).copied().unwrap_or(0)
    }
    fn w16(&self, at: usize) -> u16 {
        u16_at(&self.0, 33 + at)
    }
    fn w32(&self, at: usize) -> u32 {
        u32_at(&self.0, 33 + at)
    }
    fn w64(&self, at: usize) -> u64 {
        self.0.get(33 + at..41 + at).map_or(0, |s| u64::from_le_bytes(s.try_into().unwrap()))
    }
    /// The data bytes after the words.
    fn bytes(&self) -> &[u8] {
        let at = 33 + 2 * self.wc();
        let n = u16_at(&self.0, at) as usize;
        self.0.get(at + 2..(at + 2 + n).min(self.0.len())).unwrap_or(&[])
    }
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
    uid: u16,
    tid: u16,
    mid: u16,
    /// The signing key and the next request's sequence number.
    signing: Option<([u8; 16], u32)>,
    max_buffer: u32,
    large_read: bool,
}

impl Conn {
    pub fn signing(&self) -> bool {
        self.signing.is_some()
    }

    pub fn connect(wire: Wire, host: &str, share: &str, creds: &Creds) -> Result<Conn, Error> {
        let mut c = Conn { wire, uid: 0, tid: 0, mid: 0, signing: None, max_buffer: 4356, large_read: false };
        let r = c.request(NEGOTIATE, &[], b"\x02NT LM 0.12\0")?.ok()?;
        if r.wc() != 17 || r.w16(0) != 0 {
            return Err(Error::Other("the server doesn't speak SMB 1 (NT LM 0.12)".into()));
        }
        let security = r.w8(2);
        let max_mpx = r.w16(3);
        c.max_buffer = r.w32(7).max(1024);
        let session_key = r.w32(15);
        let caps = r.w32(19);
        if caps & 0x8000_0000 == 0 {
            return Err(Error::Other("this SMB 1 server only signs in without extended security, which isn't supported".into()));
        }
        c.large_read = caps & 0x4000 != 0;
        let setup = |c: &mut Conn, blob: &[u8]| {
            let mut w = vec![0xff, 0, 0, 0];
            w.extend(61440u16.to_le_bytes());
            w.extend(max_mpx.max(1).to_le_bytes());
            w.extend(1u16.to_le_bytes());
            w.extend(session_key.to_le_bytes());
            w.extend((blob.len() as u16).to_le_bytes());
            w.extend(0u32.to_le_bytes());
            w.extend((0x4u32 | 0x8 | 0x10 | 0x40 | 0x4000 | 0x8000_0000).to_le_bytes());
            let mut b = blob.to_vec();
            // native OS and LAN manager: empty, after a pad to even
            if (59 + b.len()) % 2 == 1 {
                b.push(0);
            }
            b.extend([0, 0, 0, 0]);
            c.request(SESSION_SETUP, &w, &b)
        };
        let r = setup(&mut c, &ntlm::negotiate())?;
        if r.status() != super::v2::MORE_PROCESSING {
            return Err(Error::Status(r.status()));
        }
        c.uid = u16_at(&r.0, 28);
        let challenge = r.bytes().get(..r.w16(6) as usize).unwrap_or(&[]).to_vec();
        let auth = ntlm::authenticate(&challenge, creds).map_err(Error::Other)?;
        let r = setup(&mut c, &auth.blob)?.ok()?;
        let guest = r.w16(4) & 1 != 0;
        if let (Some(key), false) = (auth.key, guest) {
            let unsigned = r.0[14..22] == [0; 8];
            if security & 0x08 != 0 || !unsigned {
                // the sign-in answer was number 1; Samba leaves it unsigned
                if !unsigned && Self::mac(&key, 1, &r.0) != r.0[14..22] {
                    return Err(Error::Other("the server's sign-in answer isn't signed correctly".into()));
                }
                c.signing = Some((key, 2));
            }
        } else if security & 0x08 != 0 {
            return Err(Error::Status(0xC000_A100));
        }
        let mut b = vec![0u8];
        b.extend(utf16z(&format!("\\\\{}\\{}", host.to_uppercase(), share)));
        b.extend(b"?????\0");
        let r = c.request(TREE_CONNECT, &[0xff, 0, 0, 0, 0, 0, 1, 0], &b)?.ok()?;
        c.tid = u16_at(&r.0, 24);
        Ok(c)
    }

    fn mac(key: &[u8; 16], seq: u32, msg: &[u8]) -> [u8; 8] {
        let mut m = msg.to_vec();
        m[14..18].copy_from_slice(&seq.to_le_bytes());
        m[18..22].fill(0);
        Md5::new().chain_update(key).chain_update(&m).finalize()[..8].try_into().unwrap()
    }

    fn send(&mut self, cmd: u8, words: &[u8], bytes: &[u8]) -> Result<(u16, Option<u32>), Error> {
        let mid = self.mid;
        self.mid = self.mid.wrapping_add(1);
        let mut m = Vec::with_capacity(35 + words.len() + bytes.len());
        m.extend(b"\xffSMB");
        m.push(cmd);
        m.extend(0u32.to_le_bytes());
        m.push(0x18); // case-insensitive, canonical names
        m.extend((FLAGS2 | if self.signing.is_some() || cmd == SESSION_SETUP { SIGNED } else { 0 }).to_le_bytes());
        m.extend([0u8; 12]); // PID high, signature, reserved
        m.extend(self.tid.to_le_bytes());
        m.extend(0xfeffu16.to_le_bytes());
        m.extend(self.uid.to_le_bytes());
        m.extend(mid.to_le_bytes());
        m.push((words.len() / 2) as u8);
        m.extend_from_slice(words);
        m.extend((bytes.len() as u16).to_le_bytes());
        m.extend_from_slice(bytes);
        let seq = match &mut self.signing {
            Some((key, seq)) => {
                let s = *seq;
                let mac = Self::mac(key, s, &m);
                m[14..22].copy_from_slice(&mac);
                *seq += 2;
                Some(s)
            }
            None => None,
        };
        self.wire.send(&m)?;
        Ok((mid, seq))
    }

    /// The next answer to request `mid`.
    fn recv(&mut self, mid: u16, seq: Option<u32>) -> Result<Resp, Error> {
        loop {
            let r = self.wire.recv()?;
            if r.len() < 35 || !r.starts_with(b"\xffSMB") {
                return Err(Error::Gone("not an SMB 1 answer".into()));
            }
            if u16_at(&r, 30) != mid {
                continue;
            }
            if let (Some((key, _)), Some(seq)) = (&self.signing, seq) {
                if Self::mac(key, seq + 1, &r) != r[14..22] {
                    return Err(Error::Gone("an answer from the server isn't signed correctly".into()));
                }
            }
            return Ok(Resp(r));
        }
    }

    fn request(&mut self, cmd: u8, words: &[u8], bytes: &[u8]) -> Result<Resp, Error> {
        let (mid, seq) = self.send(cmd, words, bytes)?;
        self.recv(mid, seq)
    }

    /// A TRANS2 request; returns its status, parameters and data (gathered
    /// from every part of the answer).
    fn trans2(&mut self, setup: u16, params: &[u8]) -> Result<(u32, Vec<u8>, Vec<u8>), Error> {
        const PARAM_OFF: usize = 68;
        let max_data = (self.max_buffer as usize).clamp(1024, 65535) - 512;
        let mut w = Vec::with_capacity(30);
        w.extend((params.len() as u16).to_le_bytes());
        w.extend(0u16.to_le_bytes());
        w.extend(256u16.to_le_bytes());
        w.extend((max_data as u16).to_le_bytes());
        w.extend([0, 0]);
        w.extend(0u16.to_le_bytes());
        w.extend(0u32.to_le_bytes());
        w.extend(0u16.to_le_bytes());
        w.extend((params.len() as u16).to_le_bytes());
        w.extend((PARAM_OFF as u16).to_le_bytes());
        w.extend(0u16.to_le_bytes());
        w.extend(((PARAM_OFF + params.len()) as u16).to_le_bytes());
        w.extend([1, 0]);
        w.extend(setup.to_le_bytes());
        // a pad, the (empty) name, then the parameters at 68
        let mut b = vec![0u8, 0, 0];
        b.extend_from_slice(params);
        let (mid, seq) = self.send(TRANS2, &w, &b)?;
        let (mut p, mut d) = (Vec::new(), Vec::new());
        loop {
            let r = self.recv(mid, seq)?;
            if r.status() != 0 && r.wc() < 10 {
                return Ok((r.status(), p, d));
            }
            let (tp, td) = (r.w16(0) as usize, r.w16(2) as usize);
            let (pc, po, pd) = (r.w16(6) as usize, r.w16(8) as usize, r.w16(10) as usize);
            let (dc, doff, dd) = (r.w16(12) as usize, r.w16(14) as usize, r.w16(16) as usize);
            p.resize(p.len().max(pd + pc).max(tp.min(pd + pc)), 0);
            p[pd..pd + pc].copy_from_slice(r.slice(po, pc).get(..pc).ok_or_else(|| Error::Gone("a short answer".into()))?);
            d.resize(d.len().max(dd + dc), 0);
            d[dd..dd + dc].copy_from_slice(r.slice(doff, dc).get(..dc).ok_or_else(|| Error::Gone("a short answer".into()))?);
            if pd + pc >= tp && dd + dc >= td {
                return Ok((r.status(), p, d));
            }
        }
    }

    pub fn list(&mut self, path: &str) -> Result<Vec<Entry>, Error> {
        let pattern = {
            let p = smb_path(path);
            if p == "\\" { "\\*".to_string() } else { format!("{p}\\*") }
        };
        let count = 512u16;
        let mut params = Vec::new();
        params.extend(0x16u16.to_le_bytes()); // hidden, system and folders too
        params.extend(count.to_le_bytes());
        params.extend(0x02u16.to_le_bytes()); // close at the end
        params.extend(0x0101u16.to_le_bytes()); // SMB_FIND_FILE_DIRECTORY_INFO
        params.extend(0u32.to_le_bytes());
        params.extend(utf16z(&pattern));
        let (status, p, d) = self.trans2(1, &params)?;
        match status {
            0 => {}
            NO_SUCH_FILE => return Ok(Vec::new()),
            s => return Err(Error::Status(s)),
        }
        let sid = u16_at(&p, 0);
        let mut end = u16_at(&p, 4) != 0;
        let mut out = dir_entries(&d);
        let mut last = last_name(&d);
        while !end {
            let Some(name) = last.take() else { break };
            let mut params = Vec::new();
            params.extend(sid.to_le_bytes());
            params.extend(count.to_le_bytes());
            params.extend(0x0101u16.to_le_bytes());
            params.extend(0u32.to_le_bytes());
            params.extend(0x0au16.to_le_bytes()); // close at the end; continue
            params.extend(utf16z(&name));
            let (status, p, d) = self.trans2(2, &params)?;
            match status {
                0 => {}
                NO_MORE_FILES | NO_SUCH_FILE => break,
                s => return Err(Error::Status(s)),
            }
            end = u16_at(&p, 2) != 0 || u16_at(&p, 0) == 0;
            out.extend(dir_entries(&d));
            last = last_name(&d);
        }
        if !end {
            let mut w = Vec::new();
            w.extend(sid.to_le_bytes());
            let _ = self.request(FIND_CLOSE2, &w, &[]);
        }
        for e in &mut out {
            // some servers count the terminating NUL in the name
            while e.name.ends_with('\0') {
                e.name.pop();
            }
        }
        Ok(out)
    }

    fn create(&mut self, path: &str, access: u32, options: u32) -> Result<(Fid, Resp), Error> {
        let name = smb_path(path);
        let mut w = vec![0xff, 0, 0, 0, 0];
        w.extend(((name.encode_utf16().count() * 2) as u16).to_le_bytes());
        w.extend(0u32.to_le_bytes()); // flags
        w.extend(0u32.to_le_bytes()); // root folder
        w.extend(access.to_le_bytes());
        w.extend(0u64.to_le_bytes());
        w.extend(0u32.to_le_bytes()); // attributes
        w.extend(7u32.to_le_bytes()); // share read, write and delete
        w.extend(1u32.to_le_bytes()); // open an existing one
        w.extend(options.to_le_bytes());
        w.extend(2u32.to_le_bytes()); // impersonation
        w.push(0);
        let mut b = vec![0u8]; // pad to even
        b.extend(utf16z(&name));
        let r = self.request(NT_CREATE, &w, &b)?.ok()?;
        let mut fid = [0u8; 16];
        fid[..2].copy_from_slice(&r.w16(5).to_le_bytes());
        Ok((fid, r))
    }

    pub fn open(&mut self, path: &str) -> Result<(Fid, u64), Error> {
        let (fid, r) = self.create(path, 0x0012_0089, 0x40)?;
        Ok((fid, r.w64(55)))
    }

    pub fn close(&mut self, fid: &Fid) -> Result<(), Error> {
        let mut w = fid[..2].to_vec();
        w.extend(u32::MAX.to_le_bytes());
        self.request(CLOSE, &w, &[])?.ok().map(|_| ())
    }

    pub fn read(&mut self, fid: &Fid, offset: u64, buf: &mut [u8]) -> Result<usize, Error> {
        let most = if self.large_read { 61440 } else { self.max_buffer as usize - 64 };
        let len = buf.len().min(most);
        let mut w = vec![0xff, 0, 0, 0];
        w.extend_from_slice(&fid[..2]);
        w.extend((offset as u32).to_le_bytes());
        w.extend((len as u16).to_le_bytes());
        w.extend((len as u16).to_le_bytes());
        w.extend(((len >> 16) as u32).to_le_bytes());
        w.extend(0u16.to_le_bytes());
        w.extend(((offset >> 32) as u32).to_le_bytes());
        let r = self.request(READ, &w, &[])?;
        match r.status() {
            0 => {}
            END_OF_FILE => return Ok(0),
            s => return Err(Error::Status(s)),
        }
        let n = r.w16(10) as usize | (r.w16(14) as usize) << 16;
        let data = r.slice(r.w16(12) as usize, n);
        let n = data.len().min(len);
        buf[..n].copy_from_slice(&data[..n]);
        Ok(n)
    }

    pub fn delete(&mut self, path: &str, dir: bool) -> Result<(), Error> {
        let (fid, _) = self.create(path, 0x0001_0080, 0x1000 | if dir { 0x1 } else { 0x40 })?;
        self.close(&fid)
    }
}

/// The name of the last record, where FIND_NEXT2 carries on from.
fn last_name(d: &[u8]) -> Option<String> {
    let mut at = 0usize;
    loop {
        let next = u32_at(d, at) as usize;
        if next == 0 {
            break;
        }
        at += next;
    }
    let len = u32_at(d, at + 60) as usize;
    let name = d.get(at + 64..at + 64 + len)?;
    let s = String::from_utf16_lossy(&name.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect::<Vec<_>>());
    Some(s.trim_end_matches('\0').to_string())
}
