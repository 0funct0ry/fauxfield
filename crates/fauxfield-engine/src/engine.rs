use std::collections::VecDeque;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use bytes::Bytes;
use fauxfield_model::Server;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use hyper::service::service_fn;
use hyper::{Request, Response as HyperResponse, StatusCode};
use hyper_util::server::graceful::GracefulShutdown;
use serde::{Deserialize, Serialize};
use tokio::net::TcpListener;
use tokio::sync::{broadcast, watch};
use tokio::task::JoinHandle;

mod proxy;
pub use proxy::RespBody;

use crate::problem;
use crate::recording::{Recording, Timing, body_text, redact};
use crate::router::RouteTable;
use crate::sim::{self, BodyMode, Fault, FaultIo, Limiter, NetworkProfile, SimBody};
use crate::testreq::{self, TEST_HEADER, TestRequest, TestResponse};
use crate::time::{now_ms, rfc3339};
use crate::tls::Conn;

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
    /// Runtime network profile at start (`fauxfield run --network`).
    pub network: Option<NetworkProfile>,
    /// Chaos fault rate for every Server at start (`fauxfield run --chaos`).
    pub chaos: Option<f64>,
    /// Where the dev CA lives (`<dir>/ca/`); default is the platform data dir (SPEC §5.8).
    pub data_dir: Option<PathBuf>,
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
    /// Chaos fault rate while the chaos switch is on (runtime only, SPEC §5.6).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub chaos: Option<f64>,
    /// Serves HTTPS (SPEC §5.8).
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub https: bool,
    /// Certificate expiry details while a TLS server is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cert: Option<crate::tls::CertStatus>,
}

#[derive(Debug, Clone)]
pub enum EngineEvent {
    /// The workspace network profile changed.
    Network {
        profile: NetworkProfile,
    },
    /// A Server's chaos switch changed.
    Chaos {
        server_id: String,
        rate: Option<f64>,
    },
    Status {
        server_id: String,
        status: ServerStatus,
    },
    Traffic(Box<Recording>),
    /// A Dataset's rows changed (CRUD request, reset, import).
    Dataset {
        server_id: String,
        name: String,
        rows: usize,
    },
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
    #[error("{0} is disabled. Enable it first.")]
    Disabled(String),
    #[error("{0}")]
    Request(String),
    #[error("no dataset `{0}`")]
    UnknownDataset(String),
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
    /// The `also_http_port` listener.
    extra: Option<JoinHandle<()>>,
    https: bool,
    cert: Option<crate::tls::CertStatus>,
}

struct ServerRt {
    def: Mutex<Server>,
    table: RwLock<Arc<RouteTable>>,
    ring: Mutex<VecDeque<Recording>>,
    run: Mutex<Option<Running>>,
    status: Mutex<ServerStatus>,
    counters: crate::rules::Counters,
    compiled: crate::rules::Compiled,
    seq: crate::template::Sequences,
    data: Mutex<crate::data::DataStore>,
    limiter: Limiter,
    chaos: Mutex<Option<f64>>,
}

struct Inner {
    cfg: EngineConfig,
    servers: Mutex<Vec<(String, Arc<ServerRt>)>>,
    seq: AtomicU64,
    events: broadcast::Sender<EngineEvent>,
    handle: tokio::runtime::Handle,
    vars: RwLock<crate::vars::VarContext>,
    profile: RwLock<NetworkProfile>,
    offline: watch::Sender<bool>,
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
        let profile = cfg.network.unwrap_or_default();
        Engine {
            inner: Arc::new(Inner {
                profile: RwLock::new(profile),
                offline: watch::channel(profile == NetworkProfile::Offline).0,
                cfg,
                servers: Mutex::new(vec![]),
                seq: AtomicU64::new(1),
                events,
                handle,
                vars: RwLock::new(Default::default()),
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
                let table = Arc::new(self.compile(&s));
                if let Some((_, rt)) = list.iter().find(|(id, _)| *id == s.id) {
                    let old = self.bind_of(&rt.def.lock().unwrap());
                    let old_tls = rt.def.lock().unwrap().tls.clone();
                    let new = self.bind_of(&s);
                    let disabled = !s.enabled;
                    *rt.def.lock().unwrap() = s.clone();
                    *rt.table.write().unwrap() = table;
                    rt.compiled.clear();
                    rt.data
                        .lock()
                        .unwrap()
                        .sync(&self.inner.cfg.root, &s.id, &s);
                    if rt.run.lock().unwrap().is_some() {
                        if disabled {
                            to_stop.push((s.id.clone(), rt.clone()));
                        } else if old != new || old_tls != s.tls {
                            to_restart.push(s.id.clone());
                        }
                    }
                } else {
                    let id = s.id.clone();
                    let mut data = crate::data::DataStore::default();
                    data.sync(&self.inner.cfg.root, &id, &s);
                    list.push((
                        id,
                        Arc::new(ServerRt {
                            data: Mutex::new(data),
                            def: Mutex::new(s),
                            table: RwLock::new(table),
                            ring: Mutex::new(VecDeque::new()),
                            run: Mutex::new(None),
                            status: Mutex::new(ServerStatus::Stopped),
                            counters: Default::default(),
                            compiled: Default::default(),
                            seq: Default::default(),
                            limiter: Default::default(),
                            chaos: Mutex::new(self.inner.cfg.chaos),
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
        // Row counts may have changed (new or reseeded Datasets): tell the GUI.
        let all: Vec<(String, Arc<ServerRt>)> = self.inner.servers.lock().unwrap().clone();
        for (id, rt) in all {
            for (name, rows) in rt.data.lock().unwrap().counts() {
                let _ = self.inner.events.send(EngineEvent::Dataset {
                    server_id: id.clone(),
                    name,
                    rows,
                });
            }
        }
    }

    pub fn list(&self) -> Vec<ServerInfo> {
        let list = self.inner.servers.lock().unwrap();
        list.iter()
            .map(|(id, rt)| {
                let d = rt.def.lock().unwrap();
                let (host, port) = self.bind_of(&d);
                let run = rt
                    .run
                    .lock()
                    .unwrap()
                    .as_ref()
                    .map(|r| (r.https, r.cert.clone()));
                ServerInfo {
                    chaos: *rt.chaos.lock().unwrap(),
                    https: run.as_ref().is_some_and(|r| r.0),
                    cert: run.and_then(|r| r.1),
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
        if !rt.def.lock().unwrap().enabled {
            let name = rt.def.lock().unwrap().name.clone();
            return Err(EngineError::Disabled(name));
        }
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
        let def = rt.def.lock().unwrap().clone();
        let tls = match def.tls.as_ref().filter(|t| t.enabled) {
            None => None,
            Some(_) => match crate::tls::build(
                &def,
                &self.inner.cfg.root,
                self.inner.cfg.data_dir.as_deref(),
                &host,
            ) {
                Ok(t) => Some(t),
                Err(message) => {
                    self.set_status(
                        &id,
                        &rt,
                        ServerStatus::Failed {
                            error: format!("TLS: {message}"),
                            port_in_use: false,
                        },
                    );
                    return Err(EngineError::Bind {
                        server: name,
                        message: format!("TLS: {message}"),
                    });
                }
            },
        };
        let also = def
            .tls
            .as_ref()
            .filter(|t| t.enabled)
            .and_then(|t| t.also_http_port)
            .map(|p| p.saturating_add(self.inner.cfg.port_offset));
        let extra_listener = match also {
            None => None,
            Some(p) => match self.inner.handle.block_on_bind(&host, p) {
                Ok(l) => Some(l),
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
                        EngineError::PortInUse {
                            server: name,
                            port: p,
                        }
                    } else {
                        EngineError::Bind {
                            server: name,
                            message: e.to_string(),
                        }
                    });
                }
            },
        };
        let (stop, stop_rx) = watch::channel(false);
        let https = tls.is_some();
        let cert = tls.as_ref().and_then(|t| t.status.clone());
        let extra = extra_listener.map(|l| {
            self.inner.handle.spawn(serve(
                self.clone(),
                id.clone(),
                rt.clone(),
                l,
                None,
                stop_rx.clone(),
            ))
        });
        let task = self.inner.handle.spawn(serve(
            self.clone(),
            id.clone(),
            rt.clone(),
            listener,
            tls.map(|t| t.acceptor),
            stop_rx,
        ));
        *rt.run.lock().unwrap() = Some(Running {
            addr,
            stop,
            task,
            extra,
            https,
            cert,
        });
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
            if let Some(x) = r.extra {
                let _ = self.inner.handle.block_on_join(x);
            }
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

    /// Reset runtime state (SPEC §6.4): Datasets back to their seed (persisted state deleted),
    /// per-route call counters, sequential cursors and runtime variables. `server` limits the
    /// reset to one Server (id, name or slug); runtime variables are cleared only for all.
    pub fn reset_state(&self, server: Option<&str>) -> Result<(), EngineError> {
        let targets: Vec<(String, Arc<ServerRt>)> = match server {
            Some(k) => vec![self.find(k)?],
            None => self.inner.servers.lock().unwrap().clone(),
        };
        for (id, rt) in &targets {
            rt.counters.reset();
            rt.seq.lock().unwrap().clear();
            rt.limiter.clear();
            self.reset_datasets(id, rt, None);
        }
        if server.is_none() {
            self.inner.vars.write().unwrap().runtime.clear();
        }
        self.recompile();
        Ok(())
    }

    fn reset_datasets(&self, id: &str, rt: &ServerRt, only: Option<&str>) {
        let def = rt.def.lock().unwrap().clone();
        let counts: Vec<(String, usize)> = {
            let mut d = rt.data.lock().unwrap();
            d.reset(&self.inner.cfg.root, id, &def, only);
            d.sets
                .iter()
                .filter(|s| only.is_none_or(|n| n == s.def.name))
                .map(|s| (s.def.name.clone(), s.len()))
                .collect()
        };
        for (name, rows) in counts {
            let _ = self.inner.events.send(EngineEvent::Dataset {
                server_id: id.to_string(),
                name,
                rows,
            });
        }
    }

    /// Reset one Dataset to its seed.
    pub fn dataset_reset(&self, server: &str, name: &str) -> Result<(), EngineError> {
        let (id, rt) = self.find(server)?;
        if rt.data.lock().unwrap().get(name).is_none() {
            return Err(EngineError::UnknownDataset(name.into()));
        }
        self.reset_datasets(&id, &rt, Some(name));
        Ok(())
    }

    /// Every Dataset of every Server (or one Server) with its live row count.
    pub fn datasets(
        &self,
        server: Option<&str>,
    ) -> Result<Vec<crate::data::DatasetInfo>, EngineError> {
        let list: Vec<(String, Arc<ServerRt>)> = match server {
            Some(k) => vec![self.find(k)?],
            None => self.inner.servers.lock().unwrap().clone(),
        };
        Ok(list
            .iter()
            .flat_map(|(id, rt)| {
                let name = rt.def.lock().unwrap().name.clone();
                rt.data.lock().unwrap().info(id, &name)
            })
            .collect())
    }

    /// Live rows of one Dataset.
    pub fn dataset_rows(&self, server: &str, name: &str) -> Result<serde_json::Value, EngineError> {
        let (_, rt) = self.find(server)?;
        rt.data
            .lock()
            .unwrap()
            .get(name)
            .map(|s| s.rows.clone())
            .ok_or_else(|| EngineError::UnknownDataset(name.into()))
    }

    /// A window of a Dataset's rows without cloning the rest: `(total, rows[start..start+limit])`.
    /// An object (singleton) Dataset is one row.
    pub fn dataset_page(
        &self,
        server: &str,
        name: &str,
        start: usize,
        limit: usize,
    ) -> Result<(usize, Vec<serde_json::Value>), EngineError> {
        let (_, rt) = self.find(server)?;
        let d = rt.data.lock().unwrap();
        let ds = d
            .get(name)
            .ok_or_else(|| EngineError::UnknownDataset(name.into()))?;
        Ok(match &ds.rows {
            serde_json::Value::Array(a) => {
                (a.len(), a.iter().skip(start).take(limit).cloned().collect())
            }
            other => (
                1,
                if start == 0 {
                    vec![other.clone()]
                } else {
                    vec![]
                },
            ),
        })
    }

    /// Replace a Dataset's live rows (inline edit, `data import`).
    pub fn dataset_set(
        &self,
        server: &str,
        name: &str,
        rows: serde_json::Value,
    ) -> Result<(), EngineError> {
        if !rows.is_array() && !rows.is_object() {
            return Err(EngineError::Request(
                "Dataset rows must be a JSON array or object.".into(),
            ));
        }
        let (id, rt) = self.find(server)?;
        let n = {
            let mut d = rt.data.lock().unwrap();
            if !d.replace(&self.inner.cfg.root, &id, name, rows) {
                return Err(EngineError::UnknownDataset(name.into()));
            }
            d.get(name).map_or(0, |s| s.len())
        };
        let _ = self.inner.events.send(EngineEvent::Dataset {
            server_id: id,
            name: name.into(),
            rows: n,
        });
        Ok(())
    }

    fn dataset_event(&self, id: String, name: &str, rt: &ServerRt) {
        let n = rt.data.lock().unwrap().get(name).map_or(0, |s| s.len());
        let _ = self.inner.events.send(EngineEvent::Dataset {
            server_id: id,
            name: name.into(),
            rows: n,
        });
    }

    /// Set one cell of a live row (M10.04).
    pub fn dataset_update_cell(
        &self,
        server: &str,
        name: &str,
        index: usize,
        column: &str,
        val: serde_json::Value,
    ) -> Result<(), EngineError> {
        let (id, rt) = self.find(server)?;
        rt.data
            .lock()
            .unwrap()
            .update_cell(&self.inner.cfg.root, &id, name, index, column, val)
            .map_err(|e| self.data_err(name, e))?;
        self.dataset_event(id, name, &rt);
        Ok(())
    }

    /// Insert live rows at `at` (append when `None`); returns the first new row's index.
    pub fn dataset_insert(
        &self,
        server: &str,
        name: &str,
        at: Option<usize>,
        rows: Vec<serde_json::Value>,
    ) -> Result<usize, EngineError> {
        let (id, rt) = self.find(server)?;
        let at = rt
            .data
            .lock()
            .unwrap()
            .insert_rows(&self.inner.cfg.root, &id, name, at, rows)
            .map_err(|e| self.data_err(name, e))?;
        self.dataset_event(id, name, &rt);
        Ok(at)
    }

    /// Delete live rows by index; returns how many were removed.
    pub fn dataset_delete(
        &self,
        server: &str,
        name: &str,
        indices: &[usize],
    ) -> Result<usize, EngineError> {
        let (id, rt) = self.find(server)?;
        let n = rt
            .data
            .lock()
            .unwrap()
            .delete_rows(&self.inner.cfg.root, &id, name, indices)
            .map_err(|e| self.data_err(name, e))?;
        self.dataset_event(id, name, &rt);
        Ok(n)
    }

    /// Append copies of live rows with fresh key and unique values; returns the new indices.
    pub fn dataset_duplicate(
        &self,
        server: &str,
        name: &str,
        indices: &[usize],
    ) -> Result<Vec<usize>, EngineError> {
        let (id, rt) = self.find(server)?;
        let out = rt
            .data
            .lock()
            .unwrap()
            .duplicate_rows(&self.inner.cfg.root, &id, name, indices)
            .map_err(|e| self.data_err(name, e))?;
        self.dataset_event(id, name, &rt);
        Ok(out)
    }

    /// Empty a Dataset's live rows (the seed file is untouched; Reset restores it).
    pub fn dataset_clear(&self, server: &str, name: &str) -> Result<(), EngineError> {
        let empty = match self.dataset_rows(server, name)? {
            serde_json::Value::Object(_) => serde_json::json!({}),
            _ => serde_json::json!([]),
        };
        self.dataset_set(server, name, empty)
    }

    fn data_err(&self, name: &str, e: String) -> EngineError {
        if e == "unknown dataset" {
            EngineError::UnknownDataset(name.into())
        } else {
            EngineError::Request(e)
        }
    }

    /// Compile a Route table with the Server `prefix` templated against the current variables
    /// (SPEC §4.4: templated once at start; recompiled when the variable context changes).
    fn compile(&self, s: &Server) -> RouteTable {
        let mut s = s.clone();
        if let Some(p) = s.prefix.as_deref()
            && (p.contains("{{") || p.contains("{%"))
            && let Ok(r) = self.render_str(&s, p)
        {
            s.prefix = Some(r.trim().to_string());
        }
        RouteTable::compile(&s)
    }

    /// Render a Server-level template string (prefix, proxy upstream and headers) against the
    /// current variables. Plain text is returned unchanged.
    pub(crate) fn render_str(&self, s: &Server, text: &str) -> Result<String, String> {
        if !text.contains("{{") && !text.contains("{%") {
            return Ok(text.to_string());
        }
        let vc = self.inner.vars.read().unwrap();
        let ctx = serde_json::json!({
            "vars": vc.vars_json(s, None),
            "env": vc.active_env,
            "state": vc.state_json(),
            "server": { "name": s.name, "port": s.port },
        });
        let input = crate::template::Input {
            context: ctx,
            root: &self.inner.cfg.root,
            seed: s.fake_seed,
            route_id: "",
            counter: 0,
            seq: None,
        };
        crate::template::render(text, &input).map_err(|e| e.to_string())
    }

    /// Recompile every Route table and drop rule caches (variables changed). No restart.
    fn recompile(&self) {
        let list = self.inner.servers.lock().unwrap();
        for (_, rt) in list.iter() {
            let def = rt.def.lock().unwrap().clone();
            *rt.table.write().unwrap() = Arc::new(self.compile(&def));
            rt.compiled.clear();
        }
    }

    /// Replace the variable context (Environment switched, variables edited, files reloaded).
    /// Route tables are recompiled in place; no listener restarts.
    pub fn set_var_context(&self, vc: crate::vars::VarContext) {
        *self.inner.vars.write().unwrap() = vc;
        self.recompile();
    }

    pub fn var_context(&self) -> crate::vars::VarContext {
        self.inner.vars.read().unwrap().clone()
    }

    /// Switch the active Environment live.
    pub fn set_active_env(&self, name: &str) {
        self.inner.vars.write().unwrap().active_env = name.to_string();
        self.recompile();
    }

    /// Set or unset a runtime variable (lives until state reset).
    pub fn set_runtime_var(&self, key: &str, val: Option<&str>) {
        {
            let mut v = self.inner.vars.write().unwrap();
            match val {
                Some(x) => {
                    v.runtime.insert(key.to_string(), x.to_string());
                }
                None => {
                    v.runtime.remove(key);
                }
            }
        }
        self.recompile();
    }

    /// Variables in scope for a Server key (id, name or slug) and optional Route.
    pub fn vars_in_scope(
        &self,
        server: &str,
        route_id: Option<&str>,
    ) -> Result<Vec<fauxfield_model::vars::ResolvedVar>, EngineError> {
        let (_, rt) = self.find(server)?;
        let def = rt.def.lock().unwrap().clone();
        Ok(self.inner.vars.read().unwrap().resolve(&def, route_id))
    }

    /// The workspace-wide runtime network profile.
    pub fn network_profile(&self) -> NetworkProfile {
        *self.inner.profile.read().unwrap()
    }

    /// Override every Server's `[network]` at runtime; `Offline` refuses new connections.
    pub fn set_network_profile(&self, p: NetworkProfile) {
        *self.inner.profile.write().unwrap() = p;
        self.inner
            .offline
            .send_replace(p == NetworkProfile::Offline);
        let _ = self.inner.events.send(EngineEvent::Network { profile: p });
    }

    /// Chaos fault rate of a Server (None = off).
    pub fn chaos(&self, key: &str) -> Result<Option<f64>, EngineError> {
        let (_, rt) = self.find(key)?;
        Ok(*rt.chaos.lock().unwrap())
    }

    /// Turn a Server's chaos switch on (rate 0..=1) or off. `key` `None` means every Server.
    pub fn set_chaos(&self, key: Option<&str>, rate: Option<f64>) -> Result<(), EngineError> {
        let rate = rate.map(|r| r.clamp(0.0, 1.0));
        let targets: Vec<(String, Arc<ServerRt>)> = match key {
            Some(k) => vec![self.find(k)?],
            None => self.inner.servers.lock().unwrap().clone(),
        };
        for (id, rt) in targets {
            *rt.chaos.lock().unwrap() = rate;
            let _ = self.inner.events.send(EngineEvent::Chaos {
                server_id: id,
                rate,
            });
        }
        Ok(())
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
        let (addr, https) = rt
            .run
            .lock()
            .unwrap()
            .as_ref()
            .map(|r| (r.addr, r.https))
            .ok_or_else(|| EngineError::NotRunning(key.to_string()))?;
        // Always loopback, even when the server binds 0.0.0.0.
        let target = SocketAddr::from(([127, 0, 0, 1], addr.port()));
        let target = if addr.is_ipv6() {
            SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], addr.port()))
        } else {
            target
        };
        let before = rt.ring.lock().unwrap().back().map(|r| r.id);
        let mut res = testreq::send(
            target,
            req,
            https.then(|| crate::tls::client_config(self.inner.cfg.data_dir.as_deref())),
        )
        .await
        .map_err(EngineError::Request)?;
        // The Recording of this request carries the Dataset changes its actions made.
        res.dataset_changes = rt
            .ring
            .lock()
            .unwrap()
            .iter()
            .rev()
            .take_while(|r| Some(r.id) != before)
            .find(|r| r.source == "test" && r.method == req.method)
            .map(|r| r.dataset_changes.clone())
            .unwrap_or_default();
        Ok(res)
    }

    /// Base URL of a running server, for cURL.
    pub fn base_url(&self, key: &str) -> Option<String> {
        let (_, rt) = self.find(key).ok()?;
        let r = rt.run.lock().unwrap();
        r.as_ref()
            .map(|r| format!("{}://{}", if r.https { "https" } else { "http" }, r.addr))
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
    acceptor: Option<tokio_rustls::TlsAcceptor>,
    mut stop: watch::Receiver<bool>,
) {
    let graceful = GracefulShutdown::new();
    let addr = listener.local_addr().ok();
    let mut offline = engine.inner.offline.subscribe();
    let mut listener = if *offline.borrow() {
        None
    } else {
        Some(listener)
    };
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = offline.changed() => {
                if *offline.borrow() {
                    // Offline: really refuse connections until it is lifted.
                    listener = None;
                } else if listener.is_none() {
                    for _ in 0..40 {
                        if let Some(a) = addr
                            && let Ok(l) = TcpListener::bind(a).await
                        {
                            listener = Some(l);
                            break;
                        }
                        tokio::time::sleep(Duration::from_millis(50)).await;
                    }
                }
            }
            accepted = async {
                match &listener {
                    Some(l) => l.accept().await,
                    None => std::future::pending().await,
                }
            } => {
                let Ok((stream, peer)) = accepted else { continue };
                let ip = peer.ip().to_string();
                let eng = engine.clone();
                let rt2 = rt.clone();
                let id2 = id.clone();
                let watcher = graceful.watcher();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    // The TLS handshake runs here so a slow client never blocks accept.
                    let conn = match acceptor {
                        None => Conn::Plain(stream),
                        Some(a) => match a.accept(stream).await {
                            Ok(t) => Conn::Tls(Box::new(t)),
                            Err(_) => return,
                        },
                    };
                    let (io, reset) = FaultIo::new(conn);
                    let svc = service_fn(move |req| {
                        let (eng, rt, id, ip, reset) =
                            (eng.clone(), rt2.clone(), id2.clone(), ip.clone(), reset.clone());
                        async move { handle(eng, id, rt, ip, reset, req).await }
                    });
                    let conn = hyper::server::conn::http1::Builder::new().serve_connection(io, svc);
                    let _ = watcher.watch(conn).await;
                });
            }
        }
    }
    drop(listener);
    let _ = tokio::time::timeout(DRAIN, graceful.shutdown()).await;
}

/// Returned by the service to abort a connection (fault `reset`).
#[derive(Debug)]
struct ConnAbort;
impl std::fmt::Display for ConnAbort {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("connection aborted by fault injection")
    }
}
impl std::error::Error for ConnAbort {}

pub(crate) fn json_response(
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
    ip: String,
    reset: Arc<std::sync::atomic::AtomicBool>,
    req: Request<Incoming>,
) -> Result<HyperResponse<RespBody>, ConnAbort> {
    let started = Instant::now();
    let (parts, body_in) = req.into_parts();
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
    let found = table.find(&method, &path);
    let route = found.as_ref().map(|m| m.route.clone());
    let (params, route_pattern) = found
        .map(|m| {
            (
                m.params
                    .into_iter()
                    .map(|(k, v)| {
                        let v = crate::router::mask_param(&k, &v);
                        (k, v)
                    })
                    .collect::<Vec<_>>(),
                m.pattern,
            )
        })
        .unwrap_or_default();
    let match_us = match_start.elapsed().as_micros() as u64;

    let profile = engine.network_profile();
    let (server_net, rate_cfg, cors_cfg) = {
        let d = rt.def.lock().unwrap();
        (d.network.clone(), d.rate_limit.clone(), d.cors.clone())
    };
    // Automatic preflight (SPEC §5.7) unless a Route explicitly handles the request.
    let preflight = route.is_none()
        && cors_cfg.as_ref().is_some_and(|c| c.enabled)
        && crate::cors::is_preflight(&method, &req_headers);
    let rate = rate_cfg.as_ref().filter(|_| !preflight).and_then(|c| {
        sim::rate_key(c, &ip, &req_headers, &url).and_then(|k| rt.limiter.check(c, &k))
    });
    let rate_limited = rate.as_ref().is_some_and(|r| r.limited);
    let proxied = if preflight || rate_limited {
        None
    } else {
        let def = rt.def.lock().unwrap().clone();
        proxy::plan(&def, route.as_ref()).map(|p| (def, p))
    };
    if let Some((def, plan)) = proxied {
        let net = profile.network().or(server_net);
        let network_ms = net.as_ref().map_or(0, sim::latency_ms);
        let rate_headers = rate.as_ref().map(|r| r.headers()).unwrap_or_default();
        return proxy::forward(
            proxy::Call {
                engine,
                rt,
                server_id: id,
                def,
                proxy: plan.proxy,
                ip,
                parts,
                req_headers,
                method,
                url,
                path,
                is_test,
                match_us,
                cors: cors_cfg,
                rate_headers,
                network_ms,
                started,
            },
            body_in,
        )
        .await;
    }
    let body = body_in
        .collect()
        .await
        .map(|c| c.to_bytes())
        .unwrap_or_default();
    let mut fault: Option<(Fault, &'static str)> = None;
    let mut render_us = 0u64;
    let mut delay_ms = 0;
    let mut trace = None;
    let mut chosen: (String, String) = Default::default();
    let mut render_error: Option<String> = None;
    let mut dataset_changes: Vec<crate::actions::DatasetChange> = vec![];
    let (route_id, (mut status, mut headers, mut bytes)) = match &route {
        _ if preflight => ("cors-preflight".to_string(), (204, vec![], Bytes::new())),
        _ if rate_limited => {
            let st = rate.clone().unwrap_or_else(|| unreachable!());
            let mut out = json_response(
                429,
                problem::too_many_requests(&path, st.reset_s),
                "application/problem+json",
            );
            out.1.extend(st.headers());
            (
                route
                    .as_ref()
                    .map_or("unmatched".to_string(), |r| r.id.clone()),
                out,
            )
        }
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
        Some(r) if r.kind == "static" => {
            let call = rt.counters.bump(&r.id);
            let rel = params
                .iter()
                .find(|(k, _)| k == "*")
                .map(|(_, v)| v.clone())
                .unwrap_or_default();
            let server_def = rt.def.lock().unwrap().clone();
            let req_id = engine.inner.seq.load(Ordering::Relaxed);
            let tpl = crate::render::TplEnv {
                context: crate::render::context(
                    &server_def,
                    &method,
                    &url,
                    &req_headers,
                    &params,
                    Some(&ip),
                    &body,
                    req_id,
                    call,
                    &engine.inner.vars.read().unwrap(),
                    Some(&r.id),
                ),
                seed: server_def.fake_seed,
                route_id: r.id.clone(),
                counter: call,
                seq: Some(rt.seq.clone()),
            };
            let t0 = Instant::now();
            let o = crate::statics::serve(
                &engine.inner.cfg.root,
                r,
                &rel,
                &method,
                &req_headers,
                &tpl,
                &path,
            );
            render_us = t0.elapsed().as_micros() as u64;
            chosen = (String::new(), o.file.clone().unwrap_or_default());
            render_error = o.error;
            let mut headers = o.headers;
            for (k, v) in &r.headers {
                if !headers.iter().any(|(n, _)| n.eq_ignore_ascii_case(k)) {
                    headers.push((k.to_ascii_lowercase(), v.clone()));
                }
            }
            (r.id.clone(), (o.status, headers, o.body))
        }
        Some(r) if r.kind == "crud" => {
            let call = rt.counters.bump(&r.id);
            chosen = (String::new(), format!("CRUD {method}"));
            let item_id = route_pattern
                .ends_with("/:id")
                .then(|| {
                    params
                        .iter()
                        .find(|(k, _)| k == "id")
                        .map(|(_, v)| v.clone())
                })
                .flatten();
            let ds_name = r.dataset.clone().unwrap_or_default();
            let page_size = {
                let def = rt.def.lock().unwrap().clone();
                engine
                    .inner
                    .vars
                    .read()
                    .unwrap()
                    .resolve(&def, Some(&r.id))
                    .into_iter()
                    .find(|v| v.key == "page_size")
                    .and_then(|v| v.value.trim().parse::<usize>().ok())
            };
            let query: Vec<(String, String)> = url
                .split_once('?')
                .map(|(_, q)| {
                    form_urlencoded::parse(q.as_bytes())
                        .map(|(k, v)| (k.into_owned(), v.into_owned()))
                        .collect()
                })
                .unwrap_or_default();
            let schema = r
                .validate
                .as_deref()
                .and_then(|p| rt.compiled.schema(&engine.inner.cfg.root, p));
            let _ = call;
            let (out, changed, rows) = {
                let mut d = rt.data.lock().unwrap();
                match d.get_mut(&ds_name) {
                    None => (
                        json_response(
                            500,
                            problem::server_error(&format!("unknown dataset `{ds_name}`"), &path),
                            "application/problem+json",
                        ),
                        false,
                        0,
                    ),
                    Some(ds) => {
                        let c = crate::data::Call {
                            method: &method,
                            path: &path,
                            id: item_id.as_deref(),
                            query: &query,
                            body: &body,
                            page_size,
                            schema,
                            root: &engine.inner.cfg.root,
                            server_id: &id,
                            ops: &r.crud_ops,
                        };
                        let (out, changed) = crate::data::handle(ds, &c);
                        (out, changed, ds.len())
                    }
                }
            };
            if changed {
                let _ = engine.inner.events.send(EngineEvent::Dataset {
                    server_id: id.clone(),
                    name: ds_name,
                    rows,
                });
            }
            if out.0 >= 500 {
                render_error = Some(String::from_utf8_lossy(&out.2).into_owned());
            }
            (r.id.clone(), out)
        }
        Some(r) => {
            let call = rt.counters.bump(&r.id);
            let lc_headers: Vec<(String, String)> = req_headers
                .iter()
                .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
                .collect();
            let server_def = rt.def.lock().unwrap().clone();
            let req_id = engine.inner.seq.load(Ordering::Relaxed);
            let mut tpl = crate::render::TplEnv {
                context: crate::render::context(
                    &server_def,
                    &method,
                    &url,
                    &lc_headers,
                    &params,
                    Some(&ip),
                    &body,
                    req_id,
                    call,
                    &engine.inner.vars.read().unwrap(),
                    Some(&r.id),
                ),
                seed: server_def.fake_seed,
                route_id: r.id.clone(),
                counter: call,
                seq: Some(rt.seq.clone()),
            };
            {
                let snap = rt.data.lock().unwrap().snapshot();
                if snap.as_object().is_some_and(|m| !m.is_empty()) {
                    tpl.context["data"] = (*snap).clone();
                }
            }
            let mut ctx = crate::rules::Ctx::new(
                &method,
                &url,
                &lc_headers,
                &params,
                Some(ip),
                &body,
                call,
                &engine.inner.cfg.root,
            );
            ctx.tpl = Some(tpl.clone());
            let (picked, t) = crate::rules::select(r, &ctx, &rt.compiled);
            trace = Some(t);
            let out = match picked {
                None => (200, vec![], Bytes::new()),
                Some(resp) => {
                    chosen = (resp.id.clone(), resp.name.clone().unwrap_or_default());
                    let mut action_failed: Option<(u16, String)> = None;
                    if resp.actions.iter().any(|a| !a.disabled) {
                        let env = crate::actions::Env {
                            root: &engine.inner.cfg.root,
                            server_id: &id,
                            server: &server_def,
                            route_id: &r.id,
                            counter: call,
                        };
                        let res = {
                            let mut store = rt.data.lock().unwrap();
                            crate::actions::run(&mut store, &env, &resp.actions, &mut tpl.context)
                        };
                        match res {
                            Ok(ch) => {
                                for c in &ch {
                                    let n = rt
                                        .data
                                        .lock()
                                        .unwrap()
                                        .get(&c.dataset)
                                        .map_or(0, |s| s.len());
                                    let _ = engine.inner.events.send(EngineEvent::Dataset {
                                        server_id: id.clone(),
                                        name: c.dataset.clone(),
                                        rows: n,
                                    });
                                }
                                dataset_changes = ch;
                            }
                            Err(e) => {
                                action_failed = Some((
                                    resp.action_error_status.unwrap_or(409),
                                    format!("action {} failed: {}", e.step, e.message),
                                ));
                            }
                        }
                    }
                    if let Some((status, msg)) = action_failed {
                        render_error = Some(msg.clone());
                        let body = serde_json::json!({
                            "type": "about:blank", "title": "Dataset action failed",
                            "status": status, "detail": msg, "instance": path,
                        });
                        crate::engine::json_response(status, body, "application/problem+json")
                    } else {
                        let (lo, hi) = resp
                            .delay_ms
                            .as_ref()
                            .and_then(fauxfield_model::Delay::bounds)
                            .unwrap_or((0, 0));
                        delay_ms = sim::sample(lo, hi);
                        if delay_ms > 0 {
                            tokio::time::sleep(Duration::from_millis(delay_ms)).await;
                        }
                        if let Some(f) = resp.fault.as_deref().and_then(Fault::parse)
                            && sim::roll(resp.fault_rate.unwrap_or(1.0))
                        {
                            fault = Some((f, ""));
                        }
                        {
                            let t0 = Instant::now();
                            let out =
                                crate::render::build(&engine.inner.cfg.root, r, resp, &tpl, &path);
                            render_us = t0.elapsed().as_micros() as u64;
                            render_error = out.error;
                            (out.status, out.headers, out.body)
                        }
                    }
                }
            };
            (r.id.clone(), out)
        }
    };

    let mut fault_label: Option<String> = rate_limited.then(|| "rate_limited".to_string());
    if !rate_limited {
        if fault.is_none() {
            let chaos = *rt.chaos.lock().unwrap();
            if let Some(rate) = chaos
                && sim::roll(rate)
            {
                fault = Some((sim::chaos_fault(), "chaos:"));
            } else if sim::roll(profile.drop_rate()) {
                fault = Some((Fault::Reset, "profile:"));
            }
        }
        if let Some((f, prefix)) = fault {
            fault_label = Some(format!("{prefix}{}", f.name()));
        }
    }
    if let Some(r) = &rate
        && !r.limited
    {
        headers.extend(r.headers());
    }
    if let Some(c) = &cors_cfg {
        crate::cors::apply(c, &req_headers, &mut headers, preflight);
    }
    let net = profile.network().or(server_net);
    let network_ms = net.as_ref().map_or(0, sim::latency_ms);
    if network_ms > 0 {
        tokio::time::sleep(Duration::from_millis(network_ms)).await;
    }
    let mut mode = match net.as_ref().map(|n| n.bandwidth_kbps) {
        Some(k) if k > 0 => BodyMode::Throttle(k * 125),
        _ => BodyMode::Full,
    };
    let mut wire_status = status;
    if let Some((f, _)) = fault {
        match f {
            Fault::Empty => bytes = Bytes::new(),
            Fault::ServerError => {
                (status, headers, bytes) =
                    json_response(500, problem::chaos_error(&path), "application/problem+json");
                wire_status = status;
            }
            Fault::Close => mode = BodyMode::Abort,
            Fault::Malformed => mode = BodyMode::Truncate,
            Fault::SlowBody => {
                let k = net
                    .as_ref()
                    .map(|n| n.bandwidth_kbps)
                    .filter(|k| *k > 0)
                    .unwrap_or(sim::DEFAULT_SLOW_KBPS);
                mode = BodyMode::Throttle(k * 125);
            }
            Fault::Timeout | Fault::Reset => wire_status = 0,
        }
    }
    let transfer_ms = match mode {
        BodyMode::Throttle(bps) if bps > 0 => bytes.len() as u64 * 1000 / bps,
        _ => 0,
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
            response_id: chosen.0,
            response_name: chosen.1,
            trace,
            params,
            route_pattern,
            method,
            url,
            request_headers: redact(req_headers),
            request_body: req_body,
            status: wire_status,
            response_headers: redact(headers.clone()),
            response_body: resp_body,
            timing: Timing {
                match_us,
                delay_ms: delay_ms + network_ms,
                render_us,
                network_ms,
                transfer_ms,
                upstream_ms: 0,
                total_ms: started.elapsed().as_millis() as u64 + transfer_ms,
            },
            truncated: req_cut || resp_cut,
            error: render_error,
            fault: fault_label,
            dataset_changes,
            source: if is_test { "test" } else { "mock" }.into(),
            upstream: None,
        },
    );

    match fault.map(|f| f.0) {
        Some(Fault::Timeout) => {
            // Hold the request until the client gives up (bounded so tasks never leak).
            tokio::time::sleep(Duration::from_secs(600)).await;
        }
        Some(Fault::Reset) => {
            reset.store(true, Ordering::SeqCst);
            return Err(ConnAbort);
        }
        _ => {}
    }

    let mut b = HyperResponse::builder()
        .status(StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR));
    for (k, v) in &headers {
        b = b.header(k.as_str(), v.as_str());
    }
    Ok(b.body(RespBody::Sim(SimBody::new(bytes, mode, None)))
        .unwrap_or_else(|_| {
            HyperResponse::new(RespBody::Sim(SimBody::new(
                Bytes::from_static(b"invalid response headers"),
                BodyMode::Full,
                None,
            )))
        }))
}

pub(crate) fn read_confined(root: &std::path::Path, file: &str) -> Result<Vec<u8>, String> {
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    let full = root.join(file).canonicalize().map_err(|e| e.to_string())?;
    if !full.starts_with(&root) {
        return Err("path escapes the workspace".into());
    }
    std::fs::read(full).map_err(|e| e.to_string())
}
