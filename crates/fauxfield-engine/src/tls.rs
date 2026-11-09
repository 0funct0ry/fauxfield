//! HTTPS (SPEC §5.8): the local development CA (rcgen), leaf certificates per server, `files`
//! mode, expiry checks, and the platform commands that trust the CA. The engine never edits a
//! trust store itself: it only *prints* the command; `run_command` is for callers that have
//! asked the user to confirm.

use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use fauxfield_model::Server;
use rcgen::{
    BasicConstraints, CertificateParams, DistinguishedName, DnType, IsCa, KeyPair, KeyUsagePurpose,
    SanType, date_time_ymd,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::TcpStream;

pub const CA_NAME: &str = "Fauxfield Dev CA";
/// Warn this many days before a certificate expires (SPEC §5.8).
pub const WARN_DAYS: i64 = 14;
const RENEW_DAYS: i64 = 30;

/// A server connection: plain TCP or TLS over TCP.
pub enum Conn {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::server::TlsStream<TcpStream>>),
}

impl Conn {
    pub fn tcp(&self) -> &TcpStream {
        match self {
            Conn::Plain(s) => s,
            Conn::Tls(t) => t.get_ref().0,
        }
    }
}

impl AsyncRead for Conn {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_read(cx, buf),
            Conn::Tls(s) => Pin::new(s).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for Conn {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_write(cx, buf),
            Conn::Tls(s) => Pin::new(s).poll_write(cx, buf),
        }
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_flush(cx),
            Conn::Tls(s) => Pin::new(s).poll_flush(cx),
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Conn::Plain(s) => Pin::new(s).poll_shutdown(cx),
            Conn::Tls(s) => Pin::new(s).poll_shutdown(cx),
        }
    }
}

/// Platform data dir (`~/.local/share/fauxfield` on Linux; `FAUXFIELD_DATA_DIR` overrides it),
/// where `ca/` lives.
pub fn default_data_dir() -> Option<PathBuf> {
    if let Some(d) = std::env::var_os("FAUXFIELD_DATA_DIR").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    directories::ProjectDirs::from("dev", "fauxfield", "fauxfield")
        .map(|d| d.data_dir().to_path_buf())
}

pub fn ca_dir(data_dir: &Path) -> PathBuf {
    data_dir.join("ca")
}
pub fn ca_cert_path(data_dir: &Path) -> PathBuf {
    ca_dir(data_dir).join("ca.pem")
}

fn ca_params() -> CertificateParams {
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, CA_NAME);
    dn.push(DnType::OrganizationName, "Fauxfield");
    p.distinguished_name = dn;
    p.is_ca = IsCa::Ca(BasicConstraints::Unconstrained);
    p.key_usages = vec![KeyUsagePurpose::KeyCertSign, KeyUsagePurpose::CrlSign];
    p.not_before = date_time_ymd(2024, 1, 1);
    p.not_after = date_time_ymd(2044, 1, 1);
    p
}

fn write_private(path: &Path, data: &str) -> io::Result<()> {
    std::fs::write(path, data)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}

pub struct DevCa {
    pub cert_pem: String,
    /// Same subject and key as the stored CA, re-signed in memory so it can issue leaves.
    cert: rcgen::Certificate,
    key: KeyPair,
}

/// Load the dev CA from `<data>/ca/`, creating it on first use.
pub fn load_or_create_ca(data_dir: &Path) -> Result<DevCa, String> {
    let dir = ca_dir(data_dir);
    let (cp, kp) = (dir.join("ca.pem"), dir.join("ca-key.pem"));
    if let (Ok(c), Ok(k)) = (std::fs::read_to_string(&cp), std::fs::read_to_string(&kp)) {
        let key = KeyPair::from_pem(&k).map_err(|e| format!("dev CA key: {e}"))?;
        let cert = ca_params().self_signed(&key).map_err(|e| e.to_string())?;
        return Ok(DevCa {
            cert_pem: c,
            cert,
            key,
        });
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let key = KeyPair::generate().map_err(|e| e.to_string())?;
    let cert = ca_params().self_signed(&key).map_err(|e| e.to_string())?;
    std::fs::write(&cp, cert.pem()).map_err(|e| e.to_string())?;
    write_private(&kp, &key.serialize_pem()).map_err(|e| e.to_string())?;
    Ok(DevCa {
        cert_pem: cert.pem(),
        cert,
        key,
    })
}

fn der_of(pem: &str) -> Option<Vec<u8>> {
    rustls_pemfile::certs(&mut pem.as_bytes())
        .next()?
        .ok()
        .map(|d| d.as_ref().to_vec())
}

/// Seconds-since-epoch of `notAfter` for the first certificate in a PEM.
fn not_after(pem: &str) -> Option<i64> {
    let der = der_of(pem)?;
    let (_, c) = x509_parser::parse_x509_certificate(&der).ok()?;
    Some(c.validity().not_after.timestamp())
}

fn subject_of(pem: &str) -> Option<String> {
    let der = der_of(pem)?;
    let (_, c) = x509_parser::parse_x509_certificate(&der).ok()?;
    Some(c.subject().to_string())
}

fn now_s() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

fn days_left(na: i64) -> i64 {
    (na - now_s()).div_euclid(86_400)
}

fn fingerprint(pem: &str) -> Option<String> {
    let h = Sha256::digest(der_of(pem)?);
    Some(
        h.iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(":"),
    )
}

/// SANs for a Server: `localhost`, loopback addresses and the Server host.
pub fn sans_for(host: &str) -> Vec<String> {
    let mut v: Vec<String> = ["localhost", "127.0.0.1", "::1"].map(String::from).to_vec();
    let h = host.trim().trim_matches(['[', ']']);
    if !h.is_empty() && h != "0.0.0.0" && h != "::" && !v.iter().any(|x| x == h) {
        v.push(h.to_string());
    }
    v
}

fn san_types(names: &[String]) -> Result<Vec<SanType>, String> {
    names
        .iter()
        .map(|n| match n.parse::<std::net::IpAddr>() {
            Ok(ip) => Ok(SanType::IpAddress(ip)),
            Err(_) => Ok(SanType::DnsName(
                n.as_str()
                    .try_into()
                    .map_err(|e| format!("bad host `{n}`: {e}"))?,
            )),
        })
        .collect()
}

/// PEM cert chain and key for a Server in `dev-ca` mode (cached under `ca/leaves/`).
pub fn leaf_pems(data_dir: &Path, ca: &DevCa, host: &str) -> Result<(String, String), String> {
    let names = sans_for(host);
    let tag = {
        let mut h = Sha256::new();
        h.update(ca.cert_pem.as_bytes());
        h.update(names.join(",").as_bytes());
        h.finalize()
            .iter()
            .take(8)
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let dir = ca_dir(data_dir).join("leaves");
    let (cp, kp) = (
        dir.join(format!("{tag}.pem")),
        dir.join(format!("{tag}-key.pem")),
    );
    if let (Ok(c), Ok(k)) = (std::fs::read_to_string(&cp), std::fs::read_to_string(&kp))
        && not_after(&c).is_some_and(|na| days_left(na) > RENEW_DAYS)
    {
        return Ok((c, k));
    }
    let key = KeyPair::generate().map_err(|e| e.to_string())?;
    let mut p = CertificateParams::default();
    let mut dn = DistinguishedName::new();
    dn.push(DnType::CommonName, "Fauxfield dev server");
    p.distinguished_name = dn;
    p.subject_alt_names = san_types(&names)?;
    p.key_usages = vec![KeyUsagePurpose::DigitalSignature];
    p.not_before = date_time_ymd(2024, 1, 1);
    let end = time::OffsetDateTime::now_utc() + time::Duration::days(397);
    p.not_after = end;
    let cert = p
        .signed_by(&key, &ca.cert, &ca.key)
        .map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let pem = format!("{}{}", cert.pem(), ca.cert_pem);
    std::fs::write(&cp, &pem).map_err(|e| e.to_string())?;
    write_private(&kp, &key.serialize_pem()).map_err(|e| e.to_string())?;
    Ok((pem, key.serialize_pem()))
}

/// Certificate details shown in the TLS section, `certs status` and `doctor`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CertStatus {
    pub mode: String,
    pub subject: String,
    pub not_after: i64,
    pub days_left: i64,
    /// Set when expired or within 14 days of expiry.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

fn status_of(mode: &str, pem: &str) -> Option<CertStatus> {
    let na = not_after(pem)?;
    let d = days_left(na);
    Some(CertStatus {
        mode: mode.into(),
        subject: subject_of(pem).unwrap_or_default(),
        not_after: na,
        days_left: d,
        warning: if d < 0 {
            Some(format!("certificate expired {} days ago", -d))
        } else if d <= WARN_DAYS {
            Some(format!("certificate expires in {d} days"))
        } else {
            None
        },
    })
}

/// A ready TLS acceptor plus the certificate's status.
pub struct Tls {
    pub acceptor: tokio_rustls::TlsAcceptor,
    pub status: Option<CertStatus>,
}

/// Build the acceptor for a Server (`dev-ca` or `files`). Errors are shown as the Failed status.
pub fn build(
    server: &Server,
    root: &Path,
    data_dir: Option<&Path>,
    host: &str,
) -> Result<Tls, String> {
    let t = server.tls.as_ref().ok_or("tls is not configured")?;
    let (chain_pem, key_pem, mode) = if t.mode_name() == "files" {
        let read = |what: &str, p: &Option<String>| -> Result<String, String> {
            let p = p
                .as_deref()
                .filter(|p| !p.is_empty())
                .ok_or(format!("tls `{what}` is not set"))?;
            crate::engine::read_confined(root, p)
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .map_err(|e| format!("tls {what} `{p}`: {e}"))
        };
        (read("cert", &t.cert)?, read("key", &t.key)?, "files")
    } else {
        let dd = data_dir
            .map(Path::to_path_buf)
            .or_else(default_data_dir)
            .ok_or("no platform data directory for the dev CA")?;
        let ca = load_or_create_ca(&dd)?;
        let (c, k) = leaf_pems(&dd, &ca, host)?;
        (c, k, "dev-ca")
    };
    let certs: Vec<_> = rustls_pemfile::certs(&mut chain_pem.as_bytes())
        .collect::<Result<_, _>>()
        .map_err(|e| format!("certificate: {e}"))?;
    if certs.is_empty() {
        return Err("no certificate found in the PEM file".into());
    }
    let key = rustls_pemfile::private_key(&mut key_pem.as_bytes())
        .map_err(|e| format!("private key: {e}"))?
        .ok_or("no private key found in the PEM file")?;
    let mut cfg = rustls::ServerConfig::builder_with_provider(
        rustls::crypto::ring::default_provider().into(),
    )
    .with_safe_default_protocol_versions()
    .map_err(|e| e.to_string())?
    .with_no_client_auth()
    .with_single_cert(certs, key)
    .map_err(|e| format!("certificate and key do not match: {e}"))?;
    cfg.alpn_protocols = vec![b"http/1.1".to_vec()];
    Ok(Tls {
        acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(cfg)),
        status: status_of(mode, &chain_pem),
    })
}

// ----- CA status and trust -----

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TrustState {
    Trusted,
    Untrusted,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct CaStatus {
    pub path: String,
    pub exists: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fingerprint: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub not_after: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub days_left: Option<i64>,
    pub trust: TrustState,
    /// The exact command `certs trust` prints.
    pub trust_command: String,
    pub untrust_command: String,
}

const LINUX_DEBIAN: &str = "/usr/local/share/ca-certificates/fauxfield-dev-ca.crt";
const LINUX_FEDORA: &str = "/etc/pki/ca-trust/source/anchors/fauxfield-dev-ca.pem";

fn q(p: &Path) -> String {
    format!("\"{}\"", p.display())
}

/// The platform command that trusts the CA (printed, never run silently).
pub fn trust_command(ca_pem: &Path) -> String {
    if cfg!(target_os = "macos") {
        format!(
            "sudo security add-trusted-cert -d -r trustRoot -k /Library/Keychains/System.keychain {}",
            q(ca_pem)
        )
    } else if cfg!(windows) {
        format!("certutil -user -addstore Root {}", q(ca_pem))
    } else if Path::new("/etc/pki/ca-trust").exists() {
        format!(
            "sudo cp {} {LINUX_FEDORA} && sudo update-ca-trust",
            q(ca_pem)
        )
    } else {
        format!(
            "sudo cp {} {LINUX_DEBIAN} && sudo update-ca-certificates",
            q(ca_pem)
        )
    }
}

pub fn untrust_command(ca_pem: &Path) -> String {
    if cfg!(target_os = "macos") {
        format!("sudo security remove-trusted-cert -d {}", q(ca_pem))
    } else if cfg!(windows) {
        format!("certutil -user -delstore Root \"{CA_NAME}\"")
    } else if Path::new("/etc/pki/ca-trust").exists() {
        format!("sudo rm -f {LINUX_FEDORA} && sudo update-ca-trust")
    } else {
        format!("sudo rm -f {LINUX_DEBIAN} && sudo update-ca-certificates --fresh")
    }
}

fn silent(cmd: &str, args: &[&str]) -> Option<bool> {
    std::process::Command::new(cmd)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()
        .map(|s| s.success())
}

pub fn trust_state(ca_pem: &Path) -> TrustState {
    if !ca_pem.exists() {
        return TrustState::Untrusted;
    }
    let ok = if cfg!(target_os = "macos") {
        silent(
            "security",
            &[
                "verify-cert",
                "-c",
                &ca_pem.to_string_lossy(),
                "-p",
                "basic",
            ],
        )
    } else if cfg!(windows) {
        silent("certutil", &["-user", "-store", "Root", CA_NAME])
    } else {
        Some(Path::new(LINUX_DEBIAN).exists() || Path::new(LINUX_FEDORA).exists())
    };
    match ok {
        Some(true) => TrustState::Trusted,
        Some(false) => TrustState::Untrusted,
        None => TrustState::Unknown,
    }
}

pub fn ca_status(data_dir: &Path) -> CaStatus {
    let path = ca_cert_path(data_dir);
    let pem = std::fs::read_to_string(&path).ok();
    let na = pem.as_deref().and_then(not_after);
    CaStatus {
        path: path.display().to_string(),
        exists: pem.is_some(),
        fingerprint: pem.as_deref().and_then(fingerprint),
        not_after: na,
        days_left: na.map(days_left),
        trust: trust_state(&path),
        trust_command: trust_command(&path),
        untrust_command: untrust_command(&path),
    }
}

/// The form of a trust command a GUI can run: no terminal means no `sudo` password prompt, so
/// macOS lets `security` ask through its own dialog and Linux asks through polkit (`pkexec`).
/// The GUI shows exactly this text before running it.
pub fn gui_form(cmd: &str) -> String {
    if cfg!(windows) {
        cmd.to_string()
    } else if cfg!(target_os = "macos") {
        cmd.replace("sudo ", "")
    } else {
        format!(
            "pkexec sh -c '{}'",
            cmd.replace("sudo ", "").replace('\'', "'\\''")
        )
    }
}

/// Run a trust/untrust command through the platform shell. Call only after the user confirmed
/// the exact command text.
pub fn run_command(cmd: &str) -> Result<(), String> {
    let status = if cfg!(windows) {
        std::process::Command::new("cmd").args(["/C", cmd]).status()
    } else {
        std::process::Command::new("sh").args(["-c", cmd]).status()
    }
    .map_err(|e| e.to_string())?;
    status
        .success()
        .then_some(())
        .ok_or_else(|| format!("command exited with {status}"))
}

/// Client config trusting the dev CA (for Send test and tests against our own listener).
pub(crate) fn client_config(data_dir: Option<&Path>) -> Arc<rustls::ClientConfig> {
    let mut roots = rustls::RootCertStore::empty();
    if let Some(dd) = data_dir.map(Path::to_path_buf).or_else(default_data_dir)
        && let Ok(pem) = std::fs::read_to_string(ca_cert_path(&dd))
    {
        for c in rustls_pemfile::certs(&mut pem.as_bytes()).flatten() {
            let _ = roots.add(c);
        }
    }
    Arc::new(
        rustls::ClientConfig::builder_with_provider(
            rustls::crypto::ring::default_provider().into(),
        )
        .with_safe_default_protocol_versions()
        .expect("protocol versions")
        .with_root_certificates(roots)
        .with_no_client_auth(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ca_and_leaf_are_created_and_reused() {
        let d = tempfile::tempdir().unwrap();
        let ca = load_or_create_ca(d.path()).unwrap();
        let ca2 = load_or_create_ca(d.path()).unwrap();
        assert_eq!(ca.cert_pem, ca2.cert_pem);
        let (c, k) = leaf_pems(d.path(), &ca, "api.test").unwrap();
        assert!(k.contains("PRIVATE KEY"));
        assert_eq!(leaf_pems(d.path(), &ca, "api.test").unwrap().0, c);
        let st = status_of("dev-ca", &c).unwrap();
        assert!(st.days_left > 300 && st.warning.is_none());
        let s = ca_status(d.path());
        assert!(s.exists && s.fingerprint.is_some() && s.days_left.unwrap() > 3000);
        assert!(s.trust_command.contains("ca.pem"));
    }

    #[test]
    fn sans() {
        assert_eq!(sans_for("0.0.0.0"), ["localhost", "127.0.0.1", "::1"]);
        assert!(sans_for("api.test").contains(&"api.test".to_string()));
    }
}
