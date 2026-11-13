//! The Add column modal (M10.07): name, type, constraints, default, generator and description in
//! one dialog. It calls [`Shell::add_column_with`], the code the prompt used.

use gpui::{
    Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::schema_draft::{ColDraft, SchemaDraft, validate_schema_draft};
use crate::shell::Shell;
use crate::text_input::{InputEvent, InputSize, TextInput};
use crate::theme::Theme;
use crate::ui::{eid, toolbar_button};

pub struct ColumnDialog {
    pub key: String,
    pub name: Entity<TextInput>,
    pub default: Entity<TextInput>,
    pub desc: Entity<TextInput>,
    pub ty: String,
    pub generator: String,
    pub primary_key: bool,
    pub unique: bool,
    pub required: bool,
    pub submitted: bool,
    pub(crate) focused: Option<Entity<TextInput>>,
    _subs: Vec<Subscription>,
}

impl Shell {
    pub(crate) fn begin_add_column(&mut self, w: &mut Window, cx: &mut Context<Self>) {
        let Some(key) = self.active_dataset_key() else {
            return;
        };
        let mk = |cx: &mut Context<Shell>, ph: &str| {
            cx.new(|cx| {
                TextInput::new(cx)
                    .size(InputSize::Small)
                    .placeholder(ph.to_string())
            })
        };
        let (name, default, desc) = (mk(cx, "e.g. email"), mk(cx, "optional"), mk(cx, "optional"));
        let mut subs = vec![];
        for i in [&name, &default, &desc] {
            let me = i.clone();
            subs.push(
                cx.subscribe_in(
                    i,
                    w,
                    move |this: &mut Shell, _, ev: &InputEvent, w, cx| match ev {
                        InputEvent::PressEnter { .. } => this.column_op("add", w, cx),
                        InputEvent::Escape => this.column_op("cancel", w, cx),
                        InputEvent::Tab { reverse } => this.column_tab(*reverse, w, cx),
                        InputEvent::Focus => {
                            if let Some(d) = this.column_dialog.as_mut() {
                                d.focused = Some(me.clone());
                            }
                        }
                        InputEvent::Change => cx.notify(),
                        _ => {}
                    },
                ),
            );
        }
        name.read(cx).focus(w);
        self.column_dialog = Some(ColumnDialog {
            key,
            name,
            default,
            desc,
            ty: "string".into(),
            generator: String::new(),
            primary_key: false,
            unique: false,
            required: false,
            submitted: false,
            focused: None,
            _subs: subs,
        });
        cx.notify();
    }

    fn column_tab(&mut self, reverse: bool, w: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.column_dialog.as_ref() else {
            return;
        };
        let all = [d.name.clone(), d.default.clone(), d.desc.clone()];
        let cur = d
            .focused
            .as_ref()
            .and_then(|f| all.iter().position(|x| x == f))
            .unwrap_or(0);
        let n = all.len();
        let next = all[(cur + if reverse { n - 1 } else { 1 }) % n].clone();
        next.update(cx, |t, cx| {
            t.focus(w);
            t.select_all(cx);
        });
    }

    /// The new column as the draft row, and the issues it has next to the existing columns.
    fn column_issues(&self, cx: &gpui::App) -> (ColDraft, Vec<crate::schema_draft::Issue>) {
        let Some(d) = self.column_dialog.as_ref() else {
            return (ColDraft::new("", "string"), vec![]);
        };
        let new = ColDraft {
            name: d.name.read(cx).value().to_string(),
            ty: d.ty.clone(),
            primary_key: d.primary_key,
            unique: d.unique,
            required: d.required,
            default: d.default.read(cx).value().to_string(),
            generator: d.generator.clone(),
            description: d.desc.read(cx).value().to_string(),
        };
        let existing: Vec<ColDraft> = self
            .ws
            .find_dataset(&d.key)
            .map(|x| {
                if x.column_defs.is_empty() {
                    x.columns.iter().map(|(n, t)| ColDraft::new(n, t)).collect()
                } else {
                    x.column_defs.iter().map(ColDraft::from_column).collect()
                }
            })
            .unwrap_or_default();
        let at = existing.len();
        let mut columns = existing;
        columns.push(new.clone());
        let draft = SchemaDraft {
            name: "x".into(),
            server: "x".into(),
            columns,
            rows: "1".into(),
            ..Default::default()
        };
        let issues = validate_schema_draft(&draft, &[], true)
            .into_iter()
            .filter(|i| matches!(i.at, crate::schema_draft::Where::Column(c, _) if c == at))
            .collect();
        (new, issues)
    }

    pub(crate) fn column_op(&mut self, op: &str, w: &mut Window, cx: &mut Context<Self>) {
        match op {
            "cancel" => {
                self.column_dialog = None;
                w.focus(&self.focus);
            }
            "pk" | "unique" | "required" => {
                if let Some(d) = self.column_dialog.as_mut() {
                    match op {
                        "pk" => d.primary_key = !d.primary_key,
                        "unique" => d.unique = !d.unique,
                        _ => d.required = !d.required,
                    }
                }
            }
            "add" => {
                if let Some(d) = self.column_dialog.as_mut() {
                    d.submitted = true;
                }
                let (new, issues) = self.column_issues(cx);
                if !issues.is_empty() || new.name.trim().is_empty() {
                    return cx.notify();
                }
                let Some(d) = self.column_dialog.take() else {
                    return;
                };
                w.focus(&self.focus);
                let col = crate::schema_draft::to_columns(&SchemaDraft {
                    columns: vec![new.clone()],
                    ..Default::default()
                })
                .remove(0);
                let spec = crate::dataset_edit::spec_from_text(new.generator.trim()).ok();
                self.add_column_with(&d.key, col, spec, cx);
            }
            _ => {}
        }
        cx.notify();
    }

    pub(crate) fn render_column_dialog(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let d = self.column_dialog.as_ref().expect("dialog is open");
        let (new, issues) = self.column_issues(cx);
        let show = |m: &str| d.submitted || m != "Column name is empty.";
        let msgs: Vec<String> = issues
            .iter()
            .map(|i| i.message.clone())
            .filter(|m| show(m))
            .collect();
        let label = |s: &'static str| {
            div()
                .w(px(90.))
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(s)
        };
        let ty = self.combo_field(
            cx,
            ComboSite {
                key: "column-type".into(),
                items: fauxfield_model::COLUMN_TYPES
                    .iter()
                    .map(|x| ComboItem::plain(x.to_string()))
                    .collect(),
                value: Some(d.ty.clone().into()),
                fallback: Some(d.ty.clone().into()),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(140.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(|this, ev, _| {
                    if let ComboEvent::Change(v) = ev
                        && let Some(d) = this.column_dialog.as_mut()
                    {
                        d.ty = v.to_string();
                    }
                }),
            },
        );
        let gen_combo = self.combo_field(
            cx,
            ComboSite {
                key: "column-generator".into(),
                items: crate::schema_dialog::generator_items(),
                value: Some(d.generator.clone().into()),
                fallback: Some("(infer)".into()),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(220.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(|this, ev, _| {
                    if let ComboEvent::Change(v) = ev
                        && let Some(d) = this.column_dialog.as_mut()
                    {
                        d.generator = v.to_string();
                    }
                }),
            },
        );
        let toggle = |op: &'static str, on: bool, text: &'static str| {
            let id = format!("column-{op}");
            let b = toolbar_button(t, eid(id.clone()))
                .h(px(22.))
                .px(px(8.))
                .when(on, |b| b.bg(t.element_selected))
                .child(format!("{} {text}", if on { "☑" } else { "☐" }))
                .on_click(cx.listener(move |this, _, w, cx| this.column_op(op, w, cx)));
            crate::dev_id!(b, id, Button, text, Press(format!("column-{op}")))
        };
        let field = |l: &'static str, e: Entity<TextInput>, id: &'static str| {
            let _ = id;
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(label(l))
                .child(crate::dev_id!(div().flex_1().child(e), id, Input, l, None))
        };
        let ok = msgs.is_empty() && !new.name.trim().is_empty();
        let add = toolbar_button(t, eid("column-add"))
            .h(px(26.))
            .px(px(12.))
            .when(ok, |b| b.bg(t.element_selected))
            .when(!ok, |b| b.opacity(0.5))
            .child("Add column")
            .on_click(cx.listener(|this, _, w, cx| this.column_op("add", w, cx)));
        let cancel = toolbar_button(t, eid("column-cancel"))
            .h(px(26.))
            .px(px(12.))
            .child("Cancel")
            .on_click(cx.listener(|this, _, w, cx| this.column_op("cancel", w, cx)));
        crate::dev_id_z!(
            div()
                .id("column-backdrop")
                .absolute()
                .inset_0()
                .flex()
                .flex_col()
                .items_center()
                .pt(px(90.))
                .bg(t.background.opacity(0.6))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("column-dialog")
                        .key_context("Dialog")
                        .track_focus(&self.column_dialog_focus)
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            match ev.keystroke.key.as_str() {
                                "escape" => this.column_op("cancel", w, cx),
                                "enter" => this.column_op("add", w, cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .w(px(520.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(8.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .shadow_lg()
                        .child(div().text_size(px(14.)).child("Add column"))
                        .child(field("Name", d.name.clone(), "column-name"))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(label("Type"))
                                .child(ty)
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .child(label("Constraints"))
                                .child(toggle("pk", d.primary_key, "Primary key"))
                                .child(toggle("unique", d.unique, "Unique"))
                                .child(toggle("required", d.required, "Required")),
                        )
                        .child(field("Default", d.default.clone(), "column-default"))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(label("Fills rows"))
                                .child(gen_combo),
                        )
                        .child(field("Description", d.desc.clone(), "column-description"))
                        .child(div().text_size(px(11.)).text_color(t.text_muted).child(
                            "Existing rows get a value from the generator (inferred when not set).",
                        ))
                        .children(
                            msgs.into_iter().map(|m| {
                                div().text_size(px(11.)).text_color(t.deleted).child(m)
                            })
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(
                                    cancel,
                                    "column-cancel",
                                    Button,
                                    "Cancel",
                                    Press("column-cancel".into())
                                ))
                                .child(crate::dev_id!(
                                    add,
                                    "column-add",
                                    Button,
                                    "Add column",
                                    Press("column-add".into())
                                )),
                        ),
                ),
            2,
            "column-dialog",
            Modal,
            "Add column",
            None
        )
    }
}
