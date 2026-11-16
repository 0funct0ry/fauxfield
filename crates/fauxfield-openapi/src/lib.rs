//! OpenAPI 3.1 generator for a Server (SPEC §12.4, M17.06). Pure and deterministic: the same
//! [`ServerSnapshot`] always yields the same [`Document`]. No GUI or engine dependency; Tera
//! rendering is supplied by the caller through [`Options::renderer`].

mod document;
mod schema;
pub mod ui;

pub use document::generate;

use fauxfield_model::Server;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;

/// Everything the generator reads.
pub struct ServerSnapshot<'a> {
    pub server: &'a Server,
    /// Workspace root, for schema files and `body_file`; `None` skips file reads.
    pub root: Option<&'a Path>,
    /// Seed rows per Dataset name (used for inference when a Dataset has no columns).
    pub seeds: BTreeMap<String, Vec<Value>>,
}

/// Renders a Tera body once in preview mode (no side effects, `fake_seed` 0, 250 ms limit,
/// secret Variables shown as `***`). The caller owns those guarantees.
pub type Renderer<'a> = &'a dyn Fn(&str) -> Result<String, String>;

#[derive(Default)]
pub struct Options<'a> {
    pub renderer: Option<Renderer<'a>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Stats {
    pub documented: usize,
    pub skipped: usize,
}

#[derive(Debug, Clone)]
pub struct Document {
    pub value: Value,
    pub stats: Stats,
    /// Problems-panel warnings (a Tera body failed to render, a schema file was unreadable).
    pub warnings: Vec<String>,
    /// Route ids skipped because the document cannot express them (W097).
    pub unrepresentable: Vec<String>,
}

impl Document {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(&self.value).unwrap_or_default()
    }
}
