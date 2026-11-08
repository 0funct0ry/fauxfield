//! `ui.audit` rules (SPEC §11.4): pure functions over the semantic tree and data payloads
//! gathered by the GUI. No GPUI here, so every rule is tested with hand-built input.

pub mod contrast;
pub mod rules;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, JsonSchema,
)]
pub enum Rule {
    Ux101,
    Ux102,
    Ux103,
    Ux110,
    Ux120,
    Ux130,
    Ux131,
    Ux140,
    Ux150,
}

impl Rule {
    pub const ALL: [Rule; 9] = [
        Rule::Ux101,
        Rule::Ux102,
        Rule::Ux103,
        Rule::Ux110,
        Rule::Ux120,
        Rule::Ux130,
        Rule::Ux131,
        Rule::Ux140,
        Rule::Ux150,
    ];

    pub fn id(self) -> &'static str {
        match self {
            Rule::Ux101 => "UX101",
            Rule::Ux102 => "UX102",
            Rule::Ux103 => "UX103",
            Rule::Ux110 => "UX110",
            Rule::Ux120 => "UX120",
            Rule::Ux130 => "UX130",
            Rule::Ux131 => "UX131",
            Rule::Ux140 => "UX140",
            Rule::Ux150 => "UX150",
        }
    }

    pub fn parse(s: &str) -> Option<Rule> {
        Rule::ALL
            .into_iter()
            .find(|r| r.id().eq_ignore_ascii_case(s))
    }

    pub fn severity(self) -> Severity {
        match self {
            Rule::Ux102 | Rule::Ux103 | Rule::Ux110 | Rule::Ux120 | Rule::Ux130 => Severity::Error,
            _ => Severity::Warning,
        }
    }

    /// Rules evaluated on every captured frame (the rest run once per audit).
    pub fn per_frame(self) -> bool {
        matches!(self, Rule::Ux101 | Rule::Ux102 | Rule::Ux103 | Rule::Ux131)
    }
}

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Warning,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Meta {
    #[serde(default)]
    pub interactive: bool,
    /// The click target is a registered action (reachable from keyboard and palette).
    #[serde(default)]
    pub action: bool,
    #[serde(default)]
    pub tooltip: bool,
    #[serde(default)]
    pub truncates: bool,
    /// Action id that gives a keyboard equivalent for a non-action control.
    #[serde(default)]
    pub keyboard: Option<String>,
}

fn yes() -> bool {
    true
}

/// A node of `ui.tree`, as the audit needs it.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AuditNode {
    pub id: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub visible: bool,
    #[serde(default)]
    pub clipped: bool,
    #[serde(default)]
    pub z: u8,
    pub bounds: Bounds,
    #[serde(default)]
    pub parent: Option<String>,
    #[serde(default)]
    pub meta: Meta,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Pair {
    pub theme: String,
    pub fg_name: String,
    pub bg_name: String,
    pub fg: [u8; 4],
    pub bg: [u8; 4],
    /// `text` (4.5:1) or `ui` (3:1).
    pub kind: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ActionCheck {
    pub id: String,
    #[serde(default)]
    pub binding: Option<String>,
    #[serde(default)]
    pub expected_binding: Option<String>,
    pub in_palette: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ViewState {
    pub state: String,
    pub fixture: String,
}

#[derive(Clone, Debug, Deserialize)]
pub struct ViewInfo {
    pub name: String,
    #[serde(default)]
    pub states: Vec<ViewState>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Source {
    pub path: String,
    pub text: String,
}

#[derive(Clone, Debug, Default, Deserialize)]
pub struct Perf {
    #[serde(default)]
    pub frames_ms: Vec<f64>,
    #[serde(default)]
    pub key_to_paint_ms: Vec<f64>,
}

/// Everything besides the tree that the rules read.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct AuditData {
    #[serde(default)]
    pub pairs: Vec<Pair>,
    #[serde(default)]
    pub actions: Vec<ActionCheck>,
    #[serde(default)]
    pub views: Vec<ViewInfo>,
    #[serde(default)]
    pub fixtures: Vec<String>,
    /// `None` when the sources are not available (not a source checkout).
    #[serde(default)]
    pub sources: Option<Vec<Source>>,
    #[serde(default)]
    pub perf: Option<Perf>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Finding {
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    pub elements: Vec<String>,
    pub bounds: Option<Bounds>,
    pub fix: String,
}

/// Files that may contain colour values (UX120).
pub const COLOUR_ALLOWED: &[&str] = &["theme.rs", "theme_model.rs", "capture.rs", "audit"];

/// Run the per-frame rules over one captured tree.
pub fn run_frame(rules: &[Rule], nodes: &[AuditNode]) -> Vec<Finding> {
    let mut out = Vec::new();
    for r in rules {
        match r {
            Rule::Ux101 => out.extend(rules::ux101(nodes)),
            Rule::Ux102 => out.extend(rules::ux102(nodes)),
            Rule::Ux103 => out.extend(rules::ux103(nodes)),
            Rule::Ux131 => out.extend(rules::ux131(nodes)),
            _ => {}
        }
    }
    sort(&mut out);
    out
}

/// Run the rules that do not depend on a frame (contrast, sources, actions, views, timing).
pub fn run_global(rules: &[Rule], data: &AuditData) -> Vec<Finding> {
    let mut out = Vec::new();
    for r in rules {
        match r {
            Rule::Ux110 => out.extend(rules::ux110(data)),
            Rule::Ux120 => out.extend(rules::ux120(data, COLOUR_ALLOWED)),
            Rule::Ux130 => out.extend(rules::ux130(data)),
            Rule::Ux140 => out.extend(rules::ux140(data)),
            Rule::Ux150 => out.extend(rules::ux150(data)),
            _ => {}
        }
    }
    sort(&mut out);
    out
}

/// Findings in a stable order, so two audits of the same state compare equal.
pub fn sort(f: &mut [Finding]) {
    f.sort_by(|a, b| (&a.rule, &a.elements, &a.message).cmp(&(&b.rule, &b.elements, &b.message)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn node(id: &str, role: &str, x: f32, y: f32, w: f32, h: f32) -> AuditNode {
        AuditNode {
            id: id.into(),
            role: role.into(),
            label: id.into(),
            enabled: true,
            visible: true,
            bounds: Bounds { x, y, w, h },
            parent: Some("root".into()),
            meta: Meta {
                interactive: false,
                action: true,
                ..Meta::default()
            },
            ..AuditNode::default()
        }
    }

    fn ids(f: &[Finding]) -> Vec<&str> {
        f.iter().map(|f| f.rule.as_str()).collect()
    }

    #[test]
    fn clean_tree_reports_nothing() {
        let mut a = node("a", "button", 0.0, 0.0, 40.0, 24.0);
        a.meta.interactive = true;
        let b = node("b", "text", 50.0, 0.0, 40.0, 24.0);
        assert!(run_frame(&Rule::ALL, &[a, b]).is_empty());
    }

    #[test]
    fn ux101_clipped_text_without_tooltip() {
        let mut a = node("a", "text", 0.0, 0.0, 40.0, 24.0);
        a.clipped = true;
        assert_eq!(ids(&run_frame(&[Rule::Ux101], &[a.clone()])), ["UX101"]);
        a.meta.tooltip = true;
        assert!(run_frame(&[Rule::Ux101], &[a]).is_empty());
    }

    #[test]
    fn ux102_overlap_same_z_only() {
        let a = node("a", "panel", 0.0, 0.0, 50.0, 50.0);
        let mut b = node("b", "panel", 25.0, 25.0, 50.0, 50.0);
        assert_eq!(
            ids(&run_frame(&[Rule::Ux102], &[a.clone(), b.clone()])),
            ["UX102"]
        );
        b.z = 1;
        assert!(run_frame(&[Rule::Ux102], &[a, b]).is_empty());
    }

    #[test]
    fn ux103_small_target() {
        let mut a = node("a", "button", 0.0, 0.0, 10.0, 10.0);
        a.meta.interactive = true;
        assert_eq!(ids(&run_frame(&[Rule::Ux103], &[a.clone()])), ["UX103"]);
        a.enabled = false;
        assert!(run_frame(&[Rule::Ux103], &[a]).is_empty());
    }

    #[test]
    fn ux131_no_keyboard_path() {
        let mut a = node("a", "button", 0.0, 0.0, 30.0, 30.0);
        a.meta.interactive = true;
        a.meta.action = false;
        assert_eq!(ids(&run_frame(&[Rule::Ux131], &[a.clone()])), ["UX131"]);
        a.meta.keyboard = Some("save".into());
        assert!(run_frame(&[Rule::Ux131], &[a]).is_empty());
    }

    fn data() -> AuditData {
        AuditData {
            pairs: vec![Pair {
                theme: "t".into(),
                fg_name: "text".into(),
                bg_name: "bg".into(),
                fg: [0, 0, 0, 255],
                bg: [255, 255, 255, 255],
                kind: "text".into(),
            }],
            actions: vec![ActionCheck {
                id: "x".into(),
                binding: Some("cmd-x".into()),
                expected_binding: Some("cmd-x".into()),
                in_palette: true,
            }],
            views: vec![ViewInfo {
                name: "v".into(),
                states: vec![ViewState {
                    state: "empty".into(),
                    fixture: "f".into(),
                }],
            }],
            fixtures: vec!["f".into()],
            sources: Some(vec![Source {
                path: "views/a.rs".into(),
                text: "let c = t.text;".into(),
            }]),
            perf: Some(Perf {
                frames_ms: vec![2.0; 10],
                key_to_paint_ms: vec![5.0; 10],
            }),
        }
    }

    #[test]
    fn clean_data_reports_nothing() {
        assert!(run_global(&Rule::ALL, &data()).is_empty());
    }

    #[test]
    fn ux110_low_contrast() {
        let mut d = data();
        d.pairs[0].fg = [120, 120, 120, 255];
        d.pairs[0].bg = [128, 128, 128, 255];
        assert_eq!(ids(&run_global(&[Rule::Ux110], &d)), ["UX110"]);
        d.pairs[0].kind = "ui".into();
        assert_eq!(ids(&run_global(&[Rule::Ux110], &d)), ["UX110"]);
    }

    #[test]
    fn ux120_colour_literals() {
        for bad in [
            "let c = rgb(0x112233);",
            "let c = 0xff00ffaa;",
            "hsla(0.1, 0.1, 0.1, 1.)",
            "Rgba { r: 1. }",
        ] {
            let mut d = data();
            d.sources = Some(vec![Source {
                path: "views/a.rs".into(),
                text: bad.into(),
            }]);
            assert_eq!(ids(&run_global(&[Rule::Ux120], &d)), ["UX120"], "{bad}");
        }
        let mut d = data();
        d.sources = Some(vec![Source {
            path: "src/theme.rs".into(),
            text: "rgb(0x112233)".into(),
        }]);
        assert!(run_global(&[Rule::Ux120], &d).is_empty());
        d.sources = Some(vec![Source {
            path: "a.rs".into(),
            text: "// rgb(0x112233)".into(),
        }]);
        assert!(run_global(&[Rule::Ux120], &d).is_empty());
        d.sources = None;
        assert!(run_global(&[Rule::Ux120], &d).is_empty());
    }

    #[test]
    fn ux130_palette_and_binding() {
        let mut d = data();
        d.actions[0].in_palette = false;
        assert_eq!(ids(&run_global(&[Rule::Ux130], &d)), ["UX130"]);
        d.actions[0].in_palette = true;
        d.actions[0].binding = None;
        assert_eq!(ids(&run_global(&[Rule::Ux130], &d)), ["UX130"]);
    }

    #[test]
    fn ux140_missing_fixture_or_state() {
        let mut d = data();
        d.fixtures.clear();
        assert_eq!(ids(&run_global(&[Rule::Ux140], &d)), ["UX140"]);
        d.views[0].states.clear();
        assert_eq!(ids(&run_global(&[Rule::Ux140], &d)), ["UX140"]);
    }

    #[test]
    fn ux150_over_budget() {
        let mut d = data();
        d.perf = Some(Perf {
            frames_ms: vec![20.0; 10],
            key_to_paint_ms: vec![80.0; 10],
        });
        assert_eq!(ids(&run_global(&[Rule::Ux150], &d)), ["UX150", "UX150"]);
    }
}
