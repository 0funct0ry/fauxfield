//! Runs on the GPUI thread: answers one bridge request against the `Shell`. One `match` on the
//! method name; the same function is called by the host loop and by the tests.

use fauxfield_mcp::{UiError, UiRequest};
use gpui::{Context, Window};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::capture::WindowBounds;
use super::state::{Tracker, snapshot};
use crate::shell::Shell;
use crate::tabs::TabKind;

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
            let (kind, row_id) = match input.kind {
                TabKindArg::Route => (TabKind::Route, input.id.clone()),
                TabKindArg::Server => (TabKind::Server, input.id.clone()),
                TabKindArg::Environment => (TabKind::Environment, format!("env:{}", input.id)),
            };
            match shell.tab_for_row(&row_id) {
                Some((k, _)) if k == kind => {}
                _ => {
                    return Err(UiError::Failed(format!(
                        "no {} with id {:?}",
                        serde_json::to_value(kind)
                            .unwrap_or_default()
                            .as_str()
                            .unwrap_or("tab"),
                        input.id
                    )));
                }
            }
            // The same path as a single click on the tree row: a preview tab.
            shell.click_row(&row_id, 1, cx);
            let tab = shell.tabs.active().cloned();
            Ok(json!({ "tab": tab }))
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
}
