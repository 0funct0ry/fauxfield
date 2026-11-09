//! The Response pane of a Route tab: Status Code, Status (reason) and Delay fields above Body
//! and Response headers sub-tabs. Owns the inline single-line fields (status, delay, file path,
//! header key/value with autocomplete) and the multi-line body editor with mouse selection.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, Point,
    StatefulInteractiveElement, Styled, Window, canvas, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::editor::{self, Lang, TextBuf, Tok};
use crate::shell::{BodyEdit, Shell};
use crate::theme::Theme;
use crate::ui::icon;
use crate::view::Response;

/// Body editor geometry: fixed line height and gutter, so a click maps to `(line, col)`.
pub const BODY_LINE_H: f32 = 20.0;
pub const BODY_GUTTER: f32 = 44.0;
pub const BODY_FONT_SIZE: f32 = 13.0;
/// Response sub-tabs.
pub const RESPONSE_SUBTABS: [&str; 2] = ["Body", "Headers"];

/// Which inline field is being edited.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineTarget {
    Status(String),
    Delay(String),
    BodyFile(String),
    /// Key cell of header row `row` of a Response; `row == rows.len()` is a new row.
    HeaderKey {
        response: String,
        row: usize,
    },
    /// Value cell of header row `row`.
    HeaderValue {
        response: String,
        row: usize,
    },
}

impl InlineTarget {
    pub fn is_header(&self) -> bool {
        matches!(
            self,
            InlineTarget::HeaderKey { .. } | InlineTarget::HeaderValue { .. }
        )
    }
}

#[derive(Clone, Debug)]
pub struct InlineEdit {
    pub target: InlineTarget,
    pub buf: TextBuf,
    /// Highlighted completion.
    pub ac_sel: usize,
    /// The highlight was moved with Up/Down. Until then only a completion that the typed text
    /// is a prefix of is taken by Enter/Tab, so a custom header name is never overwritten.
    pub ac_explicit: bool,
    /// Completion popup shown (Esc closes it; typing or Down reopens it).
    pub ac_open: bool,
}

/// Where a committed field sends the edit next (Tab / Shift+Tab / Enter).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Nav {
    /// End the edit.
    Stay,
    Next,
    Prev,
}

/// `body_file` must stay inside the workspace (same rule as validation E042).
pub fn valid_body_file(p: &str) -> bool {
    !p.is_empty() && !p.starts_with('/') && !p.split(['/', '\\']).any(|c| c == "..")
}

/// Index of the completion Enter/Tab takes: the highlighted one once it was moved with
/// Up/Down, else the first when the typed text is a prefix of it.
pub fn chosen(
    sugg: &[crate::header_catalog::Suggestion],
    sel: usize,
    explicit: bool,
) -> Option<usize> {
    if explicit {
        (sel < sugg.len()).then_some(sel)
    } else {
        sugg.first().filter(|s| s.hit.0 == 0).map(|_| 0)
    }
}

/// Text field geometry: monospace 12 pt with 8 pt side padding, so a click maps to a column.
pub const FIELD_PAD: f32 = 8.0;

impl Shell {
    // ---- inline fields ------------------------------------------------------------------------

    pub(crate) fn begin_inline(
        &mut self,
        target: InlineTarget,
        initial: &str,
        select_all: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.inline.as_ref().is_some_and(|i| i.target == target) {
            window.focus(&self.field_focus);
            return;
        }
        self.commit_inline(Nav::Stay, window, cx);
        self.commit_body_edit(cx);
        let mut buf = TextBuf::new(initial);
        if select_all {
            buf.select_all();
        }
        self.inline = Some(InlineEdit {
            target,
            buf,
            ac_sel: 0,
            ac_explicit: false,
            ac_open: true,
        });
        window.focus(&self.field_focus);
        cx.notify();
    }

    pub(crate) fn cancel_inline(&mut self, cx: &mut Context<Self>) {
        if self.inline.take().is_some() {
            // Esc on a header row that was never given a value drops it.
            self.pending_header = None;
            cx.notify();
        }
    }

    pub(crate) fn response_by_id(&self, id: &str) -> Option<Response> {
        self.find_response(id).map(|(_, _, r)| r)
    }

    /// Write the active field, then follow `nav` (header cells only).
    pub(crate) fn commit_inline(&mut self, nav: Nav, window: &mut Window, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let Some(edit) = self.inline.take() else {
            return;
        };
        let v = edit.buf.text.trim().to_string();
        match edit.target {
            InlineTarget::Status(id) => match v.parse::<u16>() {
                Ok(n) if (100..=599).contains(&n) => {
                    self.store_op(cx, |s| {
                        s.set_int(&T::Response(id), "status", Some(n.into()))
                    });
                }
                _ => self.notify("A status code is a number from 100 to 599.".into(), cx),
            },
            InlineTarget::Delay(id) => {
                let v = if v.is_empty() { "0".into() } else { v };
                match v.parse::<u64>() {
                    Ok(n) => {
                        self.store_op(cx, |s| {
                            s.set_int(&T::Response(id), "delay_ms", (n > 0).then_some(n as i64))
                        });
                    }
                    Err(_) => self.notify("Delay is a number of milliseconds.".into(), cx),
                }
            }
            InlineTarget::BodyFile(id) => {
                if v.is_empty() {
                    // Nothing chosen: stay inline.
                } else if !valid_body_file(&v) {
                    self.notify(
                        "The file must be a path inside the workspace, e.g. bodies/user.json."
                            .into(),
                        cx,
                    );
                } else {
                    self.store_op(cx, |s| {
                        s.set_str(&T::Response(id.clone()), "body_file", Some(&v))?;
                        s.set_str(&T::Response(id), "body", None)
                    });
                }
            }
            InlineTarget::HeaderKey { response, row } => {
                self.commit_header_key(&response, row, &edit.buf.text, nav, window, cx)
            }
            InlineTarget::HeaderValue { response, row } => {
                self.commit_header_value(&response, row, &edit.buf.text, nav, window, cx)
            }
        }
        cx.notify();
    }

    pub(crate) fn on_field_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let m = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        let sugg = self.inline_suggestions();
        let is_header = self.inline.as_ref().is_some_and(|i| i.target.is_header());
        let is_key_cell = matches!(
            self.inline.as_ref().map(|i| &i.target),
            Some(InlineTarget::HeaderKey { .. })
        );
        let numeric = matches!(
            self.inline.as_ref().map(|i| &i.target),
            Some(InlineTarget::Status(_) | InlineTarget::Delay(_))
        );
        match key {
            "escape" => {
                if sugg.is_empty() {
                    self.cancel_inline(cx);
                } else if let Some(i) = self.inline.as_mut() {
                    i.ac_open = false;
                }
            }
            "enter" | "tab" => {
                // The chosen completion replaces the text first.
                if let Some(i) = self.inline.as_mut()
                    && let Some(at) = chosen(&sugg, i.ac_sel, i.ac_explicit)
                {
                    i.buf = TextBuf::new(sugg[at].label);
                }
                let nav = match (key, m.shift) {
                    ("tab", true) => Nav::Prev,
                    ("tab", false) => Nav::Next,
                    // Enter finishes a header key by moving on to its value.
                    _ if is_key_cell => Nav::Next,
                    _ => Nav::Stay,
                };
                self.commit_inline(nav, window, cx);
            }
            "up" | "down" if is_header => {
                let n = sugg.len();
                if let Some(i) = self.inline.as_mut() {
                    if n == 0 || !i.ac_open {
                        // Down reopens the popup for a known header.
                        i.ac_open = true;
                        i.ac_sel = 0;
                        i.ac_explicit = false;
                    } else if !i.ac_explicit {
                        // The first Up/Down only shows the highlight (Down: first row).
                        i.ac_explicit = true;
                        i.ac_sel = if key == "up" { n - 1 } else { 0 };
                    } else {
                        i.ac_sel = if key == "up" {
                            i.ac_sel.saturating_sub(1)
                        } else {
                            (i.ac_sel + 1).min(n - 1)
                        };
                    }
                }
            }
            _ => {
                let Some(i) = self.inline.as_mut() else {
                    return;
                };
                let b = &mut i.buf;
                let mut edited = false;
                match key {
                    "backspace" => {
                        b.backspace();
                        edited = true;
                    }
                    "delete" => {
                        b.delete();
                        edited = true;
                    }
                    "left" => b.left(m.shift),
                    "right" => b.right(m.shift),
                    "home" | "up" => b.home(m.shift),
                    "end" | "down" => b.end(m.shift),
                    _ => {
                        if m.secondary() || m.control || m.alt {
                            return;
                        }
                        let Some(ch) = ev.keystroke.key_char.as_deref() else {
                            return;
                        };
                        if numeric && !ch.chars().all(|c| c.is_ascii_digit()) {
                            cx.stop_propagation();
                            return;
                        }
                        b.insert(ch);
                        edited = true;
                    }
                }
                if edited {
                    i.ac_sel = 0;
                    i.ac_explicit = false;
                    i.ac_open = true;
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// Column under window x inside the active field, from its painted bounds.
    fn field_col(&self, x: Pixels) -> Option<usize> {
        let b = self.inline_bounds.get()?;
        let w = if self.mono_w > 0.0 { self.mono_w } else { 7.8 };
        Some(((f32::from(x - b.origin.x)) / w).round().max(0.0) as usize)
    }

    pub(crate) fn field_mouse_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        let Some(col) = self.field_col(ev.position.x) else {
            return;
        };
        if let Some(i) = self.inline.as_mut() {
            if ev.click_count >= 2 {
                i.buf.select_all();
                self.field_drag = false;
            } else {
                i.buf.set_line_col(0, col, ev.modifiers.shift);
                self.field_drag = true;
            }
            i.ac_open = false;
        }
        cx.notify();
    }

    pub(crate) fn field_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.field_drag || ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let Some(col) = self.field_col(ev.position.x) else {
            return;
        };
        if let Some(i) = self.inline.as_mut() {
            i.buf.set_line_col(0, col, true);
            cx.notify();
        }
    }

    /// A press outside the field ends the edit, unless it landed on the completion popup
    /// (whose rows must still receive the click).
    pub(crate) fn field_press_outside(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(b) = self.popup_bounds.get()
            && b.contains(&ev.position)
            && !self.inline_suggestions().is_empty()
        {
            return;
        }
        self.commit_inline(Nav::Stay, window, cx);
    }

    // ---- body editor --------------------------------------------------------------------------

    pub(crate) fn begin_body_edit(
        &mut self,
        response: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(x) = self.response_by_id(response) else {
            return;
        };
        if self
            .body_edit
            .as_ref()
            .is_some_and(|b| b.response == response)
        {
            window.focus(&self.body_focus);
            return;
        }
        self.commit_inline(Nav::Stay, window, cx);
        self.commit_body_edit(cx);
        self.body_edit = Some(BodyEdit {
            response: response.into(),
            buf: TextBuf::new(&x.body),
        });
        window.focus(&self.body_focus);
        cx.notify();
    }

    /// Write the edited body to the file (and set `application/json` for a JSON document in a
    /// Response with no content type), then leave edit mode.
    pub(crate) fn commit_body_edit(&mut self, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let Some(b) = self.body_edit.take() else {
            return;
        };
        self.body_drag = false;
        let Some(x) = self.response_by_id(&b.response) else {
            return;
        };
        let mut text = b.buf.text.clone();
        if self.layout.format_on_save
            && editor::lang_for(&x.content_type, &text) == Lang::Json
            && let Ok(pretty) = editor::format_json(&text)
        {
            text = pretty;
        }
        if text != x.body {
            let id = b.response.clone();
            let set_ct = x.content_type.is_empty() && editor::looks_like_json_document(&text);
            self.store_op(cx, |s| {
                s.set_str(
                    &T::Response(id.clone()),
                    "body",
                    (!text.is_empty()).then_some(text.as_str()),
                )?;
                if set_ct {
                    s.set_str(&T::Response(id), "content_type", Some("application/json"))?;
                }
                Ok(())
            });
        }
        cx.notify();
    }

    pub(crate) fn cancel_body_edit(&mut self, cx: &mut Context<Self>) {
        self.body_drag = false;
        if self.body_edit.take().is_some() {
            cx.notify();
        }
    }

    /// Pretty-print the body as JSON, in the editor if open, else in the file.
    pub(crate) fn format_body(&mut self, response: &str, cx: &mut Context<Self>) {
        let src = match &self.body_edit {
            Some(b) if b.response == response => b.buf.text.clone(),
            _ => match self.response_by_id(response) {
                Some(x) => x.body,
                None => return,
            },
        };
        match editor::format_json(&src) {
            Ok(pretty) => {
                if let Some(b) = self.body_edit.as_mut().filter(|b| b.response == response) {
                    b.buf = TextBuf::new(&pretty);
                    cx.notify();
                } else {
                    let id = response.to_string();
                    self.store_op(cx, |s| {
                        s.set_str(
                            &fauxfield_store::Target::Response(id),
                            "body",
                            Some(&pretty),
                        )
                    });
                }
            }
            Err(e) => self.notify(format!("Not valid JSON: {e}"), cx),
        }
    }

    pub(crate) fn on_body_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let m = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        if key == "escape" {
            self.cancel_body_edit(cx);
            cx.stop_propagation();
            return;
        }
        if key == "enter" && m.secondary() {
            self.commit_body_edit(cx);
            cx.stop_propagation();
            return;
        }
        let Some(cur) = self.body_edit.as_ref() else {
            return;
        };
        let ct = self
            .find_response(&cur.response)
            .map(|(_, _, r)| r.content_type)
            .unwrap_or_default();
        let lang = editor::editing_lang(&ct, &cur.buf.text);
        let Some(b) = self.body_edit.as_mut() else {
            return;
        };
        let s = m.shift;
        match key {
            "enter" => b.buf.newline_for(lang),
            "tab" => b.buf.insert(editor::INDENT),
            "backspace" => b.buf.backspace_for(lang),
            "delete" => b.buf.delete(),
            "left" => b.buf.left(s),
            "right" => b.buf.right(s),
            "up" => b.buf.up(s),
            "down" => b.buf.down(s),
            "home" => b.buf.home(s),
            "end" => b.buf.end(s),
            _ => {
                if !m.secondary()
                    && !m.control
                    && !m.alt
                    && let Some(ch) = ev.keystroke.key_char.as_deref()
                {
                    b.buf.type_for(lang, ch);
                } else {
                    // Cmd/Ctrl combos (copy, paste, select all, …) stay with the menu actions.
                    return;
                }
            }
        }
        cx.stop_propagation();
        cx.notify();
    }

    /// `(line, col)` under a window position, from the editor's last painted bounds.
    fn body_line_col(&self, p: Point<Pixels>) -> Option<(usize, usize)> {
        let b = self.body_bounds.get()?;
        let (x, y) = (
            f32::from(p.x) - f32::from(b.origin.x),
            f32::from(p.y) - f32::from(b.origin.y),
        );
        let w = if self.mono_w > 0.0 { self.mono_w } else { 7.8 };
        let line = (y / BODY_LINE_H).floor().max(0.0) as usize;
        let col = ((x - BODY_GUTTER) / w).round().max(0.0) as usize;
        Some((line, col))
    }

    pub(crate) fn body_mouse_down(
        &mut self,
        response: &str,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.begin_body_edit(response, window, cx);
        let pos = self.body_line_col(ev.position);
        if let (Some((l, c)), Some(b)) = (pos, self.body_edit.as_mut()) {
            if ev.click_count >= 3 {
                b.buf.set_line_col(l, 0, false);
                b.buf.end(true);
            } else {
                b.buf.set_line_col(l, c, ev.modifiers.shift);
            }
            self.body_drag = ev.click_count < 3;
        }
        cx.notify();
    }

    pub(crate) fn body_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.body_drag || ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        let pos = self.body_line_col(ev.position);
        if let (Some((l, c)), Some(b)) = (pos, self.body_edit.as_mut()) {
            b.buf.set_line_col(l, c, true);
            cx.notify();
        }
    }

    /// Body source: inline text, a workspace file, or (M8) a dataset.
    pub(crate) fn set_body_file_mode(
        &mut self,
        response: &str,
        file: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(x) = self.response_by_id(response) else {
            return;
        };
        if file {
            let cur = x.body_file.clone().unwrap_or_default();
            self.begin_inline(
                InlineTarget::BodyFile(response.into()),
                &cur,
                true,
                window,
                cx,
            );
        } else {
            if matches!(&self.inline, Some(i) if i.target == InlineTarget::BodyFile(response.into()))
            {
                self.inline = None;
            }
            if x.body_file.is_some() {
                let id = response.to_string();
                self.store_op(cx, |s| {
                    s.set_str(&fauxfield_store::Target::Response(id), "body_file", None)
                });
            }
            cx.notify();
        }
    }

    // ---- rendering ----------------------------------------------------------------------------

    /// Single-line inline field: shows the value; click edits; Enter/blur saves, Esc cancels.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn inline_field(
        &self,
        t: &Theme,
        dev_id: String,
        target: InlineTarget,
        value: &str,
        placeholder: &str,
        width: f32,
        select_all: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editing = self.inline.as_ref().filter(|i| i.target == target);
        let base = div()
            .id(crate::ui::eid(dev_id.clone()))
            .w(px(width))
            .h(px(26.))
            .px(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .overflow_hidden()
            .whitespace_nowrap()
            .rounded(px(4.))
            .border_1()
            .font_family(BUFFER_FONT)
            .text_size(px(12.));
        let el =
            match editing {
                Some(e) => {
                    let b = &e.buf;
                    let (pre, sel, post, cur_before) = match b.selection() {
                        Some((a, z)) => (
                            b.text[..a].to_string(),
                            b.text[a..z].to_string(),
                            b.text[z..].to_string(),
                            b.cursor == a,
                        ),
                        None => (
                            b.text[..b.cursor].to_string(),
                            String::new(),
                            b.text[b.cursor..].to_string(),
                            false,
                        ),
                    };
                    let caret = || div().w(px(1.5)).h(px(15.)).bg(t.accent).flex_none();
                    base.border_color(t.border_focused)
                        .bg(t.editor)
                        .key_context("Field")
                        .track_focus(&self.field_focus)
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            this.on_field_key(ev, w, cx)
                        }))
                        .on_mouse_down_out(cx.listener(|this, ev: &MouseDownEvent, w, cx| {
                            this.field_press_outside(ev, w, cx)
                        }))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                this.field_mouse_down(ev, cx);
                                cx.stop_propagation();
                            }),
                        )
                        .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                            this.field_mouse_move(ev, cx)
                        }))
                        .on_mouse_up(
                            MouseButton::Left,
                            cx.listener(|this, _, _, _| this.field_drag = false),
                        )
                        .child(
                            // Text starts here: the painted bounds map a click to a column.
                            div()
                                .relative()
                                .h_full()
                                .flex()
                                .flex_row()
                                .items_center()
                                .child(
                                    canvas(
                                        {
                                            let cell = self.inline_bounds.clone();
                                            move |b: Bounds<Pixels>, _, _| cell.set(Some(b))
                                        },
                                        |_, _, _, _| {},
                                    )
                                    .absolute()
                                    .size_full(),
                                )
                                .child(div().child(pre))
                                .when(cur_before, |d| d.child(caret()))
                                .when(!sel.is_empty(), |d| {
                                    d.child(div().bg(t.element_selected).child(sel))
                                })
                                .when(!cur_before, |d| d.child(caret()))
                                .child(div().child(post)),
                        )
                        .into_any_element()
                }
                None => {
                    let (initial, tgt) = (value.to_string(), target.clone());
                    base.border_color(t.border_variant)
                        .cursor_text()
                        .hover(|s| s.border_color(t.border))
                        .on_click(cx.listener(move |this, _: &ClickEvent, w, cx| {
                            this.begin_inline(tgt.clone(), &initial, select_all, w, cx)
                        }))
                        .child(if value.is_empty() {
                            div()
                                .text_color(t.text_placeholder)
                                .child(placeholder.to_string())
                        } else {
                            div().text_color(t.text).child(value.to_string())
                        })
                        .into_any_element()
                }
            };
        crate::dev_id!(el, dev_id, Input, placeholder, None).into_any_element()
    }

    /// Right side of the Responses sub-tab for the selected Response.
    pub(crate) fn render_response_pane(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sub = self.resp_sub.get(&resp.id).copied().unwrap_or(0);
        let reason = editor::reason_phrase(resp.status);
        let label = |s: &str| {
            div()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(s.to_string())
        };
        let field = |l: &str, el: AnyElement| {
            div()
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(label(l))
                .child(el)
        };
        let n_headers = crate::header_table::header_rows(resp, self.pending_header.as_ref())
            .iter()
            .filter(|r| !r.pending)
            .count();
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex_none()
                    .px(px(16.))
                    .py(px(10.))
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(16.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(field(
                        "Status Code",
                        self.inline_field(
                            t,
                            "resp-status-code".into(),
                            InlineTarget::Status(resp.id.clone()),
                            &resp.status.to_string(),
                            "200",
                            90.,
                            true,
                            cx,
                        ),
                    ))
                    .child(field(
                        "Status",
                        crate::dev_id!(
                            div()
                                .id("resp-status-text")
                                .w(px(240.))
                                .h(px(26.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .bg(t.surface)
                                .text_size(px(12.))
                                .text_color(t.status_color(resp.status))
                                .child(if reason.is_empty() {
                                    "(non-standard code)".to_string()
                                } else {
                                    reason.to_string()
                                }),
                            "resp-status-text",
                            Text,
                            reason,
                            None
                        )
                        .into_any_element(),
                    ))
                    .child(field(
                        "Delay (ms)",
                        self.inline_field(
                            t,
                            "resp-delay".into(),
                            InlineTarget::Delay(resp.id.clone()),
                            &resp.delay_ms.to_string(),
                            "0",
                            110.,
                            true,
                            cx,
                        ),
                    )),
            )
            .child(
                div()
                    .h(px(30.))
                    .flex_none()
                    .px(px(8.))
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .children(RESPONSE_SUBTABS.iter().enumerate().map(|(i, name)| {
                        let id = resp.id.clone();
                        let active = i == sub;
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("resp-subtab-{name}")))
                                .px(px(10.))
                                .h(px(28.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .cursor_pointer()
                                .text_size(px(12.))
                                .border_b_2()
                                .border_color(if active { t.accent } else { t.editor })
                                .text_color(if active { t.text } else { t.text_muted })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.resp_sub.insert(id.clone(), i);
                                    cx.notify();
                                }))
                                .child(*name)
                                .when(i == 1 && n_headers > 0, |d| {
                                    d.child(
                                        div()
                                            .px(px(6.))
                                            .rounded(px(8.))
                                            .bg(t.element_selected)
                                            .text_size(px(10.))
                                            .child(n_headers.to_string()),
                                    )
                                }),
                            format!("resp-subtab-{name}"),
                            Tab,
                            *name,
                            None
                        )
                    })),
            )
            .child(div().flex_1().min_h_0().child(match sub {
                1 => self.render_headers_editor(t, resp, cx),
                _ => self.render_body_tab(t, resp, cx).into_any_element(),
            }))
            .into_any_element()
    }

    fn render_body_tab(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let file_editing = self
            .inline
            .as_ref()
            .is_some_and(|i| i.target == InlineTarget::BodyFile(resp.id.clone()));
        let file_mode = resp.body_file.is_some() || file_editing;
        let seg = |id: &'static str, label: &'static str, on: bool, enabled: bool| {
            div()
                .id(id)
                .h(px(22.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .text_size(px(12.))
                .when(on, |d| d.bg(t.element_selected).text_color(t.text))
                .when(!on && enabled, |d| {
                    d.text_color(t.text_muted)
                        .cursor_pointer()
                        .hover(|s| s.bg(t.element_hover))
                })
                .when(!enabled, |d| d.text_color(t.text_placeholder))
                .child(label)
        };
        let (rid_inline, rid_file, rid_fmt) = (resp.id.clone(), resp.id.clone(), resp.id.clone());
        let body_text = if file_mode {
            String::new()
        } else {
            self.body_text(resp)
        };
        let lang = editor::lang_for(&resp.content_type, &body_text);
        div()
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(36.))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .child(crate::dev_id!(
                        seg("body-source-inline", "Inline", !file_mode, true).on_click(
                            cx.listener(move |this, _, w, cx| {
                                this.set_body_file_mode(&rid_inline, false, w, cx)
                            })
                        ),
                        "body-source-inline",
                        Button,
                        "Inline",
                        None
                    ))
                    .child(crate::dev_id!(
                        seg("body-source-file", "File", file_mode, true).on_click(cx.listener(
                            move |this, _, w, cx| this.set_body_file_mode(&rid_file, true, w, cx)
                        )),
                        "body-source-file",
                        Button,
                        "File",
                        None
                    ))
                    .child(crate::dev_id_meta!(
                        seg("body-source-dataset", "Dataset", false, false).tooltip(
                            crate::ui::tooltip("Bodies from a Dataset arrive in M8".into())
                        ),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        "body-source-dataset",
                        Button,
                        "Dataset (M8)",
                        None
                    ))
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(lang.label()),
                    )
                    .when(!file_mode && lang == Lang::Json, |d| {
                        d.child(crate::dev_id!(
                            div()
                                .id("resp-format")
                                .h(px(22.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .cursor_pointer()
                                .text_size(px(12.))
                                .hover(|s| s.bg(t.element_hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.format_body(&rid_fmt, cx)
                                }))
                                .child("Format"),
                            "resp-format",
                            Button,
                            "Format JSON",
                            None
                        ))
                    }),
            )
            .child(if file_mode {
                self.render_body_file(t, resp, cx).into_any_element()
            } else {
                self.render_body_editor(t, resp, lang, cx)
                    .into_any_element()
            })
    }

    /// The body shown in the editor: the live buffer while editing, else the stored body.
    fn body_text(&self, resp: &Response) -> String {
        match &self.body_edit {
            Some(b) if b.response == resp.id => b.buf.text.clone(),
            _ => resp.body.clone(),
        }
    }

    fn render_body_file(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = resp.body_file.clone().unwrap_or_default();
        let preview: Result<String, String> = if path.is_empty() {
            Err("Type a path relative to the workspace folder, e.g. bodies/user.json".into())
        } else {
            let full = self.ws.root.join(&path);
            match std::fs::read(&full) {
                Ok(b) if b.len() > 256 * 1024 => {
                    Err("File is larger than 256 KiB; not previewed.".into())
                }
                Ok(b) => Ok(String::from_utf8_lossy(&b).into_owned()),
                Err(e) => Err(format!("Cannot read {path}: {e}")),
            }
        };
        let lang = match &preview {
            Ok(text) => match path.rsplit('.').next().unwrap_or("") {
                "json" => Lang::Json,
                "yaml" | "yml" => Lang::Yaml,
                "xml" | "html" | "htm" => Lang::Xml,
                _ => editor::lang_for(&resp.content_type, text),
            },
            Err(_) => Lang::Plain,
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(12.))
                    .pb(px(8.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(icon("file-text", 13., t.icon_muted))
                    .child(self.inline_field(
                        t,
                        "body-file-path".into(),
                        InlineTarget::BodyFile(resp.id.clone()),
                        &path,
                        "bodies/user.json",
                        420.,
                        false,
                        cx,
                    )),
            )
            .child(match preview {
                Ok(text) => div()
                    .id("body-file-preview")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .font_family(BUFFER_FONT)
                    .text_size(px(BODY_FONT_SIZE))
                    .children(self.highlighted_lines(t, &text, lang))
                    .into_any_element(),
                Err(msg) => div()
                    .px(px(12.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child(msg)
                    .into_any_element(),
            })
    }

    fn tok_color(t: &Theme, k: Tok) -> impl Into<gpui::Hsla> {
        match k {
            Tok::Key => t.syntax_property,
            Tok::Str => t.syntax_string,
            Tok::Num => t.syntax_number,
            Tok::Keyword => t.syntax_keyword,
            Tok::Template => t.syntax_function,
            Tok::Comment => t.syntax_comment,
            Tok::Tag => t.syntax_type,
            Tok::Attr => t.syntax_property,
            Tok::Plain => t.text,
        }
    }

    fn highlighted_lines(&self, t: &Theme, text: &str, lang: Lang) -> Vec<AnyElement> {
        text.split('\n')
            .enumerate()
            .map(|(li, line)| {
                div()
                    .h(px(BODY_LINE_H))
                    .flex()
                    .flex_row()
                    .items_center()
                    .whitespace_nowrap()
                    .child(
                        div()
                            .w(px(BODY_GUTTER))
                            .flex_none()
                            .pr(px(10.))
                            .flex()
                            .justify_end()
                            .text_color(t.text_placeholder)
                            .child((li + 1).to_string()),
                    )
                    .children(
                        editor::tokenize(lang, line)
                            .into_iter()
                            .map(|(k, s)| div().text_color(Self::tok_color(t, k)).child(s)),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn render_body_editor(
        &self,
        t: &Theme,
        resp: &Response,
        lang: Lang,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let editing = self.body_edit.as_ref().filter(|b| b.response == resp.id);
        let text = self.body_text(resp);
        let w = if self.mono_w > 0.0 { self.mono_w } else { 7.8 };
        let mut overlay: Vec<AnyElement> = Vec::new();
        if let Some(b) = editing {
            if let Some((a, z)) = b.buf.selection() {
                let ((la, ca), (lz, cz)) = (b.buf.line_col_of(a), b.buf.line_col_of(z));
                let lines: Vec<&str> = b.buf.text.split('\n').collect();
                for l in la..=lz {
                    let start = if l == la { ca } else { 0 };
                    let end = if l == lz {
                        cz
                    } else {
                        lines.get(l).map_or(0, |s| s.chars().count()) + 1
                    };
                    if end > start {
                        overlay.push(
                            div()
                                .absolute()
                                .top(px(l as f32 * BODY_LINE_H))
                                .left(px(BODY_GUTTER + start as f32 * w))
                                .w(px((end - start) as f32 * w))
                                .h(px(BODY_LINE_H))
                                .bg(t.element_selected)
                                .into_any_element(),
                        );
                    }
                }
            }
            let (l, c) = b.buf.line_col();
            overlay.push(
                div()
                    .absolute()
                    .top(px(l as f32 * BODY_LINE_H + 2.))
                    .left(px(BODY_GUTTER + c as f32 * w))
                    .w(px(1.5))
                    .h(px(BODY_LINE_H - 4.))
                    .bg(t.accent)
                    .into_any_element(),
            );
        }
        let verdict: Option<(bool, String)> =
            (!text.trim().is_empty() && lang == Lang::Json).then(|| {
                match editor::validate_json(&text) {
                    Ok(()) => (true, "Valid JSON".to_string()),
                    Err(e) => (false, e),
                }
            });
        let is_editing = editing.is_some();
        let bounds = self.body_bounds.clone();
        let (rid_down, rid_empty) = (resp.id.clone(), resp.id.clone());
        let lines = self.highlighted_lines(t, &text, lang);
        let empty = text.is_empty() && !is_editing;
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .border_t_1()
            .border_color(t.border_variant)
            .child(crate::dev_id!(
                div()
                    .id("body-editor")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .py(px(8.))
                    .font_family(BUFFER_FONT)
                    .text_size(px(BODY_FONT_SIZE))
                    .cursor_text()
                    .when(is_editing, |d| {
                        d.key_context("Editor")
                            .track_focus(&self.body_focus)
                            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                                this.on_body_key(ev, cx)
                            }))
                            .on_mouse_down_out(
                                cx.listener(|this, _, _, cx| this.commit_body_edit(cx)),
                            )
                    })
                    .child(if empty {
                        div()
                            .id("body-empty")
                            .px(px(12.))
                            .text_color(t.text_placeholder)
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.begin_body_edit(&rid_empty, w, cx)
                            }))
                            .child("Empty body. Click to type, or paste a JSON, YAML or XML body.")
                            .into_any_element()
                    } else {
                        div()
                            .relative()
                            .min_h_full()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, ev: &MouseDownEvent, w, cx| {
                                    this.body_mouse_down(&rid_down, ev, w, cx)
                                }),
                            )
                            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                                this.body_mouse_move(ev, cx)
                            }))
                            .on_mouse_up(
                                MouseButton::Left,
                                cx.listener(|this, _, _, _| this.body_drag = false),
                            )
                            .child(
                                canvas(
                                    move |b: Bounds<Pixels>, _, _| bounds.set(Some(b)),
                                    |_, _, _, _| {},
                                )
                                .absolute()
                                .size_full(),
                            )
                            .children(overlay)
                            .child(div().flex().flex_col().children(lines))
                            .into_any_element()
                    }),
                "body-editor",
                Input,
                "Response body",
                None
            ))
            .child(
                div()
                    .h(px(24.))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .border_t_1()
                    .border_color(t.border_variant)
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .children(verdict.map(|(ok, msg)| {
                        crate::dev_id!(
                            div()
                                .id("body-validity")
                                .text_color(if ok { t.created } else { t.deleted })
                                .child(if ok {
                                    format!("✓ {msg}")
                                } else {
                                    format!("✗ {msg}")
                                }),
                            "body-validity",
                            Text,
                            msg,
                            None
                        )
                    }))
                    .child(div().flex_1())
                    .child(crate::dev_id!(
                        div()
                            .id("format-on-save")
                            .h(px(18.))
                            .px(px(6.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .rounded(px(3.))
                            .cursor_pointer()
                            .hover(|s| s.bg(t.element_hover))
                            .tooltip(crate::ui::tooltip(
                                "Pretty-print valid JSON bodies when they are saved".into(),
                            ))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _, _, cx| {
                                    this.layout.format_on_save = !this.layout.format_on_save;
                                    this.save_layout();
                                    cx.notify();
                                    cx.stop_propagation();
                                }),
                            )
                            .child(icon(
                                if self.layout.format_on_save {
                                    "check"
                                } else {
                                    "square"
                                },
                                12.,
                                if self.layout.format_on_save {
                                    t.created
                                } else {
                                    t.icon_muted
                                },
                            ))
                            .child("Format on save"),
                        "format-on-save",
                        Button,
                        "Format on save",
                        None
                    ))
                    .child(if is_editing {
                        "Cmd/Ctrl+Enter or click outside to save · Esc to cancel"
                    } else {
                        "Click to edit"
                    }),
            )
    }
}

/// Shared `Rc<Cell<…>>` for the body editor's painted bounds.
pub type BoundsCell = Rc<Cell<Option<Bounds<Pixels>>>>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_file_paths_stay_inside() {
        assert!(valid_body_file("bodies/a.json"));
        assert!(!valid_body_file("/etc/passwd"));
        assert!(!valid_body_file("../x"));
        assert!(!valid_body_file(""));
    }
}
