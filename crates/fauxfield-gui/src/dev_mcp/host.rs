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
        ..McpConfig::default()
    };

    let (bridge, mut rx) = GuiBridge::channel();
    let bridge: Arc<dyn UiBridge> = Arc::new(bridge);
    let mut registry = core_registry(ServerFacts {
        transports: match transport {
            TransportKind::Both => vec![TransportKind::Stdio, TransportKind::native_socket()],
            t => vec![t],
        },
        allowed: allow,
        features: vec!["dev-mcp"],
    });
    register_ui_tools(&mut registry, bridge.clone());
    // Product tools edit the same store the window shows (SPEC §11.4: one in-process host).
    fauxfield_mcp::workspace_tools::register_workspace_tools(
        &mut registry,
        std::sync::Arc::new(shared_store),
    );
    // Server lifecycle and traffic tools drive the engine the window hosts (M3).
    let control_provider: fauxfield_mcp::control_tools::ControlProvider =
        std::sync::Arc::new(|mode| {
            let c = CONTROL.lock().unwrap_or_else(|p| p.into_inner()).clone();
            match (c, mode) {
                (Some(c), _) => Ok(Some(c)),
                (None, fauxfield_mcp::control_tools::ControlMode::AttachOnly) => Ok(None),
                (None, _) => Err("no workspace is open; call ui.open_workspace".to_string()),
            }
        });
    fauxfield_mcp::control_tools::register_control_tools(&mut registry, control_provider.clone());
    // Agent sessions and calls show in the window like those of any other MCP process (M16).
    registry.set_sink(fauxfield_mcp::control_tools::control_sink(control_provider));
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
    let mut last_servers: Vec<Value> = Vec::new();
    let mut last_traffic_id = 0u64;
    let mut last_workspace: Option<(Option<String>, String)> = None;
    let mut last_menu_open = Value::Null;
    let mut last_invoked_seq = 0u64;
    let mut last_about = false;
    let mut last_sidebar = Value::Null;
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
        // Workspace lifecycle: opened / closed / renamed, derived from ui.state.
        let ws = (
            state["workspace"]["root"].as_str().map(str::to_string),
            state["workspace"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string(),
        );
        if last_workspace.as_ref() != Some(&ws) {
            if let Some(prev) = &last_workspace {
                let kind = match (&prev.0, &ws.0) {
                    (Some(a), Some(b)) if a == b => Some("renamed"),
                    (Some(_), None) => Some("closed"),
                    (None, Some(_)) => Some("opened"),
                    (Some(_), Some(_)) => Some("opened"),
                    (None, None) => None,
                };
                if let Some(kind) = kind {
                    events.push(
                        "workspace",
                        serde_json::json!({ "event": kind, "name": ws.1, "root": ws.0 }),
                    );
                }
            } else if ws.0.is_some() {
                events.push(
                    "workspace",
                    serde_json::json!({ "event": "opened", "name": ws.1, "root": ws.0 }),
                );
            }
            last_workspace = Some(ws);
        }
        // Menu: opened / closed (in-window bar) and invoked {path}; About open / close.
        let open = state["menus"]["open"].clone();
        if open != last_menu_open {
            events.push(
                "menu",
                serde_json::json!({ "event": if open.is_null() { "closed" } else { "opened" }, "path": open }),
            );
            last_menu_open = open;
        }
        let seq = state["menus"]["last_invoked"]["seq"].as_u64().unwrap_or(0);
        if seq != last_invoked_seq {
            last_invoked_seq = seq;
            events.push(
                "menu",
                serde_json::json!({ "event": "invoked", "path": state["menus"]["last_invoked"]["path"] }),
            );
        }
        let about = state["modal"].as_str() == Some("about");
        if about != last_about {
            last_about = about;
            events.push(
                "about",
                serde_json::json!({ "event": if about { "opened" } else { "closed" } }),
            );
        }
        let sidebar = state["sidebar"]["tab"].clone();
        if sidebar != last_sidebar {
            if !last_sidebar.is_null() {
                events.push("sidebar", serde_json::json!({ "tab": sidebar }));
            }
            last_sidebar = sidebar;
        }
        let toasts = state["toasts"].as_array().cloned().unwrap_or_default();
        for t in &toasts {
            if !last_toasts.contains(t) {
                events.push("toast", t.clone());
            }
        }
        last_toasts = toasts;
        // Server run state and live traffic (M3).
        let servers = state["servers"].as_array().cloned().unwrap_or_default();
        for s in &servers {
            if !last_servers.contains(s) {
                events.push("server", s.clone());
            }
        }
        last_servers = servers;
        let id = state["traffic"]["last_id"].as_u64().unwrap_or(0);
        if id != last_traffic_id {
            last_traffic_id = id;
            if id > 0 {
                events.push(
                    "traffic",
                    serde_json::json!({
                        "last_id": id,
                        "row_count": state["traffic"]["row_count"],
                        "last": state["traffic"]["last"],
                    }),
                );
            }
        }
    }
}

static STORE: std::sync::Mutex<Option<fauxfield_store::Store>> = std::sync::Mutex::new(None);

/// The store of the workspace the window shows; set by `Shell` when it opens one.
pub fn publish_store(store: Option<fauxfield_store::Store>) {
    *STORE.lock().unwrap_or_else(|p| p.into_inner()) = store;
}

type SharedControl = std::sync::Arc<dyn fauxfield_control::ControlApi>;
static CONTROL: std::sync::Mutex<Option<SharedControl>> = std::sync::Mutex::new(None);

/// The control handle of the engine the window hosts; set by `Shell` when it opens a workspace.
pub fn publish_control(control: Option<SharedControl>) {
    *CONTROL.lock().unwrap_or_else(|p| p.into_inner()) = control;
}

fn shared_store() -> Option<fauxfield_store::Store> {
    STORE.lock().unwrap_or_else(|p| p.into_inner()).clone()
}
