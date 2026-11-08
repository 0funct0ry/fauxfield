//! The in-process MCP host for agent UI testing (SPEC §3.4, §11.4). Compiled only with the
//! `dev-mcp` feature; release builds contain none of it.

pub mod audit;
pub mod bridge;
pub mod capture;
pub mod handler;
pub mod host;
pub mod input;
pub mod screenshot;
pub mod state;
pub mod tools;
pub mod tree;

use std::path::PathBuf;

/// What `fauxfield --mcp-socket [PATH] --mcp-stdio` asks for.
#[derive(Debug, Clone, Default)]
pub struct DevMcpOptions {
    /// Socket path; `None` means the default location when `unix` is set.
    pub socket: Option<PathBuf>,
    /// Serve on stdio. Stdout is then reserved for the protocol.
    pub stdio: bool,
    /// Serve on a unix socket.
    pub unix: bool,
    /// The workspace folder the app was started with, reported by `ui.state`.
    pub workspace: Option<PathBuf>,
}
