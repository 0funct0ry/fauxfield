//! One pure function per audit rule (SPEC §11.4). No GPUI; testable with hand-built trees.

use std::collections::BTreeMap;

use super::contrast::ratio;
use super::{AuditData, AuditNode, Bounds, Finding, Rule};

/// Minimum target size in points (UX103).
pub const MIN_TARGET: f32 = 20.0;
/// Frame time budget for scrolling (SPEC §15, 120 Hz), p95 in ms (UX150).
pub const FRAME_BUDGET_MS: f64 = 8.0;
/// Input-to-paint budget, p95 in ms (UX150; not in §15, recorded in §20.10).
pub const KEY_TO_PAINT_BUDGET_MS: f64 = 50.0;

const TEXT_ROLES: &[&str] = &["text", "tab", "button", "row", "menu_item"];

fn finding(
    rule: Rule,
    message: String,
    elements: Vec<String>,
    bounds: Option<Bounds>,
    fix: &str,
) -> Finding {
    Finding {
        rule: rule.id().to_string(),
        severity: rule.severity(),
        message,
        elements,
        bounds,
        fix: fix.to_string(),
    }
}

/// UX101: clipped or truncated text without a tooltip.
pub fn ux101(nodes: &[AuditNode]) -> Vec<Finding> {
    nodes
        .iter()
        .filter(|n| {
            n.visible
                && TEXT_ROLES.contains(&n.role.as_str())
                && !n.label.is_empty()
                && (n.clipped || n.meta.truncates)
                && !n.meta.tooltip
        })
        .map(|n| {
            finding(
                Rule::Ux101,
                format!(
                    "`{}` ({}) is clipped or truncated and has no tooltip",
                    n.id, n.label
                ),
                vec![n.id.clone()],
                Some(n.bounds),
                "Add a tooltip with the full text, give the element room, or shorten the label.",
            )
        })
        .collect()
}

fn intersection(a: &Bounds, b: &Bounds) -> f32 {
    let w = (a.x + a.w).min(b.x + b.w) - a.x.max(b.x);
    let h = (a.y + a.h).min(b.y + b.h) - a.y.max(b.y);
    if w > 0.0 && h > 0.0 { w * h } else { 0.0 }
}

/// UX102: overlapping visible siblings on the same z layer.
pub fn ux102(nodes: &[AuditNode]) -> Vec<Finding> {
    let mut by_parent: BTreeMap<Option<&str>, Vec<&AuditNode>> = BTreeMap::new();
    for n in nodes.iter().filter(|n| n.visible && !n.clipped) {
        by_parent.entry(n.parent.as_deref()).or_default().push(n);
    }
    let mut out = Vec::new();
    for sibs in by_parent.values() {
        for (i, a) in sibs.iter().enumerate() {
            for b in &sibs[i + 1..] {
                if a.z == b.z && intersection(&a.bounds, &b.bounds) > 1.0 {
                    out.push(finding(
                        Rule::Ux102,
                        format!("`{}` overlaps its sibling `{}`", a.id, b.id),
                        vec![a.id.clone(), b.id.clone()],
                        Some(a.bounds),
                        "Fix the layout so siblings do not overlap, or put the overlay on a higher z layer.",
                    ));
                }
            }
        }
    }
    out
}

/// UX103: interactive control smaller than 20 pt.
pub fn ux103(nodes: &[AuditNode]) -> Vec<Finding> {
    nodes
        .iter()
        .filter(|n| n.visible && n.enabled && n.meta.interactive)
        .filter(|n| n.bounds.w < MIN_TARGET || n.bounds.h < MIN_TARGET)
        .map(|n| {
            finding(
                Rule::Ux103,
                format!(
                    "`{}` is {}x{} pt, below the {MIN_TARGET} pt minimum target",
                    n.id, n.bounds.w, n.bounds.h
                ),
                vec![n.id.clone()],
                Some(n.bounds),
                "Enlarge the hit area to at least 20x20 pt (padding is enough).",
            )
        })
        .collect()
}

/// UX110: contrast of the theme token pairs, in every theme supplied.
pub fn ux110(data: &AuditData) -> Vec<Finding> {
    data.pairs
        .iter()
        .filter_map(|p| {
            let need = if p.kind == "text" { 4.5 } else { 3.0 };
            let got = ratio(p.fg, p.bg);
            (got < need).then(|| {
                finding(
                    Rule::Ux110,
                    format!(
                        "{}: {} on {} is {got:.2}:1, below {need}:1",
                        p.theme, p.fg_name, p.bg_name
                    ),
                    vec![],
                    None,
                    "Adjust the theme tokens (or pick a different token for this use).",
                )
            })
        })
        .collect()
}

fn is_colour_literal(line: &str) -> bool {
    let code = line.split("//").next().unwrap_or("");
    if code.contains("rgb(")
        || code.contains("rgba(")
        || code.contains("hsla(")
        || code.contains("hsl(")
        || code.contains("Rgba {")
        || code.contains("Hsla {")
    {
        return true;
    }
    // 0xRRGGBB / 0xRRGGBBAA hex literals.
    let b = code.as_bytes();
    let mut i = 0;
    while i + 1 < b.len() {
        if b[i] == b'0' && b[i + 1] == b'x' {
            let digits = b[i + 2..]
                .iter()
                .take_while(|c| c.is_ascii_hexdigit())
                .count();
            if digits == 6 || digits == 8 {
                return true;
            }
        }
        i += 1;
    }
    false
}

/// UX120: colour literals in view sources. `allowed` path fragments are skipped.
pub fn ux120(data: &AuditData, allowed: &[&str]) -> Vec<Finding> {
    let mut out = Vec::new();
    for s in data.sources.iter().flatten() {
        if allowed.iter().any(|a| s.path.contains(a)) {
            continue;
        }
        for (i, line) in s.text.lines().enumerate() {
            if is_colour_literal(line) {
                out.push(finding(
                    Rule::Ux120,
                    format!("{}:{} has a hard-coded colour", s.path, i + 1),
                    vec![],
                    None,
                    "Use a theme token (SPEC §9.1) instead of a colour literal.",
                ));
            }
        }
    }
    out
}

/// UX130: action missing from the palette or without the SPEC §9.8 keybinding.
pub fn ux130(data: &AuditData) -> Vec<Finding> {
    let mut out = Vec::new();
    for a in &data.actions {
        if !a.in_palette {
            out.push(finding(
                Rule::Ux130,
                format!("action `{}` is not listed in the Command Palette", a.id),
                vec![],
                None,
                "Register the action in the ACTIONS table so the palette lists it.",
            ));
        }
        if let Some(want) = &a.expected_binding
            && a.binding.as_deref() != Some(want.as_str())
        {
            out.push(finding(
                Rule::Ux130,
                format!(
                    "action `{}` is bound to {:?}, SPEC §9.8 defines {want}",
                    a.id, a.binding
                ),
                vec![],
                None,
                "Bind the action as SPEC §9.8 defines (or update the spec if the decision changed).",
            ));
        }
    }
    out
}

/// UX131: interactive element with no keyboard path.
pub fn ux131(nodes: &[AuditNode]) -> Vec<Finding> {
    nodes
        .iter()
        .filter(|n| n.visible && n.enabled && n.meta.interactive)
        .filter(|n| !n.meta.action && n.meta.keyboard.is_none())
        .map(|n| {
            finding(
                Rule::Ux131,
                format!("`{}` has no keyboard path", n.id),
                vec![n.id.clone()],
                Some(n.bounds),
                "Back the control with a registered action, or declare the keyboard equivalent.",
            )
        })
        .collect()
}

/// UX140: views without an empty/loading/error state shown by a fixture.
pub fn ux140(data: &AuditData) -> Vec<Finding> {
    let mut out = Vec::new();
    for v in &data.views {
        if v.states.is_empty() {
            out.push(finding(
                Rule::Ux140,
                format!(
                    "view `{}` declares no empty, loading, or error state",
                    v.name
                ),
                vec![],
                None,
                "Implement an empty state and add a fixture for it.",
            ));
        }
        for s in &v.states {
            if !data.fixtures.contains(&s.fixture) {
                out.push(finding(
                    Rule::Ux140,
                    format!(
                        "view `{}` {} state has no fixture `{}`",
                        v.name, s.state, s.fixture
                    ),
                    vec![],
                    None,
                    "Add the fixture to fixtures/ui-states and register it.",
                ));
            }
        }
    }
    out
}

fn p95(v: &[f64]) -> Option<f64> {
    if v.is_empty() {
        return None;
    }
    let mut s = v.to_vec();
    s.sort_by(f64::total_cmp);
    Some(s[((s.len() - 1) as f64 * 0.95).round() as usize])
}

/// UX150: frame time and key-to-paint latency over budget.
pub fn ux150(data: &AuditData) -> Vec<Finding> {
    let mut out = Vec::new();
    let Some(p) = &data.perf else { return out };
    if let Some(v) = p95(&p.frames_ms)
        && v > FRAME_BUDGET_MS
    {
        out.push(finding(
            Rule::Ux150,
            format!("frame time p95 is {v:.1} ms, budget {FRAME_BUDGET_MS} ms"),
            vec![],
            None,
            "Profile the view that was on screen; virtualise or cache what it renders.",
        ));
    }
    if let Some(v) = p95(&p.key_to_paint_ms)
        && v > KEY_TO_PAINT_BUDGET_MS
    {
        out.push(finding(
            Rule::Ux150,
            format!("key-to-paint p95 is {v:.1} ms, budget {KEY_TO_PAINT_BUDGET_MS} ms"),
            vec![],
            None,
            "Find what runs on the key path before the next paint and move it off the UI thread.",
        ));
    }
    out
}
