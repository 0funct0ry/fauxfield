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
    /// Server-wide latency simulation (SPEC §5.6).
    #[serde(default)]
    pub network: Option<Network>,
    /// Request rate limit (SPEC §5.6).
    #[serde(default)]
    pub rate_limit: Option<RateLimit>,
    /// Seed for fake data and random functions; unset means random (SPEC §8.3).
    #[serde(default)]
    pub fake_seed: Option<u64>,
    #[serde(default)]
    #[schemars(with = "BTreeMap<String, serde_json::Value>")]
    pub variables: BTreeMap<String, toml::Value>,
    #[serde(default)]
    pub datasets: Vec<Dataset>,
    #[serde(default)]
    pub folders: Vec<Folder>,
    #[serde(default)]
    pub routes: Vec<Route>,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

/// A named JSON collection backing stateful `crud` routes (SPEC §6.1).
#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonSchema)]
pub struct Dataset {
    pub name: String,
    /// Static JSON seed file (workspace-relative).
    #[serde(default)]
    pub seed: Option<String>,
    /// Tera template rendering to JSON (workspace-relative).
    #[serde(default)]
    pub seed_template: Option<String>,
    /// Field holding each item's identifier (default `id`).
    #[serde(default)]
    pub key: Option<String>,
    /// `uuid` (default), `increment` or `ulid`.
    #[serde(default)]
    pub id_strategy: Option<String>,
    /// true: mutations are written to `.fauxfield/state/` and reloaded on start.
    #[serde(default)]
    pub persist: bool,
    #[serde(flatten)]
    #[schemars(skip)]
    pub extra: Extra,
}

impl Dataset {
    pub fn key_name(&self) -> &str {
        self.key
            .as_deref()
            .filter(|k| !k.is_empty())
            .unwrap_or("id")
    }
}

/// Id strategies for new dataset items (SPEC §4.4).
pub const ID_STRATEGIES: &[&str] = &["uuid", "increment", "ulid"];
/// Known Route kinds.
pub const ROUTE_KINDS: &[&str] = &["http", "crud", "static", "websocket"];

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
    /// `crud` routes: the Dataset this Route serves.
    #[serde(default)]
    pub dataset: Option<String>,
    /// `crud` routes: JSON Schema file (workspace-relative) checked on POST/PUT/PATCH.
    #[serde(default)]
    pub validate: Option<String>,
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
    /// Milliseconds, or a range such as `"200..800"` (SPEC §5.5).
    #[serde(default)]
    pub delay_ms: Option<Delay>,
    /// `none`, `timeout`, `reset`, `close`, `empty`, `malformed` or `slow_body`.
    #[serde(default)]
    pub fault: Option<String>,
    /// 0.0-1.0 probability that `fault` applies (default 1.0).
    #[serde(default)]
    pub fault_rate: Option<f64>,
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

/// Fault kinds a Response can inject (SPEC §5.5).
pub const FAULTS: &[&str] = &[
    "none",
    "timeout",
    "reset",
    "close",
    "empty",
    "malformed",
    "slow_body",
];

/// A fixed delay in ms, or a `"min..max"` range.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Delay {
    Ms(u64),
    Range(String),
}

impl Delay {
    /// `(min, max)` in ms; `None` when the text is not a number or `a..b` with a <= b.
    pub fn bounds(&self) -> Option<(u64, u64)> {
        match self {
            Delay::Ms(n) => Some((*n, *n)),
            Delay::Range(s) => parse_delay(s),
        }
    }
    /// Text for editing: `200` or `200..800`.
    pub fn text(&self) -> String {
        match self {
            Delay::Ms(n) => n.to_string(),
            Delay::Range(s) => s.clone(),
        }
    }
}

/// Parse `"250"` or `"200..800"` (min <= max).
pub fn parse_delay(s: &str) -> Option<(u64, u64)> {
    let s = s.trim();
    match s.split_once("..") {
        Some((a, b)) => {
            let (a, b) = (a.trim().parse().ok()?, b.trim().parse().ok()?);
            (a <= b).then_some((a, b))
        }
        None => s.parse().ok().map(|n| (n, n)),
    }
}

/// Server `[network]` table: simulated latency and bandwidth.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Network {
    #[serde(default)]
    pub latency_ms: u64,
    #[serde(default)]
    pub jitter_ms: u64,
    /// 0 = unlimited.
    #[serde(default)]
    pub bandwidth_kbps: u64,
}

/// Server `[rate_limit]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct RateLimit {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "rl_requests")]
    pub requests: u64,
    /// `30s`, `1m`, `2h`.
    #[serde(default = "rl_window")]
    pub window: String,
    /// `ip`, `header:<name>` or `query:<name>`.
    #[serde(default = "rl_key")]
    pub key: String,
}
fn rl_requests() -> u64 {
    100
}
fn rl_window() -> String {
    "1m".into()
}
fn rl_key() -> String {
    "ip".into()
}

/// Window text (`30s`, `1m`, `2h`) in seconds.
pub fn parse_window(s: &str) -> Option<u64> {
    let s = s.trim();
    let (n, unit) = s.split_at(s.len().checked_sub(1)?);
    let n: u64 = n.parse().ok().filter(|n| *n > 0)?;
    Some(
        n * match unit {
            "s" => 1,
            "m" => 60,
            "h" => 3600,
            _ => return None,
        },
    )
}

/// True for `ip`, `header:<name>`, `query:<name>`.
pub fn valid_rate_key(k: &str) -> bool {
    k == "ip"
        || k.strip_prefix("header:")
            .is_some_and(|n| !n.trim().is_empty())
        || k.strip_prefix("query:")
            .is_some_and(|n| !n.trim().is_empty())
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
