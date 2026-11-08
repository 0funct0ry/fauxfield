//! Fauxfield MCP server (SPEC §11): tool registry with class filtering, stdio and unix-socket
//! transports, and the `UiBridge` trait implemented by the GUI in M1.2. Must not depend on GPUI.

pub mod audit;
pub mod bridge;
pub mod class;
pub mod condition;
pub mod events;
pub mod registry;
pub mod run;
pub mod scenario;
pub mod server;
pub mod tools;
pub mod transport;

pub use bridge::{UiBridge, UiError, UiRequest};
pub use class::{AllowedClasses, ToolClass};
pub use registry::{CallError, Registry, ToolDef};
pub use run::{build_registry, serve, serve_with};
pub use server::FauxfieldServer;
pub use transport::{McpConfig, McpSettings, TransportKind, default_socket_path};
