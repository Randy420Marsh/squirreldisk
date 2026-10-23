//! The SSH client on Android, which has no ssh program: the app runs its own
//! headless executable as `--ssh [user@]host[:port] command` where it would
//! run `ssh host command` (see `scan::remote::ssh_command`). Like ssh, it
//! passes stdin to the command, its output back, and exits with its status.
//!
//! Signs in with the phone's own key (made on first use, in the app's
//! private folder) and then with the password the app was given, if any.
//! Host keys are trusted the first time and must not change after.

use russh::client::{self, Handle};
use russh::keys::{self, known_hosts, PrivateKey, PrivateKeyWithHashAlg, PublicKeyOrCertificate};
use russh::ChannelMsg;
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn ssh_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("HOME").unwrap_or_else(|| ".".into())).join(".ssh")
}

pub fn main(args: &[String]) -> i32 {
    let (Some(target), Some(command)) = (args.first(), args.get(1)) else {
        eprintln!("usage: squirreldisk --ssh [user@]host[:port] command");
        return 255;
    };
    let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("ssh: {e}");
            return 255;
        }
    };
    match rt.block_on(run(target, command)) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            255
        }
    }
}

/// `user@host`, `user@host:port` or `user@[v6 address]:port`, maybe after
/// `ssh://`.
fn parse_target(target: &str) -> Result<(String, String, u16), String> {
    let target = target.strip_prefix("ssh://").unwrap_or(target);
    let (user, rest) = target.rsplit_once('@').ok_or_else(|| format!("{target}: give the user name too, as in user@{target}"))?;
    let (host, port) = if let Some(v6) = rest.strip_prefix('[') {
        let (h, after) = v6.split_once(']').ok_or("missing ] in the address")?;
        (h, after.strip_prefix(':'))
    } else {
        match rest.split_once(':') {
            Some((h, p)) => (h, Some(p)),
            None => (rest, None),
        }
    };
    let port = match port {
        Some(p) => p.parse().map_err(|_| format!("{p}: not a port number"))?,
        None => 22,
    };
    if user.is_empty() || host.is_empty() {
        return Err(format!("{target}: not a host"));
    }
    Ok((user.to_string(), host.to_string(), port))
}

/// Checks host keys against the app's own known_hosts; a key that changed
/// is refused (`UnknownKey`).
struct Known {
    host: String,
    port: u16,
}

impl client::Handler for Known {
    type Error = russh::Error;

    async fn check_server_key(&mut self, key: &PublicKeyOrCertificate) -> Result<bool, Self::Error> {
        let PublicKeyOrCertificate::PublicKey { key, .. } = key else { return Ok(false) };
        let path = ssh_dir().join("known_hosts");
        match known_hosts::check_known_hosts_path(&self.host, self.port, key, &path) {
            Ok(true) => Ok(true),
            Ok(false) => Ok(known_hosts::learn_known_hosts_path(&self.host, self.port, key, &path).is_ok()),
            Err(_) => Ok(false),
        }
    }
}

/// The phone's own key, made the first time.
pub fn own_key() -> Result<PrivateKey, String> {
    let path = ssh_dir().join("id_ed25519");
    if let Ok(key) = keys::load_secret_key(&path, None) {
        return Ok(key);
    }
    let mut seed = [0u8; 32];
    std::fs::File::open("/dev/urandom").and_then(|mut f| std::io::Read::read_exact(&mut f, &mut seed)).map_err(|e| e.to_string())?;
    let mut key = PrivateKey::from(keys::ssh_key::private::Ed25519Keypair::from_seed(&seed));
    key.set_comment("squirreldisk-android");
    let text = key.to_openssh(keys::ssh_key::LineEnding::LF).map_err(|e| e.to_string())?;
    std::fs::DirBuilder::new().recursive(true).mode(0o700).create(ssh_dir()).map_err(|e| e.to_string())?;
    let mut f = std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path).map_err(|e| e.to_string())?;
    f.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
    Ok(key)
}

async fn sign_in(session: &mut Handle<Known>, user: &str, secret: Option<&str>) -> Result<bool, String> {
    let rsa_hash = session.best_supported_rsa_hash().await.ok().flatten().flatten();
    let mut keys: Vec<PrivateKey> = Vec::new();
    if let Ok(key) = own_key() {
        keys.push(key);
    }
    // keys copied in by hand; with a passphrase, it is the secret given
    for name in ["id_ecdsa", "id_rsa"] {
        if let Ok(key) = keys::load_secret_key(ssh_dir().join(name), secret) {
            keys.push(key);
        }
    }
    for key in keys {
        let key = PrivateKeyWithHashAlg::new(Arc::new(key), rsa_hash);
        if session.authenticate_publickey(user, key).await.map_err(|e| e.to_string())?.success() {
            return Ok(false);
        }
    }
    if let Some(password) = secret {
        if session.authenticate_password(user, password).await.map_err(|e| e.to_string())?.success() {
            return Ok(true);
        }
    }
    Err("Permission denied (publickey,password).".into())
}

/// Runs `command` on its own channel and passes its output on; returns its
/// exit status.
async fn exec(session: &Handle<Known>, command: &str, stdin: bool) -> Result<i32, String> {
    let channel = session.channel_open_session().await.map_err(|e| e.to_string())?;
    channel.exec(true, command).await.map_err(|e| e.to_string())?;
    let (mut read, write) = channel.split();
    if stdin {
        tokio::spawn(async move {
            let _ = write.data(tokio::io::stdin()).await;
            let _ = write.eof().await;
        });
    } else {
        let _ = write.eof().await;
    }
    let mut code = 255;
    let (mut out, mut err) = (std::io::stdout().lock(), std::io::stderr().lock());
    while let Some(msg) = read.wait().await {
        match msg {
            ChannelMsg::Data { data } => out.write_all(&data).map_err(|e| e.to_string())?,
            ChannelMsg::ExtendedData { data, .. } => {
                let _ = err.write_all(&data);
            }
            ChannelMsg::ExitStatus { exit_status } => code = exit_status as i32,
            _ => {}
        }
    }
    out.flush().map_err(|e| e.to_string())?;
    Ok(code)
}

async fn run(target: &str, command: &str) -> Result<i32, String> {
    let (user, host, port) = parse_target(target)?;
    let config = Arc::new(client::Config { keepalive_interval: Some(Duration::from_secs(15)), ..Default::default() });
    let known = Known { host: host.clone(), port };
    let connect = client::connect(config, (host.as_str(), port), known);
    let mut session = match tokio::time::timeout(Duration::from_secs(12), connect).await {
        Err(_) => return Err(format!("ssh: connect to host {host} port {port}: Connection timed out")),
        Ok(Err(russh::Error::UnknownKey)) => {
            return Err(format!("The host key of {host} has changed since the last time. It could be reinstalled, or someone in between: not connecting."))
        }
        Ok(Err(e)) => return Err(format!("ssh: {host}: {e}")),
        Ok(Ok(s)) => s,
    };
    let secret = std::env::var(crate::scan::remote::ASKPASS_ENV).ok();
    let with_password = sign_in(&mut session, &user, secret.as_deref()).await?;
    if with_password && std::env::var_os(crate::scan::remote::INSTALL_KEY_ENV).is_some() {
        if let Ok(public) = own_key().and_then(|k| k.public_key().to_openssh().map_err(|e| e.to_string())) {
            let q = crate::scan::remote::sh_quote(&public);
            let script = format!(
                "umask 077; mkdir -p ~/.ssh && {{ grep -qxF {q} ~/.ssh/authorized_keys 2>/dev/null || echo {q} >> ~/.ssh/authorized_keys; }}"
            );
            let _ = exec(&session, &script, false).await;
        }
    }
    let code = exec(&session, command, true).await?;
    let _ = session.disconnect(russh::Disconnect::ByApplication, "", "en").await;
    Ok(code)
}

#[cfg(test)]
mod tests {
    use super::parse_target;

    #[test]
    fn targets() {
        assert_eq!(parse_target("pi@raspberrypi.local").unwrap(), ("pi".into(), "raspberrypi.local".into(), 22));
        assert_eq!(parse_target("root@10.0.0.7:2222").unwrap(), ("root".into(), "10.0.0.7".into(), 2222));
        assert_eq!(parse_target("me@[fe80::1]:22").unwrap(), ("me".into(), "fe80::1".into(), 22));
        assert_eq!(parse_target("ssh://ann@192.0.2.10:222").unwrap(), ("ann".into(), "192.0.2.10".into(), 222));
        assert!(parse_target("server").is_err());
        assert!(parse_target("a@b:x").is_err());
    }
}
