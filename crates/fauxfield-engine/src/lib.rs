//! Fauxfield engine (SPEC §3.4, §5): one hyper listener per server, exact route matching,
//! static responses, a recording ring buffer, and an event stream. Must not depend on GPUI.

mod engine;
mod problem;
mod recording;
mod router;
mod testreq;
mod time;

pub use engine::{
    Engine, EngineConfig, EngineError, EngineEvent, ServerInfo, ServerStatus, TrafficFilter,
    parse_status_filter,
};
pub use recording::{Recording, Timing, to_curl};
pub use router::{Match, RouteTable};
pub use testreq::{TestRequest, TestResponse};

/// Current time as RFC 3339 UTC (for `host.json` and log lines).
pub fn now_rfc3339() -> String {
    time::rfc3339(time::now_ms())
}
