//! `ui.audit` and `ui.walkthrough` (SPEC §11.4). The rules are pure functions in
//! `fauxfield-mcp::audit`; this module gathers their inputs from the running shell over the
//! `UiBridge` (tree, theme token pairs, actions, views, sources, timing) and runs sweeps.

use std::{path::Path, sync::Arc, time::Instant};

use fauxfield_mcp::{
    UiBridge,
    audit::{self, AuditData, AuditNode, Finding, Rule, Severity},
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use super::screenshot::{NAMED_STATES, Step, named_steps, resize, run_step, run_steps};
use super::tools::{call, wait_idle};

/// SPEC §9.8 defaults in the keymap's `secondary-` notation (UX130).
pub const SPEC_BINDINGS: &[(&str, &str)] = &[
    ("quick_search", "secondary-k"),
    ("command_palette", "secondary-shift-p"),
    ("new_route", "secondary-n"),
    ("new_server", "secondary-shift-n"),
    ("toggle_server", "secondary-r"),
    ("run_all", "secondary-shift-r"),
    ("send_test", "secondary-enter"),
    ("save", "secondary-s"),
    ("toggle_left_dock", "secondary-b"),
    ("toggle_bottom_dock", "secondary-`"),
    ("toggle_right_dock", "secondary-alt-b"),
    ("show_sidebar_servers", "secondary-alt-1"),
    ("show_sidebar_environments", "secondary-alt-2"),
    ("show_sidebar_datasets", "secondary-alt-3"),
    ("close_tab", "secondary-w"),
    ("select_theme", "secondary-k secondary-t"),
    ("switch_environment", "secondary-e"),
    ("duplicate", "secondary-d"),
    ("reset_state", "secondary-shift-backspace"),
];

/// Data views and the empty/loading/error states they implement, each with the fixture that
/// shows it (UX140). A view with no states is a finding; a state without its fixture is too.
pub const VIEWS: &[(&str, &[(&str, &str)])] = &[
    ("Workspace pane", &[("empty", "no-tabs")]),
    ("Quick Search", &[("empty", "quick-search-empty")]),
    ("Command Palette", &[("empty", "palette-empty")]),
    ("Problems", &[("empty", "problems-empty")]),
    ("Traffic", &[("empty", "traffic-empty")]),
    ("Logs", &[("empty", "logs-empty")]),
    ("Workspace tree", &[("empty", "empty-workspace")]),
];

fn rgba(h: gpui::Hsla) -> [u8; 4] {
    let c: gpui::Rgba = h.into();
    let b = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    [b(c.r), b(c.g), b(c.b), b(c.a)]
}

fn px8(c: crate::theme_model::Rgba8) -> [u8; 4] {
    [c.r, c.g, c.b, c.a]
}

/// Token pairs checked by UX110, for every bundled theme. Body text (text, muted) needs 4.5:1;
/// accent, status and method colours are badges, dots and chrome accents, so 3:1.
fn pairs() -> Vec<Value> {
    let mut out = Vec::new();
    for t in crate::theme::bundled() {
        let theme = crate::theme::Theme::from_tokens(t);
        let backgrounds = [
            ("background", t.background),
            ("surface", t.surface),
            ("panel", t.panel),
            ("elevated", t.elevated),
            ("editor", t.editor),
            ("tab_active", t.tab_active),
            ("tab_inactive", t.tab_inactive),
            ("status_bar", t.status_bar),
            ("title_bar", t.title_bar),
        ];
        let texts = [
            ("text", t.text, "text"),
            ("text_muted", t.text_muted, "text"),
            ("accent", t.text_accent, "ui"),
            ("text_placeholder", t.text_placeholder, "ui"),
        ];
        let mut push = |fg_name: &str, fg: [u8; 4], bg_name: &str, bg: [u8; 4], kind: &str| {
            out.push(json!({
                "theme": t.name, "fg_name": fg_name, "bg_name": bg_name,
                "fg": fg, "bg": bg, "kind": kind,
            }));
        };
        for (bn, bg) in backgrounds {
            for (fnm, fg, kind) in texts {
                // Muted and placeholder text only appears on content surfaces.
                if matches!(fnm, "text_placeholder")
                    && !matches!(bn, "panel" | "editor" | "elevated")
                {
                    continue;
                }
                push(fnm, px8(fg), bn, px8(bg), kind);
            }
        }
        for m in ["GET", "POST", "PUT", "PATCH", "DELETE"] {
            push(
                &format!("method {m}"),
                rgba(theme.method_color(m)),
                "panel",
                px8(t.panel),
                "ui",
            );
        }
        for (name, fg) in [
            ("created", t.created),
            ("modified", t.modified),
            ("deleted", t.deleted),
        ] {
            push(name, px8(fg), "panel", px8(t.panel), "ui");
        }
    }
    out
}

fn collect_sources(dir: &Path, root: &Path, out: &mut Vec<Value>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            collect_sources(&p, root, out);
        } else if p.extension().is_some_and(|e| e == "rs")
            && let Ok(text) = std::fs::read_to_string(&p)
        {
            let rel = p.strip_prefix(root).unwrap_or(&p).display().to_string();
            // Test modules may carry colour literals as fixtures; the rule is for views.
            let text = match text.find("#[cfg(test)]") {
                Some(i) => text[..i].to_string(),
                None => text,
            };
            out.push(json!({ "path": rel, "text": text }));
        }
    }
}

/// The non-tree inputs of the audit, gathered on the UI thread (`ui.audit_data`).
pub fn data(shell: &crate::shell::Shell, _cx: &gpui::App) -> Value {
    let palette: Vec<String> = crate::actions::commands(&shell.keymap)
        .into_iter()
        .map(|c| c.id)
        .collect();
    let canon = |s: &str| crate::actions::canonical(s).unwrap_or_else(|| s.to_string());
    let actions: Vec<Value> = crate::actions::ACTIONS
        .iter()
        .map(|a| {
            let canon_chord = |s: &str| s.split(' ').map(canon).collect::<Vec<_>>().join(" ");
            let binding = shell.keymap.binding_for(a.id).map(canon_chord);
            let expected = SPEC_BINDINGS
                .iter()
                .find(|(id, _)| *id == a.id)
                .map(|(_, k)| canon_chord(k));
            json!({
                "id": a.id,
                "binding": binding,
                "expected_binding": expected,
                "in_palette": palette.iter().any(|p| p == a.id),
            })
        })
        .collect();
    let views: Vec<Value> = VIEWS
        .iter()
        .map(|(name, states)| {
            json!({
                "name": name,
                "states": states.iter().map(|(s, f)| json!({"state": s, "fixture": f})).collect::<Vec<_>>(),
            })
        })
        .collect();
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let sources = if root.is_dir() {
        let mut v = Vec::new();
        collect_sources(&root, &root, &mut v);
        Value::Array(v)
    } else {
        Value::Null
    };
    json!({
        "pairs": pairs(),
        "actions": actions,
        "views": views,
        "fixtures": NAMED_STATES.iter().map(|(n, _)| *n).collect::<Vec<_>>(),
        "sources": sources,
    })
}

#[derive(Deserialize, JsonSchema, Default)]
pub struct AuditInput {
    /// Rule ids to run (`UX101`…`UX150`). Default: all except UX150.
    #[serde(default)]
    pub rules: Option<Vec<String>>,
    /// Only this element and its descendants (an id from ui.tree).
    #[serde(default)]
    pub subtree: Option<String>,
    /// Audit under this theme (`one-dark` or `one-light`); restored afterwards.
    #[serde(default)]
    pub theme: Option<String>,
    /// Audit at this size (`default`, `compact`, `min`, `WxH`); restored afterwards.
    #[serde(default)]
    pub size: Option<String>,
    /// Sweep these themes (overrides `theme`).
    #[serde(default)]
    pub themes: Option<Vec<String>>,
    /// Sweep these sizes (overrides `size`).
    #[serde(default)]
    pub sizes: Option<Vec<String>>,
    /// Sweep these named states (`["all"]` for every state fixture). Without it a sweep audits
    /// whatever is on screen.
    #[serde(default)]
    pub states: Option<Vec<String>>,
    /// Measure frame time and key-to-paint latency (also on when UX150 is in `rules`).
    #[serde(default)]
    pub perf: bool,
}

fn theme_label(t: &str) -> String {
    t.trim().to_ascii_lowercase().replace(' ', "-")
}

async fn frame_findings(
    bridge: &Arc<dyn UiBridge>,
    rules: &[Rule],
    subtree: &Option<String>,
) -> Result<Vec<Finding>, String> {
    let mut params = json!({});
    if let Some(r) = subtree {
        params["root"] = json!(r);
    }
    let tree = call(bridge, "ui.tree", params).await?;
    let nodes: Vec<AuditNode> = serde_json::from_value(tree["elements"].clone())
        .map_err(|e| format!("ui.tree output not understood: {e}"))?;
    Ok(audit::run_frame(rules, &nodes))
}

/// Drives enough typing and navigation to produce frame and key-to-paint samples.
async fn measure_perf(bridge: &Arc<dyn UiBridge>) -> Result<Value, String> {
    call(bridge, "ui.perf_probe", json!({ "clear": true })).await?;
    let mut steps = named_steps("traffic-10k")?;
    steps.extend((0..20).map(|_| Step::Key("pagedown".into())));
    steps.push(Step::Invoke("command_palette".into()));
    steps.extend("theme".chars().map(|c| Step::Key(c.to_string())));
    steps.push(Step::Key("escape".into()));
    steps.push(Step::WaitIdle);
    run_steps(bridge, &steps).await?;
    call(bridge, "ui.perf_probe", json!({ "clear": true })).await
}

fn summarize(findings: &[Finding]) -> Value {
    let errors = findings
        .iter()
        .filter(|f| f.severity == Severity::Error)
        .count();
    json!({ "errors": errors, "warnings": findings.len() - errors })
}

pub async fn audit(bridge: &Arc<dyn UiBridge>, i: AuditInput) -> Result<Value, String> {
    let started = Instant::now();
    let want_perf = i.perf
        || i.rules
            .as_ref()
            .is_some_and(|r| r.iter().any(|x| x.eq_ignore_ascii_case("UX150")));
    let rules: Vec<Rule> = match &i.rules {
        Some(list) => list
            .iter()
            .map(|s| Rule::parse(s).ok_or_else(|| format!("unknown rule {s:?}")))
            .collect::<Result<_, _>>()?,
        None => Rule::ALL
            .into_iter()
            .filter(|r| *r != Rule::Ux150 || want_perf)
            .collect(),
    };
    let frame_rules: Vec<Rule> = rules.iter().copied().filter(|r| r.per_frame()).collect();
    let global_rules: Vec<Rule> = rules.iter().copied().filter(|r| !r.per_frame()).collect();

    let mut payload = if global_rules.is_empty() {
        AuditData::default()
    } else {
        serde_json::from_value(call(bridge, "ui.audit_data", json!({})).await?)
            .map_err(|e| format!("ui.audit_data output not understood: {e}"))?
    };
    if want_perf {
        let p = measure_perf(bridge).await?;
        payload.perf = serde_json::from_value(p).ok();
    }
    let global = audit::run_global(&global_rules, &payload);

    let sweep = i.themes.is_some() || i.sizes.is_some() || i.states.is_some();
    let themes: Vec<Option<String>> = match (&i.themes, &i.theme) {
        (Some(t), _) => t.iter().cloned().map(Some).collect(),
        (None, Some(t)) => vec![Some(t.clone())],
        _ => vec![None],
    };
    let sizes: Vec<Option<String>> = match (&i.sizes, &i.size) {
        (Some(s), _) => s.iter().cloned().map(Some).collect(),
        (None, Some(s)) => vec![Some(s.clone())],
        _ => vec![None],
    };
    let states: Vec<Option<String>> = match &i.states {
        Some(s) if s.iter().any(|n| n == "all") => NAMED_STATES
            .iter()
            .map(|(n, _)| Some((*n).to_string()))
            .collect(),
        Some(s) => s.iter().cloned().map(Some).collect(),
        None => vec![None],
    };

    let before = if themes.iter().chain(&sizes).any(Option::is_some) {
        Some(call(bridge, "ui.state", json!({})).await?)
    } else {
        None
    };

    let mut cells = Vec::new();
    let mut all = global.clone();
    'sweep: for theme in &themes {
        for size in &sizes {
            for state in &states {
                let label = format!(
                    "{}/{}/{}",
                    state.as_deref().unwrap_or("current"),
                    theme.as_deref().map_or("current".into(), theme_label),
                    size.as_deref().unwrap_or("current"),
                );
                let cell = async {
                    if let Some(t) = theme {
                        call(bridge, "ui.set_theme", json!({ "name": t })).await?;
                    }
                    if let Some(s) = size {
                        resize(bridge, s).await?;
                    }
                    if let Some(n) = state {
                        run_steps(bridge, &named_steps(n)?).await?;
                    }
                    wait_idle(bridge, std::time::Duration::from_millis(5000)).await?;
                    frame_findings(bridge, &frame_rules, &i.subtree).await
                }
                .await;
                match cell {
                    Ok(f) => {
                        all.extend(f.iter().cloned().map(|mut x| {
                            x.message = format!("[{label}] {}", x.message);
                            x
                        }));
                        cells.push(json!({ "label": label, "ok": true, "summary": summarize(&f), "findings": f }));
                    }
                    Err(e) => cells.push(json!({ "label": label, "ok": false, "error": e })),
                }
                if !sweep {
                    break 'sweep;
                }
            }
        }
    }

    // Restore theme and size even when cells failed.
    let mut restore_errors = Vec::new();
    if let Some(b) = before {
        let theme = b["theme"].as_str().unwrap_or("One Dark").to_owned();
        if let Err(e) = call(bridge, "ui.set_theme", json!({ "name": theme })).await {
            restore_errors.push(format!("theme: {e}"));
        }
        let size = format!(
            "{}x{}",
            b["window"]["width"].as_f64().unwrap_or(1280.0).round(),
            b["window"]["height"].as_f64().unwrap_or(800.0).round()
        );
        if let Err(e) = resize(bridge, &size).await {
            restore_errors.push(format!("size: {e}"));
        }
    }

    let failed = cells.iter().filter(|c| c["ok"] == false).count();
    let mut sum = summarize(&all);
    sum["rules"] = json!(rules.iter().map(|r| r.id()).collect::<Vec<_>>());
    sum["cells"] = json!(cells.len());
    sum["failed_cells"] = json!(failed);
    sum["sources_scanned"] = json!(payload.sources.is_some() || !rules.contains(&Rule::Ux120));
    Ok(json!({
        "summary": sum,
        "global": global,
        "cells": cells,
        "restored": restore_errors.is_empty(),
        "restore_errors": restore_errors,
        "elapsed_ms": started.elapsed().as_millis() as u64,
    }))
}

#[derive(Deserialize, JsonSchema)]
pub struct WalkthroughInput {
    /// Steps as in `ui.screenshot`'s `state` list, plus `click_element: <id>`.
    pub steps: Vec<Step>,
}

fn step_cost(step: &Step) -> (u32, u32) {
    // (keystrokes, clicks)
    match step {
        Step::Key(k) => (k.split_whitespace().count() as u32, 0),
        Step::Type(t) => (t.chars().count() as u32, 0),
        Step::Open { .. } | Step::ClickElement(_) | Step::Command(_) => (0, 1),
        _ => (0, 0),
    }
}

fn step_name(step: &Step) -> String {
    format!("{step:?}")
        .split(['(', ' ', '{'])
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

pub async fn walkthrough(bridge: &Arc<dyn UiBridge>, i: WalkthroughInput) -> Result<Value, String> {
    let started = Instant::now();
    let (mut keys, mut clicks) = (0u32, 0u32);
    let mut toasts: Vec<String> = Vec::new();
    let mut steps = Vec::new();
    let mut failed = None;
    for (n, step) in i.steps.iter().enumerate() {
        let t0 = Instant::now();
        let r = run_step(bridge, step).await;
        let (k, c) = step_cost(step);
        keys += k;
        clicks += c;
        if let Ok(state) = call(bridge, "ui.state", json!({})).await
            && let Some(list) = state["toasts"].as_array()
        {
            for t in list {
                let text = t.as_str().map_or_else(|| t.to_string(), str::to_string);
                if !toasts.contains(&text) {
                    toasts.push(text);
                }
            }
        }
        let ok = r.is_ok();
        steps.push(json!({
            "index": n + 1, "step": step_name(step), "ok": ok,
            "error": r.as_ref().err(), "ms": t0.elapsed().as_millis() as u64,
        }));
        if let Err(e) = r {
            failed = Some(format!("step {} failed: {e}", n + 1));
            break;
        }
    }
    Ok(json!({
        "ok": failed.is_none(),
        "error": failed,
        "steps": steps,
        "totals": {
            "steps": steps.len(),
            "keystrokes": keys,
            "clicks": clicks,
            "wall_ms": started.elapsed().as_millis() as u64,
            "toasts": toasts,
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dev_mcp::tree::{Registry, TreeInput, tree};
    use crate::shell::{Shell, ShellOptions};
    use gpui::{
        Context, IntoElement, ParentElement, Render, Styled, TestAppContext, Window, div, px,
    };

    /// A view with one defect per per-frame rule, plus clean controls.
    struct Seeded;
    impl Render for Seeded {
        fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
            Registry::begin_frame(cx, None);
            div()
                .size_full()
                .flex()
                .flex_col()
                .child(crate::dev_id!(
                    div()
                        .w(px(30.))
                        .h(px(24.))
                        .overflow_hidden()
                        .child(crate::dev_id!(
                            div().w(px(120.)).h(px(24.)),
                            "seed-clipped",
                            Text,
                            "A long label",
                            None
                        )),
                    "seed-clip-box",
                    Panel,
                    "box",
                    None
                ))
                .child(crate::dev_id!(
                    div()
                        .relative()
                        .w(px(200.))
                        .h(px(100.))
                        .child(crate::dev_id!(
                            div()
                                .absolute()
                                .top(px(0.))
                                .left(px(0.))
                                .w(px(60.))
                                .h(px(60.)),
                            "seed-overlap-a",
                            Panel,
                            "a",
                            None
                        ))
                        .child(crate::dev_id!(
                            div()
                                .absolute()
                                .top(px(30.))
                                .left(px(30.))
                                .w(px(60.))
                                .h(px(60.)),
                            "seed-overlap-b",
                            Panel,
                            "b",
                            None
                        )),
                    "seed-overlap-parent",
                    Panel,
                    "parent",
                    None
                ))
                .child(crate::dev_id!(
                    div().w(px(10.)).h(px(10.)),
                    "seed-tiny",
                    Button,
                    "Tiny",
                    Action("quit")
                ))
                .child(crate::dev_id!(
                    div().w(px(60.)).h(px(24.)),
                    "seed-clean",
                    Button,
                    "Clean",
                    Action("quit")
                ))
        }
    }

    fn findings(cx: &mut gpui::VisualTestContext) -> Vec<Finding> {
        cx.update(|w, app| {
            let _ = w.draw(app);
        });
        let t = cx.update(|_, app| tree(app, &TreeInput::default()).unwrap());
        let nodes: Vec<AuditNode> = serde_json::from_value(t["elements"].clone()).unwrap();
        audit::run_frame(&Rule::ALL, &nodes)
    }

    #[gpui::test]
    fn seeded_defects_are_reported_and_clean_controls_are_not(cx: &mut TestAppContext) {
        let (_, cx) = cx.add_window_view(|_, _| Seeded);
        let f = findings(cx);
        let has = |rule: &str, id: &str| {
            f.iter()
                .any(|x| x.rule == rule && x.elements.iter().any(|e| e == id))
        };
        assert!(has("UX101", "seed-clipped"), "{f:#?}");
        assert!(has("UX102", "seed-overlap-a"), "{f:#?}");
        assert!(has("UX103", "seed-tiny"), "{f:#?}");
        assert!(
            f.iter().all(|x| !x.elements.contains(&"seed-clean".into())),
            "{f:#?}"
        );
    }

    fn shell_data(cx: &mut TestAppContext) -> AuditData {
        let (view, cx) = cx.add_window_view(|w, cx| {
            Shell::new(
                ShellOptions {
                    layout_path: None,
                    config_dir: None,
                    settings_path: None,
                    ..crate::view::test_options()
                },
                w,
                cx,
            )
        });
        cx.run_until_parked();
        let v = view.update_in(cx, |s, _, cx| data(s, cx));
        serde_json::from_value(v).expect("audit data parses")
    }

    #[gpui::test]
    fn shell_has_no_global_errors(cx: &mut TestAppContext) {
        let d = shell_data(cx);
        let errors: Vec<_> = audit::run_global(&Rule::ALL, &d)
            .into_iter()
            .filter(|f| f.severity == Severity::Error)
            .collect();
        assert!(errors.is_empty(), "{errors:#?}");
    }

    #[gpui::test]
    fn view_table_findings_are_the_known_deferred_ones(cx: &mut TestAppContext) {
        let d = shell_data(cx);
        let mut names: Vec<String> = audit::run_global(&[Rule::Ux140], &d)
            .into_iter()
            .map(|f| f.message)
            .collect();
        names.sort();
        assert_eq!(names.len(), 2, "{names:#?}"); // Traffic, Logs (arrive with the engine, M3)
    }

    #[test]
    fn spec_bindings_name_real_actions() {
        for (id, _) in SPEC_BINDINGS {
            assert!(crate::actions::find(id).is_some(), "{id}");
        }
        for (_, states) in VIEWS {
            for (_, fixture) in *states {
                // Fixtures may be pending (Problems), but names must be well-formed.
                assert!(!fixture.is_empty());
            }
        }
    }
}
