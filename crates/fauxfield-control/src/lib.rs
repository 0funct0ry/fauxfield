//! Fauxfield control plane (SPEC §3.1): the [`ControlApi`] trait, the host lock file, and a
//! JSON-RPC 2.0 control socket. The GUI uses [`LocalControl`] in-process; the CLI and MCP use
//! [`RemoteControl`] over the socket. Must not depend on GPUI.

mod api;
pub mod build_info;
mod host;
pub mod ipc;
mod reload;
mod rpc;
mod runtime;

pub use api::{ControlApi, ControlError, LocalControl};
pub use host::{HostInfo, HostLock, control_socket_path, read_host};
pub use reload::{attach_store, attach_store_with_env};
pub use rpc::{RemoteControl, handle_conn, serve};
pub use runtime::{Host, HostError, HostOptions};
