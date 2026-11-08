use std::collections::VecDeque;
use std::convert::Infallible;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use fauxfield_model::Server;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response as HyperResponse, StatusCode};
use hyper_util::rt::TokioIo;
use hyper_util::server::graceful::GracefulShutdown;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

use crate::problem;
use crate::recording::{Recording, Timing, body_text, redact};
use crate::router::{RouteTable, pick_response};
use crate::testreq::{self, TEST_HEADER, TestRequest, TestResponse};
use crate::time::{now_ms, rfc3339};

/// In-memory recordings kept per server (SPEC §4.2 `[recording] buffer`).
const DEFAULT_BUFFER: usize = 2000;
const DRAIN: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default)]
pub struct EngineConfig {
    /// Workspace root; `body_file` paths resolve inside it.
    pub root: PathBuf,
    /// Added to every server port (`fauxfield run --port-offset`).
    pub port_offset: u16,
    /// Replaces every server host (`fauxfield run --host`).
    pub host_override: Option<String>,
    /// Ring buffer size per server; 0 means the default (2000).
    pub buffer: usize,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ServerStatus {
    Stopped,
    Running { addr: String },
    Failed { error: String, port_in_use: bool },
}

impl ServerStatus {
    pub fn label(&self) -> &'static str {
        match self {
            ServerStatus::Stopped => "stopped",
            ServerStatus::Running { .. } => "running",
            ServerStatus::Failed { .. } => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub id: String,
    pub name: String,
    pub host: String,
    pub port: u16,
    pub enabled: bool,
    pub routes: usize,
    pub status: ServerStatus,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    Status {
        server_id: String,
        status: ServerStatus,
    },
    Traffic(Box<Recording>),
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("no server `{0}`")]
    UnknownServer(String),
    #[error("port {port} is already in use (server `{server}`)")]
    PortInUse { server: String, port: u16 },
    #[error("could not start `{server}`: {message}")]
    Bind { server: String, message: String },
    #[error("server `{0}` is not running")]
    NotRunning(String),
    #[error("{0}")]
    Request(String),
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TrafficFilter {
    pub server: Option<String>,
    pub route: Option<String>,
    /// Exact status (`404`) or class (`5xx`), already parsed as an inclusive range.
    pub status: Option<(u16, u16)>,
    pub since_ms: Option<u64>,
    pub after_id: Option<u64>,
    pub limit: Option<usize>,
}

/// `404` or a class such as `5xx`, as an inclusive status range.
pub fn parse_status_filter(s: &str) -> Result<(u16, u16), String> {
    let lower = s.to_ascii_lowercase();
    if let Some(d) = lower.strip_suffix("xx")
        && let Ok(n) = d.parse::<u16>()
        && (1..=5).contains(&n)
    {
        return Ok((n * 100, n * 100 + 99));
    }
    s.parse::<u16>()
        .map(|n| (n, n))
        .map_err(|_| format!("`{s}` is not a status like 404 or 5xx"))
}

struct Running {
    addr: SocketAddr,
    stop: watch::Sender<bool>,
    task: JoinHandle<()>,
}

struct ServerRt {
    def: Mutex<Server>,
    table: RwLock<Arc<RouteTable>>,
    ring: Mutex<VecDeque<Recording>>,
    run: Mutex<Option<Running>>,
    status: Mutex<ServerStatus>,
}

struct Inner {
    cfg: EngineConfig,
    servers: Mutex<Vec<(String, Arc<ServerRt>)>>,
    seq: AtomicU64,
    events: broadcast::Sender<EngineEvent>,
    handle: tokio::runtime::Handle,
}

/// The mock engine. Cheap to clone; shared by the GUI, the headless runner and MCP.
#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

impl Engine {
    /// `handle` is the host's Tokio runtime; listeners are spawned on it.
    pub fn new(handle: tokio::runtime::Handle, cfg: EngineConfig) -> Engine {
        let (events, _) = broadcast::channel(4096);
        Engine {
            inner: Arc::new(Inner {
                cfg,
                servers: Mutex::new(vec![]),
                seq: AtomicU64::new(1),
                events,
                handle,
            }),
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<EngineEvent> {
        self.inner.events.subscribe()
    }

    fn bind_of(&self, s: &Server) -> (String, u16) {
        let host = self
            .inner
            .cfg
            .host_override
            .clone()
            .unwrap_or_else(|| s.host.clone());
        (host, s.port.saturating_add(self.inner.cfg.port_offset))
    }

    fn find(&self, key: &str) -> Result<(String, Arc<ServerRt>), EngineError> {
        let list = self.inner.servers.lock().unwrap();
        list.iter()
            .find(|(id, rt)| {
                id == key
                    || rt.def.lock().unwrap().name == key
                    || slug(&rt.def.lock().unwrap().name) == key
            })
            .map(|(id, rt)| (id.clone(), rt.clone()))
            .ok_or_else(|| EngineError::UnknownServer(key.to_string()))
    }

    fn set_status(&self, id: &str, rt: &ServerRt, status: ServerStatus) {
        *rt.status.lock().unwrap() = status.clone();
        let _ = self.inner.events.send(EngineEvent::Status {
            server_id: id.to_string(),
            status,
        });
    }

    /// Replace the server definitions (hot reload). Route tables swap atomically; a listener
    /// restarts only when its host or port changed; removed or disabled servers stop.
    pub fn apply(&self, servers: Vec<Server>) {
        let mut to_restart = vec![];
        let mut to_stop = vec![];
        {
            let mut list = self.inner.servers.lock().unwrap();
            let keep: Vec<String> = servers.iter().map(|s| s.id.clone()).collect();
            list.retain(|(id, rt)| {
                let keep_it = keep.contains(id);
                if !keep_it {
                    to_stop.push((id.clone(), rt.clone()));
                }
                keep_it
            });
            for s in servers {
                let table = Arc::new(RouteTable::compile(&s));
                if let Some((_, rt)) = list.iter().find(|(id, _)| *id == s.id) {
                    let old = self.bind_of(&rt.def.lock().unwrap());
                    let new = self.bind_of(&s);
                    let disabled = !s.enabled;
                    *rt.def.lock().unwrap() = s.clone();
                    *rt.table.write().unwrap() = table;
                    if rt.run.lock().unwrap().is_some() {
                        if disabled {
                            to_stop.push((s.id.clone(), rt.clone()));
                        } else if old != new {
                            to_restart.push(s.id.clone());
                        }
                    }
                } else {
                    let id = s.id.clone();
                    list.push((
                        id,
                        Arc::new(ServerRt {
                            def: Mutex::new(s),
                            table: RwLock::new(table),
                            ring: Mutex::new(VecDeque::new()),
                            run: Mutex::new(None),
                            status: Mutex::new(ServerStatus::Stopped),
                        }),
                    ));
                }
            }
        }
        for (id, rt) in to_stop {
            self.stop_rt(&id, &rt);
        }
        for id in to_restart {
            let _ = self.restart(&id);
        }
    }

    pub fn list(&self) -> Vec<ServerInfo> {
        let list = self.inner.servers.lock().unwrap();
        list.iter()
            .map(|(id, rt)| {
                let d = rt.def.lock().unwrap();
                let (host, port) = self.bind_of(&d);
                ServerInfo {
                    id: id.clone(),
                    name: d.name.clone(),
                    host,
                    port,
                    enabled: d.enabled,
                    routes: rt.table.read().unwrap().len(),
                    status: rt.status.lock().unwrap().clone(),
                }
            })
            .collect()
    }

    /// Start one server (by id, name or slug). Idempotent when already running.
    pub fn start(&self, key: &str) -> Result<SocketAddr, EngineError> {
        let (id, rt) = self.find(key)?;
        if let Some(r) = rt.run.lock().unwrap().as_ref() {
            return Ok(r.addr);
        }
        let (host, port) = self.bind_of(&rt.def.lock().unwrap());
        let name = rt.def.lock().unwrap().name.clone();
        let bound = self.inner.handle.block_on_bind(&host, port);
        let listener = match bound {
            Ok(l) => l,
            Err(e) => {
                let in_use = e.kind() == std::io::ErrorKind::AddrInUse;
                self.set_status(
                    &id,
                    &rt,
                    ServerStatus::Failed {
                        error: e.to_string(),
                        port_in_use: in_use,
                    },
                );
                return Err(if in_use {
                    EngineError::PortInUse { server: name, port }
                } else {
                    EngineError::Bind {
                        server: name,
                        message: e.to_string(),
                    }
                });
            }
        };
        let addr = listener.local_addr().map_err(|e| EngineError::Bind {
            server: name.clone(),
            message: e.to_string(),
        })?;
        let (stop, stop_rx) = watch::channel(false);
        let task = self.inner.handle.spawn(serve(
            self.clone(),
            id.clone(),
            rt.clone(),
            listener,
            stop_rx,
        ));
        *rt.run.lock().unwrap() = Some(Running { addr, stop, task });
        self.set_status(
            &id,
            &rt,
            ServerStatus::Running {
                addr: addr.to_string(),
            },
        );
        Ok(addr)
    }

    /// Start every enabled, stopped server. Returns per-server errors.
    pub fn start_all(&self) -> Vec<(String, EngineError)> {
        let ids: Vec<String> = self
            .list()
            .into_iter()
            .filter(|s| s.enabled)
            .map(|s| s.id)
            .collect();
        ids.into_iter()
            .filter_map(|id| self.start(&id).err().map(|e| (id, e)))
            .collect()
    }

    fn stop_rt(&self, id: &str, rt: &ServerRt) {
        let running = rt.run.lock().unwrap().take();
        if let Some(r) = running {
            let _ = r.stop.send(true);
            // Wait for the drain so the port is free when this returns.
            let _ = self.inner.handle.block_on_join(r.task);
        }
        self.set_status(id, rt, ServerStatus::Stopped);
    }

    pub fn stop(&self, key: &str) -> Result<(), EngineError> {
        let (id, rt) = self.find(key)?;
        self.stop_rt(&id, &rt);
        Ok(())
    }

    pub fn stop_all(&self) {
        let all: Vec<_> = self.inner.servers.lock().unwrap().clone();
        for (id, rt) in all {
            self.stop_rt(&id, &rt);
        }
    }

    pub fn restart(&self, key: &str) -> Result<SocketAddr, EngineError> {
        let (id, rt) = self.find(key)?;
        self.stop_rt(&id, &rt);
        self.start(&id)
    }

    pub fn running_count(&self) -> usize {
        self.list()
            .iter()
            .filter(|s| matches!(s.status, ServerStatus::Running { .. }))
            .count()
    }

    /// Recordings across servers, oldest first, after filtering.
    pub fn traffic(&self, f: &TrafficFilter) -> Vec<Recording> {
        let servers: Vec<(String, Arc<ServerRt>)> = self.inner.servers.lock().unwrap().clone();
        let mut out: Vec<Recording> = vec![];
        for (id, rt) in servers {
            if let Some(want) = &f.server {
                let d = rt.def.lock().unwrap();
                if *want != id && *want != d.name && *want != slug(&d.name) {
                    continue;
                }
            }
            out.extend(
                rt.ring
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|r| {
                        f.route.as_ref().is_none_or(|x| *x == r.route)
                            && f.status
                                .is_none_or(|(lo, hi)| (lo..=hi).contains(&r.status))
                            && f.since_ms.is_none_or(|t| r.ts_ms >= t)
                            && f.after_id.is_none_or(|a| r.id > a)
                    })
                    .cloned(),
            );
        }
        out.sort_by_key(|r| r.id);
        if let Some(n) = f.limit
            && out.len() > n
        {
            out.drain(..out.len() - n);
        }
        out
    }

    pub fn clear_traffic(&self) {
        for (_, rt) in self.inner.servers.lock().unwrap().iter() {
            rt.ring.lock().unwrap().clear();
        }
    }

    /// Fire a request at a running server over loopback (Send test, SPEC §5.1).
    pub async fn send_test(
        &self,
        key: &str,
        req: &TestRequest,
    ) -> Result<TestResponse, EngineError> {
        let (_, rt) = self.find(key)?;
        let addr = rt
            .run
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| r.addr)
            .ok_or_else(|| EngineError::NotRunning(key.to_string()))?;
        // Always loopback, even when the server binds 0.0.0.0.
        let target = SocketAddr::from(([127, 0, 0, 1], addr.port()));
        let target = if addr.is_ipv6() {
            SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], addr.port()))
        } else {
            target
        };
        testreq::send(target, req)
            .await
            .map_err(EngineError::Request)
    }

    /// Base URL of a running server, for cURL.
    pub fn base_url(&self, key: &str) -> Option<String> {
        let (_, rt) = self.find(key).ok()?;
        let r = rt.run.lock().unwrap();
        r.as_ref().map(|r| format!("http://{}", r.addr))
    }

    fn record(&self, rt: &ServerRt, rec: Recording) {
        {
            let mut ring = rt.ring.lock().unwrap();
            let cap = if self.inner.cfg.buffer == 0 {
                DEFAULT_BUFFER
            } else {
                self.inner.cfg.buffer
            };
            while ring.len() >= cap {
                ring.pop_front();
            }
            ring.push_back(rec.clone());
        }
        let _ = self.inner.events.send(EngineEvent::Traffic(Box::new(rec)));
    }
}

fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}

// Bridges for calling async-only bits from the synchronous control surface (GUI and CLI
// callers are not inside the runtime).
trait HandleExt {
    fn block_on_bind(&self, host: &str, port: u16) -> std::io::Result<TcpListener>;
    fn block_on_join(&self, task: JoinHandle<()>) -> Result<(), tokio::task::JoinError>;
}

impl HandleExt for tokio::runtime::Handle {
    fn block_on_bind(&self, host: &str, port: u16) -> std::io::Result<TcpListener> {
        let host = host.to_string();
        let (tx, rx) = std::sync::mpsc::channel();
        self.spawn(async move {
            let _ = tx.send(TcpListener::bind((host.as_str(), port)).await);
        });
        rx.recv()
            .unwrap_or_else(|_| Err(std::io::Error::other("runtime stopped")))
    }
    fn block_on_join(&self, task: JoinHandle<()>) -> Result<(), tokio::task::JoinError> {
        let (tx, rx) = std::sync::mpsc::channel();
        self.spawn(async move {
            let _ = tx.send(task.await);
        });
        rx.recv().unwrap_or(Ok(()))
    }
}

async fn serve(
    engine: Engine,
    id: String,
    rt: Arc<ServerRt>,
    listener: TcpListener,
    mut stop: watch::Receiver<bool>,
) {
    let graceful = GracefulShutdown::new();
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            accepted = listener.accept() => {
                let Ok((stream, _)) = accepted else { continue };
                let eng = engine.clone();
                let rt2 = rt.clone();
                let id2 = id.clone();
                let svc = service_fn(move |req| {
                    let (eng, rt, id) = (eng.clone(), rt2.clone(), id2.clone());
                    async move { Ok::<_, Infallible>(handle(eng, id, rt, req).await) }
                });
                let conn = hyper::server::conn::http1::Builder::new()
                    .serve_connection(TokioIo::new(stream), svc);
                let watched = graceful.watch(conn);
                tokio::spawn(async move {
                    let _ = watched.await;
                });
            }
        }
    }
    drop(listener);
    let _ = tokio::time::timeout(DRAIN, graceful.shutdown()).await;
}

fn json_response(
    status: u16,
    body: serde_json::Value,
    ct: &str,
) -> (u16, Vec<(String, String)>, Bytes) {
    (
        status,
        vec![("content-type".into(), ct.into())],
        Bytes::from(body.to_string()),
    )
}

async fn handle(
    engine: Engine,
    id: String,
    rt: Arc<ServerRt>,
    req: Request<Incoming>,
) -> HyperResponse<Full<Bytes>> {
    let started = Instant::now();
    let (parts, body) = req.parse_body().await;
    let method = parts.method.as_str().to_string();
    let url = parts
        .uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let path = parts.uri.path().to_string();
    let req_headers: Vec<(String, String)> = parts
        .headers
        .iter()
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    let is_test = parts.headers.contains_key(TEST_HEADER);

    let match_start = Instant::now();
    let table = rt.table.read().unwrap().clone();
    let route = table.find(&method, &path).cloned();
    let match_us = match_start.elapsed().as_micros() as u64;

    let mut delay_ms = 0;
    let (route_id, (status, headers, bytes)) = match &route {
        None => {
            let closest = table.closest(&method, &path);
            (
                "unmatched".to_string(),
                json_response(
                    404,
                    problem::not_found(&method, &path, &closest, table.unmatched_hint),
                    "application/problem+json",
                ),
            )
        }
        Some(r) => {
            let out = match pick_response(r) {
                None => (200, vec![], Bytes::new()),
                Some(resp) => {
                    delay_ms = resp.delay_ms.unwrap_or(0);
                    if delay_ms > 0 {
                        tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                    }
                    build_response(&engine.inner.cfg.root, resp, &path)
                }
            };
            (r.id.clone(), out)
        }
    };

    let (server_name, _) = {
        let d = rt.def.lock().unwrap();
        (d.name.clone(), ())
    };
    let (req_body, req_cut) = body_text(&body);
    let (resp_body, resp_cut) = body_text(&bytes);
    let ts_ms = now_ms();
    engine.record(
        &rt,
        Recording {
            id: engine.inner.seq.fetch_add(1, Ordering::Relaxed),
            ts: rfc3339(ts_ms),
            ts_ms,
            server_id: id,
            server: server_name,
            route: route_id,
            method,
            url,
            request_headers: redact(req_headers),
            request_body: req_body,
            status,
            response_headers: redact(headers.clone()),
            response_body: resp_body,
            timing: Timing {
                match_us,
                delay_ms,
                total_ms: started.elapsed().as_millis() as u64,
            },
            truncated: req_cut || resp_cut,
            source: if is_test { "test" } else { "mock" }.into(),
        },
    );

    let mut b = HyperResponse::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    for (k, v) in &headers {
        b = b.header(k.as_str(), v.as_str());
    }
    b.body(Full::new(bytes)).unwrap_or_else(|_| {
        HyperResponse::new(Full::new(Bytes::from_static(b"invalid response headers")))
    })
}

fn build_response(
    root: &std::path::Path,
    resp: &fauxfield_model::Response,
    path: &str,
) -> (u16, Vec<(String, String)>, Bytes) {
    let mut headers: Vec<(String, String)> = resp
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect();
    let body: Bytes = if let Some(file) = &resp.body_file {
        match read_confined(root, file) {
            Ok(b) => Bytes::from(b),
            Err(e) => {
                return json_response(
                    500,
                    problem::server_error(&format!("body_file `{file}`: {e}"), path),
                    "application/problem+json",
                );
            }
        }
    } else {
        Bytes::from(resp.body.clone().unwrap_or_default())
    };
    if let Some(ct) = &resp.content_type {
        headers.retain(|(k, _)| k != "content-type");
        headers.push(("content-type".into(), ct.clone()));
    } else if !body.is_empty() && !headers.iter().any(|(k, _)| k == "content-type") {
        headers.push(("content-type".into(), "text/plain; charset=utf-8".into()));
    }
    let status = if (100..=599).contains(&resp.status) {
        resp.status
    } else {
        500
    };
    (status, headers, body)
}

fn read_confined(root: &std::path::Path, file: &str) -> Result<Vec<u8>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let full = root.join(file).canonicalize().map_err(|e| e.to_string())?;
    if !full.starts_with(&root) {
        return Err("path escapes the workspace".into());
    }
    std::fs::read(full).map_err(|e| e.to_string())
}

trait ParseBody {
    async fn parse_body(self) -> (hyper::http::request::Parts, Bytes);
}

impl ParseBody for Request<Incoming> {
    async fn parse_body(self) -> (hyper::http::request::Parts, Bytes) {
        let (parts, body) = self.into_parts();
        let bytes = body
            .collect()
            .await
            .map(|c| c.to_bytes())
            .unwrap_or_default();
        (parts, bytes)
    }
}
