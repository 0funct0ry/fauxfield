use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use fauxfield_engine::{
    DatasetInfo, Recording, ServerInfo, TestRequest, TestResponse, TrafficFilter,
};
use fauxfield_model::vars::ResolvedVar;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::api::{ControlApi, ControlError};

/// Newline-delimited JSON-RPC 2.0. Methods: `ping`, `status`, `start`, `stop`, `restart`,
/// `traffic.get`, `test.send`, `shutdown`. Errors carry `{kind}` in `error.data`.
fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: Value, code: i64, e: &ControlError) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": e.message, "data": {"kind": e.kind}}})
}

fn param<T: DeserializeOwned>(params: &Value, key: &str) -> Result<T, ControlError> {
    serde_json::from_value(params.get(key).cloned().unwrap_or(Value::Null))
        .map_err(|e| ControlError::new("invalid_params", format!("`{key}`: {e}")))
}

async fn dispatch(
    api: &dyn ControlApi,
    method: &str,
    params: &Value,
) -> Result<Value, ControlError> {
    let to = |v: Result<Vec<ServerInfo>, ControlError>| v.map(|s| json!({"servers": s}));
    match method {
        "ping" => Ok(json!({"pong": true})),
        "status" => to(api.status().await),
        "start" => to(api.start(param(params, "server")?).await),
        "stop" => to(api.stop(param(params, "server")?).await),
        "restart" => to(api.restart(param(params, "server")?).await),
        "traffic.get" => {
            let f: TrafficFilter = serde_json::from_value(params.clone())
                .map_err(|e| ControlError::new("invalid_params", e.to_string()))?;
            api.traffic(f).await.map(|r| json!({"traffic": r}))
        }
        "test.send" => {
            let r = api
                .send_test(param(params, "server")?, param(params, "request")?)
                .await?;
            Ok(serde_json::to_value(r).unwrap_or_default())
        }
        "env.active" => api.env_active().await.map(|n| json!({"active": n})),
        "env.use" => api
            .env_use(param(params, "name")?)
            .await
            .map(|n| json!({"active": n})),
        "var.runtime" => api
            .var_runtime(param(params, "key")?, param(params, "value")?)
            .await
            .map(|_| json!({})),
        "vars" => api
            .vars(param(params, "server")?, param(params, "route")?)
            .await
            .map(|v| json!({"vars": v})),
        "data.list" => api
            .data_list(param(params, "server")?)
            .await
            .map(|v| json!({"datasets": v})),
        "data.show" => api
            .data_show(param(params, "server")?, param(params, "dataset")?)
            .await
            .map(|v| json!({"rows": v})),
        "data.set" => api
            .data_set(
                param(params, "server")?,
                param(params, "dataset")?,
                param(params, "rows")?,
            )
            .await
            .map(|_| json!({})),
        "data.update_cell" => api
            .data_update_cell(
                param(params, "server")?,
                param(params, "dataset")?,
                param(params, "index")?,
                param(params, "column")?,
                param(params, "value")?,
            )
            .await
            .map(|_| json!({})),
        "data.insert" => api
            .data_insert(
                param(params, "server")?,
                param(params, "dataset")?,
                param(params, "at")?,
                param(params, "rows")?,
            )
            .await
            .map(|i| json!({"index": i})),
        "data.delete" => api
            .data_delete(
                param(params, "server")?,
                param(params, "dataset")?,
                param(params, "indices")?,
            )
            .await
            .map(|n| json!({"deleted": n})),
        "state.reset" => api
            .reset_state(param(params, "server")?, param(params, "dataset")?)
            .await
            .map(|_| json!({})),
        "network.get" => api.network_get().await.map(|p| json!({"profile": p})),
        "network.set" => api
            .network_set(param(params, "profile")?)
            .await
            .map(|p| json!({"profile": p})),
        "chaos.set" => to(api
            .chaos_set(param(params, "server")?, param(params, "rate")?)
            .await),
        "ws.connections" => api
            .ws_connections(param(params, "server")?, param(params, "route")?)
            .await
            .map(|c| json!({"connections": c})),
        "ws.messages" => api
            .ws_messages(
                param(params, "server")?,
                param(params, "route")?,
                param(params, "connection")?,
                param::<Option<u64>>(params, "after")?.unwrap_or(0),
                param::<Option<usize>>(params, "limit")?.unwrap_or(0),
            )
            .await
            .map(|f| json!({"frames": f})),
        "ws.send" => api
            .ws_send(
                param(params, "server")?,
                param(params, "route")?,
                param(params, "connection")?,
                param(params, "body")?,
                param::<Option<bool>>(params, "binary")?.unwrap_or(false),
            )
            .await
            .map(|n| json!({"sent": n})),
        "ws.disconnect" => api
            .ws_disconnect(
                param(params, "server")?,
                param(params, "connection")?,
                param(params, "code")?,
            )
            .await
            .map(|_| json!({})),
        "callback.refire" => api
            .callback_refire(
                param(params, "server")?,
                param(params, "recording")?,
                param(params, "callback")?,
            )
            .await
            .map(|r| json!({"recordings": r})),
        "shutdown" => api.shutdown().await.map(|_| json!({})),
        other => Err(ControlError::new(
            "method_not_found",
            format!("unknown method `{other}`"),
        )),
    }
}

async fn handle_line(api: &dyn ControlApi, line: &str) -> Value {
    let Ok(req) = serde_json::from_str::<Value>(line) else {
        return err(
            Value::Null,
            -32700,
            &ControlError::new("parse", "invalid JSON"),
        );
    };
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = req.get("method").and_then(Value::as_str) else {
        return err(
            id,
            -32600,
            &ControlError::new("invalid_request", "missing method"),
        );
    };
    let params = req.get("params").cloned().unwrap_or(Value::Null);
    match dispatch(api, method, &params).await {
        Ok(v) => ok(id, v),
        Err(e) => {
            let code = if e.kind == "method_not_found" {
                -32601
            } else {
                -32000
            };
            err(id, code, &e)
        }
    }
}

/// Serve JSON-RPC lines on any duplex stream until it closes. Shared by the Unix listener
/// and the Windows pipe accept loop.
pub async fn handle_conn<S>(stream: S, api: Arc<dyn ControlApi>)
where
    S: tokio::io::AsyncRead + tokio::io::AsyncWrite + Send + 'static,
{
    let (r, mut w) = tokio::io::split(stream);
    let mut lines = BufReader::new(r).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let mut out = handle_line(api.as_ref(), &line).await.to_string();
        out.push('\n');
        if w.write_all(out.as_bytes()).await.is_err() {
            break;
        }
    }
}

/// Accept loop over an already-bound listener (see [`crate::ipc::bind_unix`]).
#[cfg(unix)]
pub async fn serve(listener: tokio::net::UnixListener, api: Arc<dyn ControlApi>) {
    loop {
        let Ok((stream, _)) = listener.accept().await else {
            continue;
        };
        tokio::spawn(handle_conn(stream, api.clone()));
    }
}

/// Accept loop over a Windows named pipe created by [`crate::ipc::bind_pipe`].
#[cfg(windows)]
pub async fn serve(
    first: tokio::net::windows::named_pipe::NamedPipeServer,
    name: String,
    api: Arc<dyn ControlApi>,
) {
    let mut server = first;
    loop {
        if server.connect().await.is_err() {
            continue;
        }
        let Ok(next) = crate::ipc::next_pipe_instance(&name) else {
            return;
        };
        let conn = std::mem::replace(&mut server, next);
        tokio::spawn(handle_conn(conn, api.clone()));
    }
}

/// Client implementing [`ControlApi`] over the control socket, one connection per call.
#[derive(Clone)]
pub struct RemoteControl {
    socket: PathBuf,
}

impl RemoteControl {
    pub fn new(socket: &Path) -> RemoteControl {
        RemoteControl {
            socket: socket.to_path_buf(),
        }
    }

    #[cfg(unix)]
    async fn connect(&self) -> std::io::Result<tokio::net::UnixStream> {
        tokio::net::UnixStream::connect(&self.socket).await
    }

    /// Open the named pipe, retrying ERROR_PIPE_BUSY (231) for up to 2 s.
    #[cfg(windows)]
    async fn connect(&self) -> std::io::Result<tokio::net::windows::named_pipe::NamedPipeClient> {
        let start = std::time::Instant::now();
        loop {
            match tokio::net::windows::named_pipe::ClientOptions::new().open(&self.socket) {
                Ok(c) => return Ok(c),
                Err(e) if e.raw_os_error() == Some(231) && start.elapsed().as_secs() < 2 => {
                    tokio::time::sleep(std::time::Duration::from_millis(25)).await;
                }
                Err(e) => return Err(e),
            }
        }
    }

    #[cfg(any(unix, windows))]
    async fn call(&self, method: &str, params: Value) -> Result<Value, ControlError> {
        let stream = self
            .connect()
            .await
            .map_err(|e| ControlError::new("no_host", format!("cannot reach the host: {e}")))?;
        let (r, mut w) = tokio::io::split(stream);
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": method, "params": params});
        w.write_all(format!("{req}\n").as_bytes())
            .await
            .map_err(|e| ControlError::new("transport", e.to_string()))?;
        let line = BufReader::new(r)
            .lines()
            .next_line()
            .await
            .map_err(|e| ControlError::new("transport", e.to_string()))?
            .ok_or_else(|| ControlError::new("transport", "host closed the connection"))?;
        let v: Value = serde_json::from_str(&line)
            .map_err(|e| ControlError::new("transport", e.to_string()))?;
        if let Some(e) = v.get("error") {
            let kind = e["data"]["kind"].as_str().unwrap_or("request");
            let msg = e["message"].as_str().unwrap_or("error");
            return Err(ControlError::new(kind, msg));
        }
        Ok(v["result"].clone())
    }

    #[cfg(not(any(unix, windows)))]
    async fn call(&self, _method: &str, _params: Value) -> Result<Value, ControlError> {
        Err(ControlError::new(
            "no_host",
            "no control socket on this platform",
        ))
    }

    fn servers(v: Value) -> Result<Vec<ServerInfo>, ControlError> {
        serde_json::from_value(v["servers"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
}

#[async_trait]
impl ControlApi for RemoteControl {
    async fn status(&self) -> Result<Vec<ServerInfo>, ControlError> {
        Self::servers(self.call("status", Value::Null).await?)
    }
    async fn start(&self, server: Option<String>) -> Result<Vec<ServerInfo>, ControlError> {
        Self::servers(self.call("start", json!({"server": server})).await?)
    }
    async fn stop(&self, server: Option<String>) -> Result<Vec<ServerInfo>, ControlError> {
        Self::servers(self.call("stop", json!({"server": server})).await?)
    }
    async fn restart(&self, server: String) -> Result<Vec<ServerInfo>, ControlError> {
        Self::servers(self.call("restart", json!({"server": server})).await?)
    }
    async fn traffic(&self, filter: TrafficFilter) -> Result<Vec<Recording>, ControlError> {
        let v = self
            .call(
                "traffic.get",
                serde_json::to_value(filter).unwrap_or_default(),
            )
            .await?;
        serde_json::from_value(v["traffic"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn send_test(
        &self,
        server: String,
        request: TestRequest,
    ) -> Result<TestResponse, ControlError> {
        let v = self
            .call("test.send", json!({"server": server, "request": request}))
            .await?;
        serde_json::from_value(v).map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn network_get(&self) -> Result<fauxfield_engine::NetworkProfile, ControlError> {
        let v = self.call("network.get", Value::Null).await?;
        serde_json::from_value(v["profile"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn network_set(
        &self,
        profile: fauxfield_engine::NetworkProfile,
    ) -> Result<fauxfield_engine::NetworkProfile, ControlError> {
        let v = self
            .call("network.set", json!({"profile": profile}))
            .await?;
        serde_json::from_value(v["profile"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn chaos_set(
        &self,
        server: Option<String>,
        rate: Option<f64>,
    ) -> Result<Vec<ServerInfo>, ControlError> {
        Self::servers(
            self.call("chaos.set", json!({"server": server, "rate": rate}))
                .await?,
        )
    }
    async fn ws_connections(
        &self,
        server: String,
        route: Option<String>,
    ) -> Result<Vec<fauxfield_engine::WsConnectionInfo>, ControlError> {
        let v = self
            .call("ws.connections", json!({"server": server, "route": route}))
            .await?;
        serde_json::from_value(v["connections"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn ws_messages(
        &self,
        server: String,
        route: Option<String>,
        connection: Option<String>,
        after: u64,
        limit: usize,
    ) -> Result<Vec<fauxfield_engine::WsFrame>, ControlError> {
        let v = self
            .call(
                "ws.messages",
                json!({"server": server, "route": route, "connection": connection,
                       "after": after, "limit": limit}),
            )
            .await?;
        serde_json::from_value(v["frames"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn ws_send(
        &self,
        server: String,
        route: Option<String>,
        connection: Option<String>,
        body: String,
        binary: bool,
    ) -> Result<usize, ControlError> {
        let v = self
            .call(
                "ws.send",
                json!({"server": server, "route": route, "connection": connection,
                       "body": body, "binary": binary}),
            )
            .await?;
        Ok(v["sent"].as_u64().unwrap_or(0) as usize)
    }
    async fn ws_disconnect(
        &self,
        server: String,
        connection: String,
        code: Option<u16>,
    ) -> Result<(), ControlError> {
        self.call(
            "ws.disconnect",
            json!({"server": server, "connection": connection, "code": code}),
        )
        .await
        .map(|_| ())
    }
    async fn callback_refire(
        &self,
        server: String,
        recording: u64,
        callback: Option<String>,
    ) -> Result<Vec<Recording>, ControlError> {
        let v = self
            .call(
                "callback.refire",
                json!({"server": server, "recording": recording, "callback": callback}),
            )
            .await?;
        serde_json::from_value(v["recordings"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn env_active(&self) -> Result<String, ControlError> {
        let v = self.call("env.active", Value::Null).await?;
        Ok(v["active"].as_str().unwrap_or_default().to_string())
    }
    async fn env_use(&self, name: String) -> Result<String, ControlError> {
        let v = self.call("env.use", json!({"name": name})).await?;
        Ok(v["active"].as_str().unwrap_or_default().to_string())
    }
    async fn var_runtime(&self, key: String, value: Option<String>) -> Result<(), ControlError> {
        self.call("var.runtime", json!({"key": key, "value": value}))
            .await
            .map(|_| ())
    }
    async fn vars(
        &self,
        server: String,
        route: Option<String>,
    ) -> Result<Vec<ResolvedVar>, ControlError> {
        let v = self
            .call("vars", json!({"server": server, "route": route}))
            .await?;
        serde_json::from_value(v["vars"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn data_list(&self, server: Option<String>) -> Result<Vec<DatasetInfo>, ControlError> {
        let v = self.call("data.list", json!({"server": server})).await?;
        serde_json::from_value(v["datasets"].clone())
            .map_err(|e| ControlError::new("transport", e.to_string()))
    }
    async fn data_show(&self, server: String, dataset: String) -> Result<Value, ControlError> {
        let v = self
            .call("data.show", json!({"server": server, "dataset": dataset}))
            .await?;
        Ok(v["rows"].clone())
    }
    async fn data_set(
        &self,
        server: String,
        dataset: String,
        rows: Value,
    ) -> Result<(), ControlError> {
        self.call(
            "data.set",
            json!({"server": server, "dataset": dataset, "rows": rows}),
        )
        .await
        .map(|_| ())
    }
    async fn data_update_cell(
        &self,
        server: String,
        dataset: String,
        index: usize,
        column: String,
        value: Value,
    ) -> Result<(), ControlError> {
        self.call(
            "data.update_cell",
            json!({"server": server, "dataset": dataset, "index": index, "column": column, "value": value}),
        )
        .await
        .map(|_| ())
    }
    async fn data_insert(
        &self,
        server: String,
        dataset: String,
        at: Option<usize>,
        rows: Vec<Value>,
    ) -> Result<usize, ControlError> {
        let v = self
            .call(
                "data.insert",
                json!({"server": server, "dataset": dataset, "at": at, "rows": rows}),
            )
            .await?;
        Ok(v["index"].as_u64().unwrap_or(0) as usize)
    }
    async fn data_delete(
        &self,
        server: String,
        dataset: String,
        indices: Vec<usize>,
    ) -> Result<usize, ControlError> {
        let v = self
            .call(
                "data.delete",
                json!({"server": server, "dataset": dataset, "indices": indices}),
            )
            .await?;
        Ok(v["deleted"].as_u64().unwrap_or(0) as usize)
    }
    async fn reset_state(
        &self,
        server: Option<String>,
        dataset: Option<String>,
    ) -> Result<(), ControlError> {
        self.call("state.reset", json!({"server": server, "dataset": dataset}))
            .await
            .map(|_| ())
    }
    async fn shutdown(&self) -> Result<(), ControlError> {
        self.call("shutdown", Value::Null).await.map(|_| ())
    }
}
