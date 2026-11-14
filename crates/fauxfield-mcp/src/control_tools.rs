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
struct ResetState {
    /// Server ID, name or file slug. Omit to reset every Server.
    server: Option<String>,
    /// Only this Dataset (needs `server`).
    dataset: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct ClearDataset {
    /// Server ID, name or file slug.
    server: String,
    /// Dataset name.
    dataset: String,
}

#[derive(Deserialize, JsonSchema)]
struct SetNetwork {
    /// `none`, `fast-3g`, `slow-3g`, `flaky-wifi` or `offline`. Omit to read the current profile.
    profile: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct SetChaos {
    /// Server ID, name or file slug. Omit for every Server.
    server: Option<String>,
    /// Fault rate 0 to 1 to turn chaos on; 0 or omitted turns it off.
    rate: Option<f64>,
}

#[derive(Deserialize, JsonSchema)]
struct WsList {
    /// Server ID, name or file slug.
    server: String,
    /// Only this WebSocket Route ID.
    route: Option<String>,
    /// Also return timeline frames with an id greater than this (0 returns the whole timeline).
    after: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
struct WsSend {
    /// Server ID, name or file slug.
    server: String,
    /// Route ID: send to every connection of this Route. Omit for every connection.
    route: Option<String>,
    /// One connection ID (`wsc_...`). Omit to send to all.
    connection: Option<String>,
    /// Message body (text, or base64 when `binary`).
    body: String,
    /// Send as a binary frame (body is base64).
    binary: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct WsDisconnect {
    /// Server ID, name or file slug.
    server: String,
    /// Connection ID (`wsc_...`).
    connection: String,
    /// Close code (default 1000).
    code: Option<u16>,
}

#[derive(Deserialize, JsonSchema)]
struct CallbackRefire {
    /// Server ID, name or file slug.
    server: String,
    /// Recording id of the request that triggered the Callback (or of one of its callback entries).
    recording: u64,
    /// Callback id; omit to fire every Callback of that Response.
    callback: Option<String>,
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
    r.register(ToolDef::new::<ResetState>(
        "reset_state",
        "Reset state: Datasets back to their seed (persisted state deleted), response counters, sequence cursors and runtime variables. Scope with `server` and optionally `dataset`; omit both to reset the whole workspace.",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<ResetState>(m)?;
                host(&p)?
                    .reset_state(a.server.clone(), a.dataset.clone())
                    .await
                    .map_err(fail)?;
                Ok(json!({ "reset": true, "server": a.server, "dataset": a.dataset }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<ClearDataset>(
        "clear_dataset",
        "Empty a Dataset's live rows (runtime only; the seed file is untouched and `reset_state` restores the rows from it).",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<ClearDataset>(m)?;
                let c = host(&p)?;
                let empty = match c
                    .data_show(a.server.clone(), a.dataset.clone())
                    .await
                    .map_err(fail)?
                {
                    serde_json::Value::Object(_) => json!({}),
                    _ => json!([]),
                };
                c.data_set(a.server.clone(), a.dataset.clone(), empty)
                    .await
                    .map_err(fail)?;
                Ok(json!({ "cleared": true, "server": a.server, "dataset": a.dataset }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<SetNetwork>(
        "set_network_profile",
        "Read or set the workspace network profile (runtime only, not written to files): none, fast-3g, slow-3g, flaky-wifi or offline. A profile overrides every Server's [network]; offline refuses connections.",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<SetNetwork>(m)?;
                let c = host(&p)?;
                let profile = match a.profile {
                    None => c.network_get().await,
                    Some(n) => {
                        let prof = fauxfield_engine::NetworkProfile::parse(&n).ok_or_else(|| {
                            format!("`{n}` is not a profile; use none, fast-3g, slow-3g, flaky-wifi or offline")
                        })?;
                        c.network_set(prof).await
                    }
                }
                .map_err(fail)?;
                Ok(json!({ "profile": profile.slug(), "label": profile.label() }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<SetChaos>(
        "set_chaos",
        "Turn the chaos switch on or off (runtime only): with a rate above 0 that fraction of requests gets a random fault (reset, close, empty, malformed or 500).",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<SetChaos>(m)?;
                if a.rate.is_some_and(|r| !(0.0..=1.0).contains(&r)) {
                    return Err("rate must be between 0 and 1".into());
                }
                let rate = a.rate.filter(|r| *r > 0.0);
                let servers = host(&p)?.chaos_set(a.server, rate).await.map_err(fail)?;
                Ok(json!({ "servers": servers }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<WsList>(
        "list_ws_connections",
        "List live WebSocket connections of a Server and the message timeline (in/out frames).",
        ToolClass::Read,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<WsList>(m)?;
                let h = host(&p)?;
                let connections = h
                    .ws_connections(a.server.clone(), a.route.clone())
                    .await
                    .map_err(fail)?;
                let frames = h
                    .ws_messages(a.server, a.route, None, a.after.unwrap_or(0), 200)
                    .await
                    .map_err(fail)?;
                Ok(json!({ "connections": connections, "frames": frames }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<WsSend>(
        "send_ws_message",
        "Send a message to one WebSocket connection, or to every connection of a Route or Server.",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<WsSend>(m)?;
                let sent = host(&p)?
                    .ws_send(
                        a.server,
                        a.route,
                        a.connection,
                        a.body,
                        a.binary.unwrap_or(false),
                    )
                    .await
                    .map_err(fail)?;
                Ok(json!({ "sent": sent }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<WsDisconnect>(
        "disconnect_ws_client",
        "Close one WebSocket connection with a close code.",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<WsDisconnect>(m)?;
                host(&p)?
                    .ws_disconnect(a.server, a.connection, a.code)
                    .await
                    .map_err(fail)?;
                Ok(json!({ "disconnected": true }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<CallbackRefire>(
        "refire_callback",
        "Fire the Callbacks (webhooks) of a recorded request again, now, and return the outgoing entries with their status and error.",
        ToolClass::Execute,
        move |m| -> ToolFuture {
            let p = p.clone();
            Box::pin(async move {
                let a = parse::<CallbackRefire>(m)?;
                let recs = host(&p)?
                    .callback_refire(a.server, a.recording, a.callback)
                    .await
                    .map_err(fail)?;
                Ok(json!({ "callbacks": recs }))
            })
        },
    ));

    let p = provider.clone();
    r.register(ToolDef::new::<SendTest>(
        "send_test_request",
        "Send one HTTP request to a running Server over loopback and return the response and the Dataset changes its actions made.",
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
                    "dataset_changes": resp.dataset_changes,
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
