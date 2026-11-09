//! Fauxfield engine (SPEC §3.4, §5): one hyper listener per server, exact route matching,
//! static responses, a recording ring buffer, and an event stream. Must not depend on GPUI.

mod cors;
mod data;
mod engine;
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

pub use data::{DatasetInfo, PLAIN_FIELD_KINDS, crud_seed_template};
pub use engine::{
    Engine, EngineConfig, EngineError, EngineEvent, ServerInfo, ServerStatus, TrafficFilter,
    parse_status_filter,
};
pub use recording::{Recording, Timing, to_curl};
pub use render::{Preview, SampleRequest, preview, preview_with};
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
