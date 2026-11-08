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

/// Placeholder so the host struct is the same on every platform: a pipe vanishes with its
/// last handle, so there is nothing to clean up.
#[cfg(not(unix))]
pub struct SocketGuard;

#[cfg(windows)]
pub use windows::{bind_pipe, next_pipe_instance};

/// Windows named pipes (SPEC §14, §20.13). No SECURITY_ATTRIBUTES (the workspace forbids
/// unsafe): the unguessable random suffix in the pipe name is the capability, the first
/// instance cannot be pre-created by a squatter, and remote clients are rejected.
#[cfg(windows)]
mod windows {
    use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};

    /// Create the first instance of `name`; fails if the name is already taken.
    pub fn bind_pipe(name: &str) -> Result<NamedPipeServer, String> {
        ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(name)
            .map_err(|e| format!("cannot create pipe {name}: {e}"))
    }

    /// A further instance for the next client.
    pub fn next_pipe_instance(name: &str) -> std::io::Result<NamedPipeServer> {
        ServerOptions::new()
            .reject_remote_clients(true)
            .create(name)
    }
}
