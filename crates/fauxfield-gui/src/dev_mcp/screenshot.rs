//! `ui.screenshot`, `ui.screenshot_all`, `ui.resize` and the state-step runner (SPEC §11.4).
//! Runs in the tool layer over the `UiBridge`, so the UI thread never blocks on capture or waits.

use std::{sync::Arc, time::Duration};

use base64::Engine as _;
use fauxfield_mcp::{UiBridge, condition::Condition};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::capture::{self, Capture, WindowBounds};
use super::handler::parse_size;
use super::tools::{call, wait_for, wait_idle};

const IDLE_LIMIT: Duration = Duration::from_millis(5000);
/// Let the window come to the front and the compositor settle before grabbing.
const SETTLE: Duration = Duration::from_millis(300);

/// One step of a state: the same operations the input tools offer, plus `set_theme` and `resize`.
/// In YAML: `- key: cmd-shift-p`, `- open: {kind: route, id: rt_users_list}`, `- wait_idle`.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Step {
    /// Baseline: no picker, no tabs, default docks, Traffic tab, no selection.
    Reset,
    /// Open a workspace folder (`ui.open_workspace`); `reset` returns to the launch workspace.
    OpenWorkspace(String),
    /// Open a tab as a preview (`ui.open_tab`).
    Open {
        kind: String,
        id: String,
    },
    /// Run an action by name (`ui.invoke_action`).
    Invoke(String),
    /// Keystroke or chord (`ui.key`).
    Key(String),
    /// Type text into the focused input (`ui.type_text`).
    Type(String),
    /// Click an element by id (`ui.click_element`).
    ClickElement(String),
    /// A `ui.command` object, e.g. `{command: toggle_dock, dock: left}`.
    Command(Map<String, Value>),
    /// `one-dark` or `one-light`, not persisted.
    SetTheme(String),
    /// `default`, `compact`, `min` or `WxH`.
    Resize(String),
    /// Condition over `ui.state`.
    WaitFor {
        condition: Condition,
        #[serde(default)]
        timeout_ms: Option<u64>,
    },
    WaitIdle,
}

/// A named state from `fixtures/ui-states` or an inline list of steps.
#[derive(Clone, Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum StateRef {
    Named(String),
    Steps(Vec<Step>),
}

#[derive(Deserialize)]
struct Fixture {
    steps: Vec<Step>,
}

macro_rules! fixtures {
    ($($name:literal),* $(,)?) => {
        /// Named states, embedded so the app finds them from any working directory.
        pub const NAMED_STATES: &[(&str, &str)] = &[
            $(($name, include_str!(concat!("../../../../fixtures/ui-states/", $name, ".yaml")))),*
        ];
    };
}

fixtures!(
    "route-tab",
    "settings-appearance",
    "keymap-conflict",
    "response-body-editor",
    "server-tab",
    "environment-tab",
    "traffic-10k",
    "palette-open",
    "quick-search-open",
    "theme-selector-open",
    "docks-collapsed",
    "problems-open",
    "problems-empty",
    "empty-workspace",
    "no-tabs",
    "quick-search-empty",
    "palette-empty",
    "server-running",
    "traffic-live",
    "traffic-empty",
    "welcome-empty",
    "welcome-recent",
    "server-general",
    "server-disabled",
    "port-in-use-toast",
    "quit-confirm",
    "close-confirm",
    "delete-confirm",
    "workspace-menu-open",
    "rename-workspace-prompt",
    "menubar-closed",
    "menubar-file",
    "menubar-recent",
    "menubar-disabled",
    "menubar-collapsed",
    "about-box",
    "sidebar-servers",
    "sidebar-environments",
    "sidebar-datasets",
    "sidebar-servers-empty",
    "sidebar-environments-empty",
    "sidebar-datasets-empty",
    "sidebar-many-servers",
    "server-network",
    "server-proxy",
    "traffic-proxy",
    "route-callbacks",
    "network-profile-open",
);

pub fn named_steps(name: &str) -> Result<Vec<Step>, String> {
    let (_, text) = NAMED_STATES
        .iter()
        .find(|(n, _)| *n == name)
        .ok_or_else(|| {
            format!(
                "no state {name:?}; known states: {}",
                NAMED_STATES
                    .iter()
                    .map(|(n, _)| *n)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    // YAML goes through JSON values: serde_yaml_ng writes enums as `!tag`, not one-key maps.
    serde_yaml_ng::from_str::<Value>(text)
        .map_err(|e| e.to_string())
        .and_then(|v| serde_json::from_value::<Fixture>(v).map_err(|e| e.to_string()))
        .map(|f| f.steps)
        .map_err(|e| format!("state {name:?} is invalid: {e}"))
}

impl StateRef {
    pub fn steps(&self) -> Result<Vec<Step>, String> {
        match self {
            StateRef::Named(n) => named_steps(n),
            StateRef::Steps(s) => Ok(s.clone()),
        }
    }
}

/// Sets the window size and waits until `ui.state` reports it (resize is asynchronous on macOS).
pub async fn resize(bridge: &Arc<dyn UiBridge>, size: &str) -> Result<Value, String> {
    let (w, h) = parse_size(size)?;
    call(bridge, "ui.resize", json!({ "size": size })).await?;
    let start = std::time::Instant::now();
    loop {
        let s = call(bridge, "ui.state", json!({})).await?;
        let (cw, ch) = (
            s["window"]["width"].as_f64().unwrap_or(0.0) as f32,
            s["window"]["height"].as_f64().unwrap_or(0.0) as f32,
        );
        if (cw - w).abs() <= 1.0 && (ch - h).abs() <= 1.0 {
            wait_idle(bridge, IDLE_LIMIT).await?;
            return Ok(json!({ "width": cw, "height": ch }));
        }
        if start.elapsed() > Duration::from_millis(3000) {
            return Err(format!(
                "window did not reach {w}x{h} (it is {cw}x{ch}); the display may be too small"
            ));
        }
        tokio::time::sleep(Duration::from_millis(40)).await;
    }
}

pub(super) async fn run_step(bridge: &Arc<dyn UiBridge>, step: &Step) -> Result<(), String> {
    match step {
        Step::Reset => call(bridge, "ui.reset_view", json!({})).await.map(drop),
        Step::OpenWorkspace(path) => call(bridge, "ui.open_workspace", json!({ "path": path }))
            .await
            .map(drop),
        Step::Open { kind, id } => call(bridge, "ui.open_tab", json!({ "kind": kind, "id": id }))
            .await
            .map(drop),
        Step::Invoke(name) => call(bridge, "ui.invoke_action", json!({ "name": name }))
            .await
            .map(drop),
        Step::Key(keys) => call(bridge, "ui.key", json!({ "keys": keys }))
            .await
            .map(drop),
        Step::Type(text) => call(bridge, "ui.type_text", json!({ "text": text }))
            .await
            .map(drop),
        Step::ClickElement(id) => call(bridge, "ui.click_element", json!({ "id": id }))
            .await
            .map(drop),
        Step::Command(args) => call(bridge, "ui.command", Value::Object(args.clone()))
            .await
            .map(drop),
        Step::SetTheme(name) => call(bridge, "ui.set_theme", json!({ "name": name }))
            .await
            .map(drop),
        Step::Resize(size) => resize(bridge, size).await.map(drop),
        Step::WaitFor {
            condition,
            timeout_ms,
        } => wait_for(
            bridge,
            condition,
            Duration::from_millis(timeout_ms.unwrap_or(5000)),
        )
        .await
        .map(drop),
        Step::WaitIdle => wait_idle(bridge, IDLE_LIMIT).await.map(drop),
    }
}

/// Runs the steps in order; the first failure names its position.
pub async fn run_steps(bridge: &Arc<dyn UiBridge>, steps: &[Step]) -> Result<(), String> {
    for (i, step) in steps.iter().enumerate() {
        run_step(bridge, step)
            .await
            .map_err(|e| format!("step {} ({step:?}): {e}", i + 1))?;
    }
    Ok(())
}

#[derive(Deserialize, JsonSchema)]
pub struct ScreenshotInput {
    /// Crop to this element id (from ui.tree / ui.find). Default: the whole window.
    #[serde(default)]
    pub element: Option<String>,
    /// A named state (fixtures/ui-states) or a list of steps to run before capturing.
    #[serde(default)]
    pub state: Option<StateRef>,
    /// Also save the PNG to this path.
    #[serde(default)]
    pub out: Option<String>,
    /// `false` returns only ui.state (for hosts without Screen Recording permission).
    #[serde(default)]
    pub capture: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
pub struct ResizeToolInput {
    /// `default` (1280x800), `compact` (900x600), `min`, or `WxH`.
    pub size: String,
}

#[derive(Deserialize, JsonSchema)]
pub struct ScreenshotAllInput {
    /// Named states. Default: every state in fixtures/ui-states.
    #[serde(default)]
    pub states: Option<Vec<String>>,
    /// `one-dark` and/or `one-light`. Default: both.
    #[serde(default)]
    pub themes: Option<Vec<String>>,
    /// Sizes: `default`, `compact`, `min` or `WxH`. Default: `default`.
    #[serde(default)]
    pub sizes: Option<Vec<String>>,
    /// Directory to save `<state>_<theme>_<size>.png` files into.
    #[serde(default)]
    pub out_dir: Option<String>,
}

struct Shot {
    png: Vec<u8>,
    meta: Value,
}

/// Captures the current window (or an element of it) after waiting for idle.
async fn shoot(
    bridge: &Arc<dyn UiBridge>,
    cap: &Arc<dyn Capture>,
    element: Option<&str>,
) -> Result<Shot, String> {
    call(bridge, "ui.set_mask", json!({ "on": true })).await?;
    let result = shoot_masked(bridge, cap, element).await;
    // Masking is for the capture only.
    let _ = call(bridge, "ui.set_mask", json!({ "on": false })).await;
    result
}

async fn shoot_masked(
    bridge: &Arc<dyn UiBridge>,
    cap: &Arc<dyn Capture>,
    element: Option<&str>,
) -> Result<Shot, String> {
    wait_idle(bridge, IDLE_LIMIT).await?;
    // `ui.window_bounds` also brings the window to the front.
    let b = call(bridge, "ui.window_bounds", json!({})).await?;
    let bounds: WindowBounds = serde_json::from_value(b).map_err(|e| e.to_string())?;
    tokio::time::sleep(SETTLE).await;
    wait_idle(bridge, IDLE_LIMIT).await?;
    let rect = match element {
        Some(id) => Some(element_rect(bridge, id).await?),
        None => None,
    };
    let cap = cap.clone();
    let raw = tokio::task::spawn_blocking(move || cap.grab(bounds))
        .await
        .map_err(|e| e.to_string())??;
    let state = call(bridge, "ui.state", json!({})).await?;
    let (img, png) = tokio::task::spawn_blocking(move || {
        let img = capture::verify(&raw, &bounds)?;
        match rect {
            Some(r) => {
                let c = capture::crop(&img, r, bounds.scale)?;
                let png = capture::encode(&c)?;
                Ok::<_, String>((c, png))
            }
            None => Ok((img, raw)),
        }
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(Shot {
        meta: json!({
            "width": img.width,
            "height": img.height,
            "scale": bounds.scale,
            "theme": state["theme"],
            "bounds": bounds,
            "element": element,
            "state": state,
        }),
        png,
    })
}

async fn element_rect(bridge: &Arc<dyn UiBridge>, id: &str) -> Result<[f32; 4], String> {
    let t = call(bridge, "ui.tree", json!({ "root": id })).await?;
    let node = t["elements"]
        .as_array()
        .and_then(|a| a.iter().find(|e| e["id"] == id))
        .ok_or_else(|| format!("no element {id:?}"))?;
    if node["visible"] == false {
        return Err(format!("element {id:?} is not visible"));
    }
    let n = |k: &str| node["bounds"][k].as_f64().unwrap_or(0.0) as f32;
    Ok([n("x"), n("y"), n("w"), n("h")])
}

fn image_block(png: &[u8]) -> Value {
    json!({
        "data": base64::engine::general_purpose::STANDARD.encode(png),
        "mime": "image/png",
    })
}

pub async fn screenshot(
    bridge: &Arc<dyn UiBridge>,
    cap: Result<Arc<dyn Capture>, String>,
    i: ScreenshotInput,
) -> Result<Value, String> {
    if let Some(state) = &i.state {
        run_steps(bridge, &state.steps()?).await?;
    }
    if i.capture == Some(false) {
        wait_idle(bridge, IDLE_LIMIT).await?;
        let state = call(bridge, "ui.state", json!({})).await?;
        return Ok(json!({ "captured": false, "state": state }));
    }
    let cap = cap?;
    let tier = cap.tier();
    let shot = shoot(bridge, &cap, i.element.as_deref()).await?;
    let mut meta = shot.meta;
    meta["tier"] = json!(tier);
    if let Some(out) = &i.out {
        std::fs::write(out, &shot.png).map_err(|e| format!("could not write {out}: {e}"))?;
        meta["path"] = json!(out);
    }
    meta["_images"] = json!([image_block(&shot.png)]);
    Ok(meta)
}

fn theme_label(t: &str) -> String {
    t.trim().to_ascii_lowercase().replace(' ', "-")
}

pub async fn screenshot_all(
    bridge: &Arc<dyn UiBridge>,
    cap: Result<Arc<dyn Capture>, String>,
    i: ScreenshotAllInput,
) -> Result<Value, String> {
    let cap = cap?;
    let tier = cap.tier();
    let states: Vec<String> = i
        .states
        .unwrap_or_else(|| NAMED_STATES.iter().map(|(n, _)| (*n).to_owned()).collect());
    let themes = i
        .themes
        .unwrap_or_else(|| vec!["one-dark".into(), "one-light".into()]);
    let sizes = i.sizes.unwrap_or_else(|| vec!["default".into()]);
    if let Some(dir) = &i.out_dir {
        std::fs::create_dir_all(dir).map_err(|e| format!("could not create {dir}: {e}"))?;
    }

    let before = call(bridge, "ui.state", json!({})).await?;
    let mut cells = Vec::new();
    let mut images = Vec::new();
    for state in &states {
        for theme in &themes {
            for size in &sizes {
                let label = format!("{state}/{}/{size}", theme_label(theme));
                let cell = async {
                    call(bridge, "ui.set_theme", json!({ "name": theme })).await?;
                    resize(bridge, size).await?;
                    run_steps(bridge, &named_steps(state)?).await?;
                    shoot(bridge, &cap, None).await
                }
                .await;
                match cell {
                    Ok(shot) => {
                        let mut path = Value::Null;
                        if let Some(dir) = &i.out_dir {
                            let file = format!("{dir}/{}_{}_{size}.png", state, theme_label(theme));
                            std::fs::write(&file, &shot.png)
                                .map_err(|e| format!("could not write {file}: {e}"))?;
                            path = json!(file);
                        }
                        cells.push(json!({
                            "label": label, "ok": true, "image": images.len(), "path": path,
                            "width": shot.meta["width"], "height": shot.meta["height"],
                            "generation": shot.meta["state"]["generation"],
                        }));
                        images.push(image_block(&shot.png));
                    }
                    Err(e) => cells.push(json!({ "label": label, "ok": false, "error": e })),
                }
            }
        }
    }

    // Restore theme and size even when cells failed.
    let mut restore_errors = Vec::new();
    let theme = before["theme"].as_str().unwrap_or("One Dark").to_owned();
    if let Err(e) = call(bridge, "ui.set_theme", json!({ "name": theme })).await {
        restore_errors.push(format!("theme: {e}"));
    }
    let size = format!(
        "{}x{}",
        before["window"]["width"].as_f64().unwrap_or(1280.0).round(),
        before["window"]["height"].as_f64().unwrap_or(800.0).round()
    );
    if let Err(e) = resize(bridge, &size).await {
        restore_errors.push(format!("size: {e}"));
    }
    let failed = cells.iter().filter(|c| c["ok"] == false).count();
    Ok(json!({
        "tier": tier,
        "cells": cells,
        "failed": failed,
        "restored": restore_errors.is_empty(),
        "restore_errors": restore_errors,
        "_images": images,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_fixture_file_is_embedded_and_parses() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/ui-states");
        let mut on_disk: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .filter_map(|e| e.ok()?.path().file_stem()?.to_str().map(String::from))
            .collect();
        on_disk.sort();
        let mut embedded: Vec<String> = NAMED_STATES.iter().map(|(n, _)| (*n).to_owned()).collect();
        embedded.sort();
        assert_eq!(on_disk, embedded);
        for (n, _) in NAMED_STATES {
            let steps = named_steps(n).unwrap();
            assert!(
                matches!(steps.first(), Some(Step::Reset)),
                "{n} starts with reset"
            );
        }
    }

    #[test]
    fn unknown_state_lists_the_known_ones() {
        let e = named_steps("nope").unwrap_err();
        assert!(
            e.contains("route-tab") && e.contains("problems-open"),
            "{e}"
        );
    }

    #[test]
    fn steps_parse_from_json_and_yaml() {
        let s: Vec<Step> = serde_json::from_value(json!([
            "reset", "wait_idle", {"key": "cmd-k"}, {"open": {"kind": "route", "id": "x"}},
            {"command": {"command": "toggle_dock", "dock": "left"}}, {"set_theme": "one-light"},
        ]))
        .unwrap();
        assert_eq!(s.len(), 6);
        let r: StateRef = serde_json::from_value(json!("route-tab")).unwrap();
        assert!(matches!(r, StateRef::Named(_)));
        let r: StateRef = serde_json::from_value(json!(["reset"])).unwrap();
        assert!(matches!(r, StateRef::Steps(_)));
    }

    use fauxfield_mcp::{UiRequest, bridge::UiFuture};
    use std::sync::Mutex;

    /// Records requests and answers like an idle app whose window follows `ui.resize`.
    struct Fake {
        log: Mutex<Vec<(String, Value)>>,
        size: Mutex<(f32, f32)>,
    }

    impl UiBridge for Fake {
        fn request(&self, req: UiRequest) -> UiFuture {
            self.log
                .lock()
                .unwrap()
                .push((req.method.clone(), req.params.clone()));
            let out = match req.method.as_str() {
                "ui.resize" => {
                    let size = parse_size(req.params["size"].as_str().unwrap()).unwrap();
                    *self.size.lock().unwrap() = size;
                    json!({})
                }
                "ui.idle_probe" => json!({ "generation": 1, "pending": false }),
                "ui.state" => {
                    let (w, h) = *self.size.lock().unwrap();
                    json!({ "generation": 1, "theme": "One Light",
                            "window": { "width": w, "height": h, "scale": 2.0 } })
                }
                "ui.window_bounds" => {
                    json!({ "x": 0.0, "y": 0.0, "width": 1000.0, "height": 700.0, "scale": 2.0 })
                }
                _ => json!({}),
            };
            Box::pin(async move { Ok(out) })
        }
    }

    struct Failing;
    impl Capture for Failing {
        fn tier(&self) -> &'static str {
            "fake"
        }
        fn grab(&self, _: WindowBounds) -> Result<Vec<u8>, String> {
            Err("Screen Recording denied".into())
        }
    }

    fn fake() -> Arc<Fake> {
        Arc::new(Fake {
            log: Mutex::new(Vec::new()),
            size: Mutex::new((1000.0, 700.0)),
        })
    }

    #[tokio::test]
    async fn screenshot_all_restores_theme_and_size_when_cells_fail() {
        let f = fake();
        let bridge: Arc<dyn UiBridge> = f.clone();
        let out = screenshot_all(
            &bridge,
            Ok(Arc::new(Failing)),
            ScreenshotAllInput {
                states: Some(vec!["route-tab".into()]),
                themes: Some(vec!["one-dark".into()]),
                sizes: Some(vec!["compact".into()]),
                out_dir: None,
            },
        )
        .await
        .unwrap();
        assert_eq!(out["failed"], 1);
        assert_eq!(out["cells"][0]["label"], "route-tab/one-dark/compact");
        assert!(
            out["cells"][0]["error"]
                .as_str()
                .unwrap()
                .contains("Screen Recording")
        );
        assert_eq!(out["restored"], true);
        let log = f.log.lock().unwrap();
        let sets: Vec<_> = log.iter().filter(|(m, _)| m == "ui.set_theme").collect();
        assert_eq!(sets.last().unwrap().1["name"], "One Light");
        let resizes: Vec<_> = log.iter().filter(|(m, _)| m == "ui.resize").collect();
        assert_eq!(resizes.last().unwrap().1["size"], "1000x700");
        // The mask is switched off again after the failed capture.
        let masks: Vec<_> = log.iter().filter(|(m, _)| m == "ui.set_mask").collect();
        assert_eq!(masks.last().unwrap().1["on"], false);
    }

    #[tokio::test]
    async fn screenshot_runs_inline_steps_in_order_and_reports_capture_errors() {
        let f = fake();
        let bridge: Arc<dyn UiBridge> = f.clone();
        let err = screenshot(
            &bridge,
            Ok(Arc::new(Failing)),
            ScreenshotInput {
                element: None,
                state: Some(serde_json::from_value(json!(["reset", {"key": "cmd-k"}])).unwrap()),
                out: None,
                capture: None,
            },
        )
        .await
        .unwrap_err();
        assert!(err.contains("Screen Recording"), "{err}");
        let log = f.log.lock().unwrap();
        let methods: Vec<&str> = log.iter().map(|(m, _)| m.as_str()).collect();
        assert_eq!(&methods[..2], ["ui.reset_view", "ui.key"]);
    }

    #[tokio::test]
    async fn capture_false_returns_state_without_grabbing() {
        let bridge: Arc<dyn UiBridge> = fake();
        let out = screenshot(
            &bridge,
            Err("no tier".into()),
            ScreenshotInput {
                element: None,
                state: None,
                out: None,
                capture: Some(false),
            },
        )
        .await
        .unwrap();
        assert_eq!(out["captured"], false);
        assert_eq!(out["state"]["theme"], "One Light");
    }

    #[tokio::test]
    async fn resize_waits_for_the_reported_size() {
        let bridge: Arc<dyn UiBridge> = fake();
        let r = resize(&bridge, "compact").await.unwrap();
        assert_eq!(r["width"], 900.0);
    }
}
