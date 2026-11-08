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

/// `<runtime dir>/fauxfield/ctl-<hash of workspace>.sock`.
pub fn control_socket_path(root: &Path) -> PathBuf {
    let mut h = DefaultHasher::new();
    root.hash(&mut h);
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("fauxfield")
        .join(format!("ctl-{:016x}.sock", h.finish()))
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

#[cfg(not(unix))]
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
        if let Some(live) = read_host(root) {
            return Err(live);
        }
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
        let _ = std::fs::write(&path, text);
        Ok(HostLock { path })
    }
}

impl Drop for HostLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}
