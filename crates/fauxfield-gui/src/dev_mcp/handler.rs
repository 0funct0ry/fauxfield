//! Runs on the GPUI thread: answers one bridge request against the `Shell`. One `match` on the
//! method name; the same function is called by the host loop and by the tests.

use fauxfield_mcp::{UiError, UiRequest};
use gpui::{App, Context, Window};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::capture::WindowBounds;
use super::input;
use super::state::{Tracker, snapshot};
use crate::shell::Shell;

/// Mutable host state that lives next to the loop that drains the bridge.
#[derive(Default)]
pub struct HostState {
    pub tracker: Tracker,
    pub workspace_path: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenWorkspaceInput {
    /// Workspace folder. Before M2 this selects the mock workspace and only checks the folder exists.
    pub path: String,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabKindArg {
    Route,
    Server,
    Environment,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenTabInput {
    /// Tab kind: `route`, `server` or `environment`.
    pub kind: TabKindArg,
    /// Route id, server id or environment name.
    pub id: String,
}

fn args<T: for<'de> Deserialize<'de>>(params: &Value) -> Result<T, UiError> {
    serde_json::from_value(params.clone())
        .map_err(|e| UiError::Failed(format!("bad arguments: {e}")))
}

/// `ui.key` and `ui.type_text` run the shell's own key listeners, which update the `Shell`
/// entity, so they must be served outside `dispatch` (which holds the `Shell` mutably).
pub fn is_key_method(method: &str) -> bool {
    matches!(method, "ui.key" | "ui.type_text")
}

pub fn dispatch_keys(req: &UiRequest, window: &mut Window, cx: &mut App) -> Result<Value, UiError> {
    match req.method.as_str() {
        "ui.key" => {
            let i: input::KeyInput = args(&req.params)?;
            input::key(&i.keys, window, cx)
        }
        "ui.type_text" => {
            let i: input::TypeTextInput = args(&req.params)?;
            input::type_text(&i.text, window, cx)
        }
        other => Err(UiError::Failed(format!("unknown ui method {other:?}"))),
    }
}

pub fn dispatch(
    req: &UiRequest,
    host: &mut HostState,
    shell: &mut Shell,
    window: &mut Window,
    cx: &mut Context<Shell>,
) -> Result<Value, UiError> {
    match req.method.as_str() {
        "ui.state" => {
            let s = snapshot(shell, window, cx, host.workspace_path.as_deref());
            Ok(serde_json::to_value(host.tracker.observe(s)).expect("UiState serializes"))
        }
        "ui.open_workspace" => {
            let input: OpenWorkspaceInput = args(&req.params)?;
            if !std::path::Path::new(&input.path).is_dir() {
                return Err(UiError::Failed(format!("{} is not a folder", input.path)));
            }
            host.workspace_path = Some(input.path.clone());
            cx.notify();
            Ok(json!({ "workspace": shell.ws.name, "path": input.path, "mock": true }))
        }
        "ui.open_tab" => {
            let input: OpenTabInput = args(&req.params)?;
            input::open_tab(shell, input.kind, &input.id, cx)
        }
        "ui.list_actions" => Ok(input::list_actions(shell, window, cx)),
        "ui.invoke_action" => {
            let i: input::InvokeActionInput = args(&req.params)?;
            input::invoke_action(&i.name, window, cx)
        }
        "ui.command" => input::command(args(&req.params)?, shell, window, cx),
        "ui.click" => input::click(args(&req.params)?),
        "ui.scroll" => input::scroll(args(&req.params)?),
        "ui.idle_probe" => {
            let s = snapshot(shell, window, cx, host.workspace_path.as_deref());
            let generation = host.tracker.observe(s).generation;
            Ok(json!({
                "generation": generation,
                "pending": shell.chord_pending() || shell.resize.is_some(),
            }))
        }
        "ui.window_bounds" => {
            // Bring the window forward so a capture sees it.
            window.activate_window();
            let b = window.bounds();
            let bounds = WindowBounds {
                x: f32::from(b.origin.x),
                y: f32::from(b.origin.y),
                width: f32::from(b.size.width),
                height: f32::from(b.size.height),
                scale: window.scale_factor(),
            };
            Ok(serde_json::to_value(bounds).expect("bounds serialize"))
        }
        other => Err(UiError::Failed(format!("unknown ui method {other:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shell::{PickerKind, ShellOptions};
    use gpui::{Entity, TestAppContext, VisualTestContext};

    fn setup(cx: &mut TestAppContext) -> (Entity<Shell>, &mut VisualTestContext) {
        let (view, cx) = cx.add_window_view(|w, cx| {
            Shell::new(
                ShellOptions {
                    layout_path: None,
                    config_dir: None,
                },
                w,
                cx,
            )
        });
        cx.run_until_parked();
        (view, cx)
    }

    fn call(
        view: &Entity<Shell>,
        cx: &mut VisualTestContext,
        host: &mut HostState,
        method: &str,
        params: Value,
    ) -> Result<Value, UiError> {
        let req = UiRequest {
            method: method.into(),
            params,
        };
        if is_key_method(method) {
            return cx.update(|w, app| dispatch_keys(&req, w, app));
        }
        view.update_in(cx, |s, w, cx| dispatch(&req, host, s, w, cx))
    }

    fn state(view: &Entity<Shell>, cx: &mut VisualTestContext, h: &mut HostState) -> Value {
        call(view, cx, h, "ui.state", json!({})).unwrap()
    }

    #[gpui::test]
    fn ui_state_matches_shell(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();

        let s = state(&view, cx, &mut h);
        assert_eq!(s["docks"]["left"]["open"], true);
        assert_eq!(s["docks"]["right"]["open"], false);
        assert_eq!(s["bottom_tab"], "traffic");
        assert_eq!(s["traffic"]["row_count"], crate::shell::TRAFFIC_ROWS);
        assert_eq!(s["tabs"], json!([]));
        assert_eq!(s["quick_search"]["open"], false);
        assert_eq!(s["window"]["scale"], cx.update(|w, _| w.scale_factor()));
        let (name, env, problems, logs) = view.read_with(cx, |s, _| {
            (
                s.ws.name.clone(),
                s.active_env.clone(),
                s.problems.len(),
                s.logs.len(),
            )
        });
        assert_eq!(s["workspace"]["name"], name);
        assert_eq!(s["active_environment"], env);
        assert_eq!(s["problems"], problems);
        assert_eq!(s["logs"], logs);

        // Quick Search: open, type a query.
        cx.simulate_keystrokes("secondary-shift-p");
        cx.simulate_input("theme");
        let s = state(&view, cx, &mut h);
        let (query, count, hl) = view.read_with(cx, |s, _| {
            let p = s.picker.as_ref().unwrap();
            (
                p.query.clone(),
                p.hits.len(),
                p.hits.get(p.selected).map(|h| h.label.clone()),
            )
        });
        assert_eq!(s["command_palette"]["open"], true);
        assert_eq!(s["command_palette"]["query"], query);
        assert_eq!(s["command_palette"]["result_count"], count);
        assert_eq!(s["command_palette"]["highlighted"], json!(hl));
        assert_eq!(s["quick_search"]["open"], false);
        cx.simulate_keystrokes("escape");

        // Tabs: preview, then pinned; docks; bottom tab; theme selector.
        let tab = call(
            &view,
            cx,
            &mut h,
            "ui.open_tab",
            json!({ "kind": "route", "id": "rt_health" }),
        )
        .unwrap();
        assert_eq!(tab["tab"]["id"], "rt_health");
        let s = state(&view, cx, &mut h);
        assert_eq!(s["tabs"][0]["preview"], true);
        assert_eq!(s["tabs"][0]["pinned"], false);
        assert_eq!(s["active_tab"], "rt_health");
        assert_eq!(s["tree"]["selected"], "rt_health");

        cx.simulate_keystrokes("secondary-b secondary-alt-b");
        cx.simulate_keystrokes("secondary-k secondary-t");
        let s = state(&view, cx, &mut h);
        let (left, right, hl, theme) = view.read_with(cx, |s, cx| {
            let p = s.picker.as_ref().unwrap();
            (
                s.layout.left_open,
                s.layout.right_open,
                p.hits[p.selected].label.clone(),
                crate::theme::theme(cx).name.clone(),
            )
        });
        assert_eq!(s["docks"]["left"]["open"], left);
        assert_eq!(s["docks"]["right"]["open"], right);
        assert_eq!(s["theme_selector"]["open"], true);
        assert_eq!(s["theme_selector"]["highlighted"], json!(hl));
        assert_eq!(s["theme"], theme);
        cx.simulate_keystrokes("escape");

        view.update_in(cx, |s, w, cx| s.open_picker(PickerKind::Search, w, cx));
        let s = state(&view, cx, &mut h);
        assert_eq!(s["quick_search"]["open"], true);
    }

    #[gpui::test]
    fn generation_increases_only_on_change(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let g1 = state(&view, cx, &mut h)["generation"].as_u64().unwrap();
        let g2 = state(&view, cx, &mut h)["generation"].as_u64().unwrap();
        assert_eq!(g1, g2, "reading twice must not bump the generation");
        view.update(cx, |s, cx| s.toggle_right(cx));
        let g3 = state(&view, cx, &mut h)["generation"].as_u64().unwrap();
        assert_eq!(g3, g2 + 1);
        assert_eq!(state(&view, cx, &mut h)["generation"].as_u64().unwrap(), g3);
    }

    #[gpui::test]
    fn open_tab_and_workspace_errors(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let e = call(
            &view,
            cx,
            &mut h,
            "ui.open_tab",
            json!({ "kind": "route", "id": "nope" }),
        )
        .unwrap_err();
        assert!(e.to_string().contains("no route"), "{e}");
        let e = call(
            &view,
            cx,
            &mut h,
            "ui.open_tab",
            json!({ "kind": "bogus", "id": "x" }),
        )
        .unwrap_err();
        assert!(e.to_string().contains("bad arguments"), "{e}");
        let e = call(
            &view,
            cx,
            &mut h,
            "ui.open_workspace",
            json!({ "path": "/definitely/not/here" }),
        )
        .unwrap_err();
        assert!(e.to_string().contains("not a folder"), "{e}");

        let dir = std::env::temp_dir();
        let r = call(
            &view,
            cx,
            &mut h,
            "ui.open_workspace",
            json!({ "path": dir.display().to_string() }),
        )
        .unwrap();
        assert_eq!(r["mock"], true);
        let s = state(&view, cx, &mut h);
        assert_eq!(s["workspace"]["path"], dir.display().to_string());
        assert!(call(&view, cx, &mut h, "ui.nope", json!({})).is_err());
    }

    #[gpui::test]
    fn window_bounds_reports_size_and_scale(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let b = call(&view, cx, &mut h, "ui.window_bounds", json!({})).unwrap();
        assert!(b["width"].as_f64().unwrap() > 0.0);
        assert!(b["scale"].as_f64().unwrap() >= 1.0);
    }

    fn run(
        view: &Entity<Shell>,
        cx: &mut VisualTestContext,
        h: &mut HostState,
        method: &str,
        params: Value,
    ) -> Result<Value, UiError> {
        let r = call(view, cx, h, method, params);
        cx.run_until_parked();
        r
    }

    #[gpui::test]
    fn list_actions_covers_the_table(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let v = run(&view, cx, &mut h, "ui.list_actions", json!({})).unwrap();
        let list = v["actions"].as_array().unwrap();
        assert_eq!(list.len(), crate::actions::ACTIONS.len());
        let theme = list.iter().find(|a| a["name"] == "select_theme").unwrap();
        assert_eq!(theme["binding"], "secondary-k secondary-t");
        assert_eq!(theme["label"], "Theme Selector: Toggle");
        assert!(list.iter().all(|a| a["enabled"].is_boolean()));
        for a in list {
            eprintln!("{} enabled={}", a["name"], a["enabled"]);
        }
    }

    #[gpui::test]
    fn invoke_action_errors_and_effects(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let e = run(
            &view,
            cx,
            &mut h,
            "ui.invoke_action",
            json!({"name": "nope"}),
        )
        .unwrap_err();
        assert!(e.to_string().contains("unknown action"), "{e}");
        let e = run(
            &view,
            cx,
            &mut h,
            "ui.invoke_action",
            json!({"name": "quit"}),
        )
        .unwrap_err();
        assert!(e.to_string().contains("refused"), "{e}");

        run(
            &view,
            cx,
            &mut h,
            "ui.invoke_action",
            json!({"name": "toggle_right_dock"}),
        )
        .unwrap();
        assert_eq!(state(&view, cx, &mut h)["docks"]["right"]["open"], true);
        run(
            &view,
            cx,
            &mut h,
            "ui.invoke_action",
            json!({"name": "command_palette"}),
        )
        .unwrap();
        assert_eq!(state(&view, cx, &mut h)["command_palette"]["open"], true);
    }

    #[gpui::test]
    fn scripted_sequence_through_tools_only(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let theme0 = state(&view, cx, &mut h)["theme"].clone();

        // Palette by keystroke, filter to "theme", run the theme selector.
        run(
            &view,
            cx,
            &mut h,
            "ui.key",
            json!({"keys": "secondary-shift-p"}),
        )
        .unwrap();
        run(&view, cx, &mut h, "ui.type_text", json!({"text": "theme"})).unwrap();
        let s = state(&view, cx, &mut h);
        assert_eq!(s["command_palette"]["query"], "theme");
        assert!(s["command_palette"]["result_count"].as_u64().unwrap() >= 1);
        run(&view, cx, &mut h, "ui.key", json!({"keys": "enter"})).unwrap();
        let s = state(&view, cx, &mut h);
        assert_eq!(s["theme_selector"]["open"], true);
        assert_eq!(s["command_palette"]["open"], false);

        // Move the highlight: the theme previews live, escape reverts it.
        // The highlight starts on the current theme, which may be the last entry: try both ways.
        run(&view, cx, &mut h, "ui.key", json!({"keys": "down"})).unwrap();
        run(&view, cx, &mut h, "ui.key", json!({"keys": "up up"})).unwrap();
        let s = state(&view, cx, &mut h);
        assert_ne!(s["theme"], theme0, "highlight previews another theme");
        run(&view, cx, &mut h, "ui.key", json!({"keys": "escape"})).unwrap();
        let s = state(&view, cx, &mut h);
        assert_eq!(s["theme"], theme0);
        assert_eq!(s["theme_selector"]["open"], false);

        // Quick Search with the `/` prefix, open the route as a preview tab.
        run(
            &view,
            cx,
            &mut h,
            "ui.command",
            json!({"command": "search", "query": "/health"}),
        )
        .unwrap();
        let s = state(&view, cx, &mut h);
        assert_eq!(s["quick_search"]["open"], true);
        assert!(s["quick_search"]["result_count"].as_u64().unwrap() >= 1);
        run(&view, cx, &mut h, "ui.key", json!({"keys": "enter"})).unwrap();
        let s = state(&view, cx, &mut h);
        assert_eq!(s["quick_search"]["open"], false);
        assert_eq!(s["tabs"][0]["preview"], true);
        assert_eq!(s["tabs"].as_array().unwrap().len(), 1);

        // All three docks, by command and by key.
        let before = state(&view, cx, &mut h)["docks"].clone();
        for d in ["left", "right", "bottom"] {
            run(
                &view,
                cx,
                &mut h,
                "ui.command",
                json!({"command": "toggle_dock", "dock": d}),
            )
            .unwrap();
        }
        let after = state(&view, cx, &mut h)["docks"].clone();
        for d in ["left", "right", "bottom"] {
            assert_ne!(before[d]["open"], after[d]["open"], "{d}");
        }
        run(
            &view,
            cx,
            &mut h,
            "ui.key",
            json!({"keys": "secondary-b secondary-alt-b secondary-`"}),
        )
        .unwrap();
        assert_eq!(state(&view, cx, &mut h)["docks"], before);
    }

    #[gpui::test]
    fn chord_timeout_matches_the_keyboard(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let probe = |view: &Entity<Shell>, cx: &mut VisualTestContext, h: &mut HostState| {
            call(view, cx, h, "ui.idle_probe", json!({})).unwrap()["pending"]
                .as_bool()
                .unwrap()
        };

        // The full chord opens the theme selector.
        run(
            &view,
            cx,
            &mut h,
            "ui.key",
            json!({"keys": "secondary-k secondary-t"}),
        )
        .unwrap();
        assert_eq!(state(&view, cx, &mut h)["theme_selector"]["open"], true);
        run(&view, cx, &mut h, "ui.key", json!({"keys": "escape"})).unwrap();

        // A lone Cmd+K waits 400 ms, then opens Quick Search.
        run(&view, cx, &mut h, "ui.key", json!({"keys": "secondary-k"})).unwrap();
        assert!(probe(&view, cx, &mut h), "chord pending");
        assert_eq!(state(&view, cx, &mut h)["quick_search"]["open"], false);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(300));
        cx.run_until_parked();
        assert_eq!(state(&view, cx, &mut h)["quick_search"]["open"], false);
        cx.executor()
            .advance_clock(std::time::Duration::from_millis(150));
        cx.run_until_parked();
        assert_eq!(state(&view, cx, &mut h)["quick_search"]["open"], true);
        assert!(!probe(&view, cx, &mut h));
    }

    #[gpui::test]
    fn command_arguments_and_pointer_errors(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        let e = run(
            &view,
            cx,
            &mut h,
            "ui.command",
            json!({"command": "toggle_dock"}),
        )
        .unwrap_err();
        assert!(e.to_string().contains("needs `dock`"), "{e}");
        let e = run(
            &view,
            cx,
            &mut h,
            "ui.command",
            json!({"command": "set_environment", "name": "zzz"}),
        )
        .unwrap_err();
        assert!(e.to_string().contains("no environment"), "{e}");
        let e = run(
            &view,
            cx,
            &mut h,
            "ui.command",
            json!({"command": "select_tab", "index": 3}),
        )
        .unwrap_err();
        assert!(e.to_string().contains("out of range"), "{e}");
        run(
            &view,
            cx,
            &mut h,
            "ui.command",
            json!({"command": "open_tab", "kind": "route", "id": "rt_health"}),
        )
        .unwrap();
        run(
            &view,
            cx,
            &mut h,
            "ui.command",
            json!({"command": "select_tab", "index": 1}),
        )
        .unwrap();
        let e = run(&view, cx, &mut h, "ui.key", json!({"keys": "not-a-key-"})).unwrap_err();
        assert!(e.to_string().contains("bad keystroke"), "{e}");
        let e = run(&view, cx, &mut h, "ui.click", json!({"x": 10, "y": 10})).unwrap_err();
        assert!(e.to_string().contains("pointer injection"), "{e}");
        let e = run(
            &view,
            cx,
            &mut h,
            "ui.scroll",
            json!({"x": 10, "y": 10, "dy": -50}),
        )
        .unwrap_err();
        assert!(e.to_string().contains("pointer injection"), "{e}");
    }
}
