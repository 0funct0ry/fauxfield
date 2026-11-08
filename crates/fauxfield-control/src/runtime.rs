use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use fauxfield_engine::{Engine, EngineConfig, now_rfc3339};
use fauxfield_store::{Background, Store};

use crate::api::LocalControl;
use crate::host::{HostInfo, HostLock, control_socket_path};
use crate::reload::attach_store;
use crate::rpc::serve;

#[derive(Debug, thiserror::Error)]
pub enum HostError {
    #[error("this workspace is already hosted by pid {} ({})", .0.pid, .0.socket.display())]
    Already(HostInfo),
    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone, Default)]
pub struct HostOptions {
    pub port_offset: u16,
    pub host_override: Option<String>,
    /// Watch the workspace folder and hot-reload external edits.
    pub watch: bool,
    /// Serve the control socket and write `host.json`. Off for throwaway engines in tests.
    pub publish: bool,
}

/// The one host process per workspace (SPEC §3.1): engine, control socket, `host.json`, and
/// hot reload. Used by `fauxfield run`, the GUI, and `fauxfield mcp` when it becomes the host.
pub struct Host {
    engine: Engine,
    control: Arc<LocalControl>,
    store: Store,
    // Field order is drop order: stop the engine's users before the runtime goes away.
    _background: Option<Background>,
    _socket: Option<crate::ipc::SocketGuard>,
    _lock: Option<HostLock>,
    rt: Option<tokio::runtime::Runtime>,
    handle: tokio::runtime::Handle,
}

impl Host {
    /// Open the workspace at `root` and host it on a private runtime.
    pub fn start(root: &Path, opts: HostOptions) -> Result<Host, HostError> {
        let store = Store::open(root).map_err(|e| HostError::Other(e.to_string()))?;
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .thread_name("fauxfield-engine")
            .build()
            .map_err(|e| HostError::Other(format!("cannot start runtime: {e}")))?;
        let handle = rt.handle().clone();
        Self::build(store, opts, Some(rt), handle)
    }

    /// Host an already-open `store` on the caller's runtime (the GUI keeps its own store,
    /// autosave and watcher).
    pub fn with_store(
        store: Store,
        handle: tokio::runtime::Handle,
        opts: HostOptions,
    ) -> Result<Host, HostError> {
        Self::build(store, opts, None, handle)
    }

    fn build(
        store: Store,
        opts: HostOptions,
        rt: Option<tokio::runtime::Runtime>,
        handle: tokio::runtime::Handle,
    ) -> Result<Host, HostError> {
        let root = store.root();
        let engine = Engine::new(
            handle.clone(),
            EngineConfig {
                root: root.clone(),
                port_offset: opts.port_offset,
                host_override: opts.host_override.clone(),
                buffer: 0,
            },
        );
        let control = Arc::new(LocalControl::new(engine.clone()));
        let (mut lock, mut socket) = (None, None);
        if opts.publish {
            let path = control_socket_path(&root);
            lock =
                Some(HostLock::acquire(&root, &path, now_rfc3339()).map_err(HostError::Already)?);
            #[cfg(unix)]
            {
                let bound = {
                    let (tx, rx) = std::sync::mpsc::channel();
                    let p = path.clone();
                    handle.spawn(async move {
                        let _ = tx.send(
                            crate::ipc::bind_unix(
                                &p,
                                "Fauxfield host",
                                "another process hosts this workspace",
                            )
                            .await,
                        );
                    });
                    rx.recv().map_err(|e| HostError::Other(e.to_string()))?
                };
                let (listener, guard) = bound.map_err(HostError::Other)?;
                handle.spawn(serve(listener, control.clone()));
                socket = Some(guard);
            }
        }
        attach_store(&engine, &store);
        let background = if opts.watch {
            store
                .start_background(Duration::from_millis(300))
                .map_err(|e| tracing::warn!("file watcher disabled: {e}"))
                .ok()
        } else {
            None
        };
        Ok(Host {
            engine,
            control,
            store,
            _background: background,
            _socket: socket,
            _lock: lock,
            rt,
            handle,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn control(&self) -> Arc<LocalControl> {
        self.control.clone()
    }
    pub fn store(&self) -> &Store {
        &self.store
    }
    pub fn handle(&self) -> &tokio::runtime::Handle {
        &self.handle
    }
    /// Run a future to completion on the host's own runtime (not for use inside a runtime).
    pub fn block_on<F: std::future::Future>(&self, f: F) -> F::Output {
        match &self.rt {
            Some(rt) => rt.block_on(f),
            None => self.handle.block_on(f),
        }
    }
}

impl Drop for Host {
    fn drop(&mut self) {
        self.engine.stop_all();
    }
}
