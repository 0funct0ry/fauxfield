//! Owner-only local sockets with stale-socket handling, shared by the control socket and the
//! MCP unix transport (SPEC §3.1, §11.1).

#[cfg(unix)]
pub use unix::{SocketGuard, bind_unix};

#[cfg(unix)]
mod unix {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    use std::path::{Path, PathBuf};
    use tokio::net::{UnixListener, UnixStream};

    /// Removes the socket file when dropped (clean exit).
    pub struct SocketGuard(PathBuf);

    impl SocketGuard {
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for SocketGuard {
        fn drop(&mut self) {
            let _ = std::fs::remove_file(&self.0);
        }
    }

    /// Bind at `path`: stale socket files are replaced, live ones refused, mode 0600.
    /// `what` names the owner in the refusal, e.g. "Fauxfield MCP server".
    pub async fn bind_unix(
        path: &Path,
        what: &str,
        hint: &str,
    ) -> Result<(UnixListener, SocketGuard), String> {
        if let Some(parent) = path.parent()
            && !parent.exists()
        {
            // Owner-only parent closes the window between bind and chmod.
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(parent)
                .map_err(|e| format!("cannot create {}: {e}", parent.display()))?;
        }
        if path.exists() {
            match UnixStream::connect(path).await {
                Ok(_) => {
                    return Err(format!(
                        "{} is in use by a running {what}; {hint}",
                        path.display()
                    ));
                }
                Err(_) => std::fs::remove_file(path)
                    .map_err(|e| format!("cannot remove stale socket {}: {e}", path.display()))?,
            }
        }
        let listener =
            UnixListener::bind(path).map_err(|e| format!("cannot bind {}: {e}", path.display()))?;
        let guard = SocketGuard(path.to_path_buf());
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("cannot chmod {}: {e}", path.display()))?;
        Ok((listener, guard))
    }
}
