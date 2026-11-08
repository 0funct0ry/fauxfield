//! The `ui.*` tools of M1.2. Each forwards one request over the `UiBridge`; none touches GPUI.

use std::sync::Arc;

use std::time::{Duration, Instant};

use fauxfield_mcp::{
    Registry, ToolClass, ToolDef, UiBridge, UiRequest, condition::Condition, tools::NoInput,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::handler::{OpenTabInput, OpenWorkspaceInput};
use super::input::{
    ClickInput, CommandInput, InvokeActionInput, KeyInput, ScrollInput, TypeTextInput,
};
use super::screenshot::{
    ResizeToolInput, ScreenshotAllInput, ScreenshotInput, screenshot, screenshot_all,
};

const POLL: Duration = Duration::from_millis(40);
const DEFAULT_WAIT_MS: u64 = 5000;

#[derive(Deserialize, JsonSchema)]
pub struct WaitIdleInput {
    /// Give up after this many milliseconds (default 5000).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
pub struct WaitForInput {
    /// Condition over `ui.state`: a JSON pointer plus one of equals / contains / matches.
    pub condition: Condition,
    /// Give up after this many milliseconds (default 5000).
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

pub(super) async fn call(
    bridge: &Arc<dyn UiBridge>,
    method: &'static str,
    params: Value,
) -> Result<Value, String> {
    bridge
        .request(UiRequest {
            method: method.into(),
            params,
        })
        .await
        .map_err(|e| e.to_string())
}

/// Input tools return the new `ui.state` generation and the toasts showing after the input.
/// The follow-up `ui.state` request is served after the input's deferred effects have run.
async fn with_outcome(
    bridge: &Arc<dyn UiBridge>,
    method: &'static str,
    args: Map<String, Value>,
) -> Result<Value, String> {
    let mut out = call(bridge, method, Value::Object(args)).await?;
    let state = call(bridge, "ui.state", json!({})).await?;
    if let Some(o) = out.as_object_mut() {
        o.insert("generation".into(), state["generation"].clone());
        o.insert("toasts".into(), state["toasts"].clone());
    }
    Ok(out)
}

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

fn input_tool<I: JsonSchema>(
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
            let bridge = bridge.clone();
            Box::pin(async move { with_outcome(&bridge, name, args).await })
        },
    ));
}

fn timeout(ms: Option<u64>) -> Duration {
    Duration::from_millis(ms.unwrap_or(DEFAULT_WAIT_MS))
}

/// Polls from the tool layer so the UI thread never blocks: idle means two consecutive probes
/// with the same generation and nothing pending (chord timer, resize drag).
pub(super) async fn wait_idle(
    bridge: &Arc<dyn UiBridge>,
    limit: Duration,
) -> Result<Value, String> {
    let start = Instant::now();
    let mut last: Option<u64> = None;
    loop {
        let p = call(bridge, "ui.idle_probe", json!({})).await?;
        let generation = p["generation"].as_u64().unwrap_or(0);
        let pending = p["pending"].as_bool().unwrap_or(true);
        if !pending && last == Some(generation) {
            return Ok(json!({
                "idle": true,
                "generation": generation,
                "waited_ms": start.elapsed().as_millis() as u64,
            }));
        }
        last = if pending { None } else { Some(generation) };
        if start.elapsed() >= limit {
            return Err(format!(
                "ui not idle after {} ms (generation {generation}, pending {pending})",
                limit.as_millis()
            ));
        }
        tokio::time::sleep(POLL).await;
    }
}

pub(super) async fn wait_for(
    bridge: &Arc<dyn UiBridge>,
    cond: &Condition,
    limit: Duration,
) -> Result<Value, String> {
    let start = Instant::now();
    loop {
        let state = call(bridge, "ui.state", json!({})).await?;
        if cond.eval(&state)? {
            return Ok(json!({
                "satisfied": true,
                "generation": state["generation"],
                "waited_ms": start.elapsed().as_millis() as u64,
            }));
        }
        if start.elapsed() >= limit {
            return Err(format!(
                "condition on {} not met after {} ms (generation {}, value {})",
                cond.pointer,
                limit.as_millis(),
                state["generation"],
                state.pointer(&cond.pointer).unwrap_or(&Value::Null)
            ));
        }
        tokio::time::sleep(POLL).await;
    }
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
        "Open a workspace folder (replaces the open one; servers stop first).",
    );
    tool::<super::handler::CreateWorkspaceInput>(
        registry,
        &bridge,
        "ui.create_workspace",
        "Create a workspace in a folder and open it, through the same flow as the Create \
         workspace button (refuses an existing workspace; a non-empty folder asks to confirm \
         with modal `create_confirm`). Native folder dialogs cannot be driven, so use this.",
    );
    tool::<OpenTabInput>(
        registry,
        &bridge,
        "ui.open_tab",
        "Open a tab as a preview tab, like a single click in the tree. kind: route, server or \
         environment; id: route id, server id or environment name.",
    );

    tool::<super::tree::TreeInput>(
        registry,
        &bridge,
        "ui.tree",
        "Semantic UI tree of the last painted frame: every tagged element with id, role, label, \
         enabled, visible, clipped (by an ancestor), focused, z, bounds (window points) and \
         parent. Filter by `root` (subtree), `role`, `visible_only`. Virtualised lists report \
         `total`.",
    );
    tool::<super::tree::FindInput>(
        registry,
        &bridge,
        "ui.find",
        "Find elements by label, role or text (case-insensitive substrings of label or id).",
    );
    input_tool::<super::tree::ClickElementInput>(
        registry,
        &bridge,
        "ui.click_element",
        "Click an element by id from ui.tree/ui.find. Runs the same Shell method the pointer \
         handler runs (synthetic pointer events are impossible in gpui 0.2.2). Returns the new \
         ui.state generation and toasts.",
    );
    tool::<NoInput>(
        registry,
        &bridge,
        "ui.list_actions",
        "List every action of the ACTIONS table: name, label, binding, key context and whether \
         it is enabled now.",
    );
    input_tool::<InvokeActionInput>(
        registry,
        &bridge,
        "ui.invoke_action",
        "Run an action by name through GPUI dispatch. A disabled action is an error. Returns \
         the new ui.state generation and toasts.",
    );
    input_tool::<CommandInput>(
        registry,
        &bridge,
        "ui.command",
        "Typed operations that take arguments: open_tab, select_tab, toggle_dock, \
         set_environment, search. One flat object: `command` plus its arguments.",
    );
    input_tool::<KeyInput>(
        registry,
        &bridge,
        "ui.key",
        "Press a keystroke (`cmd-shift-p`) or chord (`cmd-k cmd-t`) as if typed. The Cmd+K chord \
         timeout (400 ms) behaves as on the keyboard; follow with ui.wait_for_idle.",
    );
    input_tool::<TypeTextInput>(
        registry,
        &bridge,
        "ui.type_text",
        "Type text into the focused input, one keystroke per character.",
    );
    input_tool::<ClickInput>(
        registry,
        &bridge,
        "ui.click",
        "Click at window coordinates. Not available with the pinned GPUI (no synthetic pointer \
         events): returns an error; use ui.command or ui.invoke_action.",
    );
    input_tool::<ScrollInput>(
        registry,
        &bridge,
        "ui.scroll",
        "Scroll at window coordinates. Not available with the pinned GPUI: returns an error.",
    );
    let b = bridge.clone();
    registry.register(ToolDef::new::<WaitIdleInput>(
        "ui.wait_for_idle",
        "Wait until the UI is idle: no pending chord or drag, and two polls with the same \
         ui.state generation. Errors on timeout.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let b = b.clone();
            Box::pin(async move {
                let i: WaitIdleInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                wait_idle(&b, timeout(i.timeout_ms)).await
            })
        },
    ));
    let b = bridge.clone();
    registry.register(ToolDef::new::<WaitForInput>(
        "ui.wait_for",
        "Wait until a condition over ui.state holds (JSON pointer with equals, contains or \
         matches; `not` inverts). Errors on timeout.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let b = b.clone();
            Box::pin(async move {
                let i: WaitForInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                wait_for(&b, &i.condition, timeout(i.timeout_ms)).await
            })
        },
    ));

    input_tool::<super::handler::SetThemeInput>(
        registry,
        &bridge,
        "ui.set_theme",
        "Switch the theme (`one-dark` or `one-light`) without persisting it.",
    );
    let b = bridge.clone();
    registry.register(ToolDef::new::<ResizeToolInput>(
        "ui.resize",
        "Resize the window: `default` (1280x800), `compact` (900x600), `min` or `WxH`. Waits \
         until ui.state reports the new size.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let b = b.clone();
            Box::pin(async move {
                let i: ResizeToolInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                super::screenshot::resize(&b, &i.size).await
            })
        },
    ));
    let b = bridge.clone();
    registry.register(ToolDef::new::<ScreenshotInput>(
        "ui.screenshot",
        "Screenshot of the window or one element (`element`: id from ui.tree), optionally after \
         running a `state`: a named state (route-tab, server-tab, environment-tab, traffic-10k, \
         palette-open, quick-search-open, theme-selector-open, docks-collapsed, problems-open) or \
         a list of steps (reset, open, invoke, key, type, command, set_theme, resize, wait_for, \
         wait_idle). Returns the PNG plus ui.state, size, theme, scale and capture tier. Volatile \
         text is masked. A missing permission or tool is reported, never a blank image. \
         `capture: false` returns only ui.state.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let b = b.clone();
            Box::pin(async move {
                let i: ScreenshotInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                screenshot(&b, super::capture::select().map(Arc::from), i).await
            })
        },
    ));
    let b = bridge.clone();
    registry.register(ToolDef::new::<super::audit::AuditInput>(
        "ui.audit",
        "Lint the UI (SPEC §11.4): UX101 clipped text without tooltip, UX102 overlapping \
         siblings, UX103 target under 20 pt, UX110 token contrast (both themes), UX120 colour \
         literals, UX130 actions missing from the palette or SPEC §9.8 binding, UX131 no \
         keyboard path, UX140 view without empty-state fixture, UX150 frame/key-to-paint time \
         (only with `perf` or in `rules`). Audits what is on screen; `themes`, `sizes` and \
         `states` (`[\"all\"]` for every fixture) sweep a matrix and restore theme and size. \
         Findings carry rule, severity, message, element ids, bounds and a suggested fix.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let b = b.clone();
            Box::pin(async move {
                let i: super::audit::AuditInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                super::audit::audit(&b, i).await
            })
        },
    ));
    let b = bridge.clone();
    registry.register(ToolDef::new::<super::audit::WalkthroughInput>(
        "ui.walkthrough",
        "Run a list of steps (reset, open, invoke, key, type, command, click_element, set_theme, \
         resize, wait_for, wait_idle) and report per-step time and totals: steps, keystrokes, \
         clicks, wall-clock ms and toasts, to compare the cost of a task over time.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let b = b.clone();
            Box::pin(async move {
                let i: super::audit::WalkthroughInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                super::audit::walkthrough(&b, i).await
            })
        },
    ));
    let b = bridge;
    registry.register(ToolDef::new::<ScreenshotAllInput>(
        "ui.screenshot_all",
        "Capture a matrix of named states x themes x sizes. Returns labelled cells and the \
         images in cell order; theme and size are restored afterwards even if a cell fails.",
        ToolClass::Ui,
        move |args: Map<String, Value>| {
            let b = b.clone();
            Box::pin(async move {
                let i: ScreenshotAllInput = serde_json::from_value(Value::Object(args))
                    .map_err(|e| format!("bad arguments: {e}"))?;
                screenshot_all(&b, super::capture::select().map(Arc::from), i).await
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
            "ui.set_theme",
            "ui.resize",
            "ui.screenshot",
            "ui.screenshot_all",
            "ui.audit",
            "ui.walkthrough",
            "ui.list_actions",
            "ui.tree",
            "ui.find",
            "ui.click_element",
            "ui.invoke_action",
            "ui.command",
            "ui.key",
            "ui.type_text",
            "ui.click",
            "ui.scroll",
            "ui.wait_for_idle",
            "ui.wait_for",
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

    /// Answers `ui.state` with a counter that reaches 3, and `ui.idle_probe` from a script.
    struct Fake {
        n: std::sync::atomic::AtomicU64,
        probes: std::sync::Mutex<Vec<(u64, bool)>>,
    }
    impl UiBridge for Fake {
        fn request(&self, req: UiRequest) -> fauxfield_mcp::bridge::UiFuture {
            use std::sync::atomic::Ordering::SeqCst;
            let v = match req.method.as_str() {
                "ui.state" => {
                    let n = self.n.fetch_add(1, SeqCst);
                    json!({"generation": n, "toasts": ["hi"], "count": n})
                }
                "ui.idle_probe" => {
                    let mut p = self.probes.lock().unwrap();
                    let (g, pending) = if p.len() > 1 { p.remove(0) } else { p[0] };
                    json!({"generation": g, "pending": pending})
                }
                m => json!({"method": m}),
            };
            Box::pin(async move { Ok(v) })
        }
    }

    fn fake(probes: Vec<(u64, bool)>) -> Arc<dyn UiBridge> {
        Arc::new(Fake {
            n: 0.into(),
            probes: std::sync::Mutex::new(probes),
        })
    }

    #[tokio::test]
    async fn wait_for_succeeds_and_times_out() {
        let b = fake(vec![]);
        let c: Condition =
            serde_json::from_value(json!({"pointer": "/count", "equals": 3})).unwrap();
        let ok = wait_for(&b, &c, Duration::from_secs(2)).await.unwrap();
        assert_eq!(ok["satisfied"], true);
        let never: Condition =
            serde_json::from_value(json!({"pointer": "/count", "equals": -1})).unwrap();
        let e = wait_for(&b, &never, Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(e.contains("not met"), "{e}");
    }

    #[tokio::test]
    async fn idle_needs_two_equal_polls_and_nothing_pending() {
        // pending, then generation changes, then two equal polls.
        let b = fake(vec![(1, true), (2, false), (3, false), (3, false)]);
        let r = wait_idle(&b, Duration::from_secs(2)).await.unwrap();
        assert_eq!(r["generation"], 3);
        let busy = fake(vec![(1, true)]);
        let e = wait_idle(&busy, Duration::from_millis(100))
            .await
            .unwrap_err();
        assert!(e.contains("not idle"), "{e}");
    }

    #[tokio::test]
    async fn input_tools_report_generation_and_toasts() {
        let b = fake(vec![]);
        let v = with_outcome(&b, "ui.key", Map::new()).await.unwrap();
        assert_eq!(v["method"], "ui.key");
        assert_eq!(v["toasts"], json!(["hi"]));
        assert!(v["generation"].is_u64());
    }
}
