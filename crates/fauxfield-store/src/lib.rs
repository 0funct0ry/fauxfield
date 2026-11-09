//! Workspace store (SPEC §4, §13): files are authoritative. Edits go through
//! `toml_edit` so comments and ordering survive; writes are atomic and debounced;
//! a watcher hot-reloads external edits. Must not depend on GPUI.

mod init;
mod recent;
mod store;
mod vars;
mod watch;

pub use init::{discover, init_workspace, slugify};
pub use recent::{clear_recent, load_recent, push_recent, remove_recent};
pub use store::{
    Event, RouteOpts, Snapshot, Store, StoreError, Target, load_ui_state, save_ui_state,
};
pub use watch::Background;
