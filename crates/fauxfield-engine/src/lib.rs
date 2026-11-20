//! Fauxfield engine (SPEC §3.4, §5): one hyper listener per server, exact route matching,
//! static responses, a recording ring buffer, and an event stream. Must not depend on GPUI.

pub mod actions;
mod cors;
mod data;
mod engine;
pub mod generate;
pub mod har;
pub mod import;
mod problem;
mod recording;
pub mod render;
mod router;
mod rules;
mod sim;
mod statics;
pub mod template;
mod testreq;
mod time;
pub mod tls;
pub mod vars;

pub use actions::DatasetChange;
pub use data::{DatasetInfo, PLAIN_FIELD_KINDS, crud_seed_template, source_kind};
pub use engine::{
    CallbackPreview, Engine, EngineConfig, EngineError, EngineEvent, OpenapiInfo, ServerInfo,
    ServerStatus, TrafficFilter, WsConnectionInfo, WsFrame, callback_preview, openapi_document,
    parse_status_filter, ws_apply_format,
};
pub use recording::{Recording, Timing, to_curl};
pub use render::{Preview, SampleRequest, preview, preview_with, render_sample};
pub use router::{Match, RouteTable};
pub use rules::{ResponseTrace, RuleTrace, Trace};
pub use sim::{DEFAULT_CHAOS_RATE, Fault, NetworkProfile, PROFILES};
pub use statics::{StaticFile, list_files as static_files, mime_of};
pub use testreq::{TestRequest, TestResponse};
pub use vars::{EnvData, VarContext};

/// Current time as RFC 3339 UTC (for `host.json` and log lines).
pub fn now_rfc3339() -> String {
    time::rfc3339(time::now_ms())
}
