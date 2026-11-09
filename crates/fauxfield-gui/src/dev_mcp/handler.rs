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
    /// The workspace the app launched with; `ui.reset_view` returns to it.
    pub baseline: Option<std::path::PathBuf>,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenWorkspaceInput {
    /// Workspace folder containing fauxfield.toml (see `fauxfield init`).
    pub path: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct CreateWorkspaceInput {
    /// Folder to create the workspace in (SPEC §4.1). Goes through the same Shell method as the
    /// Create workspace button, including the existing-workspace and non-empty-folder rules.
    pub path: String,
}

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabKindArg {
    Route,
    Server,
    Environment,
    Dataset,
}

#[derive(Deserialize, JsonSchema)]
pub struct OpenTabInput {
    /// Tab kind: `route`, `server`, `environment` or `dataset`.
    pub kind: TabKindArg,
    /// Route id, server id, environment name or dataset key (`<server id>:<name>`).
    pub id: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct SetThemeInput {
    /// `one-dark` or `one-light` (the theme names also work). Not persisted.
    pub name: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ResizeInput {
    /// `default` (1280x800), `compact` (900x600), `min`, or `WxH` such as `1000x700`.
    pub size: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct MaskInput {
    /// Render volatile text (Traffic timestamps and durations) as fixed placeholders.
    pub on: bool,
}

/// Window size in points for a `ui.resize` preset or `WxH`.
pub fn parse_size(s: &str) -> Result<(f32, f32), String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "default" => Ok((1280.0, 800.0)),
        "compact" => Ok((900.0, 600.0)),
        "min" => Ok((MIN_SIZE.0, MIN_SIZE.1)),
        other => {
            let (w, h) = other
                .split_once('x')
                .ok_or_else(|| format!("size {s:?} is not default, compact, min or WxH"))?;
            let (w, h): (f32, f32) = (
                w.parse().map_err(|_| format!("bad width in {s:?}"))?,
                h.parse().map_err(|_| format!("bad height in {s:?}"))?,
            );
            if w < MIN_SIZE.0 || h < MIN_SIZE.1 {
                return Err(format!(
                    "{s:?} is below the minimum window size {}x{}",
                    MIN_SIZE.0, MIN_SIZE.1
                ));
            }
            Ok((w, h))
        }
    }
}

/// Matches `window_min_size` in `lib.rs`.
const MIN_SIZE: (f32, f32) = (900.0, 600.0);

fn theme_name(s: &str) -> Option<&'static str> {
    match s.trim().to_ascii_lowercase().replace(' ', "-").as_str() {
        "one-dark" => Some(crate::theme::DEFAULT_DARK),
        "one-light" => Some(crate::theme::DEFAULT_LIGHT),
        _ => None,
    }
}

/// Puts the shell in a known baseline for state fixtures: no picker, no tabs, default docks,
/// Traffic tab, no selection, no masking. Never writes the layout file.
fn reset_view(shell: &mut Shell, window: &mut Window, cx: &mut Context<Shell>) {
    if shell.picker.is_some() {
        shell.close_picker(true, window, cx);
    }
    while !shell.tabs.is_empty() {
        shell.tabs.close(0);
    }
    shell.sidebar_tab = crate::tree::SidebarTab::Servers;
    shell.layout.sidebar_tab = "servers".into();
    shell.layout.left_open = true;
    shell.layout.right_open = false;
    shell.layout.bottom_open = true;
    shell.layout.bottom_tab = crate::layout::BottomTab::Traffic;
    shell.traffic_selected = None;
    shell.mask_volatile = false;
    shell.quit_confirm = false;
    shell.notice_action = None;
    shell.notice = None;
    shell.menu = None;
    shell.prompt = None;
    shell.lc.method_menu = None;
    shell.lc.close_confirm = None;
    shell.lc.delete_confirm = None;
    shell.lc.create_confirm = None;
    shell.lc.prompt_error = None;
    shell.about_open = false;
    shell.menu_style = crate::menu::menu_style();
    shell.menu_last = None;
    shell.bar.open = None;
    shell.bar.hi = [None, None];
    shell.bar.sub = None;
    super::input::release_held_ports();
    // Baseline Traffic is the synthetic 10k rows; real requests and the engine's buffer go.
    shell.traffic = crate::traffic::synthetic(crate::shell::TRAFFIC_ROWS);
    if let Some(h) = &shell.host {
        h.engine().stop_all();
        h.engine().clear_traffic();
    }
    shell.sync_running();
    cx.notify();
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
    if host.baseline.is_none() && shell.store.is_some() {
        host.baseline = Some(shell.ws.root.clone());
    }
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
            shell
                .open_workspace(std::path::Path::new(&input.path), cx)
                .map_err(UiError::Failed)?;
            host.workspace_path = Some(shell.ws.root.display().to_string());
            Ok(json!({ "workspace": shell.ws.name, "path": shell.ws.root.display().to_string() }))
        }
        "ui.create_workspace" => {
            let input: CreateWorkspaceInput = args(&req.params)?;
            let path = std::path::PathBuf::from(&input.path);
            if !path.is_dir() {
                return Err(UiError::Failed(format!("{} is not a folder", input.path)));
            }
            // Same flow as the button: refusals and confirmations surface as toasts and modals.
            shell.create_folder_chosen(path, cx);
            if shell.store.is_some() {
                host.workspace_path = Some(shell.ws.root.display().to_string());
            }
            Ok(json!({
                "workspace": shell.ws.name,
                "path": shell.store.as_ref().map(|_| shell.ws.root.display().to_string()),
                "modal": shell.dialog_name(),
            }))
        }
        "ui.open_tab" => {
            let input: OpenTabInput = args(&req.params)?;
            input::open_tab(shell, input.kind, &input.id, cx)
        }
        "ui.list_actions" => Ok(input::list_actions(shell, window, cx)),
        "ui.invoke_action" => {
            let i: input::InvokeActionInput = args(&req.params)?;
            input::invoke_action(&i.name, shell, window, cx)
        }
        "ui.menu" => input::menu(args(&req.params)?, shell, window, cx),
        "ui.command" => input::command(args(&req.params)?, shell, window, cx),
        "ui.tree" => super::tree::tree(cx, &args(&req.params)?).map_err(UiError::Failed),
        "ui.find" => Ok(super::tree::find(cx, &args(&req.params)?)),
        "ui.click_element" => {
            let i: super::tree::ClickElementInput = args(&req.params)?;
            super::tree::click_element(&i.id, shell, window, cx)
        }
        "ui.click" => input::click(args(&req.params)?),
        "ui.scroll" => input::scroll(args(&req.params)?),
        "ui.idle_probe" => {
            let s = snapshot(shell, window, cx, host.workspace_path.as_deref());
            let generation = host.tracker.observe(s).generation;
            Ok(json!({
                "generation": generation,
                "pending": shell.chord_pending() || shell.resize.is_some() || shell.dev_pending > 0,
                "dispatching": shell.dev_pending > 0,
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
        "ui.set_theme" => {
            let i: SetThemeInput = args(&req.params)?;
            let name = theme_name(&i.name).ok_or_else(|| {
                UiError::Failed(format!(
                    "unknown theme {:?}; use one-dark or one-light",
                    i.name
                ))
            })?;
            shell.apply_theme(name, cx);
            Ok(json!({ "theme": name }))
        }
        "ui.resize" => {
            let i: ResizeInput = args(&req.params)?;
            let (w, h) = parse_size(&i.size).map_err(UiError::Failed)?;
            window.resize(gpui::size(gpui::px(w), gpui::px(h)));
            Ok(json!({ "width": w, "height": h }))
        }
        "ui.set_mask" => {
            let i: MaskInput = args(&req.params)?;
            shell.mask_volatile = i.on;
            cx.notify();
            Ok(json!({ "mask": i.on }))
        }
        "ui.audit_data" => Ok(super::audit::data(shell, cx)),
        "ui.perf_probe" => {
            let clear = req.params["clear"].as_bool().unwrap_or(false);
            Ok(super::tree::Registry::perf(cx, clear))
        }
        "ui.reset_view" => {
            reset_view(shell, window, cx);
            match &host.baseline {
                Some(b) if shell.store.is_none() || shell.ws.root != *b => {
                    shell.open_workspace(b, cx).map_err(UiError::Failed)?;
                }
                _ => {}
            }
            Ok(json!({ "reset": true }))
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
                    ..crate::view::test_options()
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

        // A folder that is not a workspace is refused.
        let plain = tempfile::tempdir().unwrap();
        let e = call(
            &view,
            cx,
            &mut h,
            "ui.open_workspace",
            json!({ "path": plain.path().display().to_string() }),
        )
        .unwrap_err();
        assert!(e.to_string().contains("not a workspace"), "{e}");

        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("demo");
        fauxfield_store::init_workspace(&ws).unwrap();
        let r = call(
            &view,
            cx,
            &mut h,
            "ui.open_workspace",
            json!({ "path": ws.display().to_string() }),
        )
        .unwrap();
        assert_eq!(r["workspace"], "demo");
        let s = state(&view, cx, &mut h);
        assert_eq!(s["view"], "workspace");
        assert_eq!(s["servers"][0]["name"], "Example API");
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

    #[test]
    fn size_presets_and_limits() {
        assert_eq!(parse_size("default"), Ok((1280.0, 800.0)));
        assert_eq!(parse_size("compact"), Ok((900.0, 600.0)));
        assert_eq!(parse_size("min"), Ok((900.0, 600.0)));
        assert_eq!(parse_size("1000x700"), Ok((1000.0, 700.0)));
        assert!(parse_size("100x100").unwrap_err().contains("minimum"));
        assert!(parse_size("wide").is_err());
    }

    #[gpui::test]
    fn set_theme_switches_without_persisting(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let layout = dir.path().join("layout.json");
        let (view, cx) = cx.add_window_view(|w, cx| {
            Shell::new(
                ShellOptions {
                    layout_path: Some(layout.clone()),
                    config_dir: None,
                    ..crate::view::test_options()
                },
                w,
                cx,
            )
        });
        cx.run_until_parked();
        let mut h = HostState::default();
        call(
            &view,
            cx,
            &mut h,
            "ui.set_theme",
            json!({ "name": "one-light" }),
        )
        .unwrap();
        assert_eq!(state(&view, cx, &mut h)["theme"], "One Light");
        call(
            &view,
            cx,
            &mut h,
            "ui.set_theme",
            json!({ "name": "One Dark" }),
        )
        .unwrap();
        assert_eq!(state(&view, cx, &mut h)["theme"], "One Dark");
        assert!(
            !layout.exists(),
            "ui.set_theme must not write the layout file"
        );
        let e = call(
            &view,
            cx,
            &mut h,
            "ui.set_theme",
            json!({ "name": "solarized" }),
        )
        .unwrap_err();
        assert!(e.to_string().contains("one-dark"), "{e}");
    }

    #[gpui::test]
    fn resize_changes_the_reported_window_size(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        call(&view, cx, &mut h, "ui.resize", json!({ "size": "compact" })).unwrap();
        cx.run_until_parked();
        // The test platform updates the window bounds but not the viewport size; the real
        // platform reports the new size in `ui.state` once its resize callback fires.
        let b = call(&view, cx, &mut h, "ui.window_bounds", json!({})).unwrap();
        assert_eq!(b["width"], 900.0);
        assert_eq!(b["height"], 600.0);
        let e = call(&view, cx, &mut h, "ui.resize", json!({ "size": "10x10" })).unwrap_err();
        assert!(e.to_string().contains("minimum"), "{e}");
    }

    #[gpui::test]
    fn reset_view_restores_the_baseline(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        call(
            &view,
            cx,
            &mut h,
            "ui.open_tab",
            json!({ "kind": "route", "id": "rt_users_list" }),
        )
        .unwrap();
        cx.simulate_keystrokes("secondary-shift-p");
        call(&view, cx, &mut h, "ui.reset_view", json!({})).unwrap();
        cx.run_until_parked();
        let s = state(&view, cx, &mut h);
        assert_eq!(s["tabs"].as_array().unwrap().len(), 0);
        assert_eq!(s["command_palette"]["open"], false);
        assert_eq!(s["docks"]["left"]["open"], true);
        assert_eq!(s["bottom_tab"], "traffic");
    }

    #[gpui::test]
    fn mask_flag_toggles(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        let mut h = HostState::default();
        call(&view, cx, &mut h, "ui.set_mask", json!({ "on": true })).unwrap();
        assert!(view.read_with(cx, |s, _| s.mask_volatile));
        call(&view, cx, &mut h, "ui.set_mask", json!({ "on": false })).unwrap();
        assert!(!view.read_with(cx, |s, _| s.mask_volatile));
    }
}
