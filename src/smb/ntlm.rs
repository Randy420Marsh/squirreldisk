//! NTLMv2 sign-in (MS-NLMP), for every SMB version: the three messages, and
//! the session key both ends then sign (and with SMB 3, encrypt) with. The
//! messages travel wrapped in SPNEGO, as servers expect.

use hmac::{Hmac, KeyInit, Mac};
use md4::{Digest, Md4};
use md5::Md5;

/// Who signs in. An empty user is an anonymous sign-in (no key, no signing).
#[derive(Clone, Default)]
pub struct Creds {
    pub domain: String,
    pub user: String,
    pub password: String,
}

impl Creds {
    /// "DOMAIN\user" (or "user@domain") as typed, and the password.
    pub fn new(user: &str, password: &str) -> Self {
        let (domain, user) = match user.split_once('\\') {
            Some((d, u)) => (d, u),
            None => ("", user),
        };
        Creds { domain: domain.into(), user: user.into(), password: password.into() }
    }

    fn anonymous(&self) -> bool {
        self.user.is_empty()
    }
}

const UNICODE: u32 = 0x1;
const REQUEST_TARGET: u32 = 0x4;
const SIGN: u32 = 0x10;
const NTLM: u32 = 0x200;
const ANONYMOUS: u32 = 0x800;
const ALWAYS_SIGN: u32 = 0x8000;
const EXTENDED_SESSIONSECURITY: u32 = 0x80000;
const TARGET_INFO: u32 = 0x800000;
const N128: u32 = 0x20000000;
const N56: u32 = 0x80000000;
const FLAGS: u32 = UNICODE | REQUEST_TARGET | SIGN | NTLM | ALWAYS_SIGN | EXTENDED_SESSIONSECURITY | TARGET_INFO | N128 | N56;

const SPNEGO: &[u8] = &[0x2b, 0x06, 0x01, 0x05, 0x05, 0x02];
const NTLMSSP: &[u8] = &[0x2b, 0x06, 0x01, 0x04, 0x01, 0x82, 0x37, 0x02, 0x02, 0x0a];

fn hmac_md5(key: &[u8], parts: &[&[u8]]) -> [u8; 16] {
    let mut m = <Hmac<Md5> as KeyInit>::new_from_slice(key).expect("any key length");
    for p in parts {
        m.update(p);
    }
    m.finalize().into_bytes().into()
}

fn utf16(s: &str) -> Vec<u8> {
    s.encode_utf16().flat_map(|c| c.to_le_bytes()).collect()
}

fn ntowfv2(c: &Creds) -> [u8; 16] {
    let nt: [u8; 16] = Md4::digest(utf16(&c.password)).into();
    hmac_md5(&nt, &[&utf16(&(c.user.to_uppercase() + &c.domain))])
}

/// A DER length.
fn der_len(n: usize) -> Vec<u8> {
    match n {
        0..=0x7f => vec![n as u8],
        0x80..=0xff => vec![0x81, n as u8],
        _ => vec![0x82, (n >> 8) as u8, n as u8],
    }
}

fn der(tag: u8, body: &[u8]) -> Vec<u8> {
    let mut v = vec![tag];
    v.extend(der_len(body.len()));
    v.extend_from_slice(body);
    v
}

/// The first message, in a SPNEGO NegTokenInit offering only NTLM.
pub fn negotiate() -> Vec<u8> {
    let mut m = b"NTLMSSP\0".to_vec();
    m.extend(1u32.to_le_bytes());
    m.extend(FLAGS.to_le_bytes());
    m.extend([0u8; 16]); // no domain or workstation
    let mech_types = der(0xa0, &der(0x30, &der(0x06, NTLMSSP)));
    let token = der(0xa2, &der(0x04, &m));
    let init = der(0xa0, &der(0x30, &[mech_types, token].concat()));
    der(0x60, &[der(0x06, SPNEGO), init].concat())
}

/// The NTLM message inside a SPNEGO token (or a bare one).
pub fn unwrap(blob: &[u8]) -> Option<&[u8]> {
    let at = blob.windows(8).position(|w| w == b"NTLMSSP\0")?;
    Some(&blob[at..])
}

pub struct Auth {
    /// The third message, wrapped in a SPNEGO NegTokenResp.
    pub blob: Vec<u8>,
    /// What both ends sign with; None for an anonymous sign-in.
    pub key: Option<[u8; 16]>,
}

fn field(m: &[u8], at: usize) -> Option<&[u8]> {
    let len = u16::from_le_bytes(m.get(at..at + 2)?.try_into().ok()?) as usize;
    let off = u32::from_le_bytes(m.get(at + 4..at + 8)?.try_into().ok()?) as usize;
    m.get(off..off + len)
}

/// The timestamp a server put in its target info (MsvAvTimestamp).
fn av_timestamp(info: &[u8]) -> Option<u64> {
    let mut i = 0;
    while i + 4 <= info.len() {
        let id = u16::from_le_bytes([info[i], info[i + 1]]);
        let len = u16::from_le_bytes([info[i + 2], info[i + 3]]) as usize;
        let v = info.get(i + 4..i + 4 + len)?;
        match id {
            0 => return None,
            7 if len == 8 => return Some(u64::from_le_bytes(v.try_into().ok()?)),
            _ => i += 4 + len,
        }
    }
    None
}

/// Now as a Windows FILETIME.
fn filetime_now() -> u64 {
    let unix = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    (unix.as_secs() + 11_644_473_600) * 10_000_000 + unix.subsec_nanos() as u64 / 100
}

/// The NTLMv2 responses: (NT response, LM response, session base key).
fn responses(c: &Creds, server: &[u8; 8], client: &[u8; 8], time: u64, info: &[u8], lm_zero: bool) -> (Vec<u8>, Vec<u8>, [u8; 16]) {
    let key = ntowfv2(c);
    let mut temp = vec![1, 1, 0, 0, 0, 0, 0, 0];
    temp.extend(time.to_le_bytes());
    temp.extend(client);
    temp.extend([0; 4]);
    temp.extend(info);
    temp.extend([0; 4]);
    let proof = hmac_md5(&key, &[server, &temp]);
    let session = hmac_md5(&key, &[&proof]);
    let lm = if lm_zero { vec![0; 24] } else { [&hmac_md5(&key, &[server, client])[..], client].concat() };
    ([&proof[..], &temp].concat(), lm, session)
}

/// The third message, answering the server's challenge (in its SPNEGO token).
pub fn authenticate(blob: &[u8], c: &Creds) -> Result<Auth, String> {
    let m = unwrap(blob).ok_or("the server didn't offer NTLM sign-in")?;
    if m.len() < 32 || m[8..12] != 2u32.to_le_bytes() {
        return Err("the server's sign-in challenge is malformed".into());
    }
    let server_flags = u32::from_le_bytes(m[20..24].try_into().unwrap());
    let challenge: [u8; 8] = m[24..32].try_into().unwrap();
    let info = field(m, 40).unwrap_or(&[]);
    let mut flags = FLAGS & server_flags | UNICODE;
    let (nt, lm, key) = if c.anonymous() {
        flags |= ANONYMOUS;
        (Vec::new(), vec![0], None)
    } else {
        let mut client = [0u8; 8];
        getrandom::fill(&mut client).map_err(|e| e.to_string())?;
        let stamp = av_timestamp(info);
        let (nt, lm, key) = responses(c, &challenge, &client, stamp.unwrap_or_else(filetime_now), info, stamp.is_some());
        (nt, lm, Some(key))
    };
    let (domain, user) = (utf16(&c.domain), utf16(&c.user));
    let mut out = b"NTLMSSP\0".to_vec();
    out.extend(3u32.to_le_bytes());
    let mut payload = Vec::new();
    for part in [&lm, &nt, &domain, &user, &Vec::new(), &Vec::new()] {
        let off = 64 + payload.len();
        out.extend((part.len() as u16).to_le_bytes());
        out.extend((part.len() as u16).to_le_bytes());
        out.extend((off as u32).to_le_bytes());
        payload.extend_from_slice(part);
    }
    out.extend(flags.to_le_bytes());
    out.extend(payload);
    let resp = der(0xa1, &der(0x30, &der(0xa2, &der(0x04, &out))));
    Ok(Auth { blob: resp, key })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> Vec<u8> {
        let s: String = s.split_whitespace().collect();
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn hmac_md5_rfc2202() {
        assert_eq!(hmac_md5(b"Jefe", &[b"what do ya want ", b"for nothing?"]).to_vec(), hex("750c783e6ab0b503eaa86e310a5db738"));
    }

    // MS-NLMP 4.2.4, NTLMv2 authentication
    #[test]
    fn ntlmv2_vectors() {
        let c = Creds { domain: "Domain".into(), user: "User".into(), password: "Password".into() };
        assert_eq!(ntowfv2(&c).to_vec(), hex("0c868a403bfd7a93a3001ef22ef02e3f"));
        let server: [u8; 8] = hex("0123456789abcdef").try_into().unwrap();
        let client = [0xaa; 8];
        let info = hex("02000c0044006f006d00610069006e0001000c005300650072007600650072000000 0000");
        let (nt, lm, key) = responses(&c, &server, &client, 0, &info, false);
        assert_eq!(nt[..16].to_vec(), hex("68cd0ab851e51c96aabc927bebef6a1c"));
        assert_eq!(key.to_vec(), hex("8de40ccadbc14a82f15cb0ad0de95ca3"));
        assert_eq!(lm, hex("86c35097ac9cec102554764a57cccc19aaaaaaaaaaaaaaaa"));
    }

    #[test]
    fn users_with_a_domain() {
        let c = Creds::new("WORK\\ann", "pw");
        assert_eq!((c.domain.as_str(), c.user.as_str()), ("WORK", "ann"));
        assert_eq!(Creds::new("ann", "").domain, "");
    }

    #[test]
    fn spnego_holds_the_message() {
        let n = negotiate();
        assert_eq!(n[0], 0x60);
        assert_eq!(&unwrap(&n).unwrap()[8..12], &1u32.to_le_bytes());
    }
}
