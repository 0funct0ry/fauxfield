use async_trait::async_trait;
use fauxfield_engine::{
    DatasetInfo, Engine, EngineError, Recording, ServerInfo, TestRequest, TestResponse,
    TrafficFilter,
};
use fauxfield_model::vars::ResolvedVar;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Notify;

/// Error shared by the in-process and socket implementations. `kind` is stable:
/// `unknown_server`, `port_in_use`, `bind`, `not_running`, `request`, `no_host`, `transport`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, thiserror::Error)]
#[error("{message}")]
pub struct ControlError {
    pub kind: String,
    pub message: String,
}

impl ControlError {
    pub fn new(kind: &str, message: impl Into<String>) -> Self {
        ControlError {
            kind: kind.into(),
            message: message.into(),
        }
    }
}

impl From<EngineError> for ControlError {
    fn from(e: EngineError) -> Self {
        let kind = match &e {
            EngineError::UnknownServer(_) => "unknown_server",
            EngineError::PortInUse { .. } => "port_in_use",
            EngineError::Bind { .. } => "bind",
            EngineError::NotRunning(_) => "not_running",
            EngineError::Disabled(_) => "disabled",
            EngineError::Request(_) => "request",
            EngineError::UnknownDataset(_) => "unknown_dataset",
        };
        ControlError::new(kind, e.to_string())
    }
}

/// What the GUI, CLI and MCP can ask of the host (SPEC §3.4).
#[async_trait]
pub trait ControlApi: Send + Sync {
    async fn status(&self) -> Result<Vec<ServerInfo>, ControlError>;
    /// `None` starts every enabled server; the first failure is returned after the rest ran.
    async fn start(&self, server: Option<String>) -> Result<Vec<ServerInfo>, ControlError>;
    async fn stop(&self, server: Option<String>) -> Result<Vec<ServerInfo>, ControlError>;
    async fn restart(&self, server: String) -> Result<Vec<ServerInfo>, ControlError>;
    async fn traffic(&self, filter: TrafficFilter) -> Result<Vec<Recording>, ControlError>;
    async fn send_test(
        &self,
        server: String,
        request: TestRequest,
    ) -> Result<TestResponse, ControlError>;
    /// Active Environment name ("" when none).
    async fn env_active(&self) -> Result<String, ControlError>;
    /// Switch the active Environment live (routes recompile; no restart).
    async fn env_use(&self, name: String) -> Result<String, ControlError>;
    /// Set (`Some`) or unset (`None`) a runtime variable.
    async fn var_runtime(&self, key: String, value: Option<String>) -> Result<(), ControlError>;
    /// Variables in scope for a Server (and optional Route) with scope and source.
    async fn vars(
        &self,
        server: String,
        route: Option<String>,
    ) -> Result<Vec<ResolvedVar>, ControlError>;
    /// Datasets (all, or one Server's) with live row counts.
    async fn data_list(&self, server: Option<String>) -> Result<Vec<DatasetInfo>, ControlError>;
    /// Live rows of one Dataset.
    async fn data_show(
        &self,
        server: String,
        dataset: String,
    ) -> Result<serde_json::Value, ControlError>;
    /// Replace a Dataset's live rows (import).
    async fn data_set(
        &self,
        server: String,
        dataset: String,
        rows: serde_json::Value,
    ) -> Result<(), ControlError>;
    /// Reset state (SPEC §6.4): all Servers, one Server, or one Dataset of a Server.
    async fn reset_state(
        &self,
        server: Option<String>,
        dataset: Option<String>,
    ) -> Result<(), ControlError>;
    /// The workspace network profile (SPEC §5.6).
    async fn network_get(&self) -> Result<fauxfield_engine::NetworkProfile, ControlError>;
    /// Set the workspace network profile (runtime only).
    async fn network_set(
        &self,
        profile: fauxfield_engine::NetworkProfile,
    ) -> Result<fauxfield_engine::NetworkProfile, ControlError>;
    /// Chaos switch: a rate (0..=1) turns it on, `None` off; no server means every Server.
    async fn chaos_set(
        &self,
        server: Option<String>,
        rate: Option<f64>,
    ) -> Result<Vec<ServerInfo>, ControlError>;
    /// Asks the host to exit (graceful drain happens in the host).
    async fn shutdown(&self) -> Result<(), ControlError>;
}

/// In-process implementation over an [`Engine`].
#[derive(Clone)]
pub struct LocalControl {
    engine: Engine,
    shutdown: Arc<Notify>,
}

impl LocalControl {
    pub fn new(engine: Engine) -> Self {
        LocalControl {
            engine,
            shutdown: Arc::new(Notify::new()),
        }
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Resolves when a client called `shutdown`.
    pub async fn shutdown_requested(&self) {
        self.shutdown.notified().await
    }

    /// Engine start/stop block briefly on the runtime, so run them off the async workers.
    async fn blocking<T: Send + 'static>(
        &self,
        f: impl FnOnce(Engine) -> T + Send + 'static,
    ) -> Result<T, ControlError> {
        let e = self.engine.clone();
        tokio::task::spawn_blocking(move || f(e))
            .await
            .map_err(|e| ControlError::new("request", e.to_string()))
    }
}

#[async_trait]
impl ControlApi for LocalControl {
    async fn status(&self) -> Result<Vec<ServerInfo>, ControlError> {
        Ok(self.engine.list())
    }

    async fn start(&self, server: Option<String>) -> Result<Vec<ServerInfo>, ControlError> {
        let r = self
            .blocking(move |e| match server {
                Some(s) => e.start(&s).map(|_| ()).map_err(ControlError::from),
                None => match e.start_all().into_iter().next() {
                    Some((_, err)) => Err(err.into()),
                    None => Ok(()),
                },
            })
            .await?;
        r?;
        Ok(self.engine.list())
    }

    async fn stop(&self, server: Option<String>) -> Result<Vec<ServerInfo>, ControlError> {
        let r = self
            .blocking(move |e| match server {
                Some(s) => e.stop(&s).map_err(ControlError::from),
                None => {
                    e.stop_all();
                    Ok(())
                }
            })
            .await?;
        r?;
        Ok(self.engine.list())
    }

    async fn restart(&self, server: String) -> Result<Vec<ServerInfo>, ControlError> {
        let r = self
            .blocking(move |e| e.restart(&server).map(|_| ()).map_err(ControlError::from))
            .await?;
        r?;
        Ok(self.engine.list())
    }

    async fn traffic(&self, filter: TrafficFilter) -> Result<Vec<Recording>, ControlError> {
        Ok(self.engine.traffic(&filter))
    }

    async fn send_test(
        &self,
        server: String,
        request: TestRequest,
    ) -> Result<TestResponse, ControlError> {
        Ok(self.engine.send_test(&server, &request).await?)
    }

    async fn env_active(&self) -> Result<String, ControlError> {
        Ok(self.engine.var_context().active_env)
    }

    async fn env_use(&self, name: String) -> Result<String, ControlError> {
        if !self.engine.var_context().envs.contains_key(&name) {
            return Err(ControlError::new(
                "unknown_environment",
                format!("no environment named `{name}`"),
            ));
        }
        self.engine.set_active_env(&name);
        Ok(name)
    }

    async fn var_runtime(&self, key: String, value: Option<String>) -> Result<(), ControlError> {
        self.engine.set_runtime_var(&key, value.as_deref());
        Ok(())
    }

    async fn vars(
        &self,
        server: String,
        route: Option<String>,
    ) -> Result<Vec<ResolvedVar>, ControlError> {
        Ok(self.engine.vars_in_scope(&server, route.as_deref())?)
    }

    async fn data_list(&self, server: Option<String>) -> Result<Vec<DatasetInfo>, ControlError> {
        Ok(self.engine.datasets(server.as_deref())?)
    }

    async fn data_show(
        &self,
        server: String,
        dataset: String,
    ) -> Result<serde_json::Value, ControlError> {
        Ok(self.engine.dataset_rows(&server, &dataset)?)
    }

    async fn data_set(
        &self,
        server: String,
        dataset: String,
        rows: serde_json::Value,
    ) -> Result<(), ControlError> {
        Ok(self.engine.dataset_set(&server, &dataset, rows)?)
    }

    async fn reset_state(
        &self,
        server: Option<String>,
        dataset: Option<String>,
    ) -> Result<(), ControlError> {
        match (server, dataset) {
            (Some(s), Some(d)) => Ok(self.engine.dataset_reset(&s, &d)?),
            (None, Some(_)) => Err(ControlError::new(
                "request",
                "a dataset reset needs a server",
            )),
            (s, None) => Ok(self.engine.reset_state(s.as_deref())?),
        }
    }

    async fn network_get(&self) -> Result<fauxfield_engine::NetworkProfile, ControlError> {
        Ok(self.engine.network_profile())
    }

    async fn network_set(
        &self,
        profile: fauxfield_engine::NetworkProfile,
    ) -> Result<fauxfield_engine::NetworkProfile, ControlError> {
        self.engine.set_network_profile(profile);
        Ok(profile)
    }

    async fn chaos_set(
        &self,
        server: Option<String>,
        rate: Option<f64>,
    ) -> Result<Vec<ServerInfo>, ControlError> {
        self.engine.set_chaos(server.as_deref(), rate)?;
        Ok(self.engine.list())
    }

    async fn shutdown(&self) -> Result<(), ControlError> {
        self.shutdown.notify_one();
        Ok(())
    }
}
