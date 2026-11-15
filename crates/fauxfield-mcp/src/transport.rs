//! Transport selection, configuration merging and the unix-socket lifecycle (SPEC §11.1, §13).

use std::{fmt, path::PathBuf, str::FromStr};

use serde::Deserialize;

use crate::class::AllowedClasses;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransportKind {
    Stdio,
    Unix,
    /// Windows named pipe (`\\\\.\\pipe\\fauxfield-mcp-<pid>`).
    Pipe,
    Both,
    Http,
}

impl TransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Unix => "unix",
            Self::Pipe => "pipe",
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
    pub fn uses_pipe(self) -> bool {
        matches!(self, Self::Pipe | Self::Both)
    }
    /// The socket transport `both` means on this platform: a unix socket, or a pipe on Windows.
    pub fn native_socket() -> TransportKind {
        if cfg!(windows) {
            Self::Pipe
        } else {
            Self::Unix
        }
    }
    /// True when this transport listens on the platform's native socket (unix socket or pipe).
    pub fn uses_native_socket(self) -> bool {
        match Self::native_socket() {
            Self::Pipe => self.uses_pipe(),
            _ => self.uses_unix(),
        }
    }
    /// Reject a transport the platform cannot serve, naming the right value.
    pub fn check_platform(self) -> Result<(), String> {
        if cfg!(windows) && self == Self::Unix {
            return Err("the unix transport is not available on Windows; use `--transport pipe` (or `both`)".into());
        }
        if !cfg!(windows) && self == Self::Pipe {
            return Err("the pipe transport is only available on Windows; use `--transport unix` (or `both`)".into());
        }
        Ok(())
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
            "pipe" => Ok(Self::Pipe),
            "both" => Ok(Self::Both),
            "http" => Ok(Self::Http),
            o => Err(format!(
                "unknown transport `{o}` (expected stdio, unix, pipe, both or http)"
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
    /// Port of the `http` transport (random free port when `None`).
    pub port: Option<u16>,
    /// Extra name globs whose variable values are redacted (`[mcp] redact`).
    pub redact: Vec<String>,
    /// Where the `http` transport writes `mcp-http.json` (`<workspace>/.fauxfield/run`).
    pub run_dir: Option<PathBuf>,
}

impl Default for McpConfig {
    fn default() -> Self {
        Self {
            transport: TransportKind::Stdio,
            socket: None,
            allow: AllowedClasses::all(),
            port: None,
            redact: Vec::new(),
            run_dir: None,
        }
    }
}

/// The `[mcp]` table of settings.toml (SPEC §13). Every key is optional.
#[derive(Debug, Default, Deserialize)]
pub struct McpSettings {
    pub transport: Option<TransportKind>,
    pub allow: Option<Vec<String>>,
    pub socket: Option<PathBuf>,
    pub port: Option<u16>,
    #[serde(default)]
    pub redact: Vec<String>,
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
            port: self.port,
            redact: self.redact,
            run_dir: None,
        })
    }
}

pub fn settings_path() -> Option<PathBuf> {
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

/// Unix: `$XDG_RUNTIME_DIR/fauxfield/mcp-<pid>.sock` (elsewhere the per-user temp dir; on macOS
/// `$TMPDIR` is already per-user). Windows: `\\.\pipe\fauxfield-mcp-<pid>`.
pub fn default_socket_path() -> PathBuf {
    if cfg!(windows) {
        return PathBuf::from(format!(r"\\.\pipe\fauxfield-mcp-{}", std::process::id()));
    }
    let base = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join("fauxfield")
        .join(format!("mcp-{}.sock", std::process::id()))
}

#[cfg(unix)]
pub use fauxfield_control::ipc::SocketGuard;

/// Bind the MCP socket; stale-socket logic is shared with the control socket.
#[cfg(unix)]
pub async fn bind_unix(
    path: &std::path::Path,
) -> Result<(tokio::net::UnixListener, SocketGuard), String> {
    fauxfield_control::ipc::bind_unix(
        path,
        "Fauxfield MCP server",
        "choose another path with --socket",
    )
    .await
}

/// Create the first instance of the MCP pipe; a live name is refused (first instance wins).
#[cfg(windows)]
pub fn bind_pipe(name: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, String> {
    fauxfield_control::ipc::bind_pipe(name)
        .map_err(|e| format!("{e}; choose another name with --socket"))
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
        assert_eq!(c.socket.as_deref(), Some(std::path::Path::new("/a.sock")));
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
        assert_eq!(
            "pipe".parse::<TransportKind>().unwrap(),
            TransportKind::Pipe
        );
        let wrong = if cfg!(windows) {
            TransportKind::Unix
        } else {
            TransportKind::Pipe
        };
        assert!(wrong.check_platform().unwrap_err().contains("--transport"));
        assert!(TransportKind::Both.check_platform().is_ok());
    }
}
