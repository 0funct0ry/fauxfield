//! Workspace store (SPEC §4, §13): files are authoritative. Edits go through
//! `toml_edit` so comments and ordering survive; writes are atomic and debounced;
//! a watcher hot-reloads external edits. Must not depend on GPUI.

mod datasets;
mod init;
mod recent;
mod store;
mod templates;
mod vars;
mod watch;

pub use datasets::{
    ActionRef, DatasetRefs, DatasetSpec, DeleteOpts, RenameReport, RouteRef, RuleRef, TextRef,
    confined, state_path,
};
pub use init::{discover, init_workspace, slugify};
pub use recent::{clear_recent, load_recent, push_recent, remove_recent};
pub use store::{
    Event, FAULT_PRESETS, FaultPreset, RouteOpts, Snapshot, Store, StoreError, Target,
    load_ui_state, save_ui_state,
};
pub use templates::{BuiltinTemplate, builtin_templates};
pub use watch::Background;
