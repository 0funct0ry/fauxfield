//! Rules sub-tab (SPEC §20.15): one table for the whole Route, grouped by Response in
//! evaluation order, with a Response column and an and/or switch per group.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
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
    pub(crate) fn set_rule_target(
        &mut self,
        resp: &str,
        idx: usize,
        target: &str,
        cx: &mut Context<Self>,
    ) {
        self.edit_rule(resp, idx, cx, |r| {
            r.target = target.into();
            if !fauxfield_model::rules::needs_key(target) && target != "body" {
                r.key = None;
            }
        });
    }

    pub(crate) fn set_rule_op(&mut self, resp: &str, idx: usize, op: &str, cx: &mut Context<Self>) {
        self.edit_rule(resp, idx, cx, |r| {
            r.op = op.into();
            if matches!(op, "exists" | "absent") {
                r.value = None;
            } else if r.value.is_none() {
                r.value = Some(toml::Value::String(String::new()));
            }
        });
    }

    pub(crate) fn set_response_mode_value(
        &mut self,
        route: &str,
        mode: &str,
        cx: &mut Context<Self>,
    ) {
        let (route, mode) = (route.to_string(), mode.to_string());
        self.store_op(cx, |s| s.set_response_mode(&route, &mode));
    }

    /// A ComboBox over a fixed list of plain values, flexing with its row.
    fn rule_combo(
        &self,
        kind: &'static str,
        values: &'static [&'static str],
        resp: &str,
        i: usize,
        current: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (r, key) = (resp.to_string(), format!("rule-{kind}-{resp}-{i}"));
        self.combo_field(
            cx,
            ComboSite {
                key,
                items: values.iter().map(|v| ComboItem::plain(*v)).collect(),
                value: Some(current.to_string().into()),
                fallback: Some(current.to_string().into()),
                color: None,
                tip: None,
                width: ComboWidth::Flex,
                look: ComboLook::Field,
                size: ComboSize::Medium,
                on_event: std::rc::Rc::new(move |this, ev, cx| {
                    if let ComboEvent::Change(v) = ev {
                        if kind == "target" {
                            this.set_rule_target(&r, i, v, cx)
                        } else {
                            this.set_rule_op(&r, i, v, cx)
                        }
                    }
                }),
            },
        )
    }

    pub(crate) fn render_rules(&self, t: &Theme, r: &Route, cx: &mut Context<Self>) -> AnyElement {
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
        let key_used = fauxfield_model::rules::needs_key(&rule.target) || rule.target == "body";
        let value_used = !matches!(rule.op.as_str(), "exists" | "absent");
        let blank = || {
            div()
                .flex_1()
                .min_w(px(0.))
                .text_color(t.text_muted)
                .child("–")
        };
        let negate_id = resp.id.clone();
        let (up_id, down_id, del_id, curl_id) = (
            resp.id.clone(),
            resp.id.clone(),
            resp.id.clone(),
            resp.id.clone(),
        );
        // Icon-only button with a tooltip.
        let small = |id: String, glyph: &'static str, tip: &str, enabled: bool| {
            div()
                .id(crate::ui::eid(id))
                .size(px(24.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(4.))
                .when(enabled, |d| {
                    d.cursor_pointer().hover(|s| s.bg(t.element_hover))
                })
                .child(crate::ui::icon(
                    glyph,
                    14.,
                    if enabled { t.text } else { t.text_placeholder },
                ))
                .tooltip(crate::ui::tooltip(tip.to_string()))
        };
        let error = fauxfield_model::rules::check_rule(rule).err();
        let mut row = div().flex().flex_col().child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .h(px(30.))
                .child(self.rule_combo(
                    "target",
                    fauxfield_model::rules::TARGETS,
                    &resp.id,
                    i,
                    &rule.target,
                    cx,
                ))
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
                        0.,
                        false,
                        cx,
                    )
                } else {
                    blank().into_any_element()
                })
                .child(self.rule_combo(
                    "op",
                    fauxfield_model::rules::OPS,
                    &resp.id,
                    i,
                    &rule.op,
                    cx,
                ))
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
                        0.,
                        false,
                        cx,
                    )
                } else {
                    blank().into_any_element()
                })
                .child(crate::dev_id_meta!(
                    small(
                        format!("rule-negate-{}-{i}", resp.id),
                        "exclamation",
                        if rule.negate {
                            "Negated: matches when the condition is false (click to turn off)"
                        } else {
                            "Negate: match when the condition is false"
                        },
                        true,
                    )
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
                    crate::dev_mcp::tree::Meta {
                        tooltip: true,
                        ..Default::default()
                    },
                    format!("rule-negate-{}-{i}", resp.id),
                    Button,
                    "Negate rule",
                    Press(format!("rule-negate:{}:{i}", resp.id))
                ))
                .child(crate::dev_id_meta!(
                    small(
                        format!("rule-up-{}-{i}", resp.id),
                        "arrow-up",
                        "Move rule up",
                        i > 0
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if i > 0 {
                            let id = up_id.clone();
                            this.store_op(cx, |s| s.move_rule(&id, i, i - 1));
                        }
                    })),
                    crate::dev_mcp::tree::Meta {
                        tooltip: true,
                        ..Default::default()
                    },
                    format!("rule-up-{}-{i}", resp.id),
                    Button,
                    "Move rule up",
                    Press(format!("rule-up:{}:{i}", resp.id))
                ))
                .child(crate::dev_id_meta!(
                    small(
                        format!("rule-down-{}-{i}", resp.id),
                        "arrow-down",
                        "Move rule down",
                        i + 1 < n
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if i + 1 < n {
                            let id = down_id.clone();
                            this.store_op(cx, |s| s.move_rule(&id, i, i + 1));
                        }
                    })),
                    crate::dev_mcp::tree::Meta {
                        tooltip: true,
                        ..Default::default()
                    },
                    format!("rule-down-{}-{i}", resp.id),
                    Button,
                    "Move rule down",
                    Press(format!("rule-down:{}:{i}", resp.id))
                ))
                .child(crate::dev_id_meta!(
                    small(
                        format!("rule-curl-{}-{i}", resp.id),
                        "terminal",
                        if resp.rules_logic == "or" || n == 1 {
                            "Copy cURL that triggers this rule"
                        } else {
                            "Copy cURL that satisfies all rules of this Response"
                        },
                        true
                    )
                    .on_click(
                        cx.listener(move |this, _, _, cx| { this.copy_rule_curl(&curl_id, i, cx) })
                    ),
                    crate::dev_mcp::tree::Meta {
                        tooltip: true,
                        ..Default::default()
                    },
                    format!("rule-curl-{}-{i}", resp.id),
                    Button,
                    "Copy rule cURL",
                    Press(format!("rule-curl:{}:{i}", resp.id))
                ))
                .child(crate::dev_id_meta!(
                    small(
                        format!("rule-remove-{}-{i}", resp.id),
                        "x",
                        "Remove rule",
                        true
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let id = del_id.clone();
                        this.store_op(cx, |s| s.delete_rule(&id, i));
                    })),
                    crate::dev_mcp::tree::Meta {
                        tooltip: true,
                        ..Default::default()
                    },
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
            "resp-add-preset" => {
                self.open_picker_menu(&id, crate::tree::RowKind::ErrorPresets, x, y - 300., cx)
            }
            "combo" => {
                if !self.combo_open(&id, window, cx) {
                    return Err(format!("no combo box {id:?} on screen"));
                }
            }
            "rules-add" => self.add_rule(&id, cx),
            "ds-view" => {
                if self.ds_json_mode && id != "json" && !self.leave_json_view(cx) {
                    return Ok(());
                }
                self.ds_json_mode = id == "json";
                self.ds_seed_mode = id == "seed";
                cx.notify();
            }
            "rules-logic" => {
                let or = self
                    .find_response(&id)
                    .is_some_and(|(_, _, r)| r.rules_logic == "or");
                let next = if or { "and" } else { "or" };
                let rid = id.clone();
                self.store_op(cx, |s| s.set_rules_logic(&rid, next));
            }
            "rule-curl" => self.copy_rule_curl(&id, need()?, cx),
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
            "ds-select" => {
                if let Some(r) = id.parse::<usize>().ok().or(idx)
                    && let Some(key) = self.active_dataset_key()
                    && let Some(t) = self.ds_tables.borrow().get(&key).cloned()
                {
                    t.update(cx, |s, cx| {
                        let d = s.delegate_mut();
                        let live = d.actual_index(r);
                        d.selection.toggle(live);
                        s.refresh(cx);
                    });
                    cx.notify();
                }
            }
            "date" | "date-step" | "date-view" | "date-m" | "date-y" | "date-day" => {
                let picker = self
                    .seed_dialog
                    .as_ref()
                    .and_then(|d| {
                        d.cols
                            .iter()
                            .flat_map(|c| c.dates.iter())
                            .find(|(_, p)| p.read(cx).id().as_ref() == id)
                    })
                    .map(|(_, p)| p.clone())
                    .ok_or_else(|| format!("no date picker {id:?}"))?;
                let n = idx.unwrap_or(0);
                picker.update(cx, |p, cx| match name {
                    "date" => p.toggle(window, cx),
                    "date-step" => p.step(if n == 0 { -1 } else { 1 }, cx),
                    "date-view" => p.toggle_view(
                        if n == 0 {
                            crate::date_picker::CalendarView::Month
                        } else {
                            crate::date_picker::CalendarView::Year
                        },
                        cx,
                    ),
                    "date-m" => p.choose_month(n as u32, cx),
                    "date-y" => p.choose_year(n as i32, cx),
                    _ => {
                        let (y, m, d) = (n / 10000, (n / 100) % 100, n % 100);
                        if let Some(day) =
                            chrono::NaiveDate::from_ymd_opt(y as i32, m as u32, d as u32)
                        {
                            p.pick(day, cx);
                        }
                    }
                });
                cx.notify();
            }
            "dropdown" | "dropdown-item" | "dropdown-sub" => {
                self.dropdown_press(name, &id, idx, window, cx)?
            }
            "ds-popover" => {
                self.ds_popover = !self.ds_popover;
                cx.notify();
            }
            "ds-banner-dismiss" => {
                self.ds_banner_dismissed.insert(id.replace('|', ":"));
                cx.notify();
            }
            other if other.starts_with("ws-") => self.ws_op(&other["ws-".len()..], &id, idx, cx),
            other if other.starts_with("schema-") => {
                let arg = if id.is_empty() {
                    String::new()
                } else {
                    id.clone()
                };
                self.schema_op(&other["schema-".len()..], &arg, window, cx)
            }
            other if other.starts_with("export-") => {
                self.export_op(&other["export-".len()..], &id, window, cx)
            }
            other if other.starts_with("seed-") => {
                self.seed_op(&other["seed-".len()..], &id, window, cx)
            }
            other if other.starts_with("column-") => {
                self.column_op(&other["column-".len()..], window, cx)
            }
            other if other.starts_with("crud-") => {
                self.crud_op(&other["crud-".len()..], &id, window, cx)
            }
            other
                if other.starts_with("action-")
                    || other.starts_with("dsread-")
                    || other == "body-source-dataset" =>
            {
                return self.action_press(other, &id, idx, cx, window);
            }
            "set-cat" | "set-toggle" | "set-reset" | "km-record" | "km-clear" | "km-reset"
            | "km-save" | "km-cancel" | "km-resolve" | "km-stroke" => {
                self.press_settings(name, &id, cx)?
            }
            other => return Err(format!("unknown press op {other:?}")),
        }
        Ok(())
    }
}
