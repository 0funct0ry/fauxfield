//! Starts the MCP server on its own runtime and drains the bridge on the GPUI thread.

use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};

use fauxfield_mcp::{
    AllowedClasses, McpConfig, ToolClass, TransportKind, UiBridge, UiRequest,
    events::EventBuffer,
    scenario::{self, Recorder, ScenarioEnv},
    serve_with,
    tools::{ServerFacts, core_registry},
};
use gpui::{App, AppContext as _, WindowHandle};
use serde_json::Value;

use super::DevMcpOptions;
use super::bridge::GuiBridge;
use super::handler::{HostState, dispatch};
use super::tools::register_ui_tools;
use crate::shell::Shell;

/// Internal request sent by the host thread when the server ends (signal, stdin closed, error).
/// Not reachable from any tool.
const QUIT: &str = "app.quit";

pub fn start(opts: DevMcpOptions, window: WindowHandle<Shell>, cx: &mut App) {
    let transport = match (opts.stdio, opts.unix) {
        (true, true) => TransportKind::Both,
        (true, false) => TransportKind::Stdio,
        _ => TransportKind::Unix,
    };
    let allow = AllowedClasses::all().with(ToolClass::Ui);
    let config = McpConfig {
        transport,
        socket: opts.socket.clone(),
        allow,
    };

    let (bridge, mut rx) = GuiBridge::channel();
    let bridge: Arc<dyn UiBridge> = Arc::new(bridge);
    let mut registry = core_registry(ServerFacts {
        transports: match transport {
            TransportKind::Both => vec![TransportKind::Stdio, TransportKind::Unix],
            t => vec![t],
        },
        allowed: allow,
        features: vec!["dev-mcp"],
    });
    register_ui_tools(&mut registry, bridge.clone());
    let events = EventBuffer::default();
    fauxfield_mcp::events::register(&mut registry, events.clone());
    let recorder = Recorder::default();
    registry.set_observer(recorder.observer());
    let shared = Arc::new(OnceLock::new());
    scenario::register(
        &mut registry,
        ScenarioEnv {
            registry: shared.clone(),
            dir: scenario::find_scenarios_dir(),
            recorder,
        },
    );
    let _ = shared.set(registry.clone());

    if transport.uses_unix() {
        let path = opts
            .socket
            .clone()
            .unwrap_or_else(fauxfield_mcp::default_socket_path);
        eprintln!("fauxfield: MCP socket at {}", path.display());
    }
    let quit_bridge = bridge.clone();
    let poll_bridge = bridge;
    let spawned = std::thread::Builder::new()
        .name("fauxfield-mcp".into())
        .spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    eprintln!("fauxfield: could not start the MCP runtime: {e}");
                    return;
                }
            };
            rt.block_on(async move {
                tokio::spawn(feed_events(poll_bridge, events));
                if let Err(e) = serve_with(config, Some(registry)).await {
                    eprintln!("fauxfield: MCP server stopped: {e}");
                }
                let _ = quit_bridge
                    .request(UiRequest {
                        method: QUIT.into(),
                        params: Value::Null,
                    })
                    .await;
            });
        });
    if let Err(e) = spawned {
        eprintln!("fauxfield: could not start the MCP thread: {e}");
        return;
    }

    let mut host = HostState {
        workspace_path: opts.workspace.map(|p| p.display().to_string()),
        ..HostState::default()
    };
    cx.spawn(async move |cx| {
        while let Some((req, reply)) = rx.recv().await {
            let result = if req.method == QUIT {
                let _ = cx.update(|cx| cx.quit());
                Ok(Value::Null)
            } else if super::handler::is_key_method(&req.method) {
                cx.update_window(window.into(), |_, w, app| {
                    super::handler::dispatch_keys(&req, w, app)
                })
                .unwrap_or_else(|e| {
                    Err(fauxfield_mcp::UiError::Failed(format!(
                        "the window is gone: {e}"
                    )))
                })
            } else {
                window
                    .update(cx, |shell, window, cx| {
                        dispatch(&req, &mut host, shell, window, cx)
                    })
                    .unwrap_or_else(|e| {
                        Err(fauxfield_mcp::UiError::Failed(format!(
                            "the window is gone: {e}"
                        )))
                    })
            };
            let _ = reply.send(result);
        }
    })
    .detach();
}

/// Turns `ui.state` changes into `ui.events` entries. Runs in the tool layer: the UI thread only
/// answers `ui.state`, so nothing here touches GPUI.
async fn feed_events(bridge: Arc<dyn UiBridge>, events: EventBuffer) {
    let mut last_generation = None;
    let mut last_toasts: Vec<Value> = Vec::new();
    loop {
        tokio::time::sleep(Duration::from_millis(100)).await;
        let Ok(state) = bridge
            .request(UiRequest {
                method: "ui.state".into(),
                params: Value::Null,
            })
            .await
        else {
            continue;
        };
        let generation = state["generation"].as_u64();
        if generation != last_generation {
            last_generation = generation;
            events.push(
                "generation",
                serde_json::json!({ "generation": generation }),
            );
        }
        let toasts = state["toasts"].as_array().cloned().unwrap_or_default();
        for t in &toasts {
            if !last_toasts.contains(t) {
                events.push("toast", t.clone());
            }
        }
        last_toasts = toasts;
    }
}
