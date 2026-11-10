//! Semantic UI tree (M1.4, SPEC §11.4). Views tag elements with `dev_id!`; a wrapper element
//! records each tagged element's bounds during prepaint into a registry owned by the app
//! (a GPUI global), rebuilt every frame. `ui.tree` and `ui.find` read it; `ui.click_element`
//! maps a node's `Click` target onto the same `Shell` methods the pointer handlers call.

use std::cell::RefCell;

use gpui::{
    AnyElement, App, Bounds, Element, ElementId, Global, GlobalElementId, InspectorElementId,
    IntoElement, LayoutId, Pixels, Window,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::layout::BottomTab;
use crate::tree::MenuAct;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Dock,
    Toolbar,
    Button,
    Tab,
    TabList,
    Input,
    List,
    Row,
    Modal,
    Menu,
    MenuItem,
    Table,
    Text,
    Toast,
    Panel,
}

/// What `ui.click_element` does for a node. Each variant calls an existing `Shell` method.
#[derive(Clone, Debug)]
pub enum Click {
    None,
    Action(&'static str),
    Row(String),
    ToggleRow(String),
    RowMenu(String),
    Tab(String),
    CloseTab(String),
    BottomTab(BottomTab),
    PickerHit(usize),
    SubTab(String, usize),
    Section(String, usize),
    Menu(MenuAct),
    /// Open a Recent workspace by path (Welcome list).
    Recent(String),
    /// Pick an entry of a ComboBox (combo key, entry value or action key).
    ComboItem(String, String),
    /// Remove a Recent entry by path (recent.json only).
    RemoveRecent(String),
    TrafficRow(usize),
    /// Select a response of a route in the Responses sub-tab.
    Response(String, usize),
    /// In-window menu bar: open top-level menu `n` (or the hamburger when collapsed).
    BarTitle(usize),
    /// In-window menu item: run action `id` through `Shell::run_menu_action`.
    BarAction(&'static str),
    /// In-window Open Recent entry (folder path).
    BarRecent(String),
    /// In-window submenu row: open it.
    BarSubmenu(usize),
    /// About box: copy the version info.
    AboutCopy,
    /// About box: close it.
    AboutClose,
    /// About box: open a link (URL).
    AboutLink(String),
    /// M5 controls without their own variant: `<op>:<ids…>`, run by `Shell::press`.
    Press(String),
}

/// Extra facts the audit (`ui.audit`) needs. Everything defaults to "nothing declared".
#[derive(Clone, Debug, Default)]
pub struct Meta {
    /// The element shows its full text in a tooltip.
    pub tooltip: bool,
    /// The label is deliberately truncated to fit.
    pub truncates: bool,
    /// Action id that gives a keyboard equivalent for a non-action control.
    pub keyboard: Option<&'static str>,
}

impl Click {
    pub fn interactive(&self) -> bool {
        !matches!(self, Click::None)
    }

    /// Controls whose click has a keyboard path by construction: registered actions, tab
    /// activation (Cmd+1..9, close tab), bottom-dock tabs (show_* actions), picker hits
    /// (arrows and Enter).
    fn keyboard_path(&self) -> bool {
        matches!(
            self,
            Click::Action(_)
                | Click::BarTitle(_)
                | Click::BarAction(_)
                | Click::BarRecent(_)
                | Click::BarSubmenu(_)
                | Click::AboutCopy
                | Click::AboutClose
                | Click::AboutLink(_)
                | Click::Tab(_)
                | Click::CloseTab(_)
                | Click::BottomTab(_)
                | Click::PickerHit(_)
        )
    }
}

#[derive(Clone, Debug)]
pub struct Spec {
    pub id: String,
    pub role: Role,
    pub label: String,
    pub enabled: bool,
    pub z: u8,
    pub click: Click,
    pub meta: Meta,
}

#[derive(Clone, Debug)]
pub struct Node {
    pub spec: Spec,
    pub parent: Option<usize>,
    pub bounds: [f32; 4],
    pub visible: bool,
    pub clipped: bool,
}

#[derive(Default)]
struct Inner {
    nodes: Vec<Node>,
    stack: Vec<usize>,
    frame: u64,
    focused: Option<String>,
    /// Totals of virtualised lists, keyed by node id.
    totals: Vec<(String, usize)>,
    perf: Perf,
}

/// Frame and input timing for UX150. Samples are in milliseconds, capped at `PERF_CAP`.
#[derive(Default)]
struct Perf {
    frame_start: Option<std::time::Instant>,
    last_paint: Option<std::time::Instant>,
    pending_key: Option<std::time::Instant>,
    frames_ms: Vec<f64>,
    key_to_paint_ms: Vec<f64>,
}

const PERF_CAP: usize = 240;

fn push_capped(v: &mut Vec<f64>, x: f64) {
    if v.len() == PERF_CAP {
        v.remove(0);
    }
    v.push(x);
}

#[derive(Default)]
pub struct Registry(RefCell<Inner>);
impl Global for Registry {}

impl Registry {
    /// Start a frame: called from `Shell::render`.
    pub fn begin_frame(cx: &mut App, focused: Option<&str>) {
        if !cx.has_global::<Registry>() {
            cx.set_global(Registry::default());
        }
        let mut r = cx.global::<Registry>().0.borrow_mut();
        r.nodes.clear();
        r.stack.clear();
        r.totals.clear();
        r.frame += 1;
        r.focused = focused.map(str::to_string);
        // Close the previous frame: start of render to its last tagged paint.
        let p = &mut r.perf;
        if let (Some(a), Some(b)) = (p.frame_start, p.last_paint)
            && b >= a
        {
            push_capped(&mut p.frames_ms, (b - a).as_secs_f64() * 1e3);
        }
        p.last_paint = None;
        p.frame_start = Some(std::time::Instant::now());
    }

    /// Stamp the moment a synthetic key was injected; the next paint resolves it.
    pub fn stamp_key(cx: &App) {
        if let Some(r) = cx.try_global::<Registry>() {
            r.0.borrow_mut().perf.pending_key = Some(std::time::Instant::now());
        }
    }

    fn note_paint(cx: &App) {
        if let Some(r) = cx.try_global::<Registry>() {
            let mut r = r.0.borrow_mut();
            let now = std::time::Instant::now();
            r.perf.last_paint = Some(now);
            if let Some(k) = r.perf.pending_key.take() {
                push_capped(&mut r.perf.key_to_paint_ms, (now - k).as_secs_f64() * 1e3);
            }
        }
    }

    /// Timing samples since the last clear; `clear` empties them after reading.
    pub fn perf(cx: &App, clear: bool) -> Value {
        let Some(r) = cx.try_global::<Registry>() else {
            return json!({ "frames_ms": [], "key_to_paint_ms": [] });
        };
        let mut r = r.0.borrow_mut();
        let out = json!({
            "frames_ms": r.perf.frames_ms,
            "key_to_paint_ms": r.perf.key_to_paint_ms,
        });
        if clear {
            r.perf.frames_ms.clear();
            r.perf.key_to_paint_ms.clear();
        }
        out
    }

    pub fn set_total(cx: &mut App, id: &str, total: usize) {
        if cx.has_global::<Registry>() {
            cx.global::<Registry>()
                .0
                .borrow_mut()
                .totals
                .push((id.to_string(), total));
        }
    }

    pub fn find_node(cx: &App, id: &str) -> Option<Node> {
        let r = cx.try_global::<Registry>()?.0.borrow();
        r.nodes.iter().find(|n| n.spec.id == id).cloned()
    }

    pub fn nodes(cx: &App) -> Vec<(usize, Node)> {
        cx.try_global::<Registry>()
            .map(|r| r.0.borrow().nodes.iter().cloned().enumerate().collect())
            .unwrap_or_default()
    }

    pub fn frame(cx: &App) -> u64 {
        cx.try_global::<Registry>()
            .map(|r| r.0.borrow().frame)
            .unwrap_or(0)
    }

    fn json_node(cx: &App, idx: usize, n: &Node) -> Value {
        let r = cx.global::<Registry>().0.borrow();
        let focused = r.focused.as_deref() == Some(n.spec.id.as_str());
        let mut v = json!({
            "id": n.spec.id,
            "role": n.spec.role,
            "label": n.spec.label,
            "enabled": n.spec.enabled,
            "visible": n.visible,
            "clipped": n.clipped,
            "focused": focused,
            "z": n.spec.z,
            "order": idx,
            "bounds": { "x": n.bounds[0], "y": n.bounds[1], "w": n.bounds[2], "h": n.bounds[3] },
            "meta": {
                "interactive": n.spec.click.interactive(),
                "action": matches!(n.spec.click, Click::Action(_)),
                "tooltip": n.spec.meta.tooltip,
                "truncates": n.spec.meta.truncates,
                "keyboard": n.spec.meta.keyboard
                    .map(str::to_string)
                    .or_else(|| n.spec.click.keyboard_path().then(|| "built-in".to_string())),
            },
        });
        if let Some((_, total)) = r.totals.iter().find(|(id, _)| *id == n.spec.id) {
            v["total"] = json!(total);
        }
        v
    }
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct TreeInput {
    /// Only this element and its descendants.
    #[serde(default)]
    pub root: Option<String>,
    /// Only elements with this role.
    #[serde(default)]
    pub role: Option<Role>,
    /// Drop elements that are not visible (off-window or clipped away).
    #[serde(default)]
    pub visible_only: bool,
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct FindInput {
    /// Case-insensitive substring of the element label.
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub role: Option<Role>,
    /// Case-insensitive substring of the id or label.
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub visible_only: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct ClickElementInput {
    /// Element id from `ui.tree` / `ui.find`.
    pub id: String,
}

fn descends(nodes: &[(usize, Node)], mut i: usize, root: usize) -> bool {
    loop {
        if i == root {
            return true;
        }
        match nodes[i].1.parent {
            Some(p) => i = p,
            None => return false,
        }
    }
}

pub fn tree(cx: &App, input: &TreeInput) -> Result<Value, String> {
    let nodes = Registry::nodes(cx);
    let root = match &input.root {
        Some(id) => Some(
            nodes
                .iter()
                .position(|(_, n)| n.spec.id == *id)
                .ok_or_else(|| format!("no element {id:?}"))?,
        ),
        None => None,
    };
    let out: Vec<Value> = nodes
        .iter()
        .filter(|(i, _)| root.is_none_or(|r| descends(&nodes, *i, r)))
        .filter(|(_, n)| input.role.is_none_or(|r| n.spec.role == r))
        .filter(|(_, n)| !input.visible_only || (n.visible && !n.clipped))
        .map(|(i, n)| {
            let mut v = Registry::json_node(cx, *i, n);
            v["parent"] = match n.parent {
                Some(p) => json!(nodes[p].1.spec.id),
                None => Value::Null,
            };
            v
        })
        .collect();
    Ok(json!({ "frame_nodes": out.len(), "elements": out }))
}

pub fn find(cx: &App, input: &FindInput) -> Value {
    let has = |hay: &str, needle: &Option<String>| {
        needle
            .as_ref()
            .is_none_or(|n| hay.to_lowercase().contains(&n.to_lowercase()))
    };
    let out: Vec<Value> = Registry::nodes(cx)
        .iter()
        .filter(|(_, n)| has(&n.spec.label, &input.label))
        .filter(|(_, n)| input.role.is_none_or(|r| n.spec.role == r))
        .filter(|(_, n)| has(&n.spec.label, &input.text) || has(&n.spec.id, &input.text))
        .filter(|(_, n)| !input.visible_only || (n.visible && !n.clipped))
        .map(|(i, n)| Registry::json_node(cx, *i, n))
        .collect();
    json!({ "matches": out })
}

// ---- the wrapper element ---------------------------------------------------------------

/// Wraps a child element and records its bounds. Layout and paint are the child's own.
pub struct Tagged {
    child: AnyElement,
    spec: Option<Spec>,
}

pub fn tag(child: impl IntoElement, spec: Spec) -> Tagged {
    Tagged {
        child: child.into_any_element(),
        spec: Some(spec),
    }
}

impl IntoElement for Tagged {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

fn round(v: f32) -> f32 {
    (v * 2.0).round() / 2.0
}

impl Element for Tagged {
    type RequestLayoutState = ();
    type PrepaintState = bool;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        let mask = window.content_mask().bounds;
        let window_bounds = Bounds {
            origin: gpui::point(Pixels::ZERO, Pixels::ZERO),
            size: window.viewport_size(),
        };
        let b = [
            round(f32::from(bounds.origin.x)),
            round(f32::from(bounds.origin.y)),
            round(f32::from(bounds.size.width)),
            round(f32::from(bounds.size.height)),
        ];
        let non_empty = b[2] > 0.0 && b[3] > 0.0;
        let in_window = bounds.intersects(&window_bounds);
        let in_mask = bounds.intersects(&mask);
        let fully_in_mask = mask.intersect(&bounds).size == bounds.size;
        let spec = self.spec.take().expect("prepaint runs once");
        let idx = {
            let mut r = cx.global::<Registry>().0.borrow_mut();
            let parent = r.stack.last().copied();
            r.nodes.push(Node {
                spec,
                parent,
                bounds: b,
                visible: non_empty && in_window && in_mask,
                clipped: non_empty && !fully_in_mask,
            });
            let idx = r.nodes.len() - 1;
            r.stack.push(idx);
            idx
        };
        let _ = idx;
        self.child.prepaint(window, cx);
        cx.global::<Registry>().0.borrow_mut().stack.pop();
        true
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut bool,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        Registry::note_paint(cx);
    }
}

// ---- ui.click_element -------------------------------------------------------------------

/// Triggers the node's click target through the same `Shell` methods the pointer handlers call.
pub fn click_element(
    id: &str,
    shell: &mut crate::shell::Shell,
    window: &mut Window,
    cx: &mut gpui::Context<crate::shell::Shell>,
) -> Result<Value, fauxfield_mcp::UiError> {
    use crate::shell::OpenMode;
    let fail = |m: String| Err(fauxfield_mcp::UiError::Failed(m));
    let Some(node) = Registry::find_node(cx, id) else {
        return fail(format!("no element {id:?}; see ui.find"));
    };
    if !node.spec.enabled {
        return fail(format!("element {id:?} is disabled"));
    }
    if !node.visible {
        return fail(format!("element {id:?} is not visible"));
    }
    match node.spec.click.clone() {
        Click::None => return fail(format!("element {id:?} has no click action")),
        Click::Action(name) => {
            super::input::invoke_action(name, shell, window, cx)?;
        }
        Click::Row(row) => shell.click_row(&row, 1, cx),
        Click::ToggleRow(row) => {
            shell.toggle_row(&row);
            cx.notify();
        }
        Click::RowMenu(row) => {
            let [x, y, w, h] = node.bounds;
            shell.open_menu(&row, x + w / 2.0, y + h / 2.0, cx);
        }
        Click::Tab(tab) => match shell.tabs.tabs().iter().position(|t| t.id == tab) {
            Some(i) => shell.activate_tab(i, cx),
            None => return fail(format!("tab {tab:?} is gone")),
        },
        Click::CloseTab(tab) => match shell.tabs.tabs().iter().position(|t| t.id == tab) {
            Some(i) => shell.close_tab_at(i, cx),
            None => return fail(format!("tab {tab:?} is gone")),
        },
        Click::BottomTab(t) => shell.show_bottom(t, cx),
        Click::PickerHit(i) => shell.picker_select(i, OpenMode::Preview, window, cx),
        Click::SubTab(route, i) => {
            shell.route_sub.insert(route, i);
            cx.notify();
        }
        Click::Section(server, i) => {
            shell.server_section.insert(server, i);
            cx.notify();
        }
        Click::Menu(act) => shell.run_menu(act, window, cx),
        Click::ComboItem(key, value) => {
            if !shell.combo_pick(&key, &value, window, cx) {
                return fail(format!("combo box {key:?} has no enabled entry {value:?}"));
            }
        }
        Click::BarTitle(i) => shell.bar_toggle(i, cx),
        Click::BarAction(id) => {
            if let Err(e) = shell.run_menu_action(id, window, cx) {
                return fail(e);
            }
        }
        Click::BarRecent(path) => {
            shell.bar_close(cx);
            window.dispatch_action(Box::new(crate::actions::OpenRecent { path }), cx);
        }
        Click::BarSubmenu(i) => shell.bar_open_sub(i, cx),
        Click::AboutCopy => shell.copy_version_info(cx),
        Click::AboutClose => shell.close_about(cx),
        Click::AboutLink(url) => shell.open_link(&url, cx),
        Click::Press(op) => {
            if let Err(e) = shell.press(&op, node.bounds, window, cx) {
                return fail(e);
            }
        }
        Click::Recent(path) => shell.open_recent(std::path::Path::new(&path), cx),
        Click::RemoveRecent(path) => shell.remove_recent(std::path::Path::new(&path), cx),
        Click::Response(route, i) => {
            shell.resp_sel.insert(route, i);
            cx.notify();
        }
        Click::TrafficRow(i) => {
            shell.traffic_selected = Some(i);
            shell.layout.right_open = true;
            cx.notify();
        }
    }
    Ok(json!({ "ok": true, "clicked": id }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev_mcp::handler::{HostState, dispatch};
    use crate::shell::{Shell, ShellOptions};
    use fauxfield_mcp::{UiError, UiRequest};
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
        method: &str,
        params: Value,
    ) -> Result<Value, UiError> {
        let mut host = HostState::default();
        let req = UiRequest {
            method: method.into(),
            params,
        };
        let r = view.update_in(cx, |s, w, cx| dispatch(&req, &mut host, s, w, cx));
        cx.run_until_parked();
        cx.update(|w, app| {
            let _ = w.draw(app);
        });
        r
    }

    fn ids(v: &Value, key: &str) -> Vec<String> {
        v[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| e["id"].as_str().unwrap().to_string())
            .collect()
    }

    fn click(view: &Entity<Shell>, cx: &mut VisualTestContext, id: &str) -> Result<Value, UiError> {
        call(view, cx, "ui.click_element", json!({ "id": id }))
    }

    fn state(view: &Entity<Shell>, cx: &mut VisualTestContext) -> Value {
        call(view, cx, "ui.state", json!({})).unwrap()
    }

    #[gpui::test]
    fn tree_is_deterministic(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        call(&view, cx, "ui.state", json!({})).unwrap();
        let a = call(&view, cx, "ui.tree", json!({})).unwrap();
        let b = call(&view, cx, "ui.tree", json!({})).unwrap();
        assert_eq!(a, b, "same state, same tree");
        assert!(a["elements"].as_array().unwrap().len() > 20, "{a}");
        insta::assert_json_snapshot!("tree_default", a);
    }

    #[gpui::test]
    fn find_and_click_the_m1_controls(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        call(&view, cx, "ui.state", json!({})).unwrap();

        for (id, key) in [
            ("status-left-dock", "left"),
            ("status-right-dock", "right"),
            ("status-bottom-dock", "bottom"),
        ] {
            let before = state(&view, cx)["docks"][key]["open"].clone();
            let f = call(&view, cx, "ui.find", json!({ "text": id })).unwrap();
            assert_eq!(ids(&f, "matches"), vec![id.to_string()]);
            click(&view, cx, id).unwrap();
            assert_ne!(state(&view, cx)["docks"][key]["open"], before, "{id}");
        }
        // Restore the default layout: left open, right closed, bottom open.
        click(&view, cx, "status-left-dock").unwrap();
        click(&view, cx, "status-right-dock").unwrap();
        click(&view, cx, "status-bottom-dock").unwrap();

        let t = call(
            &view,
            cx,
            "ui.find",
            json!({ "label": "Traffic", "role": "table" }),
        )
        .unwrap();
        assert_eq!(t["matches"][0]["total"], crate::shell::TRAFFIC_ROWS);
        let rows = call(&view, cx, "ui.find", json!({ "text": "traffic-row-" })).unwrap();
        let n = rows["matches"].as_array().unwrap().len();
        assert!(n > 0 && n < crate::shell::TRAFFIC_ROWS, "virtualised: {n}");

        click(&view, cx, "bottom-tab-Problems").unwrap();
        assert_eq!(state(&view, cx)["bottom_tab"], "problems");
        click(&view, cx, "bottom-tab-Traffic").unwrap();

        click(&view, cx, "row-rt_health").unwrap();
        assert_eq!(state(&view, cx)["tabs"][0]["preview"], true);
        let tabs = call(&view, cx, "ui.find", json!({ "role": "tab" })).unwrap();
        assert!(ids(&tabs, "matches").contains(&"tab-rt_health".to_string()));
        click(&view, cx, "tab-close-rt_health").unwrap();
        assert_eq!(state(&view, cx)["tabs"], json!([]));

        click(&view, cx, "title-search").unwrap();
        let m = call(&view, cx, "ui.find", json!({ "role": "modal" })).unwrap();
        assert_eq!(ids(&m, "matches"), vec!["quick-search"]);
        assert_eq!(m["matches"][0]["z"], 1);
        let inp = call(&view, cx, "ui.find", json!({ "role": "input" })).unwrap();
        assert_eq!(inp["matches"][0]["focused"], true);
        let hits = call(&view, cx, "ui.find", json!({ "text": "hit-" })).unwrap();
        let first = hits["matches"][0]["id"].as_str().unwrap().to_string();
        click(&view, cx, &first).unwrap();
        assert_eq!(state(&view, cx)["quick_search"]["open"], false);

        call(
            &view,
            cx,
            "ui.invoke_action",
            json!({ "name": "command_palette" }),
        )
        .unwrap();
        let m = call(&view, cx, "ui.find", json!({ "role": "modal" })).unwrap();
        assert_eq!(ids(&m, "matches"), vec!["command-palette"]);
        cx.simulate_keystrokes("escape");
        call(
            &view,
            cx,
            "ui.invoke_action",
            json!({ "name": "select_theme" }),
        )
        .unwrap();
        let m = call(&view, cx, "ui.find", json!({ "role": "modal" })).unwrap();
        assert_eq!(ids(&m, "matches"), vec!["theme-selector"]);
    }

    #[gpui::test]
    fn errors_and_filters(cx: &mut TestAppContext) {
        let (view, cx) = setup(cx);
        call(&view, cx, "ui.state", json!({})).unwrap();
        let e = click(&view, cx, "nope").unwrap_err();
        assert!(e.to_string().contains("no element"), "{e}");
        let e = click(&view, cx, "title-bar").unwrap_err();
        assert!(e.to_string().contains("no click action"), "{e}");
        let e = call(&view, cx, "ui.tree", json!({ "root": "nope" })).unwrap_err();
        assert!(e.to_string().contains("no element"), "{e}");

        let sub = call(&view, cx, "ui.tree", json!({ "root": "status-bar" })).unwrap();
        let sub_ids = ids(&sub, "elements");
        assert!(sub_ids.contains(&"status-left-dock".to_string()));
        assert!(!sub_ids.contains(&"title-search".to_string()));
        let buttons = call(&view, cx, "ui.tree", json!({ "role": "button" })).unwrap();
        assert!(
            buttons["elements"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["role"] == "button")
        );

        let all = call(&view, cx, "ui.tree", json!({})).unwrap();
        let mut seen = std::collections::HashSet::new();
        for id in ids(&all, "elements") {
            assert!(seen.insert(id.clone()), "duplicate id {id}");
        }
    }
}
