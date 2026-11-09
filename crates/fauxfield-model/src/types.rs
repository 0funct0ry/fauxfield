use std::collections::BTreeMap;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Unknown keys are kept so future fields survive (the real round trip is toml_edit's job).
type Extra = BTreeMap<String, toml::Value>;

fn schema_v() -> u32 {
    SCHEMA_VERSION
}
fn yes() -> bool {
    true
}
fn host() -> String {
    "127.0.0.1".into()
}
fn http() -> String {
    "http".into()
}
fn get() -> String {
    "GET".into()
}
fn ok_status() -> u16 {
    200
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Manifest {
    #[serde(default = "schema_v")]
    pub schema: u32,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub default_environment: Option<String>,
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    pub variables: BTreeMap<String, toml::Value>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Environment {
    #[serde(default)]
    pub name: String,
    /// Dotenv file (workspace-relative); its values are never written back (SPEC §4.3).
    #[serde(default)]
    pub dotenv: Option<String>,
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    pub variables: BTreeMap<String, toml::Value>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Server {
    #[serde(default = "schema_v")]
    pub schema: u32,
    pub id: String,
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default = "host")]
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub prefix: Option<String>,
    /// Add the closest-routes hint to 404 bodies (default true).
    #[serde(default)]
    pub unmatched_hint: Option<bool>,
    /// true: `/users` and `/users/` are different paths (SPEC §5.2).
    #[serde(default)]
    pub strict_slash: Option<bool>,
    #[serde(default)]
    pub docs: Option<String>,
    /// Seed for fake data and random functions; unset means random (SPEC §8.3).
    #[serde(default)]
    pub fake_seed: Option<u64>,
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    pub variables: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub routes: Vec<Route>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Folder {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub docs: Option<String>,
    /// false: every Route inside is skipped by the matcher (default true).
    #[serde(default = "yes")]
    pub enabled: bool,
    /// true: starts collapsed in the tree.
    #[serde(default)]
    pub collapsed: bool,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Route {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub folder: Option<String>,
    #[serde(default = "http")]
    pub kind: String,
    #[serde(default = "get")]
    pub method: String,
    pub path: String,
    #[serde(default, rename = "match")]
    pub match_mode: Option<String>,
    /// Higher wins; default 0.
    #[serde(default)]
    pub priority: Option<i32>,
    #[serde(default)]
    pub response_mode: Option<String>,
    #[serde(default)]
    pub docs: Option<String>,
    /// Route headers (values templated); a Response header of the same name wins (SPEC §5.5).
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, String>")]
    pub headers: indexmap::IndexMap<String, String>,
    /// Route-scope variables (SPEC §8.1).
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    pub variables: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub responses: Vec<Response>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Response {
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    /// Optional one-line note shown under the name (SPEC §5.5). Not sent.
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub default: bool,
    /// true: skipped by response selection (SPEC §5.3).
    #[serde(default)]
    pub disabled: bool,
    /// `and` (default) or `or` (SPEC §5.4).
    #[serde(default)]
    pub rules_logic: Option<String>,
    #[serde(default)]
    pub rules: Vec<Rule>,
    #[serde(default = "ok_status")]
    pub status: u16,
    #[serde(default)]
    pub content_type: Option<String>,
    #[serde(default)]
    pub body: Option<String>,
    #[serde(default)]
    pub body_file: Option<String>,
    /// false: body, body_file and header values are sent verbatim (default true).
    #[serde(default)]
    pub template: Option<bool>,
    #[serde(default)]
    pub delay_ms: Option<u64>,
    /// In file order (rows of the Response headers editor keep the order they were written in).
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, String>")]
    pub headers: indexmap::IndexMap<String, String>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// One condition of a Response (SPEC §5.4). Values are plain strings, numbers or arrays until M6.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Rule {
    pub target: String,
    #[serde(default)]
    pub key: Option<String>,
    pub op: String,
    #[serde(default)]
    #[schemars(with = "Option<serde_json::Value>")]
    pub value: Option<toml::Value>,
    #[serde(default)]
    pub negate: bool,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

pub fn parse_manifest(text: &str) -> Result<Manifest, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}
pub fn parse_environment(text: &str) -> Result<Environment, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}
pub fn parse_server(text: &str) -> Result<Server, String> {
    toml::from_str(text).map_err(|e| e.to_string())
}
