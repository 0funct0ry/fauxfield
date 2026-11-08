//! Server lifecycle and traffic tools (M3, SPEC §11.1). They go through [`ControlApi`], so a
//! GUI-hosted engine, a `fauxfield run` host, and an engine started by this MCP process behave
//! the same.

use std::collections::BTreeMap;
use std::sync::Arc;

use fauxfield_control::ControlApi;
use fauxfield_engine::{TestRequest, TrafficFilter, parse_status_filter};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    class::ToolClass,
    registry::{Registry, ToolDef, ToolFuture},
};

/// What a tool needs from the host (SPEC §20.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ControlMode {
    /// Read-only: use a live host if there is one, never create one (`Ok(None)` otherwise).
    AttachOnly,
    /// Lifecycle tools: attach to a live host, or become the host. Always `Some`.
    HostIfNeeded,
}

/// Where the tools find the host: attach to a live one, ours, or (attach-only) none.
pub type ControlProvider =
    Arc<dyn Fn(ControlMode) -> Result<Option<Arc<dyn ControlApi>>, String> + Send + Sync>;

fn host(p: &ControlProvider) -> Result<Arc<dyn ControlApi>, String> {
    p(ControlMode::HostIfNeeded)?.ok_or_else(|| "no host is available".to_string())
}

#[derive(Deserialize, JsonSchema)]
struct OptServer {
    /// Server ID, name, or file slug. Omit to act on every enabled Server.
    server: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ServerRef {
    /// Server ID, name, or file slug.
    server: String,
}

#[derive(Deserialize, JsonSchema)]
struct SendTest {
    /// Server ID, name, or file slug. It must be running.
    server: String,
    /// HTTP method (default GET).
    method: Option<String>,
    /// Path and query as the client would send it, including the Server prefix.
    path: String,
    headers: Option<BTreeMap<String, String>>,
    body: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct GetTraffic {
    server: Option<String>,
    /// Route ID, or `unmatched`.
    route: Option<String>,
    /// Status such as `404` or a class like `5xx`.
    status: Option<String>,
    /// Only requests from the last N seconds.
    since_seconds: Option<u64>,
    /// Most recent N entries (default 50, max 1000).
    limit: Option<usize>,
}

fn parse<T: for<'de> Deserialize<'de>>(m: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(m)).map_err(|e| format!("invalid arguments: {e}"))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn fail(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn lifecycle(r: &mut Registry, provider: &ControlProvider, name: &'static str, doc: &'static str) {
    let p = provider.clone();
    r.register(ToolDef::new::<OptServer>(
        name,
        doc,
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<OptServer>(m)?;
                let c = host(&p)?;
                let servers = match name {
                    "start_server" => c.start(a.server).await,
                    "stop_server" => c.stop(a.server).await,
                    _ => unreachable!("only start and stop are registered here"),
                }
                .map_err(fail)?;
                Ok(json!({ "servers": servers }))
            })
        },
    ));
}

pub fn register_control_tools(r: &mut Registry, provider: ControlProvider) {
    lifecycle(
        r,
        &provider,
        "start_server",
        "Start a Server, or every enabled Server when `server` is omitted. Binds loopback unless the server file says otherwise.",
    );
    lifecycle(
        r,
        &provider,
        "stop_server",
        "Stop a Server, or every Server when `server` is omitted.",
    );
    let p = provider.clone();
    r.register(ToolDef::new::<ServerRef>(
        "restart_server",
        "Restart a Server (stop, then start).",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<ServerRef>(m)?;
                let servers = host(&p)?.restart(a.server).await.map_err(fail)?;
                Ok(json!({ "servers": servers }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<SendTest>(
        "send_test_request",
        "Send one HTTP request to a running Server over loopback and return the response.",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<SendTest>(m)?;
                let c = host(&p)?;
                let resp = c
                    .send_test(
                        a.server,
                        TestRequest {
                            method: a.method.unwrap_or_else(|| "GET".into()),
                            path: a.path,
                            headers: a.headers.unwrap_or_default().into_iter().collect(),
                            body: a.body.unwrap_or_default(),
                        },
                    )
                    .await
                    .map_err(fail)?;
                Ok(json!({
                    "status": resp.status,
                    "headers": resp.headers,
                    "body": resp.body,
                }))
            })
        },
    ));

    let p = provider;
    r.register(ToolDef::new::<GetTraffic>(
        "get_traffic",
        "Recent recorded requests (newest last) with the route that matched, status and timing. Secret headers are masked.",
        ToolClass::Read,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<GetTraffic>(m)?;
                let Some(c) = p(ControlMode::AttachOnly)? else {
                    return Ok(json!({ "count": 0, "traffic": [], "host": "not running" }));
                };
                let status = a.status.as_deref().map(parse_status_filter).transpose()?;
                let filter = TrafficFilter {
                    server: a.server,
                    route: a.route,
                    status,
                    since_ms: a.since_seconds.map(|s| now_ms().saturating_sub(s * 1000)),
                    after_id: None,
                    limit: Some(a.limit.unwrap_or(50).clamp(1, 1000)),
                };
                let traffic = c.traffic(filter).await.map_err(fail)?;
                Ok(json!({ "count": traffic.len(), "traffic": traffic }))
            })
        },
    ));
}
