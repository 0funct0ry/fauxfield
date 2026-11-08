//! Transport selection, configuration merging and the unix-socket lifecycle (SPEC §11.1, §13).

use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
};

use serde::Deserialize;

use crate::class::AllowedClasses;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Stdio,
    Unix,
    Both,
    Http,
}

impl TransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Unix => "unix",
            Self::Both => "both",
            Self::Http => "http",
        }
    }
    pub fn uses_stdio(self) -> bool {
        matches!(self, Self::Stdio | Self::Both)
    }
    pub fn uses_unix(self) -> bool {
        matches!(self, Self::Unix | Self::Both)
    }
}

impl fmt::Display for TransportKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for TransportKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "stdio" => Ok(Self::Stdio),
            "unix" => Ok(Self::Unix),
            "both" => Ok(Self::Both),
            "http" => Ok(Self::Http),
            o => Err(format!(
                "unknown transport `{o}` (expected stdio, unix, both or http)"
            )),
        }
    }
}

/// Resolved server configuration: settings.toml `[mcp]` under flags.
#[derive(Debug, Clone)]
pub struct McpConfig {
    pub transport: TransportKind,
    pub socket: Option<PathBuf>,
    pub allow: AllowedClasses,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            transport: TransportKind::Stdio,
            socket: None,
            allow: AllowedClasses::all(),
        }
    }
}

/// The `[mcp]` table of settings.toml (SPEC §13). Every key is optional.
#[derive(Debug, Default, Deserialize)]
pub struct McpSettings {
    pub transport: Option<TransportKind>,
    pub allow: Option<Vec<String>>,
    pub socket: Option<PathBuf>,
}

#[derive(Deserialize, Default)]
struct SettingsFile {
    #[serde(default)]
    mcp: McpSettings,
}

impl McpSettings {
    /// Parse the `[mcp]` table out of a settings.toml document.
    pub fn from_toml(text: &str) -> Result<Self, String> {
        toml::from_str::<SettingsFile>(text)
            .map(|f| f.mcp)
            .map_err(|e| format!("invalid settings.toml: {e}"))
    }

    /// Load `<config dir>/fauxfield/settings.toml`; a missing file means defaults.
    pub fn load_default() -> Result<Self, String> {
        let Some(path) = settings_path() else {
            return Ok(Self::default());
        };
        match std::fs::read_to_string(&path) {
            Ok(t) => Self::from_toml(&t).map_err(|e| format!("{}: {e}", path.display())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(format!("{}: {e}", path.display())),
        }
    }

    /// Settings first, then flags win.
    pub fn resolve(
        self,
        transport: Option<TransportKind>,
        socket: Option<PathBuf>,
        allow: Option<AllowedClasses>,
    ) -> Result<McpConfig, String> {
        let from_settings = match self.allow {
            Some(list) => Some(AllowedClasses::parse_list(&list.join(","))?),
            None => None,
        };
        Ok(McpConfig {
            transport: transport.or(self.transport).unwrap_or(TransportKind::Stdio),
            socket: socket.or(self.socket),
            allow: allow.or(from_settings).unwrap_or_default(),
        })
    }
}

fn settings_path() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("FAUXFIELD_SETTINGS") {
        return Some(PathBuf::from(p));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
    }?;
    Some(base.join("fauxfield").join("settings.toml"))
}

/// `$XDG_RUNTIME_DIR/fauxfield/mcp-<pid>.sock`; elsewhere the per-user temp dir
/// (on macOS `$TMPDIR` is already per-user).
pub fn default_socket_path() -> PathBuf {
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("fauxfield")
        .join(format!("mcp-{}.sock", std::process::id()))
}

#[cfg(unix)]
pub use unix::{SocketGuard, bind_unix};

#[cfg(unix)]
mod unix {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
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
    pub async fn bind_unix(path: &Path) -> Result<(UnixListener, SocketGuard), String> {
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
                        "{} is in use by a running Fauxfield MCP server; \
                         choose another path with --socket",
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags_win_over_settings() {
        let s = McpSettings::from_toml(
            "[mcp]\ntransport = \"unix\"\nallow = [\"read\"]\nsocket = \"/a.sock\"\n",
        )
        .unwrap();
        let c = s.resolve(Some(TransportKind::Both), None, None).unwrap();
        assert_eq!(c.transport, TransportKind::Both);
        assert_eq!(c.socket.as_deref(), Some(Path::new("/a.sock")));
        assert_eq!(c.allow, AllowedClasses::read_only());
    }

    #[test]
    fn defaults_and_bad_values() {
        let c = McpSettings::default().resolve(None, None, None).unwrap();
        assert_eq!(c.transport, TransportKind::Stdio);
        assert_eq!(c.allow, AllowedClasses::all());
        assert!(McpSettings::from_toml("[mcp]\ntransport = \"tcp\"").is_err());
        assert!(
            McpSettings::from_toml("[mcp]\nallow = [\"nope\"]")
                .unwrap()
                .resolve(None, None, None)
                .is_err()
        );
        assert!("http".parse::<TransportKind>().is_ok());
    }
}
