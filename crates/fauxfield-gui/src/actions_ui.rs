//! Dataset actions of a Response (M10.06, SPEC §6.3): the Actions sub-tab of the Response
//! editor, plus the Dataset body source form that generates a `data.<name>` template.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::view::Response;
use fauxfield_model::{ACTION_OPS, DatasetAction};

/// Which text field of an action step an inline edit targets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionField {
    Key,
    Row,
    Fields,
    Field,
    Value,
    By,
    Filter,
    FailIf,
}

impl ActionField {
    pub fn label(self) -> &'static str {
        match self {
            Self::Key => "key",
            Self::Row => "row",
            Self::Fields => "fields",
            Self::Field => "field",
            Self::Value => "value",
            Self::By => "by",
            Self::Filter => "filter",
            Self::FailIf => "fail if",
        }
    }
    pub fn placeholder(self) -> &'static str {
        match self {
            Self::Key => "{{ request.params.id }}",
            Self::Row => "request body (or a JSON template)",
            Self::Fields => "name={{ request.json.name }}; qty=1",
            Self::Field => "stock.count",
            Self::Value => "{{ request.json.value }}",
            Self::By => "1 (may be negative)",
            Self::Filter => "qty_gte=5&status=old",
            Self::FailIf => "{{ result.qty < 0 }}",
        }
    }
}

/// The fields an op takes, in display order.
pub fn fields_for(op: &str) -> &'static [ActionField] {
    use ActionField::*;
    match op {
        "insert" => &[Row, Fields, FailIf],
        "update" => &[Key, Row, Fields, FailIf],
        "delete" => &[Key, FailIf],
        "delete_where" => &[Filter, FailIf],
        "set" => &[Key, Field, Value, FailIf],
        "increment" => &[Key, Field, By, FailIf],
        _ => &[],
    }
}

fn join_pairs(m: &indexmap::IndexMap<String, String>, sep: &str) -> String {
    m.iter()
        .map(|(k, v)| format!("{k}={v}"))
        .collect::<Vec<_>>()
        .join(sep)
}

fn split_pairs(text: &str, sep: char) -> indexmap::IndexMap<String, String> {
    text.split(sep)
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .filter(|(k, _)| !k.is_empty())
        .collect()
}

pub fn field_text(a: &DatasetAction, f: ActionField) -> String {
    let o = |v: &Option<String>| v.clone().unwrap_or_default();
    match f {
        ActionField::Key => o(&a.key),
        ActionField::Row => o(&a.row),
        ActionField::Fields => join_pairs(&a.fields, "; "),
        ActionField::Field => o(&a.field),
        ActionField::Value => o(&a.value),
        ActionField::By => o(&a.by),
        ActionField::Filter => join_pairs(&a.filter, "&"),
        ActionField::FailIf => o(&a.fail_if),
    }
}

pub fn set_field_text(a: &mut DatasetAction, f: ActionField, text: &str) {
    let t = text.trim();
    let some = || Some(t.to_string()).filter(|s| !s.is_empty());
    match f {
        ActionField::Key => a.key = some(),
        ActionField::Row => a.row = some(),
        ActionField::Fields => a.fields = split_pairs(t, ';'),
        ActionField::Field => a.field = some(),
        ActionField::Value => a.value = some(),
        ActionField::By => a.by = some(),
        ActionField::Filter => a.filter = split_pairs(t, '&'),
        ActionField::FailIf => a.fail_if = some(),
    }
}

/// Convert `id={{ request.params.id }}` to the Tera string expression
/// `"id=" ~ request.params.id`, so a filter can use the request inside `dataset_rows(...)`.
pub fn tera_string(s: &str) -> String {
    let mut parts: Vec<String> = vec![];
    let mut rest = s;
    while let Some(i) = rest.find("{{") {
        if i > 0 {
            parts.push(format!("{:?}", &rest[..i]));
        }
        let after = &rest[i + 2..];
        match after.find("}}") {
            Some(j) => {
                parts.push(format!("({})", after[..j].trim()));
                rest = &after[j + 2..];
            }
            None => {
                parts.push(format!("{:?}", &rest[i..]));
                rest = "";
            }
        }
    }
    if !rest.is_empty() || parts.is_empty() {
        parts.push(format!("{rest:?}"));
    }
    parts.join(" ~ ")
}

/// Read modes of the Dataset body source.
pub const READ_MODES: [(&str, &str); 5] = [
    ("all", "All rows"),
    ("one", "One row by key"),
    ("filtered", "Filtered list"),
    ("count", "Count"),
    ("latest", "Newest row"),
];

/// The form behind the Dataset body source (state of the generator, not of the file).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DsRead {
    pub dataset: String,
    pub mode: String,
    /// CRUD-style filter, may use `{{ request.params.id }}`.
    pub filter: String,
    /// Sort field; a leading `-` is descending.
    pub sort: String,
    pub limit: String,
    pub page: String,
}

/// The `data.<name>` template for a read.
pub fn read_template(r: &DsRead, key: &str) -> String {
    let n = &r.dataset;
    let mut args = vec![format!("data=data.{n}")];
    let filter = match r.mode.as_str() {
        "one" if r.filter.trim().is_empty() => format!("{key}={{{{ request.params.{key} }}}}"),
        _ => r.filter.clone(),
    };
    if !filter.trim().is_empty() {
        args.push(format!("filter={}", tera_string(&filter)));
    }
    let (mut sort, mut order) = (r.sort.trim().to_string(), "");
    if r.mode == "latest" && sort.is_empty() {
        sort = format!("-{key}");
    }
    if let Some(s) = sort.strip_prefix('-') {
        order = "desc";
        sort = s.to_string();
    }
    if r.mode == "count" {
        return format!("{{{{ dataset_count({}) }}}}", args.join(", "));
    }
    if !sort.is_empty() && !matches!(r.mode.as_str(), "one") {
        args.push(format!("sort=\"{sort}\""));
        if !order.is_empty() {
            args.push(format!("order=\"{order}\""));
        }
    }
    match r.mode.as_str() {
        "latest" => args.push("limit=1".into()),
        "all" | "filtered" => {
            if let Ok(l) = r.limit.trim().parse::<u32>()
                && l > 0
            {
                args.push(format!("limit={l}"));
                if let Ok(p) = r.page.trim().parse::<u32>()
                    && p > 1
                {
                    args.push(format!("page={p}"));
                }
            }
        }
        _ => {}
    }
    let call = format!("dataset_rows({})", args.join(", "));
    match r.mode.as_str() {
        "one" | "latest" => format!("{{{{ {call} | first | json_encode() | safe }}}}"),
        _ if r.mode == "all" && args.len() == 1 => {
            format!("{{{{ data.{n} | json_encode() | safe }}}}")
        }
        _ => format!("{{{{ {call} | json_encode() | safe }}}}"),
    }
}

impl Shell {
    pub(crate) fn edit_action(
        &mut self,
        resp: &str,
        idx: usize,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut DatasetAction),
    ) {
        let Some((_, _, r)) = self.find_response(resp) else {
            return;
        };
        let Some(mut a) = r.actions.get(idx).cloned() else {
            return;
        };
        f(&mut a);
        let id = resp.to_string();
        self.store_op(cx, |s| s.set_action(&id, Some(idx), &a));
    }

    /// Append an `insert` step on the first Dataset of the Response's Server.
    pub(crate) fn add_dataset_action(&mut self, resp: &str, cx: &mut Context<Self>) {
        let Some(name) = self.response_datasets(resp).into_iter().next() else {
            self.notify("This Server has no Datasets. Create one first.".into(), cx);
            return;
        };
        let a = DatasetAction {
            dataset: name,
            op: "insert".into(),
            ..Default::default()
        };
        let id = resp.to_string();
        self.store_op(cx, |s| s.set_action(&id, None, &a));
        self.resp_sub.insert(resp.to_string(), 2);
    }

    /// Names of the Datasets of the Server that owns `resp`.
    pub(crate) fn response_datasets(&self, resp: &str) -> Vec<String> {
        let Some((rid, ..)) = self.find_response(resp) else {
            return vec![];
        };
        let Some((srv, _)) = self.ws.find_route(&rid) else {
            return vec![];
        };
        self.ws
            .datasets
            .iter()
            .filter(|d| d.server_id == srv.id)
            .map(|d| d.name.clone())
            .collect()
    }

    /// Key column of a Dataset (default `id`).
    fn dataset_key(&self, resp: &str, name: &str) -> String {
        let Some((rid, ..)) = self.find_response(resp) else {
            return "id".into();
        };
        let Some((srv, _)) = self.ws.find_route(&rid) else {
            return "id".into();
        };
        self.ws
            .datasets
            .iter()
            .find(|d| d.server_id == srv.id && d.name == name)
            .map(|d| d.key.clone())
            .filter(|k| !k.is_empty())
            .unwrap_or_else(|| "id".into())
    }

    /// Write the Dataset read template into the Response body (it stays editable).
    pub(crate) fn generate_dataset_body(&mut self, resp: &str, cx: &mut Context<Self>) {
        let Some(f) = self.ds_read.get(resp).cloned() else {
            return;
        };
        if f.dataset.is_empty() {
            self.notify("Pick a Dataset first.".into(), cx);
            return;
        }
        let key = self.dataset_key(resp, &f.dataset);
        let text = read_template(&f, &key);
        let id = resp.to_string();
        // Every generated read is JSON; without a Content-Type the body would go out as text.
        let needs_type = self
            .find_response(resp)
            .is_some_and(|(_, _, r)| r.content_type.trim().is_empty());
        self.body_edit = None;
        self.store_op(cx, |s| {
            let t = fauxfield_store::Target::Response(id);
            if needs_type {
                s.set_str(&t, "content_type", Some("application/json"))?;
            }
            s.set_str(&t, "body", Some(&text))
        });
        self.body_editors.remove(resp);
        cx.notify();
    }

    fn action_combo(
        &self,
        key: String,
        items: Vec<String>,
        current: &str,
        width: ComboWidth,
        on: impl Fn(&mut Shell, &str, &mut Context<Shell>) + 'static,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.combo_field(
            cx,
            ComboSite {
                key,
                items: items.iter().map(|v| ComboItem::plain(v.clone())).collect(),
                value: Some(current.to_string().into()),
                fallback: Some(current.to_string().into()),
                color: None,
                tip: None,
                width,
                look: ComboLook::Field,
                size: ComboSize::Medium,
                on_event: std::rc::Rc::new(move |this, ev, cx| {
                    if let ComboEvent::Change(v) = ev {
                        on(this, v, cx)
                    }
                }),
            },
        )
    }

    /// Actions sub-tab of a Response.
    pub(crate) fn render_actions_editor(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let add_id = resp.id.clone();
        let names = self.response_datasets(&resp.id);
        let n = resp.actions.len();
        let mut col = div()
            .id(crate::ui::eid(format!("actions-{}", resp.id)))
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child("Steps run in order after this Response is chosen and before the body renders. They apply together or not at all."),
                    )
                    .child(div().flex_1())
                    .child(crate::dev_id!(
                        div()
                            .id("action-add")
                            .h(px(22.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(t.border_variant)
                            .text_size(px(11.))
                            .when(names.is_empty(), |d| d.opacity(0.45))
                            .when(!names.is_empty(), |d| {
                                d.cursor_pointer()
                                    .hover(|s| s.bg(t.element_hover))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.add_dataset_action(&add_id, cx)
                                    }))
                            })
                            .child("+ Add action"),
                        "action-add",
                        Button,
                        "Add action",
                        Press(format!("action-add:{}", resp.id))
                    )),
            );
        if names.is_empty() && resp.actions.is_empty() {
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child("This Server has no Datasets. Actions change Dataset rows, so create one first.")
                    .child(crate::dev_id!(
                        div()
                            .id("action-create-dataset")
                            .h(px(22.))
                            .px(px(8.))
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(t.border_variant)
                            .text_size(px(11.))
                            .text_color(t.text)
                            .cursor_pointer()
                            .hover(|s| s.bg(t.element_hover))
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.run_action_id("new_dataset", w, cx)
                            }))
                            .child("Create Dataset…"),
                        "action-create-dataset",
                        Button,
                        "Create Dataset",
                        Action("new_dataset")
                    )),
            );
        } else if resp.actions.is_empty() {
            col = col.child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child("No actions. Add one to insert, change or delete Dataset rows; results are available to the body as action.0, action.1, ..."),
            );
        }
        for (i, a) in resp.actions.iter().enumerate() {
            col = col.child(self.render_action_row(t, resp, i, n, a, &names, cx));
        }
        col.into_any_element()
    }

    #[allow(clippy::too_many_arguments)]
    fn render_action_row(
        &self,
        t: &Theme,
        resp: &Response,
        i: usize,
        n: usize,
        a: &DatasetAction,
        names: &[String],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rid = resp.id.clone();
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
        let (r1, r2, r3, r4, r5) = (
            rid.clone(),
            rid.clone(),
            rid.clone(),
            rid.clone(),
            rid.clone(),
        );
        let (r_op, r_ds) = (rid.clone(), rid.clone());
        let mut ds_items = names.to_vec();
        if !ds_items.contains(&a.dataset) {
            ds_items.push(a.dataset.clone());
        }
        let problem = fauxfield_model::validate_action(a);
        let mut fields = div().flex().flex_row().flex_wrap().gap(px(6.));
        for f in fields_for(&a.op) {
            fields = fields.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .w(px(if matches!(f, ActionField::Fields | ActionField::Row) {
                        340.
                    } else {
                        200.
                    }))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(t.text_muted)
                            .child(f.label()),
                    )
                    .child(self.inline_field(
                        t,
                        format!("action-{}-{}-{i}", f.label().replace(' ', "-"), resp.id),
                        InlineTarget::ActionField {
                            response: resp.id.clone(),
                            row: i,
                            field: *f,
                        },
                        &field_text(a, *f),
                        f.placeholder(),
                        -1.,
                        false,
                        cx,
                    )),
            );
        }
        div()
            .id(crate::ui::eid(format!("action-row-{}-{i}", resp.id)))
            .flex()
            .flex_col()
            .gap(px(6.))
            .p(px(8.))
            .rounded(px(6.))
            .border_1()
            .border_color(t.border_variant)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        div()
                            .w(px(22.))
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(format!("{i}")),
                    )
                    .child(self.action_combo(
                        format!("action-op-{}-{i}", resp.id),
                        ACTION_OPS.iter().map(|s| s.to_string()).collect(),
                        &a.op,
                        ComboWidth::Flex,
                        move |this, v, cx| {
                            let v = v.to_string();
                            this.edit_action(&r_op, i, cx, |a| a.op = v)
                        },
                        cx,
                    ))
                    .child(self.action_combo(
                        format!("action-ds-{}-{i}", resp.id),
                        ds_items,
                        &a.dataset,
                        ComboWidth::Flex,
                        move |this, v, cx| {
                            let v = v.to_string();
                            this.edit_action(&r_ds, i, cx, |a| a.dataset = v)
                        },
                        cx,
                    ))
                    .child(crate::dev_id_meta!(
                        small(
                            format!("action-enabled-{}-{i}", resp.id),
                            "check",
                            if a.disabled {
                                "Disabled: click to enable this step"
                            } else {
                                "Enabled: click to skip this step"
                            },
                            true,
                        )
                        .border_1()
                        .border_color(if a.disabled {
                            t.border_variant
                        } else {
                            t.border_focused
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.edit_action(&r1, i, cx, |a| a.disabled = !a.disabled)
                        })),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        format!("action-enabled-{}-{i}", resp.id),
                        Button,
                        "Enable action",
                        Press(format!("action-enabled:{}:{i}", resp.id))
                    ))
                    .child(crate::dev_id_meta!(
                        small(
                            format!("action-up-{}-{i}", resp.id),
                            "arrow-up",
                            "Move action up",
                            i > 0
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if i > 0 {
                                let id = r2.clone();
                                this.store_op(cx, |s| s.move_action(&id, i, i - 1));
                            }
                        })),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        format!("action-up-{}-{i}", resp.id),
                        Button,
                        "Move action up",
                        Press(format!("action-up:{}:{i}", resp.id))
                    ))
                    .child(crate::dev_id_meta!(
                        small(
                            format!("action-down-{}-{i}", resp.id),
                            "arrow-down",
                            "Move action down",
                            i + 1 < n
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if i + 1 < n {
                                let id = r3.clone();
                                this.store_op(cx, |s| s.move_action(&id, i, i + 1));
                            }
                        })),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        format!("action-down-{}-{i}", resp.id),
                        Button,
                        "Move action down",
                        Press(format!("action-down:{}:{i}", resp.id))
                    ))
                    .child(crate::dev_id_meta!(
                        small(
                            format!("action-remove-{}-{i}", resp.id),
                            "x",
                            "Remove action",
                            true
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let id = r4.clone();
                            this.store_op(cx, |s| s.delete_action(&id, i));
                        })),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        format!("action-remove-{}-{i}", resp.id),
                        Button,
                        "Remove action",
                        Press(format!("action-remove:{}:{i}", resp.id))
                    )),
            )
            .child(fields)
            .when_some(problem, |d, m| {
                d.child(div().text_size(px(11.)).text_color(t.modified).child(m))
            })
            .when(i + 1 == n, |d| {
                let _ = &r5;
                d
            })
            .into_any_element()
    }

    /// The Dataset body source form: Dataset, mode, filter, sort, limit and a Generate button.
    pub(crate) fn render_dataset_source(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let names = self.response_datasets(&resp.id);
        let cur = self
            .ds_read
            .get(&resp.id)
            .cloned()
            .unwrap_or_else(|| DsRead {
                dataset: names.first().cloned().unwrap_or_default(),
                mode: "all".into(),
                ..Default::default()
            });
        let (rd, rm, rg) = (resp.id.clone(), resp.id.clone(), resp.id.clone());
        let (c1, c2) = (cur.clone(), cur.clone());
        let label = READ_MODES
            .iter()
            .find(|(k, _)| *k == cur.mode)
            .map_or("All rows", |(_, l)| *l);
        let mut row = div()
            .flex_none()
            .px(px(12.))
            .py(px(6.))
            .flex()
            .flex_row()
            .flex_wrap()
            .items_end()
            .gap(px(8.))
            .child(div().w(px(180.)).child(self.action_combo(
                format!("dsread-ds-{}", resp.id),
                names.clone(),
                &cur.dataset,
                ComboWidth::Flex,
                move |this, v, cx| {
                    let mut f = c1.clone();
                    f.dataset = v.to_string();
                    this.ds_read.insert(rd.clone(), f);
                    cx.notify();
                },
                cx,
            )))
            .child(div().w(px(160.)).child(self.action_combo(
                format!("dsread-mode-{}", resp.id),
                READ_MODES.iter().map(|(_, l)| l.to_string()).collect(),
                label,
                ComboWidth::Flex,
                move |this, v, cx| {
                    let mut f = c2.clone();
                    f.mode = READ_MODES
                        .iter()
                        .find(|(_, l)| *l == v)
                        .map_or("all", |(k, _)| *k)
                        .to_string();
                    this.ds_read.insert(rm.clone(), f);
                    cx.notify();
                },
                cx,
            )));
        let fields: &[(&str, &str, &str, &str)] = match cur.mode.as_str() {
            "one" => &[("filter", "key", "id={{ request.params.id }}", "")],
            "filtered" => &[
                ("filter", "filter", "role=admin&age_gte=18", ""),
                ("sort", "sort", "-created", ""),
                ("limit", "limit", "10", ""),
            ],
            "count" => &[("filter", "filter", "status=open", "")],
            "latest" => &[("sort", "sort", "-id", "")],
            _ => &[("sort", "sort", "name", ""), ("limit", "limit", "all", "")],
        };
        for (field, lbl, ph, _) in fields {
            let value = match *field {
                "filter" => cur.filter.clone(),
                "sort" => cur.sort.clone(),
                _ => cur.limit.clone(),
            };
            row = row.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .w(px(200.))
                    .child(
                        div()
                            .text_size(px(10.))
                            .text_color(t.text_muted)
                            .child(*lbl),
                    )
                    .child(self.inline_field(
                        t,
                        format!("dsread-{field}-{}", resp.id),
                        InlineTarget::DsRead {
                            response: resp.id.clone(),
                            field,
                        },
                        &value,
                        ph,
                        -1.,
                        false,
                        cx,
                    )),
            );
        }
        row.child(crate::dev_id!(
            div()
                .id("dsread-generate")
                .h(px(24.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .bg(t.element_selected)
                .text_size(px(12.))
                .cursor_pointer()
                .hover(|s| s.bg(t.element_hover))
                .on_click(
                    cx.listener(move |this, _, _, cx| { this.generate_dataset_body(&rg, cx) })
                )
                .child("Generate template"),
            "dsread-generate",
            Button,
            "Generate template",
            Press(format!("dsread-generate:{}", resp.id))
        ))
        .into_any_element()
    }

    /// Show or hide the Dataset body source form of a Response.
    pub(crate) fn toggle_dataset_source(&mut self, resp: &str, cx: &mut Context<Self>) {
        if self.ds_read.remove(resp).is_none() {
            let names = self.response_datasets(resp);
            self.ds_read.insert(
                resp.to_string(),
                DsRead {
                    dataset: names.first().cloned().unwrap_or_default(),
                    mode: "all".into(),
                    ..Default::default()
                },
            );
        }
        cx.notify();
    }

    pub(crate) fn commit_ds_read_field(&mut self, resp: &str, field: &str, v: &str) {
        let names = self.response_datasets(resp);
        let e = self
            .ds_read
            .entry(resp.to_string())
            .or_insert_with(|| DsRead {
                dataset: names.first().cloned().unwrap_or_default(),
                mode: "all".into(),
                ..Default::default()
            });
        match field {
            "filter" => e.filter = v.to_string(),
            "sort" => e.sort = v.to_string(),
            _ => e.limit = v.to_string(),
        }
    }

    #[cfg(feature = "dev-mcp")]
    pub(crate) fn action_press(
        &mut self,
        name: &str,
        id: &str,
        idx: Option<usize>,
        cx: &mut Context<Self>,
        _w: &mut gpui::Window,
    ) -> Result<(), String> {
        let need = || idx.ok_or_else(|| format!("{name} needs an index"));
        let rid = id.to_string();
        match name {
            "action-add" => self.add_dataset_action(id, cx),
            "action-enabled" => self.edit_action(id, need()?, cx, |a| a.disabled = !a.disabled),
            "action-up" | "action-down" => {
                let i = need()?;
                let to = if name == "action-up" {
                    i.saturating_sub(1)
                } else {
                    i + 1
                };
                self.store_op(cx, |s| s.move_action(&rid, i, to));
            }
            "action-remove" => {
                let i = need()?;
                self.store_op(cx, |s| s.delete_action(&rid, i));
            }
            "dsread-generate" => self.generate_dataset_body(id, cx),
            "body-source-dataset" => self.toggle_dataset_source(id, cx),
            _ => return Err(format!("unknown action op {name:?}")),
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tera_strings_concatenate_request_values() {
        assert_eq!(
            tera_string("id={{ request.params.id }}"),
            "\"id=\" ~ (request.params.id)"
        );
        assert_eq!(tera_string("a=1"), "\"a=1\"");
    }

    #[test]
    fn read_templates_by_mode() {
        let mut r = DsRead {
            dataset: "orders".into(),
            mode: "all".into(),
            ..Default::default()
        };
        assert_eq!(
            read_template(&r, "id"),
            "{{ data.orders | json_encode() | safe }}"
        );
        r.mode = "latest".into();
        assert_eq!(
            read_template(&r, "id"),
            "{{ dataset_rows(data=data.orders, sort=\"id\", order=\"desc\", limit=1) | first | json_encode() | safe }}"
        );
        r.mode = "count".into();
        r.filter = "status=open".into();
        assert_eq!(
            read_template(&r, "id"),
            "{{ dataset_count(data=data.orders, filter=\"status=open\") }}"
        );
    }

    #[test]
    fn field_text_round_trips() {
        let mut a = DatasetAction::default();
        set_field_text(&mut a, ActionField::Fields, "sku={{ x }}; qty=2");
        assert_eq!(field_text(&a, ActionField::Fields), "sku={{ x }}; qty=2");
        set_field_text(&mut a, ActionField::Filter, "qty_gte=5&a=b");
        assert_eq!(a.filter.len(), 2);
    }
}
