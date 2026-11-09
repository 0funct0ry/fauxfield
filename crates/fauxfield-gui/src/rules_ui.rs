//! Rules sub-tab (SPEC §20.15): one table for the whole Route, grouped by Response in
//! evaluation order, with a Response column and an and/or switch per group.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::tree::RowKind;
use crate::ui::ROW_H;
use crate::view::{Response, Route};

/// Text shown in the Value cell for a rule value (`in` lists are comma separated).
pub fn value_text(v: &Option<toml::Value>) -> String {
    match v {
        None => String::new(),
        Some(toml::Value::String(s)) => s.clone(),
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|x| match x {
                toml::Value::String(s) => s.clone(),
                o => o.to_string(),
            })
            .collect::<Vec<_>>()
            .join(", "),
        Some(o) => o.to_string(),
    }
}

/// Parse text typed in the Value cell for `op`: lists for `in`, numbers for comparisons.
pub fn parse_value(op: &str, text: &str) -> toml::Value {
    let scalar = |s: &str| {
        let s = s.trim();
        if matches!(op, "gt" | "gte" | "lt" | "lte" | "in")
            && let Ok(i) = s.parse::<i64>()
        {
            return toml::Value::Integer(i);
        }
        if matches!(op, "gt" | "gte" | "lt" | "lte")
            && let Ok(f) = s.parse::<f64>()
        {
            return toml::Value::Float(f);
        }
        toml::Value::String(s.to_string())
    };
    if op == "in" {
        toml::Value::Array(text.split(',').map(scalar).collect())
    } else if matches!(op, "gt" | "gte" | "lt" | "lte") {
        scalar(text)
    } else {
        toml::Value::String(text.to_string())
    }
}

impl Shell {
    pub(crate) fn render_rules(&self, t: &Theme, r: &Route, cx: &mut Context<Self>) -> AnyElement {
        let head = |s: &str, w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(s.to_uppercase())
        };
        let mut col = div()
            .id(crate::ui::eid(format!("rules-{}", r.id)))
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(2.));
        if r.response_mode != "rules" {
            col = col.child(
                div()
                    .pb(px(8.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child(format!(
                        "Response mode is {}: rules are ignored. Change it in Settings.",
                        r.response_mode
                    )),
            );
        }
        col = col.child(
            div()
                .flex()
                .flex_row()
                .gap(px(6.))
                .pb(px(4.))
                .child(head("Response", 130.))
                .child(head("Target", 84.))
                .child(head("Key", 140.))
                .child(head("Op", 100.))
                .child(head("Value", 180.))
                .child(head("Not", 36.)),
        );
        for resp in &r.responses {
            col = col.child(self.render_rule_group(t, resp, cx));
        }
        col.into_any_element()
    }

    fn render_rule_group(&self, t: &Theme, resp: &Response, cx: &mut Context<Self>) -> AnyElement {
        let (logic_id, add_id) = (resp.id.clone(), resp.id.clone());
        let or = resp.rules_logic == "or";
        let chip = |id: String, label: String, on: bool| {
            div()
                .id(crate::ui::eid(id))
                .h(px(22.))
                .px(px(8.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .border_1()
                .border_color(if on {
                    t.border_focused
                } else {
                    t.border_variant
                })
                .text_size(px(11.))
                .cursor_pointer()
                .hover(|s| s.bg(t.element_hover))
                .child(label)
        };
        let mut g = div()
            .id(crate::ui::eid(format!("rules-group-{}", resp.id)))
            .flex()
            .flex_col()
            .pt(px(10.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .h(px(ROW_H + 4.))
                    .child(
                        div()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(if resp.disabled { t.text_muted } else { t.text })
                            .child(format!(
                                "{} · {}{}{}",
                                resp.name,
                                resp.status,
                                if resp.default { " · default" } else { "" },
                                if resp.disabled { " · disabled" } else { "" },
                            )),
                    )
                    .child(crate::dev_id!(
                        chip(
                            format!("rules-logic-{}", resp.id),
                            if or {
                                "ANY (or)".into()
                            } else {
                                "ALL (and)".into()
                            },
                            or,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let next = if or { "and" } else { "or" };
                            let id = logic_id.clone();
                            this.store_op(cx, |s| s.set_rules_logic(&id, next));
                        })),
                        format!("rules-logic-{}", resp.id),
                        Button,
                        "Rules logic",
                        Press(format!("rules-logic:{}", resp.id))
                    ))
                    .child(crate::dev_id!(
                        chip(format!("rules-add-{}", resp.id), "+ Add rule".into(), false)
                            .on_click(
                                cx.listener(move |this, _, _, cx| { this.add_rule(&add_id, cx) })
                            ),
                        format!("rules-add-{}", resp.id),
                        Button,
                        "Add rule",
                        Press(format!("rules-add:{}", resp.id))
                    )),
            );
        if resp.rules.is_empty() {
            g = g.child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child("No rules: used only as the fallback."),
            );
        }
        let n = resp.rules.len();
        for (i, rule) in resp.rules.iter().enumerate() {
            g = g.child(self.render_rule_row(t, resp, i, n, rule, cx));
        }
        g.into_any_element()
    }

    fn render_rule_row(
        &self,
        t: &Theme,
        resp: &Response,
        i: usize,
        n: usize,
        rule: &fauxfield_model::Rule,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rid = resp.id.clone();
        let picker = |id: String,
                      pick_op: &'static str,
                      label: String,
                      kind: RowKind,
                      cx: &mut Context<Self>| {
            let _ = pick_op; // only read by the dev-mcp element tag
            let row_id = format!("{rid}#{i}");
            crate::dev_id!(
                div()
                    .id(crate::ui::eid(id.clone()))
                    .h(px(26.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color(t.border_variant)
                    .hover(|s| s.border_color(t.border))
                    .font_family(crate::assets::BUFFER_FONT)
                    .text_size(px(12.))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            this.open_picker_menu(
                                &row_id,
                                kind.clone(),
                                f32::from(ev.position.x),
                                f32::from(ev.position.y),
                                cx,
                            );
                        }),
                    )
                    .child(label.clone()),
                id,
                Button,
                label,
                Press(format!("{pick_op}:{rid}:{i}"))
            )
        };
        let key_used = fauxfield_model::rules::needs_key(&rule.target) || rule.target == "body";
        let value_used = !matches!(rule.op.as_str(), "exists" | "absent");
        let blank = |w: f32| {
            div()
                .w(px(w))
                .flex_none()
                .text_color(t.text_muted)
                .child("–")
        };
        let negate_id = resp.id.clone();
        let (up_id, down_id, del_id) = (resp.id.clone(), resp.id.clone(), resp.id.clone());
        let small = |id: String, glyph: &'static str, enabled: bool| {
            div()
                .id(crate::ui::eid(id))
                .size(px(22.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .text_size(px(13.))
                .text_color(if enabled { t.text } else { t.text_placeholder })
                .when(enabled, |d| {
                    d.cursor_pointer().hover(|s| s.bg(t.element_hover))
                })
                .child(glyph)
        };
        let error = fauxfield_model::rules::check_rule(rule).err();
        let mut row = div().flex().flex_col().child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .h(px(30.))
                .child(
                    div()
                        .w(px(130.))
                        .flex_none()
                        .truncate()
                        .text_size(px(12.))
                        .text_color(t.text_muted)
                        .child(resp.name.clone()),
                )
                .child(div().w(px(84.)).flex_none().child(picker(
                    format!("rule-target-{}-{i}", resp.id),
                    "rule-target",
                    rule.target.clone(),
                    RowKind::RuleTarget,
                    cx,
                )))
                .child(if key_used {
                    self.inline_field(
                        t,
                        format!("rule-key-{}-{i}", resp.id),
                        InlineTarget::RuleKey {
                            response: resp.id.clone(),
                            row: i,
                        },
                        rule.key.as_deref().unwrap_or(""),
                        if rule.target == "body" {
                            "$.user.role"
                        } else {
                            "name"
                        },
                        140.,
                        false,
                        cx,
                    )
                } else {
                    blank(140.).into_any_element()
                })
                .child(div().w(px(100.)).flex_none().child(picker(
                    format!("rule-op-{}-{i}", resp.id),
                    "rule-op",
                    rule.op.clone(),
                    RowKind::RuleOp,
                    cx,
                )))
                .child(if value_used {
                    self.inline_field(
                        t,
                        format!("rule-value-{}-{i}", resp.id),
                        InlineTarget::RuleValue {
                            response: resp.id.clone(),
                            row: i,
                        },
                        &value_text(&rule.value),
                        if rule.op == "json_schema" {
                            "schemas/user.json"
                        } else {
                            "value"
                        },
                        180.,
                        false,
                        cx,
                    )
                } else {
                    blank(180.).into_any_element()
                })
                .child(crate::dev_id!(
                    small(format!("rule-negate-{}-{i}", resp.id), "¬", true)
                        .w(px(36.))
                        .border_1()
                        .border_color(if rule.negate {
                            t.border_focused
                        } else {
                            t.border_variant
                        })
                        .bg(if rule.negate {
                            t.element_selected
                        } else {
                            t.panel
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_rule(&negate_id, i, cx, |r| r.negate = !r.negate)
                        })),
                    format!("rule-negate-{}-{i}", resp.id),
                    Button,
                    "Negate rule",
                    Press(format!("rule-negate:{}:{i}", resp.id))
                ))
                .child(crate::dev_id!(
                    small(format!("rule-up-{}-{i}", resp.id), "↑", i > 0).on_click(cx.listener(
                        move |this, _, _, cx| {
                            if i > 0 {
                                let id = up_id.clone();
                                this.store_op(cx, |s| s.move_rule(&id, i, i - 1));
                            }
                        }
                    )),
                    format!("rule-up-{}-{i}", resp.id),
                    Button,
                    "Move rule up",
                    Press(format!("rule-up:{}:{i}", resp.id))
                ))
                .child(crate::dev_id!(
                    small(format!("rule-down-{}-{i}", resp.id), "↓", i + 1 < n).on_click(
                        cx.listener(move |this, _, _, cx| {
                            if i + 1 < n {
                                let id = down_id.clone();
                                this.store_op(cx, |s| s.move_rule(&id, i, i + 1));
                            }
                        })
                    ),
                    format!("rule-down-{}-{i}", resp.id),
                    Button,
                    "Move rule down",
                    Press(format!("rule-down:{}:{i}", resp.id))
                ))
                .child(crate::dev_id!(
                    small(format!("rule-remove-{}-{i}", resp.id), "×", true).on_click(cx.listener(
                        move |this, _, _, cx| {
                            let id = del_id.clone();
                            this.store_op(cx, |s| s.delete_rule(&id, i));
                        }
                    )),
                    format!("rule-remove-{}-{i}", resp.id),
                    Button,
                    "Remove rule",
                    Press(format!("rule-remove:{}:{i}", resp.id))
                )),
        );
        if let Some((code, msg)) = error {
            row = row.child(crate::dev_id!(
                div()
                    .id(crate::ui::eid(format!("rule-error-{}-{i}", resp.id)))
                    .pl(px(136.))
                    .pb(px(2.))
                    .text_size(px(11.))
                    .text_color(t.deleted)
                    .child(format!("{code}: {msg}")),
                format!("rule-error-{}-{i}", resp.id),
                Text,
                format!("{code}: {msg}"),
                None
            ));
        }
        row.into_any_element()
    }
}

#[cfg(feature = "dev-mcp")]
impl Shell {
    /// Run a control that has no dedicated `Click` variant (`ui.click_element`).
    /// `op` is `<name>:<id>[:<index>]`; bounds are `[x, y, w, h]`.
    pub(crate) fn press(
        &mut self,
        op: &str,
        bounds: [f32; 4],
        window: &mut gpui::Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (x, y) = (bounds[0] + bounds[2] / 2.0, bounds[1] + bounds[3] / 2.0);
        let mut it = op.splitn(3, ':');
        let name = it.next().unwrap_or("");
        let id = it.next().unwrap_or("").to_string();
        let idx = it.next().and_then(|s| s.parse::<usize>().ok());
        let need = || idx.ok_or_else(|| format!("{name} needs an index"));
        match name {
            "resp-add" => self.new_response_for(&id, cx),
            "resp-add-preset" => self.open_picker_menu(&id, RowKind::ErrorPresets, x, y - 300., cx),
            "response-mode" => self.open_picker_menu(&id, RowKind::ResponseMode, x, y, cx),
            "rules-add" => self.add_rule(&id, cx),
            "rules-logic" => {
                let or = self
                    .find_response(&id)
                    .is_some_and(|(_, _, r)| r.rules_logic == "or");
                let next = if or { "and" } else { "or" };
                let rid = id.clone();
                self.store_op(cx, |s| s.set_rules_logic(&rid, next));
            }
            "rule-target" => {
                self.open_picker_menu(&format!("{id}#{}", need()?), RowKind::RuleTarget, x, y, cx)
            }
            "rule-op" => {
                self.open_picker_menu(&format!("{id}#{}", need()?), RowKind::RuleOp, x, y, cx)
            }
            "rule-negate" => self.edit_rule(&id, need()?, cx, |r| r.negate = !r.negate),
            "rule-up" | "rule-down" => {
                let i = need()?;
                let to = if name == "rule-up" {
                    i.saturating_sub(1)
                } else {
                    i + 1
                };
                let rid = id.clone();
                self.store_op(cx, |s| s.move_rule(&rid, i, to));
            }
            "rule-remove" => {
                let (rid, i) = (id.clone(), need()?);
                self.store_op(cx, |s| s.delete_rule(&rid, i));
            }
            "resp-template" => self.toggle_response_template(&id, cx),
            "resp-fault" => self.cycle_response_fault(&id, cx),
            "rate-enabled" => self.toggle_rate_limit(&id, cx),
            "seed-mode" => self.set_seed_mode(&id, need()? == 1, cx),
            "route-header-add" => {
                let at = self.ws.find_route(&id).map_or(0, |(_, r)| r.headers.len());
                self.begin_route_header_cell(&id, at, crate::header_catalog::Col::Key, window, cx);
            }
            "route-header-remove" => self.remove_route_header_row(&id, need()?, cx),
            "var-add" => {
                let (sc, ow) = id.split_once('|').unwrap_or((&id, ""));
                if let Some(scope) = fauxfield_model::vars::Scope::parse(sc) {
                    let owner = (!ow.is_empty()).then(|| ow.to_string());
                    self.begin_prompt(
                        crate::shell::PromptAction::SetVariable {
                            scope,
                            owner,
                            key: None,
                        },
                        "",
                        window,
                        cx,
                    );
                }
            }
            "sample-cancel" => self.close_sample_dialog(cx),
            "sample-save" => self.save_sample(false, window, cx),
            "sample-send" => self.save_sample(true, window, cx),
            other => return Err(format!("unknown press op {other:?}")),
        }
        Ok(())
    }
}
