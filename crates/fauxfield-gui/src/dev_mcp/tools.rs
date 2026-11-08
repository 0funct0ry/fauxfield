//! The `ui.*` tools of M1.2. Each forwards one request over the `UiBridge`; none touches GPUI.

use std::sync::Arc;

use fauxfield_mcp::{Registry, ToolClass, ToolDef, UiBridge, UiRequest, tools::NoInput};
use serde_json::{Map, Value};

use super::capture::{self, WindowBounds};
use super::handler::{OpenTabInput, OpenWorkspaceInput};

fn forward(
    bridge: &Arc<dyn UiBridge>,
    method: &'static str,
    args: Map<String, Value>,
) -> fauxfield_mcp::bridge::UiFuture {
    bridge.request(UiRequest {
        method: method.into(),
        params: Value::Object(args),
    })
}

fn tool<I: schemars::JsonSchema>(
    registry: &mut Registry,
    bridge: &Arc<dyn UiBridge>,
    name: &'static str,
    description: &'static str,
) {
    let bridge = bridge.clone();
    registry.register(ToolDef::new::<I>(
        name,
        description,
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let fut = forward(&bridge, name, args);
            Box::pin(async move { fut.await.map_err(|e| e.to_string()) })
        },
    ));
}

/// Adds the `ui.*` tools to `registry`.
pub fn register_ui_tools(registry: &mut Registry, bridge: Arc<dyn UiBridge>) {
    tool::<NoInput>(
        registry,
        &bridge,
        "ui.state",
        "Snapshot of the whole window: tabs, docks, tree, Quick Search, Command Palette, theme \
         selector, toasts, Traffic, Problems and Logs counts, servers, environment, window size, \
         scale, theme, and a `generation` that increases when the snapshot changes.",
    );
    tool::<OpenWorkspaceInput>(
        registry,
        &bridge,
        "ui.open_workspace",
        "Open a workspace folder. Before M2 this selects the mock workspace.",
    );
    tool::<OpenTabInput>(
        registry,
        &bridge,
        "ui.open_tab",
        "Open a tab as a preview tab, like a single click in the tree. kind: route, server or \
         environment; id: route id, server id or environment name.",
    );

    let b = bridge;
    registry.register(ToolDef::new::<NoInput>(
        "ui.capture_probe",
        "Prototype for M1.5: report the window bounds in screen coordinates and scale, and \
         capture the window region with screencapture (macOS). Errors name the missing \
         permission. Replaced by ui.screenshot in M1.5.",
        ToolClass::Ui,
        move |_: Map<String, Value>| {
            let fut = forward(&b, "ui.window_bounds", Map::new());
            Box::pin(async move {
                let v = fut.await.map_err(|e| e.to_string())?;
                let bounds: WindowBounds = serde_json::from_value(v).map_err(|e| e.to_string())?;
                // Let the window come to the front before capturing; run off the UI thread.
                tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                tokio::task::spawn_blocking(move || capture::probe(bounds))
                    .await
                    .map_err(|e| e.to_string())?
            })
        },
    ));
}

#[cfg(test)]
mod tests {
    use super::*;
    use fauxfield_mcp::{AllowedClasses, CallError, Registry};
    use serde_json::json;

    use super::super::bridge::GuiBridge;

    fn registry() -> Registry {
        let (bridge, mut rx) = GuiBridge::channel();
        tokio::spawn(async move {
            while let Some((req, reply)) = rx.recv().await {
                let _ = reply.send(Ok(json!({ "method": req.method, "params": req.params })));
            }
        });
        let mut r = Registry::new();
        register_ui_tools(&mut r, Arc::new(bridge));
        r
    }

    #[tokio::test]
    async fn ui_tools_need_the_ui_class() {
        let r = registry();
        let default = AllowedClasses::all();
        assert!(r.list(default).is_empty());
        let err = r.call(default, "ui.state", Map::new()).await.unwrap_err();
        assert!(matches!(err, CallError::Refused { .. }));

        let ui = default.with(ToolClass::Ui);
        let names: Vec<_> = r.list(ui).iter().map(|t| t.name).collect();
        for n in [
            "ui.state",
            "ui.open_workspace",
            "ui.open_tab",
            "ui.capture_probe",
        ] {
            assert!(names.contains(&n), "{n} missing from {names:?}");
        }
    }

    #[tokio::test]
    async fn tools_forward_their_arguments_over_the_bridge() {
        let r = registry();
        let ui = AllowedClasses::all().with(ToolClass::Ui);
        let mut args = Map::new();
        args.insert("kind".into(), json!("route"));
        args.insert("id".into(), json!("rt_health"));
        let v = r.call(ui, "ui.open_tab", args).await.unwrap();
        assert_eq!(v["method"], "ui.open_tab");
        assert_eq!(v["params"]["id"], "rt_health");
    }
}
