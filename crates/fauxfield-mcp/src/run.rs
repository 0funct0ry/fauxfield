//! `serve`: run the configured transports until stdin closes (stdio only) or SIGINT/SIGTERM.

use std::sync::Arc;

use rmcp::ServiceExt;

use crate::{
    registry::Registry,
    server::FauxfieldServer,
    tools::{ServerFacts, core_registry},
    transport::{McpConfig, TransportKind, default_socket_path},
};

/// Build the registry for this process. M1.1 has only the core tools.
pub fn build_registry(config: &McpConfig) -> Registry {
    let transports = match config.transport {
        TransportKind::Both => vec![TransportKind::Stdio, TransportKind::Unix],
        t => vec![t],
    };
    core_registry(ServerFacts {
        transports,
        allowed: config.allow,
        features: vec![],
    })
}

pub async fn serve(config: McpConfig) -> Result<(), String> {
    serve_with(config, None).await
}

/// `registry` lets tests add tools; `None` uses the core set.
pub async fn serve_with(config: McpConfig, registry: Option<Registry>) -> Result<(), String> {
    if config.transport == TransportKind::Http {
        return Err("the http transport is not available until M16".into());
    }
    #[cfg(not(unix))]
    if config.transport.uses_unix() {
        return Err(
            "the unix socket transport is not available on this platform; use stdio".into(),
        );
    }
    let registry = Arc::new(registry.unwrap_or_else(|| build_registry(&config)));
    let handler = FauxfieldServer::new(registry, config.allow);

    #[cfg(unix)]
    let socket = if config.transport.uses_unix() {
        let path = config.socket.clone().unwrap_or_else(default_socket_path);
        let (listener, guard) = crate::transport::bind_unix(&path).await?;
        tracing::info!("mcp socket listening at {}", guard.path().display());
        Some((listener, guard))
    } else {
        None
    };
    #[cfg(not(unix))]
    let _ = default_socket_path;

    let mut stdio_done = None;
    if config.transport.uses_stdio() {
        let h = handler.clone();
        let running = h
            .serve((tokio::io::stdin(), tokio::io::stdout()))
            .await
            .map_err(|e| format!("stdio session failed to start: {e}"))?;
        stdio_done = Some(tokio::spawn(async move {
            let _ = running.waiting().await;
        }));
    }

    #[cfg(unix)]
    let accept = {
        let h = handler.clone();
        socket.map(|(listener, guard)| {
            (
                guard,
                tokio::spawn(async move {
                    loop {
                        match listener.accept().await {
                            Ok((stream, _)) => {
                                let h = h.clone();
                                tokio::spawn(async move {
                                    match h.serve(stream).await {
                                        Ok(r) => {
                                            let _ = r.waiting().await;
                                        }
                                        Err(e) => tracing::debug!("socket session ended: {e}"),
                                    }
                                });
                            }
                            Err(e) => {
                                tracing::warn!("accept failed: {e}");
                                break;
                            }
                        }
                    }
                }),
            )
        })
    };

    let stdio_only = config.transport == TransportKind::Stdio;
    let stdio_wait = async {
        match stdio_done {
            // Stdin closing ends only the stdio session; with a socket we keep serving.
            Some(h) if stdio_only => {
                let _ = h.await;
            }
            Some(h) => {
                let _ = h.await;
                std::future::pending::<()>().await
            }
            None => std::future::pending::<()>().await,
        }
    };

    tokio::select! {
        _ = stdio_wait => {}
        _ = shutdown_signal() => tracing::info!("shutting down"),
    }

    #[cfg(unix)]
    if let Some((guard, task)) = accept {
        task.abort();
        drop(guard); // removes the socket file
    }
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};
        match (
            signal(SignalKind::interrupt()),
            signal(SignalKind::terminate()),
        ) {
            (Ok(mut i), Ok(mut t)) => {
                tokio::select! { _ = i.recv() => {}, _ = t.recv() => {} }
            }
            _ => std::future::pending::<()>().await,
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}
