//! WebSocket mocking (SPEC §7.4, M13): upgrade on the Server's listener, echo / script /
//! broadcast modes, live connection registry and a per-Server frame timeline.

use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use base64::Engine as _;
use bytes::Bytes;
use fauxfield_model::{Route, WsMessage, new_id, parse_duration_ms};
use futures_util::{SinkExt, StreamExt};
use hyper::upgrade::OnUpgrade;
use hyper::{Response as HyperResponse, StatusCode};
use hyper_util::rt::TokioIo;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::handshake::derive_accept_key;
use tokio_tungstenite::tungstenite::protocol::{CloseFrame, Role, frame::coding::CloseCode};

use super::{Engine, EngineError, EngineEvent, RespBody, ServerRt};
use crate::recording::{Recording, Timing};
use crate::sim::{BodyMode, SimBody};
use crate::time::{now_ms, rfc3339};

/// Frames kept per Server.
const FRAME_CAP: usize = 2000;
/// Largest frame preview stored in the timeline.
const PREVIEW: usize = 4096;

/// A live WebSocket connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WsConnectionInfo {
    pub id: String,
    pub server_id: String,
    pub route_id: String,
    pub path: String,
    pub peer: String,
    pub opened_ms: u64,
    pub messages_in: u64,
    pub messages_out: u64,
}

/// One frame of the timeline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WsFrame {
    pub id: u64,
    pub ts_ms: u64,
    pub ts: String,
    pub connection: String,
    pub route_id: String,
    /// `in` (from the client) or `out` (to the client).
    pub dir: String,
    /// `text`, `binary`, `ping`, `pong`, `close` or `error`.
    pub kind: String,
    pub size: usize,
    /// Text content (binary frames: base64), truncated to 4 KiB.
    pub body: String,
}

enum Cmd {
    Send(Message),
    Close(u16),
}

type Counts = Arc<(AtomicU64, AtomicU64)>;

/// The upgrade request of a connection, kept so ad-hoc messages render with its context.
struct ReqCtx {
    headers: Vec<(String, String)>,
    url: String,
    params: Vec<(String, String)>,
    ip: String,
}

struct Entry {
    info: WsConnectionInfo,
    tx: mpsc::UnboundedSender<Cmd>,
    counts: Counts,
    req: Arc<ReqCtx>,
}

#[derive(Clone, Debug)]
struct Bcast {
    from: String,
    msg: Message,
}

pub(super) struct WsHub {
    conns: Mutex<HashMap<String, Entry>>,
    frames: Mutex<VecDeque<WsFrame>>,
    next: AtomicU64,
    bcast: broadcast::Sender<Bcast>,
    tickers: Mutex<HashMap<String, tokio::task::JoinHandle<()>>>,
}

impl Default for WsHub {
    fn default() -> Self {
        WsHub {
            conns: Default::default(),
            frames: Default::default(),
            next: AtomicU64::new(1),
            bcast: broadcast::channel(256).0,
            tickers: Default::default(),
        }
    }
}

impl WsHub {
    pub(super) fn close_all(&self, code: u16) {
        for e in self.conns.lock().unwrap().values() {
            let _ = e.tx.send(Cmd::Close(code));
        }
    }

    fn infos(&self, route: Option<&str>) -> Vec<WsConnectionInfo> {
        let mut v: Vec<_> = self
            .conns
            .lock()
            .unwrap()
            .values()
            .filter(|e| route.is_none_or(|r| e.info.route_id == r))
            .map(|e| {
                let mut i = e.info.clone();
                i.messages_in = e.counts.0.load(Ordering::Relaxed);
                i.messages_out = e.counts.1.load(Ordering::Relaxed);
                i
            })
            .collect();
        v.sort_by_key(|i| i.opened_ms);
        v
    }
}

pub(super) struct UpgradeCall {
    pub engine: Engine,
    pub rt: Arc<ServerRt>,
    pub server_id: String,
    pub route: Route,
    pub ip: String,
    pub headers: Vec<(String, String)>,
    pub url: String,
    pub params: Vec<(String, String)>,
    pub route_pattern: String,
    pub method: String,
    pub on_upgrade: OnUpgrade,
    pub started: Instant,
    pub match_us: u64,
}

fn header<'a>(h: &'a [(String, String)], n: &str) -> Option<&'a str> {
    h.iter().find(|(k, _)| k == n).map(|(_, v)| v.as_str())
}

fn plain(status: u16, msg: &str, extra: &[(&str, &str)]) -> HyperResponse<RespBody> {
    let mut b = HyperResponse::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST))
        .header("content-type", "text/plain; charset=utf-8");
    for (k, v) in extra {
        b = b.header(*k, *v);
    }
    b.body(RespBody::Sim(SimBody::new(
        Bytes::from(msg.to_string()),
        BodyMode::Full,
        None,
    )))
    .unwrap()
}

/// Answer the handshake and spawn the session.
pub(super) fn upgrade(c: UpgradeCall) -> HyperResponse<RespBody> {
    let is_upgrade = header(&c.headers, "upgrade")
        .is_some_and(|v| v.eq_ignore_ascii_case("websocket"))
        && header(&c.headers, "connection")
            .is_some_and(|v| v.to_ascii_lowercase().contains("upgrade"));
    if !is_upgrade {
        return plain(
            426,
            "This is a WebSocket endpoint. Connect with an Upgrade: websocket request.",
            &[("upgrade", "websocket"), ("connection", "Upgrade")],
        );
    }
    let Some(key) = header(&c.headers, "sec-websocket-key") else {
        return plain(400, "missing Sec-WebSocket-Key", &[]);
    };
    if header(&c.headers, "sec-websocket-version") != Some("13") {
        return plain(
            426,
            "unsupported WebSocket version",
            &[("sec-websocket-version", "13")],
        );
    }
    let accept = derive_accept_key(key.as_bytes());
    let proto = header(&c.headers, "sec-websocket-protocol")
        .and_then(|p| p.split(',').next())
        .map(|p| p.trim().to_string());
    let mut b = HyperResponse::builder()
        .status(StatusCode::SWITCHING_PROTOCOLS)
        .header("upgrade", "websocket")
        .header("connection", "Upgrade")
        .header("sec-websocket-accept", accept);
    if let Some(p) = &proto {
        b = b.header("sec-websocket-protocol", p.as_str());
    }
    let conn_id = new_id("wsc");
    let path = c.url.split('?').next().unwrap_or("/").to_string();
    let ts_ms = now_ms();
    let rec = Recording {
        id: c.engine.inner.seq.fetch_add(1, Ordering::Relaxed),
        ts: rfc3339(ts_ms),
        ts_ms,
        server_id: c.server_id.clone(),
        server: c.rt.def.lock().unwrap().name.clone(),
        route: c.route.id.clone(),
        response_id: conn_id.clone(),
        response_name: "WebSocket".into(),
        trace: None,
        params: c.params.clone(),
        route_pattern: c.route_pattern.clone(),
        method: c.method.clone(),
        url: c.url.clone(),
        request_headers: crate::recording::redact(c.headers.clone()),
        request_body: String::new(),
        status: 101,
        response_headers: vec![("upgrade".into(), "websocket".into())],
        response_body: String::new(),
        timing: Timing {
            match_us: c.match_us,
            total_ms: c.started.elapsed().as_millis() as u64,
            ..Default::default()
        },
        truncated: false,
        error: None,
        fault: None,
        dataset_changes: vec![],
        parent_id: None,
        callback_id: None,
        source: "mock".into(),
        upstream: None,
    };
    c.engine.record(&c.rt, rec);
    let resp = b
        .body(RespBody::Sim(SimBody::new(
            Bytes::new(),
            BodyMode::Full,
            None,
        )))
        .unwrap();
    tokio::spawn(session(c, conn_id, path));
    resp
}

struct Session {
    engine: Engine,
    rt: Arc<ServerRt>,
    server_id: String,
    route: Route,
    conn_id: String,
    headers: Vec<(String, String)>,
    url: String,
    params: Vec<(String, String)>,
    ip: String,
    counts: Counts,
    seq: u64,
}

impl Session {
    fn push(&self, dir: &str, kind: &str, size: usize, mut body: String) {
        let hub = &self.rt.ws;
        let ts_ms = now_ms();
        if body.len() > PREVIEW {
            let mut n = PREVIEW;
            while !body.is_char_boundary(n) {
                n -= 1;
            }
            body.truncate(n);
        }
        let f = WsFrame {
            id: hub.next.fetch_add(1, Ordering::Relaxed),
            ts_ms,
            ts: rfc3339(ts_ms),
            connection: self.conn_id.clone(),
            route_id: self.route.id.clone(),
            dir: dir.into(),
            kind: kind.into(),
            size,
            body,
        };
        {
            let mut q = hub.frames.lock().unwrap();
            while q.len() >= FRAME_CAP {
                q.pop_front();
            }
            q.push_back(f.clone());
        }
        let _ = self.engine.inner.events.send(EngineEvent::WsMessage {
            server_id: self.server_id.clone(),
            frame: Box::new(f),
        });
    }

    fn log(&self, dir: &str, m: &Message) {
        let c = if dir == "in" {
            &self.counts.0
        } else {
            &self.counts.1
        };
        match m {
            Message::Text(t) => {
                c.fetch_add(1, Ordering::Relaxed);
                self.push(dir, "text", t.len(), t.to_string());
            }
            Message::Binary(b) => {
                c.fetch_add(1, Ordering::Relaxed);
                self.push(
                    dir,
                    "binary",
                    b.len(),
                    base64::engine::general_purpose::STANDARD.encode(b),
                );
            }
            Message::Ping(b) => self.push(dir, "ping", b.len(), String::new()),
            Message::Pong(b) => self.push(dir, "pong", b.len(), String::new()),
            Message::Close(f) => self.push(
                dir,
                "close",
                0,
                f.as_ref()
                    .map(|f| format!("{} {}", u16::from(f.code), f.reason))
                    .unwrap_or_default(),
            ),
            Message::Frame(_) => {}
        }
    }

    fn context(&self, call: u64) -> serde_json::Value {
        let server = self.rt.def.lock().unwrap().clone();
        let vc = self.engine.inner.vars.read().unwrap();
        crate::render::context(
            &server,
            "GET",
            &self.url,
            &self.headers,
            &self.params,
            Some(&self.ip),
            &[],
            0,
            call,
            &vc,
            Some(&self.route.id),
        )
    }

    fn tpl(&self, context: serde_json::Value, call: u64) -> crate::render::TplEnv {
        crate::render::TplEnv {
            context,
            seed: self.rt.def.lock().unwrap().fake_seed,
            route_id: self.route.id.clone(),
            counter: call,
            seq: Some(self.rt.seq.clone()),
        }
    }

    /// Render a scripted message for this connection (`incoming` is the triggering frame).
    fn render(&mut self, m: &WsMessage, incoming: Option<&Message>) -> Result<Message, String> {
        self.seq += 1;
        let mut ctx = self.context(self.seq);
        ctx["connection"] = serde_json::json!({
            "id": self.conn_id,
            "messages_in": self.counts.0.load(Ordering::Relaxed),
            "messages_out": self.counts.1.load(Ordering::Relaxed),
        });
        ctx["message"] = match incoming {
            Some(Message::Text(t)) => serde_json::json!({
                "text": t.as_str(),
                "json": serde_json::from_str::<serde_json::Value>(t.as_str()).unwrap_or_default(),
                "binary": false,
                "size": t.len(),
            }),
            Some(Message::Binary(b)) => serde_json::json!({
                "text": String::from_utf8_lossy(b),
                "json": null,
                "base64": base64::engine::general_purpose::STANDARD.encode(b),
                "binary": true,
                "size": b.len(),
            }),
            _ => serde_json::Value::Null,
        };
        let text = self
            .tpl(ctx, self.seq)
            .render(&self.engine.inner.cfg.root, &m.body)
            .map_err(|e| e.to_string())?;
        let text = if m.binary {
            text
        } else {
            apply_format(&text, m.format.as_deref())
        };
        build(text, m.binary)
    }

    /// Does `m`'s `when` accept the incoming frame?
    fn accepts(&self, m: &WsMessage, incoming: &Message) -> bool {
        let Some(w) = &m.when else { return true };
        let bytes: &[u8] = match incoming {
            Message::Text(t) => t.as_bytes(),
            Message::Binary(b) => b,
            _ => return false,
        };
        let root = self.engine.inner.cfg.root.clone();
        let call = self.seq + 1;
        let mut ctx = crate::rules::Ctx::new(
            "GET",
            &self.url,
            &self.headers,
            &self.params,
            Some(self.ip.clone()),
            bytes,
            call,
            &root,
        );
        ctx.tpl = Some(self.tpl(self.context(call), call));
        crate::rules::rule_passes(w, &ctx, &self.rt.compiled)
    }
}

/// Apply a message `format` (`compact` default, `pretty`, `raw`) to a rendered body. Text that
/// is not valid JSON is returned unchanged.
pub fn apply_format(text: &str, format: Option<&str>) -> String {
    if format == Some("raw") {
        return text.to_string();
    }
    let Ok(v) = serde_json::from_str::<serde_json::Value>(text) else {
        return text.to_string();
    };
    let out = if format == Some("pretty") {
        serde_json::to_string_pretty(&v)
    } else {
        serde_json::to_string(&v)
    };
    out.unwrap_or_else(|_| text.to_string())
}

fn build(text: String, binary: bool) -> Result<Message, String> {
    if binary {
        let clean: String = text.chars().filter(|c| !c.is_whitespace()).collect();
        base64::engine::general_purpose::STANDARD
            .decode(clean)
            .map(|b| Message::Binary(b.into()))
            .map_err(|e| format!("binary message is not base64: {e}"))
    } else {
        Ok(Message::Text(text.into()))
    }
}

fn close_msg(code: u16) -> Message {
    Message::Close(Some(CloseFrame {
        code: CloseCode::from(code),
        reason: "".into(),
    }))
}

async fn session(c: UpgradeCall, conn_id: String, path: String) {
    let Ok(upgraded) = c.on_upgrade.await else {
        return;
    };
    let mut ws = WebSocketStream::from_raw_socket(TokioIo::new(upgraded), Role::Server, None).await;
    let counts: Counts = Default::default();
    let (tx, mut rx) = mpsc::unbounded_channel::<Cmd>();
    let info = WsConnectionInfo {
        id: conn_id.clone(),
        server_id: c.server_id.clone(),
        route_id: c.route.id.clone(),
        path,
        peer: c.ip.clone(),
        opened_ms: now_ms(),
        messages_in: 0,
        messages_out: 0,
    };
    let rt = c.rt.clone();
    rt.ws.conns.lock().unwrap().insert(
        conn_id.clone(),
        Entry {
            info: info.clone(),
            tx,
            counts: counts.clone(),
            req: Arc::new(ReqCtx {
                headers: c.headers.clone(),
                url: c.url.clone(),
                params: c.params.clone(),
                ip: c.ip.clone(),
            }),
        },
    );
    let _ = c.engine.inner.events.send(EngineEvent::WsConnection {
        server_id: c.server_id.clone(),
        connection: info.clone(),
        open: true,
    });
    let mode = c.route.mode.clone().unwrap_or_else(|| "echo".into());
    let broadcast_mode = mode == "broadcast";
    let scripted = mode != "echo";
    let messages: Vec<WsMessage> = if scripted {
        c.route.messages.clone()
    } else {
        vec![]
    };
    let mut s = Session {
        engine: c.engine.clone(),
        rt: rt.clone(),
        server_id: c.server_id.clone(),
        route: c.route.clone(),
        conn_id: conn_id.clone(),
        headers: c.headers.clone(),
        url: c.url.clone(),
        params: c.params.clone(),
        ip: c.ip.clone(),
        counts,
        seq: 0,
    };

    // Interval timers feed the loop through a channel (own connection) or, in broadcast mode,
    // one shared ticker per Route feeds every client.
    let mut timers: Vec<tokio::task::JoinHandle<()>> = vec![];
    let (itx, mut irx) = mpsc::unbounded_channel::<usize>();
    if broadcast_mode {
        start_ticker(
            &c.engine,
            &rt,
            &c.route,
            &c.headers,
            &c.url,
            &c.params,
            &c.ip,
            &c.server_id,
            &messages,
        );
    } else {
        for (i, m) in messages.iter().enumerate() {
            if m.trigger == "interval"
                && let Some(ms) = m.every.as_deref().and_then(parse_duration_ms)
            {
                let itx = itx.clone();
                timers.push(tokio::spawn(async move {
                    let mut t = tokio::time::interval(std::time::Duration::from_millis(ms));
                    t.tick().await;
                    loop {
                        t.tick().await;
                        if itx.send(i).is_err() {
                            break;
                        }
                    }
                }));
            }
        }
    }
    let mut bc = rt.ws.bcast.subscribe();

    // Every outgoing frame goes through here; `false` means the socket is gone.
    async fn out(
        s: &Session,
        ws: &mut WebSocketStream<TokioIo<hyper::upgrade::Upgraded>>,
        m: Message,
    ) -> bool {
        s.log("out", &m);
        ws.send(m).await.is_ok()
    }

    'run: {
        for m in messages.iter().filter(|m| m.trigger == "connect") {
            match s.render(m, None) {
                Ok(msg) => {
                    if !out(&s, &mut ws, msg).await {
                        break 'run;
                    }
                    if let Some(code) = m.close_code {
                        out(&s, &mut ws, close_msg(code)).await;
                        break 'run;
                    }
                }
                Err(e) => s.push("out", "error", 0, e),
            }
        }
        loop {
            tokio::select! {
                incoming = ws.next() => {
                    let Some(Ok(msg)) = incoming else { break };
                    match &msg {
                        Message::Close(_) => { s.log("in", &msg); break; }
                        Message::Ping(_) => {
                            s.log("in", &msg);
                            s.push("out", "pong", 0, String::new());
                            continue;
                        }
                        Message::Pong(_) => { s.log("in", &msg); continue; }
                        Message::Frame(_) => continue,
                        _ => {}
                    }
                    s.log("in", &msg);
                    if !scripted {
                        if !out(&s, &mut ws, msg).await { break; }
                        continue;
                    }
                    let hit = messages.iter().find(|m| m.trigger == "message" && s.accepts(m, &msg));
                    match hit {
                        Some(m) => match s.render(m, Some(&msg)) {
                            Ok(reply) => {
                                if !out(&s, &mut ws, reply).await { break; }
                                if let Some(code) = m.close_code {
                                    out(&s, &mut ws, close_msg(code)).await;
                                    break;
                                }
                            }
                            Err(e) => s.push("out", "error", 0, e),
                        },
                        None if broadcast_mode => {
                            let _ = rt.ws.bcast.send(Bcast { from: conn_id.clone(), msg: msg.clone() });
                        }
                        None => {}
                    }
                }
                Some(i) = irx.recv() => {
                    if let Some(m) = messages.get(i) {
                        match s.render(m, None) {
                            Ok(msg) => { if !out(&s, &mut ws, msg).await { break; } }
                            Err(e) => s.push("out", "error", 0, e),
                        }
                    }
                }
                b = bc.recv() => {
                    if let Ok(b) = b && b.from != conn_id
                        && !out(&s, &mut ws, b.msg).await { break; }
                }
                cmd = rx.recv() => match cmd {
                    Some(Cmd::Send(m)) => { if !out(&s, &mut ws, m).await { break; } }
                    Some(Cmd::Close(code)) => {
                        out(&s, &mut ws, close_msg(code)).await;
                        break;
                    }
                    None => break,
                }
            }
        }
    }
    for t in timers {
        t.abort();
    }
    let _ = ws.close(None).await;
    rt.ws.conns.lock().unwrap().remove(&conn_id);
    let _ = c.engine.inner.events.send(EngineEvent::WsConnection {
        server_id: c.server_id.clone(),
        connection: info,
        open: false,
    });
}

/// One shared interval ticker per broadcast Route; it ends when the last client leaves.
#[allow(clippy::too_many_arguments)]
fn start_ticker(
    engine: &Engine,
    rt: &Arc<ServerRt>,
    route: &Route,
    headers: &[(String, String)],
    url: &str,
    params: &[(String, String)],
    ip: &str,
    server_id: &str,
    messages: &[WsMessage],
) {
    let intervals: Vec<(WsMessage, u64)> = messages
        .iter()
        .filter(|m| m.trigger == "interval")
        .filter_map(|m| Some((m.clone(), m.every.as_deref().and_then(parse_duration_ms)?)))
        .collect();
    if intervals.is_empty() {
        return;
    }
    let mut tickers = rt.ws.tickers.lock().unwrap();
    if tickers.get(&route.id).is_some_and(|h| !h.is_finished()) {
        return;
    }
    let mut s = Session {
        engine: engine.clone(),
        rt: rt.clone(),
        server_id: server_id.to_string(),
        route: route.clone(),
        conn_id: "broadcast".into(),
        headers: headers.to_vec(),
        url: url.to_string(),
        params: params.to_vec(),
        ip: ip.to_string(),
        counts: Default::default(),
        seq: 0,
    };
    let rt2 = rt.clone();
    let route_id = route.id.clone();
    let h = tokio::spawn(async move {
        let base = intervals
            .iter()
            .map(|(_, ms)| *ms)
            .min()
            .unwrap_or(1000)
            .max(10);
        let mut t = tokio::time::interval(std::time::Duration::from_millis(base));
        let mut elapsed = vec![0u64; intervals.len()];
        t.tick().await;
        loop {
            t.tick().await;
            let open = rt2
                .ws
                .conns
                .lock()
                .unwrap()
                .values()
                .any(|e| e.info.route_id == route_id);
            if !open {
                break;
            }
            for (i, (m, ms)) in intervals.iter().enumerate() {
                elapsed[i] += base;
                if elapsed[i] >= *ms {
                    elapsed[i] = 0;
                    if let Ok(msg) = s.render(m, None) {
                        s.log("out", &msg);
                        let _ = rt2.ws.bcast.send(Bcast {
                            from: "broadcast".into(),
                            msg,
                        });
                    }
                }
            }
        }
    });
    tickers.insert(route.id.clone(), h);
}

impl Engine {
    /// Live WebSocket connections of a Server (optionally one Route).
    pub fn ws_connections(
        &self,
        server: &str,
        route: Option<&str>,
    ) -> Result<Vec<WsConnectionInfo>, EngineError> {
        let (_, rt) = self.find(server)?;
        Ok(rt.ws.infos(route))
    }

    /// Timeline frames with `id > after`, oldest first (`limit` 0 = all).
    pub fn ws_messages(
        &self,
        server: &str,
        route: Option<&str>,
        connection: Option<&str>,
        after: u64,
        limit: usize,
    ) -> Result<Vec<WsFrame>, EngineError> {
        let (_, rt) = self.find(server)?;
        let q = rt.ws.frames.lock().unwrap();
        Ok(q.iter()
            .filter(|f| f.id > after)
            .filter(|f| route.is_none_or(|r| f.route_id == r))
            .filter(|f| connection.is_none_or(|c| f.connection == c))
            .take(if limit == 0 { usize::MAX } else { limit })
            .cloned()
            .collect())
    }

    /// Send an ad-hoc frame to one connection, or every connection of `route` (or of the
    /// Server). Binary bodies are base64. Returns how many clients received it.
    pub fn ws_send(
        &self,
        server: &str,
        route: Option<&str>,
        connection: Option<&str>,
        body: &str,
        binary: bool,
    ) -> Result<usize, EngineError> {
        let (_, rt) = self.find(server)?;
        let msg = build(body.to_string(), binary).map_err(EngineError::Request)?;
        let conns = rt.ws.conns.lock().unwrap();
        if let Some(c) = connection
            && !conns.contains_key(c)
        {
            return Err(EngineError::Request(format!("no open connection `{c}`")));
        }
        let mut n = 0;
        for e in conns.values() {
            let hit = match connection {
                Some(c) => e.info.id == c,
                None => route.is_none_or(|r| e.info.route_id == r),
            };
            if hit && e.tx.send(Cmd::Send(msg.clone())).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Like [`Engine::ws_send`], but `body` is a Tera template rendered once per recipient with
    /// that connection's context (`connection.id`, `request`, `vars`, ...). Valid JSON is then
    /// sent as `format` says (`compact` when `None`). Nothing is sent if any render fails.
    pub fn ws_send_rendered(
        &self,
        server: &str,
        route: Option<&str>,
        connection: Option<&str>,
        body: &str,
        binary: bool,
        format: Option<&str>,
    ) -> Result<usize, EngineError> {
        let (id, rt) = self.find(server)?;
        let targets: Vec<(
            mpsc::UnboundedSender<Cmd>,
            WsConnectionInfo,
            Counts,
            Arc<ReqCtx>,
        )> = {
            let conns = rt.ws.conns.lock().unwrap();
            if let Some(c) = connection
                && !conns.contains_key(c)
            {
                return Err(EngineError::Request(format!("no open connection `{c}`")));
            }
            conns
                .values()
                .filter(|e| match connection {
                    Some(c) => e.info.id == c,
                    None => route.is_none_or(|r| e.info.route_id == r),
                })
                .map(|e| {
                    (
                        e.tx.clone(),
                        e.info.clone(),
                        e.counts.clone(),
                        e.req.clone(),
                    )
                })
                .collect()
        };
        let msg = WsMessage {
            trigger: "message".into(),
            every: None,
            when: None,
            body: body.to_string(),
            format: format.map(str::to_string),
            binary,
            close_code: None,
            extra: Default::default(),
        };
        let mut out = Vec::new();
        for (tx, info, counts, req) in targets {
            let Some(r) = rt
                .def
                .lock()
                .unwrap()
                .routes
                .iter()
                .find(|r| r.id == info.route_id)
                .cloned()
            else {
                continue;
            };
            let mut s = Session {
                engine: self.clone(),
                rt: rt.clone(),
                server_id: id.clone(),
                route: r,
                conn_id: info.id.clone(),
                headers: req.headers.clone(),
                url: req.url.clone(),
                params: req.params.clone(),
                ip: req.ip.clone(),
                counts,
                seq: 0,
            };
            let m = s.render(&msg, None).map_err(EngineError::Request)?;
            out.push((tx, m));
        }
        let mut n = 0;
        for (tx, m) in out {
            if tx.send(Cmd::Send(m)).is_ok() {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Close one connection (code 1000 when `None`).
    pub fn ws_disconnect(
        &self,
        server: &str,
        connection: &str,
        code: Option<u16>,
    ) -> Result<(), EngineError> {
        let (_, rt) = self.find(server)?;
        let conns = rt.ws.conns.lock().unwrap();
        let e = conns
            .get(connection)
            .ok_or_else(|| EngineError::Request(format!("no open connection `{connection}`")))?;
        let _ = e.tx.send(Cmd::Close(code.unwrap_or(1000)));
        Ok(())
    }
}
