use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// `.fauxfield/run/host.json` (SPEC §3.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostInfo {
    pub pid: u32,
    pub version: String,
    pub socket: PathBuf,
    pub started_at: String,
    pub workspace: PathBuf,
}

fn host_path(root: &Path) -> PathBuf {
    root.join(".fauxfield").join("run").join("host.json")
}

fn workspace_hash(root: &Path) -> u64 {
    let mut h = DefaultHasher::new();
    root.hash(&mut h);
    h.finish()
}

/// Unix: `<runtime dir>/fauxfield/ctl-<hash of workspace>.sock`.
#[cfg(unix)]
pub fn control_socket_path(root: &Path) -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("fauxfield")
        .join(format!("ctl-{:016x}.sock", workspace_hash(root)))
}

/// Windows: `\\.\pipe\fauxfield-<workspace hash>-<128-bit random hex>`. The random suffix is
/// the capability: it lives only in `.fauxfield/run/host.json` inside the workspace (SPEC §14).
#[cfg(not(unix))]
pub fn control_socket_path(root: &Path) -> PathBuf {
    let mut b = [0u8; 16];
    getrandom::fill(&mut b).expect("os randomness");
    let hex: String = b.iter().map(|x| format!("{x:02x}")).collect();
    PathBuf::from(format!(
        r"\\.\pipe\fauxfield-{:016x}-{hex}",
        workspace_hash(root)
    ))
}

/// The recorded host if its socket still accepts connections; stale files are ignored.
pub fn read_host(root: &Path) -> Option<HostInfo> {
    let text = std::fs::read_to_string(host_path(root)).ok()?;
    let info: HostInfo = serde_json::from_str(&text).ok()?;
    alive(&info.socket).then_some(info)
}

#[cfg(unix)]
fn alive(socket: &Path) -> bool {
    std::os::unix::net::UnixStream::connect(socket).is_ok()
}

/// A pipe is alive when a client can open it; failure means stale.
#[cfg(windows)]
fn alive(socket: &Path) -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(socket)
        .is_ok()
}

#[cfg(not(any(unix, windows)))]
fn alive(_socket: &Path) -> bool {
    false
}

/// Held by the one host process per workspace; removes `host.json` on drop.
pub struct HostLock {
    path: PathBuf,
}

impl HostLock {
    /// Claim the workspace. Fails with the live host's info when another process hosts it.
    pub fn acquire(root: &Path, socket: &Path, started_at: String) -> Result<HostLock, HostInfo> {
        let info = HostInfo {
            pid: std::process::id(),
            version: env!("CARGO_PKG_VERSION").into(),
            socket: socket.to_path_buf(),
            started_at,
            workspace: root.to_path_buf(),
        };
        let path = host_path(root);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let text = serde_json::to_string_pretty(&info).unwrap_or_default();
        // `create_new` makes the claim atomic: of two racing processes exactly one wins.
        for _ in 0..5 {
            if let Some(live) = read_host(root) {
                return Err(live);
            }
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&path)
            {
                Ok(mut f) => {
                    use std::io::Write;
                    let _ = f.write_all(text.as_bytes());
                    return Ok(HostLock { path });
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                    // Either a live host that is still starting (no socket yet) or a stale file.
                    std::thread::sleep(std::time::Duration::from_millis(40));
                    if read_host(root).is_none() && stale(&path) {
                        let _ = std::fs::remove_file(&path);
                    }
                }
                Err(_) => break,
            }
        }
        if let Some(live) = read_host(root) {
            return Err(live);
        }
        // A host that has claimed the workspace but is still binding its socket.
        if let Some(other) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|t| serde_json::from_str::<HostInfo>(&t).ok())
            && other.pid != std::process::id()
            && !stale(&path)
        {
            return Err(other);
        }
        let _ = std::fs::write(&path, text);
        Ok(HostLock { path })
    }
}

impl Drop for HostLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

/// True when host.json is old enough that its owner is not about to bind a socket.
fn stale(path: &Path) -> bool {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_none_or(|age| age > std::time::Duration::from_secs(2))
}
