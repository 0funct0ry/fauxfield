use async_trait::async_trait;
use fauxfield_engine::{
    Engine, EngineError, Recording, ServerInfo, TestRequest, TestResponse, TrafficFilter,
};
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
            EngineError::Request(_) => "request",
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

    async fn shutdown(&self) -> Result<(), ControlError> {
        self.shutdown.notify_one();
        Ok(())
    }
}
