//! Workspace file model (SPEC §4): serde types, prefixed IDs, validation.
//! Must not depend on GPUI.

mod diagnostic;
mod id;
mod types;
mod validate;

pub use diagnostic::{Diagnostic, Severity};
pub use id::new_id;
pub use types::{
    Environment, Folder, Manifest, Response, Route, SCHEMA_VERSION, Server, parse_environment,
    parse_manifest, parse_server,
};
pub use validate::{validate_environment_names, validate_servers};

/// JSON Schema (draft 2020-12) for a server file, as published for Taplo.
pub fn server_schema() -> serde_json::Value {
    serde_json::to_value(schemars::schema_for!(Server)).expect("schema serialises")
}
