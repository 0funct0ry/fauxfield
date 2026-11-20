//! The Callback modal (M14): create, view (read-only) and edit one Callback of a Response. It
//! edits a draft; nothing is written until Save. Verb, URL and the numeric settings are fields,
//! headers use a key/value editor with the header completions of the Headers tab, the body is a
//! [`CodeEditor`] (Tera completions, highlighting, format on paste) and the preview of what the
//! receiver gets sits to the right of it.

use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    ParentElement, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::code_editor::tera_ctx::TeraCompletions;
use crate::code_editor::{CodeEditor, EditorEvent, Language};
use crate::combo_box::{ComboEvent, ComboItem, ComboLook, ComboSite, ComboSize, ComboWidth};
use crate::header_catalog::{self, Col};
use crate::shell::Shell;
use crate::text_input::{InputEvent, InputSize, TextInput};
use crate::theme::Theme;
use crate::ui::{eid, icon_button, toolbar_button};
use fauxfield_model::{CALLBACK_MAX_RETRIES, CALLBACK_METHODS, Callback};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CbMode {
    Create,
    Edit,
    View,
}

pub struct KvRow {
    pub key: Entity<TextInput>,
    pub value: Entity<TextInput>,
}

pub struct CallbackDialog {
    pub response: String,
    /// Index of the Callback being edited or viewed; `None` when creating.
    pub index: Option<usize>,
    pub mode: CbMode,
    pub verb: String,
    pub disabled: bool,
    pub id: Entity<TextInput>,
    pub name: Entity<TextInput>,
    pub description: Entity<TextInput>,
    pub url: Entity<TextInput>,
    pub delay: Entity<TextInput>,
    pub retries: Entity<TextInput>,
    pub backoff: Entity<TextInput>,
    pub timeout: Entity<TextInput>,
    pub headers: Vec<KvRow>,
    pub body: Entity<CodeEditor>,
    pub submitted: bool,
    /// Bumped by *Render again* so the preview renders anew.
    pub preview_nonce: u32,
    /// Field whose help popover is open.
    pub help: Option<&'static str>,
    pub(crate) focused: Option<Entity<TextInput>>,
    subs: Vec<Subscription>,
}

impl CallbackDialog {
    fn scalar_inputs(&self) -> Vec<Entity<TextInput>> {
        vec![
            self.id.clone(),
            self.name.clone(),
            self.url.clone(),
            self.delay.clone(),
            self.retries.clone(),
            self.backoff.clone(),
            self.timeout.clone(),
            self.description.clone(),
        ]
    }

    fn all_inputs(&self) -> Vec<Entity<TextInput>> {
        let mut v = self.scalar_inputs();
        for r in &self.headers {
            v.push(r.key.clone());
            v.push(r.value.clone());
        }
        v
    }
}

fn num(t: &str) -> Result<Option<u64>, ()> {
    let t = t.trim();
    if t.is_empty() {
        Ok(None)
    } else {
        t.parse::<u64>().map(Some).map_err(|_| ())
    }
}

/// What the form says: the Callback, and the problems with it (empty when it can be saved).
fn draft_of(d: &CallbackDialog, cx: &gpui::App, taken: &[String]) -> (Callback, Vec<String>) {
    let mut problems = vec![];
    let text = |e: &Entity<TextInput>| e.read(cx).value().trim().to_string();
    let id = text(&d.id);
    if id.is_empty() {
        problems.push("Id is empty.".to_string());
    } else if taken.contains(&id) {
        problems.push(format!("Another callback already uses the id `{id}`."));
    }
    let url = text(&d.url);
    if url.is_empty() {
        problems.push("URL is empty.".into());
    } else if !(url.contains("{{")
        || url.contains("{%")
        || url.starts_with("http://")
        || url.starts_with("https://"))
    {
        problems.push("URL must start with http:// or https:// (or be a template).".into());
    }
    let mut n = |label: &str, e: &Entity<TextInput>| match num(e.read(cx).value()) {
        Ok(v) => v,
        Err(()) => {
            problems.push(format!("{label} must be a whole number."));
            None
        }
    };
    let delay = n("Delay", &d.delay);
    let retries = n("Retries", &d.retries);
    let backoff = n("Backoff", &d.backoff);
    let timeout = n("Timeout", &d.timeout);
    if retries.is_some_and(|r| r > CALLBACK_MAX_RETRIES as u64) {
        problems.push(format!("Retries is at most {CALLBACK_MAX_RETRIES}."));
    }
    let mut headers = indexmap::IndexMap::new();
    for r in &d.headers {
        let k = crate::header_table::clean_key(r.key.read(cx).value());
        if !k.is_empty() {
            headers.insert(k, r.value.read(cx).value().trim().to_string());
        }
    }
    let body = d.body.read(cx).text().to_string();
    let some = |s: String| Some(s).filter(|s| !s.is_empty());
    (
        Callback {
            id,
            name: some(text(&d.name)),
            description: some(text(&d.description)),
            method: Some(d.verb.clone()).filter(|m| m != "POST"),
            url,
            delay_ms: delay.filter(|v| *v > 0),
            headers,
            body: Some(body).filter(|b| !b.trim().is_empty()),
            retries: retries.filter(|v| *v > 0).map(|v| v as u32),
            backoff_ms: backoff.filter(|v| *v > 0),
            timeout_ms: timeout.filter(|v| *v > 0),
            disabled: d.disabled,
            ..Default::default()
        },
        problems,
    )
}

impl Shell {
    fn cb_input(
        &self,
        cx: &mut Context<Self>,
        value: &str,
        placeholder: &str,
        readonly: bool,
        subs: &mut Vec<Subscription>,
        w: &mut Window,
    ) -> Entity<TextInput> {
        let e = cx.new(|cx| {
            TextInput::new(cx)
                .size(InputSize::Small)
                .placeholder(placeholder.to_string())
                .text(value)
                .readonly(readonly)
        });
        let me = e.clone();
        subs.push(cx.subscribe_in(
            &e,
            w,
            move |this: &mut Shell, _, ev: &InputEvent, w, cx| match ev {
                InputEvent::PressEnter { .. } => this.callback_dialog_op("save", w, cx),
                InputEvent::Escape => this.callback_dialog_op("cancel", w, cx),
                InputEvent::Tab { reverse } => this.callback_tab(*reverse, w, cx),
                InputEvent::Focus => {
                    if let Some(d) = this.callback_dialog.as_mut() {
                        d.focused = Some(me.clone());
                    }
                    cx.notify();
                }
                InputEvent::Change => cx.notify(),
                _ => {}
            },
        ));
        e
    }

    /// Open the modal for `index` of `response` (or a new Callback with `None`).
    pub(crate) fn begin_callback_dialog(
        &mut self,
        response: &str,
        index: Option<usize>,
        mode: CbMode,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, _, resp)) = self.find_response(response) else {
            return;
        };
        let c = index
            .and_then(|i| resp.callbacks.get(i).cloned())
            .unwrap_or_else(|| Callback {
                id: self.next_callback_id(response),
                url: "http://127.0.0.1:9000/webhook".into(),
                delay_ms: Some(1000),
                body: Some("{\"event\":\"example\"}".into()),
                ..Default::default()
            });
        let ro = mode == CbMode::View;
        let mut subs = vec![];
        let ms = |v: Option<u64>| v.map(|v| v.to_string()).unwrap_or_default();
        let id = self.cb_input(cx, &c.id, "", ro, &mut subs, w);
        let name = self.cb_input(cx, c.name.as_deref().unwrap_or(""), "", ro, &mut subs, w);
        let description = self.cb_input(
            cx,
            c.description.as_deref().unwrap_or(""),
            "",
            ro,
            &mut subs,
            w,
        );
        let url = self.cb_input(cx, &c.url, "http(s) URL, or a template", ro, &mut subs, w);
        let delay = self.cb_input(cx, &ms(c.delay_ms), "0", ro, &mut subs, w);
        let retries = self.cb_input(
            cx,
            &c.retries.map(|v| v.to_string()).unwrap_or_default(),
            "0",
            ro,
            &mut subs,
            w,
        );
        let backoff = self.cb_input(cx, &ms(c.backoff_ms), "500", ro, &mut subs, w);
        let timeout = self.cb_input(cx, &ms(c.timeout_ms), "10000", ro, &mut subs, w);
        let headers: Vec<KvRow> = c
            .headers
            .iter()
            .map(|(k, v)| self.new_kv_row(cx, k, v, ro, &mut subs, w))
            .collect();
        let vars = self.variable_names_for(response);
        let settings = self.editor_settings.clone();
        let ct = c
            .headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "application/json".into());
        let lang = Language::detect(&ct, c.body.as_deref().unwrap_or(""));
        let body_text = c.body.clone().unwrap_or_default();
        let body = cx.new(|cx| {
            CodeEditor::new(lang, cx)
                .completion_provider(TeraCompletions { variables: vars })
                .format_paste(true)
                .read_only(ro)
                .placeholder(
                    "Callback body: JSON, text or a template such as {{ request.json.id }}",
                )
        });
        body.update(cx, |e, cx| {
            e.set_text(&body_text, cx);
            e.apply_settings(&settings, cx);
        });
        subs.push(cx.subscribe(&body, |this, _, ev: &EditorEvent, cx| {
            if matches!(ev, EditorEvent::Changed) {
                this.callback_body_changed(cx);
            }
        }));
        if !ro {
            url.read(cx).focus(w);
        } else {
            w.focus(&self.callback_dialog_focus);
        }
        self.callback_dialog = Some(CallbackDialog {
            response: response.into(),
            index,
            mode,
            verb: c
                .method
                .clone()
                .unwrap_or_else(|| "POST".into())
                .to_ascii_uppercase(),
            disabled: c.disabled,
            id,
            name,
            description,
            url,
            delay,
            retries,
            backoff,
            timeout,
            headers,
            body,
            submitted: false,
            preview_nonce: 0,
            help: None,
            focused: None,
            subs,
        });
        cx.notify();
    }

    fn callback_body_changed(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.callback_dialog.as_ref() else {
            return;
        };
        let ct = d
            .headers
            .iter()
            .find(|r| r.key.read(cx).value().eq_ignore_ascii_case("content-type"))
            .map(|r| r.value.read(cx).value().to_string())
            .unwrap_or_else(|| "application/json".into());
        let body = d.body.clone();
        body.update(cx, |e, cx| {
            let lang = Language::detect(&ct, e.text());
            e.set_language(lang, cx);
        });
        cx.notify();
    }

    fn new_kv_row(
        &self,
        cx: &mut Context<Self>,
        key: &str,
        value: &str,
        ro: bool,
        subs: &mut Vec<Subscription>,
        w: &mut Window,
    ) -> KvRow {
        KvRow {
            key: self.cb_input(cx, key, "Header name", ro, subs, w),
            value: self.cb_input(cx, value, "Value (a template)", ro, subs, w),
        }
    }

    fn callback_tab(&mut self, reverse: bool, w: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.callback_dialog.as_ref() else {
            return;
        };
        let all = d.all_inputs();
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

    fn callback_problems(&self, cx: &gpui::App) -> (Callback, Vec<String>) {
        let d = self.callback_dialog.as_ref().expect("dialog is open");
        let taken: Vec<String> = self
            .find_response(&d.response)
            .map(|(_, _, r)| {
                r.callbacks
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| Some(*i) != d.index)
                    .map(|(_, c)| c.id.clone())
                    .collect()
            })
            .unwrap_or_default();
        draft_of(d, cx, &taken)
    }

    pub(crate) fn callback_dialog_op(&mut self, op: &str, w: &mut Window, cx: &mut Context<Self>) {
        match op {
            "cancel" => {
                self.callback_dialog = None;
                w.focus(&self.cb_table_focus);
            }
            "edit" => {
                // View → Edit: reopen the same Callback in edit mode.
                if let Some(d) = self.callback_dialog.take() {
                    self.begin_callback_dialog(&d.response, d.index, CbMode::Edit, w, cx);
                }
            }
            "render_again" => {
                if let Some(d) = self.callback_dialog.as_mut() {
                    d.preview_nonce += 1;
                }
            }
            "enabled" => {
                if let Some(d) = self.callback_dialog.as_mut()
                    && d.mode != CbMode::View
                {
                    d.disabled = !d.disabled;
                }
            }
            "add_header" => {
                if self
                    .callback_dialog
                    .as_ref()
                    .is_some_and(|d| d.mode != CbMode::View)
                {
                    let mut subs = vec![];
                    let row = self.new_kv_row(cx, "", "", false, &mut subs, w);
                    let key = row.key.clone();
                    if let Some(d) = self.callback_dialog.as_mut() {
                        d.headers.push(row);
                        d.subs.append(&mut subs);
                    }
                    key.read(cx).focus(w);
                }
            }
            "save" => {
                if self
                    .callback_dialog
                    .as_ref()
                    .is_some_and(|d| d.mode == CbMode::View)
                {
                    return;
                }
                if let Some(d) = self.callback_dialog.as_mut() {
                    d.submitted = true;
                }
                let (c, problems) = self.callback_problems(cx);
                if !problems.is_empty() {
                    return cx.notify();
                }
                let Some(d) = self.callback_dialog.take() else {
                    return;
                };
                w.focus(&self.cb_table_focus);
                let (resp, idx) = (d.response.clone(), d.index);
                let at = self.store_op(cx, |s| s.set_callback(&resp, idx, &c));
                if let Some(at) = at {
                    self.cb_selected.insert(d.response, at);
                }
            }
            other => {
                if let Some(rest) = other.strip_prefix("del_header:")
                    && let Ok(i) = rest.parse::<usize>()
                    && let Some(d) = self.callback_dialog.as_mut()
                    && d.mode != CbMode::View
                    && i < d.headers.len()
                {
                    d.headers.remove(i);
                }
            }
        }
        cx.notify();
    }

    fn cb_verb_combo(&self, t: &Theme, d: &CallbackDialog, cx: &mut Context<Self>) -> AnyElement {
        if d.mode == CbMode::View {
            return div()
                .h(px(26.))
                .w(px(110.))
                .flex()
                .items_center()
                .px(px(8.))
                .rounded(px(4.))
                .border_1()
                .border_color(t.border_variant)
                .font_family(BUFFER_FONT)
                .text_size(px(12.))
                .text_color(t.method_color(&d.verb))
                .child(d.verb.clone())
                .into_any_element();
        }
        self.combo_field(
            cx,
            ComboSite {
                key: "callback-verb".into(),
                items: CALLBACK_METHODS
                    .iter()
                    .map(|v| ComboItem::plain(v.to_string()))
                    .collect(),
                value: Some(d.verb.clone().into()),
                fallback: Some(d.verb.clone().into()),
                color: None,
                tip: None,
                width: ComboWidth::Fixed(110.),
                look: ComboLook::Field,
                size: ComboSize::Small,
                on_event: std::rc::Rc::new(|this, ev, _| {
                    if let ComboEvent::Change(v) = ev
                        && let Some(d) = this.callback_dialog.as_mut()
                    {
                        d.verb = v.to_string();
                    }
                }),
            },
        )
    }

    /// The `?` button of a field and, while open, its popover (a card under the button).
    fn cb_help(
        &self,
        t: &Theme,
        d: &CallbackDialog,
        key: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let open = d.help == Some(key);
        let (title, text) = help_text(key);
        let id = format!("callback-help-{key}");
        div()
            .relative()
            .flex_none()
            .child(crate::dev_id!(
                div()
                    .id(eid(id.clone()))
                    .size(px(16.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .border_1()
                    .border_color(if open { t.border_focused } else { t.border })
                    .text_size(px(10.))
                    .text_color(t.text_muted)
                    .cursor_pointer()
                    .hover(|s| s.bg(t.element_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            if let Some(d) = this.callback_dialog.as_mut() {
                                d.help = if d.help == Some(key) { None } else { Some(key) };
                            }
                            cx.notify();
                            cx.stop_propagation();
                        }),
                    )
                    .child(self.popover_trigger_bounds(&id))
                    .child("?"),
                id.clone(),
                Button,
                format!("Help: {title}"),
                Press(id.clone())
            ))
            .when(open, |d| {
                let content = div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(div().text_size(px(13.)).child(title))
                    .child(div().h(px(1.)).bg(t.border_variant))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(text),
                    )
                    .into_any_element();
                let me = cx.entity();
                d.child(self.popover_card(t, &id, 340., content, move |_, cx| {
                    me.update(cx, |this, cx| {
                        if let Some(d) = this.callback_dialog.as_mut()
                            && d.help == Some(key)
                        {
                            d.help = None;
                            cx.notify();
                        }
                    });
                }))
            })
            .into_any_element()
    }

    /// Header-name or value suggestions for the focused header cell.
    fn cb_suggestions(
        &self,
        cx: &gpui::App,
    ) -> Option<(Entity<TextInput>, Vec<header_catalog::Suggestion>, bool)> {
        let d = self.callback_dialog.as_ref()?;
        if d.mode == CbMode::View {
            return None;
        }
        let f = d.focused.as_ref()?;
        let (row, col) = d.headers.iter().enumerate().find_map(|(i, r)| {
            if &r.key == f {
                Some((i, Col::Key))
            } else if &r.value == f {
                Some((i, Col::Value))
            } else {
                None
            }
        })?;
        let r = &d.headers[row];
        let name = r.key.read(cx).value().to_string();
        let text = f.read(cx).value().to_string();
        let s = header_catalog::suggest(col, &name, &text);
        (!s.is_empty()).then(|| (f.clone(), s, col == Col::Key))
    }

    pub(crate) fn render_callback_dialog(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let d = self.callback_dialog.as_ref().expect("dialog is open");
        let (draft, problems) = self.callback_problems(cx);
        let ro = d.mode == CbMode::View;
        let shown: Vec<String> = problems
            .iter()
            .filter(|m| d.submitted || !m.ends_with("is empty."))
            .cloned()
            .collect();
        let title = match d.mode {
            CbMode::Create => "New callback",
            CbMode::Edit => "Edit callback",
            CbMode::View => "View callback",
        };
        let label = |s: &'static str| div().text_size(px(11.)).text_color(t.text_muted).child(s);
        let field = |l: &'static str,
                     help: AnyElement,
                     e: &Entity<TextInput>,
                     w: Option<f32>,
                     id: &'static str| {
            let _ = id;
            let col = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .child(label(l))
                        .child(help),
                )
                .child(crate::dev_id!(
                    div().w_full().child(e.clone()),
                    id,
                    Input,
                    l,
                    None
                ));
            match w {
                Some(w) => col.w(px(w)).flex_none(),
                None => col.flex_1().min_w(px(0.)),
            }
        };
        let preview = {
            let p = self.callback_preview_draft(d, &draft, cx);
            match p {
                None => ("Preview unavailable.".to_string(), false),
                Some(p) if p.error.is_some() => (
                    format!("Template error in {}", p.error.clone().unwrap_or_default()),
                    true,
                ),
                Some(p) => {
                    let mut s = format!("{} {}\n", p.method, p.url);
                    for (k, v) in &p.headers {
                        s.push_str(&format!("{k}: {v}\n"));
                    }
                    if !p.body.is_empty() {
                        s.push('\n');
                        s.push_str(&p.body);
                    }
                    (s.chars().take(4000).collect(), false)
                }
            }
        };
        // Headers key/value editor.
        let mut kv = div().flex().flex_col().gap(px(4.));
        for (i, r) in d.headers.iter().enumerate() {
            let del = icon_button(
                t,
                crate::ui::eid(format!("callback-header-del-{i}")),
                "x",
                "Remove header",
                false,
                !ro,
            )
            .on_click(cx.listener(move |this, _, w, cx| {
                this.callback_dialog_op(&format!("del_header:{i}"), w, cx)
            }));
            kv = kv.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(crate::dev_id!(
                        div().w(px(280.)).flex_none().child(r.key.clone()),
                        format!("callback-header-key-{i}"),
                        Input,
                        "Header name",
                        None
                    ))
                    .child(crate::dev_id!(
                        div().flex_1().min_w(px(0.)).child(r.value.clone()),
                        format!("callback-header-value-{i}"),
                        Input,
                        "Header value",
                        None
                    ))
                    .when(!ro, |d| {
                        d.child(crate::dev_id!(
                            del,
                            format!("callback-header-del-{i}"),
                            Button,
                            "Remove header",
                            Press(format!("callback-header-del:{i}"))
                        ))
                    }),
            );
        }
        let sugg =
            self.cb_suggestions(cx).map(|(input, list, is_key)| {
                div().flex().flex_row().flex_wrap().gap(px(4.)).children(
                    list.into_iter().take(8).map(|s| {
                        let input = input.clone();
                        let label = s.label;
                        toolbar_button(t, eid(format!("callback-suggest-{label}")))
                            .h(px(20.))
                            .px(px(6.))
                            .font_family(BUFFER_FONT)
                            .text_size(px(11.))
                            .child(label.clone())
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, w, cx| {
                                    input.update(cx, |t, cx| t.set_value(&label, cx));
                                    // A header name moves on to its value.
                                    if is_key
                                        && let Some(d) = this.callback_dialog.as_ref()
                                        && let Some(r) = d.headers.iter().find(|r| r.key == input)
                                    {
                                        r.value.read(cx).focus(w);
                                    }
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            )
                    }),
                )
            });
        let add_header = toolbar_button(t, eid("callback-add-header"))
            .h(px(22.))
            .px(px(8.))
            .child("+ Add header")
            .on_click(cx.listener(|this, _, w, cx| this.callback_dialog_op("add_header", w, cx)));
        let enabled = toolbar_button(t, eid("callback-enabled"))
            .h(px(22.))
            .px(px(8.))
            .when(!draft.disabled, |b| b.bg(t.element_selected))
            .child(format!(
                "{} Enabled",
                if draft.disabled { "☐" } else { "☑" }
            ))
            .on_click(cx.listener(|this, _, w, cx| this.callback_dialog_op("enabled", w, cx)));
        let ok = problems.is_empty();
        let save = toolbar_button(t, eid("callback-save"))
            .h(px(28.))
            .px(px(14.))
            .when(ok, |b| b.bg(t.element_selected))
            .when(!ok, |b| b.opacity(0.5))
            .child(if d.mode == CbMode::Create {
                "Add callback"
            } else {
                "Save"
            })
            .on_click(cx.listener(|this, _, w, cx| this.callback_dialog_op("save", w, cx)));
        let cancel = toolbar_button(t, eid("callback-cancel"))
            .h(px(28.))
            .px(px(14.))
            .child(if ro { "Close" } else { "Cancel" })
            .on_click(cx.listener(|this, _, w, cx| this.callback_dialog_op("cancel", w, cx)));
        let edit = toolbar_button(t, eid("callback-to-edit"))
            .h(px(28.))
            .px(px(14.))
            .bg(t.element_selected)
            .child("Edit")
            .on_click(cx.listener(|this, _, w, cx| this.callback_dialog_op("edit", w, cx)));
        let boxed = |el: AnyElement| {
            div()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .rounded(px(4.))
                .border_1()
                .border_color(t.border_variant)
                .overflow_hidden()
                .child(el)
        };
        crate::dev_id_z!(
            div()
                .id("callback-backdrop")
                .absolute()
                .inset_0()
                .p(px(24.))
                .bg(t.background.opacity(0.6))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    div()
                        .id("callback-dialog")
                        .key_context("Dialog")
                        .track_focus(&self.callback_dialog_focus)
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            let secondary = ev.keystroke.modifiers.platform || ev.keystroke.modifiers.control;
                            match ev.keystroke.key.as_str() {
                                "escape"
                                    if this
                                        .callback_dialog
                                        .as_ref()
                                        .is_some_and(|d| d.help.is_some()) =>
                                {
                                    if let Some(d) = this.callback_dialog.as_mut() {
                                        d.help = None;
                                    }
                                    cx.notify();
                                }
                                "escape" => this.callback_dialog_op("cancel", w, cx),
                                "s" if secondary => this.callback_dialog_op("save", w, cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .size_full()
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(10.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .shadow_lg()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.))
                                .child(div().text_size(px(14.)).child(title))
                                .child(div().flex_1())
                                .child(crate::dev_id!(enabled, "callback-enabled", Button, "Enabled", Press("callback-enabled".into()))),
                        )
                        // Verb, URL.
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_end()
                                .gap(px(8.))
                                .child(div().flex().flex_col().gap(px(2.)).child(label("VERB")).child(self.cb_verb_combo(t, d, cx)))
                                .child(field("URL", self.cb_help(t, d, "url", cx), &d.url, None, "callback-url")),
                        )
                        // Numbers, identity.
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(8.))
                                .child(field("ID", self.cb_help(t, d, "id", cx), &d.id, Some(200.), "callback-id"))
                                .child(field("NAME", self.cb_help(t, d, "name", cx), &d.name, Some(220.), "callback-name"))
                                .child(field("DELAY (ms)", self.cb_help(t, d, "delay", cx), &d.delay, Some(110.), "callback-delay"))
                                .child(field("RETRIES", self.cb_help(t, d, "retries", cx), &d.retries, Some(90.), "callback-retries"))
                                .child(field("BACKOFF (ms)", self.cb_help(t, d, "backoff", cx), &d.backoff, Some(110.), "callback-backoff"))
                                .child(field("TIMEOUT (ms)", self.cb_help(t, d, "timeout", cx), &d.timeout, Some(110.), "callback-timeout"))
                                .child(field("DESCRIPTION", self.cb_help(t, d, "description", cx), &d.description, None, "callback-description")),
                        )
                        // Headers: a section of its own.
                        .child(crate::dev_id!(
                            div()
                                .id("callback-headers-section")
                                .flex()
                                .flex_col()
                                .flex_none()
                                .rounded(px(6.))
                                .border_1()
                                .border_color(t.border)
                                .bg(t.panel)
                                .child(
                                    div()
                                        .h(px(34.))
                                        .px(px(10.))
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .gap(px(8.))
                                        .border_b_1()
                                        .border_color(t.border_variant)
                                        .child(crate::ui::icon("list-tree", 14., t.icon_muted))
                                        .child(div().text_size(px(13.)).child("Headers"))
                                        .child(
                                            div()
                                                .px(px(6.))
                                                .rounded(px(8.))
                                                .bg(t.element_selected)
                                                .text_size(px(10.))
                                                .child(d.headers.len().to_string()),
                                        )
                                        .child(self.cb_help(t, d, "headers", cx))
                                        .child(div().flex_1())
                                        .when(!ro, |d| {
                                            d.child(crate::dev_id!(
                                                add_header,
                                                "callback-add-header",
                                                Button,
                                                "Add header",
                                                Press("callback-add-header".into())
                                            ))
                                        }),
                                )
                                .when(d.headers.is_empty(), |s| {
                                    s.child(
                                        div()
                                            .px(px(10.))
                                            .py(px(10.))
                                            .text_size(px(12.))
                                            .text_color(t.text_muted)
                                            .child(if ro {
                                                "No headers."
                                            } else {
                                                "No headers. Content-Type is added for a JSON body. Click Add header to send more, for example a signature."
                                            }),
                                    )
                                })
                                .when(!d.headers.is_empty(), |s| {
                                    s.child(
                                        div()
                                            .px(px(10.))
                                            .pt(px(6.))
                                            .flex()
                                            .flex_row()
                                            .gap(px(6.))
                                            .text_size(px(10.))
                                            .text_color(t.text_muted)
                                            .child(div().w(px(280.)).flex_none().child("NAME"))
                                            .child(div().flex_1().child("VALUE")),
                                    )
                                    .child(
                                        div()
                                            .id("callback-headers")
                                            .max_h(px(150.))
                                            .overflow_y_scroll()
                                            .px(px(10.))
                                            .py(px(6.))
                                            .child(kv),
                                    )
                                })
                                .children(sugg.map(|g| div().px(px(10.)).pb(px(8.)).child(g))),
                            "callback-headers-section",
                            Text,
                            format!("Headers ({})", d.headers.len()),
                            None
                        ))
                        // Body | Preview.
                        .child(
                            div()
                                .flex_1()
                                .min_h(px(0.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .gap(px(10.))
                                        .child(div().flex_1().child(label("BODY")))
                                        .child(
                                            div()
                                                .flex_1()
                                                .flex()
                                                .flex_row()
                                                .items_center()
                                                .gap(px(8.))
                                                .child(label("PREVIEW (what the receiver gets; uses the latest recorded request of this Route)"))
                                                .child(crate::dev_id!(
                                                    toolbar_button(t, eid("callback-render-again"))
                                                        .h(px(20.))
                                                        .px(px(6.))
                                                        .text_size(px(11.))
                                                        .child("Render again")
                                                        .on_click(cx.listener(|this, _, w, cx| {
                                                            this.callback_dialog_op("render_again", w, cx)
                                                        })),
                                                    "callback-render-again",
                                                    Button,
                                                    "Render again",
                                                    Press("callback-render-again".into())
                                                )),
                                        ),
                                )
                                .child(
                                    div()
                                        .flex_1()
                                        .min_h(px(0.))
                                        .flex()
                                        .flex_row()
                                        .gap(px(10.))
                                        .child(boxed(crate::dev_id!(
                                            div().id("callback-body").size_full().child(d.body.clone()),
                                            "callback-body",
                                            Input,
                                            "Callback body",
                                            None
                                        ).into_any_element()))
                                        .child(boxed(crate::dev_id!(
                                            div()
                                                .id("callback-preview")
                                                .size_full()
                                                .p(px(8.))
                                                .overflow_y_scroll()
                                                .text_size(px(12.))
                                                .font_family(BUFFER_FONT)
                                                .text_color(if preview.1 { t.deleted } else { t.text })
                                                .child(preview.0.clone()),
                                            "callback-preview",
                                            Text,
                                            preview.0.clone(),
                                            None
                                        ).into_any_element())),
                                ),
                        )
                        .children(shown.into_iter().map(|m| div().text_size(px(11.)).text_color(t.deleted).child(m)))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .justify_end()
                                .gap(px(8.))
                                .child(crate::dev_id!(cancel, "callback-cancel", Button, "Cancel", Press("callback-cancel".into())))
                                .child(if ro {
                                    crate::dev_id!(edit, "callback-to-edit", Button, "Edit", Press("callback-to-edit".into())).into_any_element()
                                } else {
                                    crate::dev_id!(save, "callback-save", Button, "Save", Press("callback-save".into())).into_any_element()
                                }),
                        ),
                ),
            2,
            "callback-dialog",
            Modal,
            "Callback",
            None
        )
    }

    fn callback_preview_draft(
        &self,
        d: &CallbackDialog,
        draft: &Callback,
        _cx: &gpui::App,
    ) -> Option<fauxfield_engine::CallbackPreview> {
        let (_, _, resp) = self.find_response(&d.response)?;
        let (rid, ..) = self.find_response(&d.response)?;
        let store = self.store.as_ref()?;
        let snap = store.snapshot();
        let server = snap
            .servers
            .iter()
            .find(|(_, s)| s.routes.iter().any(|r| r.id == rid))
            .map(|(_, s)| s)?;
        let sample = self
            .traffic
            .iter()
            .rev()
            .filter_map(|r| r.rec.as_deref())
            .find(|r| r.route == rid && r.source != "callback")
            .cloned();
        let mut c = draft.clone();
        if c.url.trim().is_empty() {
            c.url = "http://".into();
        }
        let key = format!(
            "{}|{c:?}|{}|{}|{}|{}",
            d.response,
            resp.status,
            resp.body,
            sample.as_ref().map_or(0, |r| r.id),
            d.preview_nonce
        );
        if let Some((k, p)) = self.cb_preview_cache.borrow().as_ref()
            && *k == key
        {
            return Some(p.clone());
        }
        let out = fauxfield_engine::callback_preview(
            &snap.root,
            server,
            &rid,
            resp.status,
            &resp.body,
            &c,
            sample.as_ref(),
        );
        *self.cb_preview_cache.borrow_mut() = Some((key, out.clone()));
        Some(out)
    }
}

/// Title and text of a field's help popover.
fn help_text(key: &str) -> (&'static str, &'static str) {
    match key {
        "id" => (
            "Callback id",
            "A short identifier, unique among this Response's callbacks: letters, digits, _ and -. It is written to the Server file, shows in Traffic as <id>#<attempt>, and is what `fauxfield callback refire --callback` and the MCP `refire_callback` tool take. Example shape: cb_payment_settled.",
        ),
        "name" => (
            "Name",
            "An optional friendly label shown in the Callbacks table instead of the id. It is only for you; it is never sent.",
        ),
        "description" => (
            "Description",
            "An optional one-line note about why this callback exists. It is stored in the file and never sent.",
        ),
        "url" => (
            "URL",
            "Where the request goes: an http:// or https:// URL, or a template such as {{ request.json.callback_url }}. It is rendered after the body, so callback.body is available.",
        ),
        "delay" => (
            "Delay",
            "Milliseconds to wait, after the Response body has been sent, before the first attempt. Empty or 0 fires at once.",
        ),
        "retries" => (
            "Retries",
            "Extra attempts after a failure (connection error, timeout or a 5xx answer), at most 10. A 4xx answer is recorded but not retried. Empty or 0 means a single attempt.",
        ),
        "backoff" => (
            "Backoff",
            "Milliseconds to wait before the first retry. The wait doubles on every further retry (default 500, capped at 30 s).",
        ),
        "timeout" => (
            "Timeout",
            "Milliseconds each attempt may take before it counts as failed (default 10000).",
        ),
        _ => (
            "Headers",
            "Request headers of the callback. Values are templates and can use callback.body to sign what is sent, for example hmac_sha256(key=vars.webhook_secret, data=callback.body). Content-Type is added automatically for a JSON body when you do not set it.",
        ),
    }
}
