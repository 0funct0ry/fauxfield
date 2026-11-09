//! The Response pane of a Route tab: Status Code, Status (reason) and Delay fields above Body
//! and Response headers sub-tabs. Owns the inline single-line fields (status, delay, file path,
//! header key/value with autocomplete) and the multi-line body editor with mouse selection.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, ClickEvent, Context, InteractiveElement, IntoElement, KeyDownEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, ParentElement, Pixels, StatefulInteractiveElement,
    Styled, Window, canvas, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::code_editor::{CodeEditor, EditorEvent, Language, format};
use crate::editor::{self, TextBuf};
use crate::shell::{BodyEdit, Shell};
use crate::theme::Theme;
use crate::ui::icon;
use crate::view::Response;
use gpui::Focusable as _;

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
    /// One-line description under the Response name.
    Description(String),
    /// Sample request dialog fields (route id).
    SampleMethod(String),
    SampleQuery(String),
    SampleCookies(String),
    /// Key / Value cell of rule `row` of a Response (Rules sub-tab).
    RuleKey {
        response: String,
        row: usize,
    },
    RuleValue {
        response: String,
        row: usize,
    },
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
    /// Key / Value cell of Route header row `row`; `row == rows.len()` is a new row.
    RouteHeaderKey {
        route: String,
        row: usize,
    },
    RouteHeaderValue {
        route: String,
        row: usize,
    },
    /// The Server's `fake_seed` (Inspector); digits only.
    Seed(String),
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
            self.pending_route_header = None;
            cx.notify();
        }
    }

    pub(crate) fn toggle_response_template(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(on) = self.response_by_id(id).map(|r| r.template) else {
            return;
        };
        self.store_op(cx, |s| s.set_response_template(id, !on));
    }

    /// Inspector Seed toggle: Deterministic picks seed 1 when none is set, Random clears it.
    pub(crate) fn set_seed_mode(
        &mut self,
        server: &str,
        deterministic: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(cur) = self
            .ws
            .servers
            .iter()
            .find(|s| s.id == server)
            .map(|s| s.fake_seed)
        else {
            return;
        };
        match (deterministic, cur) {
            (true, None) => {
                self.store_op(cx, |s| s.set_fake_seed(server, Some(1)));
            }
            (false, Some(_)) => {
                self.store_op(cx, |s| s.set_fake_seed(server, None));
            }
            _ => {}
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
            InlineTarget::SampleMethod(_) => {
                if let Some(d) = self.sample_dialog.as_mut() {
                    d.sample.method = v.to_ascii_uppercase();
                }
            }
            InlineTarget::SampleQuery(_) => {
                if let Some(d) = self.sample_dialog.as_mut() {
                    d.sample.query = v;
                }
            }
            InlineTarget::SampleCookies(_) => {
                if let Some(d) = self.sample_dialog.as_mut() {
                    d.sample.cookies = v;
                }
            }
            InlineTarget::Description(id) => {
                self.store_op(cx, |s| {
                    s.set_str(
                        &T::Response(id),
                        "description",
                        Some(&v).filter(|v| !v.is_empty()).map(String::as_str),
                    )
                });
            }
            InlineTarget::RuleKey { response, row } => {
                self.edit_rule(&response, row, cx, |r| {
                    r.key = Some(v.clone()).filter(|k| !k.is_empty())
                });
            }
            InlineTarget::RuleValue { response, row } => {
                self.edit_rule(&response, row, cx, |r| {
                    r.value = Some(crate::rules_ui::parse_value(&r.op, &v));
                });
            }
            InlineTarget::HeaderKey { response, row } => {
                self.commit_header_key(&response, row, &edit.buf.text, nav, window, cx)
            }
            InlineTarget::HeaderValue { response, row } => {
                self.commit_header_value(&response, row, &edit.buf.text, nav, window, cx)
            }
            InlineTarget::RouteHeaderKey { route, row } => {
                self.commit_route_header_key(&route, row, &edit.buf.text, nav, window, cx)
            }
            InlineTarget::RouteHeaderValue { route, row } => {
                self.commit_route_header_value(&route, row, &edit.buf.text, nav, window, cx)
            }
            InlineTarget::Seed(server) => match v.parse::<u64>() {
                Ok(n) => {
                    self.store_op(cx, |s| s.set_fake_seed(&server, Some(n)));
                }
                Err(_) if v.is_empty() => {}
                Err(_) => self.notify("A seed is a whole number.".into(), cx),
            },
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
            Some(InlineTarget::HeaderKey { .. } | InlineTarget::RouteHeaderKey { .. })
        );
        let numeric = matches!(
            self.inline.as_ref().map(|i| &i.target),
            Some(InlineTarget::Status(_) | InlineTarget::Delay(_) | InlineTarget::Seed(_))
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

    /// The CodeEditor for a Response's body (`id`) or file preview (`id#file`), created on first
    /// use. The owner pushes its text in whenever it differs and no edit session is open.
    pub(crate) fn ensure_body_editor(&mut self, response: &str, cx: &mut Context<Self>) {
        let Some(resp) = self.response_by_id(response) else {
            return;
        };
        let editing = self
            .body_edit
            .as_ref()
            .is_some_and(|b| b.response == response);
        let st = self.editor_settings.clone();
        let lang = Language::detect(&resp.content_type, &resp.body);
        let vars = if self.body_editors.contains_key(response) {
            Vec::new()
        } else {
            self.variable_names_for(response)
        };
        let ed = match self.body_editors.get(response) {
            Some(e) => e.clone(),
            None => {
                let ed = cx.new(|cx| {
                    CodeEditor::new(lang, cx)
                        .completion_provider(crate::code_editor::tera_ctx::TeraCompletions {
                            variables: vars,
                        })
                        .read_only(true)
                        .format_paste(true)
                        .placeholder(
                            "Empty body. Click to type, or paste a JSON, YAML or XML body.",
                        )
                });
                ed.update(cx, |e, cx| e.set_text(&resp.body, cx));
                let rid = response.to_string();
                cx.subscribe(&ed, move |this, _, ev: &EditorEvent, cx| match ev {
                    EditorEvent::Blurred => {
                        if this.body_edit.as_ref().is_some_and(|b| b.response == rid) {
                            this.commit_body_edit(cx);
                        }
                    }
                    EditorEvent::Changed => this.body_changed(&rid, cx),
                    _ => {}
                })
                .detach();
                self.body_editors.insert(response.to_string(), ed.clone());
                ed
            }
        };
        if !editing {
            ed.update(cx, |e, cx| {
                if e.text() != resp.body {
                    e.set_text(&resp.body, cx);
                }
                e.set_language(lang, cx);
                e.set_read_only(true, cx);
                e.apply_settings(&st, cx);
            });
        } else {
            ed.update(cx, |e, cx| e.apply_settings(&st, cx));
        }
        // File preview (read-only, 256 KiB guard in `file_preview`).
        if let Some(path) = resp.body_file.clone() {
            let (text, lang) = match self.file_preview(&path, &resp.content_type) {
                Ok((t, l)) => (t, l),
                Err(_) => (String::new(), Language::Plain),
            };
            let key = format!("{response}#file");
            let pv = self
                .body_editors
                .entry(key)
                .or_insert_with(|| cx.new(|cx| CodeEditor::new(lang, cx).read_only(true)))
                .clone();
            pv.update(cx, |e, cx| {
                if e.text() != text {
                    e.set_text(&text, cx);
                }
                e.set_language(lang, cx);
                e.apply_settings(&st, cx);
            });
        }
    }

    /// Re-detects the language while typing and refreshes the validity footer.
    fn body_changed(&mut self, response: &str, cx: &mut Context<Self>) {
        let ct = self
            .response_by_id(response)
            .map(|r| r.content_type)
            .unwrap_or_default();
        if let Some(ed) = self.body_editors.get(response).cloned() {
            ed.update(cx, |e, cx| {
                let lang = Language::detect(&ct, e.text());
                e.set_language(lang, cx);
            });
        }
        self.sync_tera_diagnostics(response, cx);
        cx.notify();
    }

    /// Text and language of a body file, `Err` with the message to show instead.
    pub(crate) fn file_preview(
        &self,
        path: &str,
        content_type: &str,
    ) -> Result<(String, Language), String> {
        if path.is_empty() {
            return Err(
                "Type a path relative to the workspace folder, e.g. bodies/user.json".into(),
            );
        }
        let full = self.ws.root.join(path);
        match std::fs::read(&full) {
            Ok(b) if b.len() > 256 * 1024 => {
                Err("File is larger than 256 KiB; not previewed.".into())
            }
            Ok(b) => {
                let text = String::from_utf8_lossy(&b).into_owned();
                let lang = match path.rsplit('.').next().unwrap_or("") {
                    "json" => Language::Json,
                    "yaml" | "yml" => Language::Yaml,
                    "xml" => Language::Xml,
                    "html" | "htm" => Language::Html,
                    "md" => Language::Markdown,
                    "toml" => Language::Toml,
                    _ => Language::detect(content_type, &text),
                };
                Ok((text, lang))
            }
            Err(e) => Err(format!("Cannot read {path}: {e}")),
        }
    }

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
            if let Some(ed) = self.body_editors.get(response) {
                window.focus(&ed.read(cx).focus_handle(cx));
            }
            return;
        }
        self.commit_inline(Nav::Stay, window, cx);
        self.commit_body_edit(cx);
        self.ensure_body_editor(response, cx);
        let Some(ed) = self.body_editors.get(response).cloned() else {
            return;
        };
        self.body_edit = Some(BodyEdit {
            response: response.into(),
            original: x.body,
        });
        ed.update(cx, |e, cx| e.set_read_only(false, cx));
        window.focus(&ed.read(cx).focus_handle(cx));
        cx.notify();
    }

    /// Write the edited body to the file (and set `application/json` for a JSON document in a
    /// Response with no content type), then leave edit mode.
    pub(crate) fn commit_body_edit(&mut self, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let Some(b) = self.body_edit.take() else {
            return;
        };
        let Some(ed) = self.body_editors.get(&b.response).cloned() else {
            return;
        };
        ed.update(cx, |e, cx| e.set_read_only(true, cx));
        let Some(x) = self.response_by_id(&b.response) else {
            return;
        };
        let mut text = ed.read(cx).text().to_string();
        if (self.layout.format_on_save || self.editor_settings.format_json_on_save)
            && Language::detect(&x.content_type, &text) == Language::Json
            && let Ok(pretty) = format::format_json(&text, "  ")
            && format::validate_json(&text).is_ok()
        {
            text = pretty;
        }
        if text != x.body {
            let id = b.response.clone();
            let set_ct = x.content_type.is_empty() && looks_like_json_document(&text);
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

    /// Esc: discard the session's edits and restore the stored body.
    pub(crate) fn cancel_body_edit(&mut self, cx: &mut Context<Self>) {
        if let Some(b) = self.body_edit.take() {
            if let Some(ed) = self.body_editors.get(&b.response).cloned() {
                ed.update(cx, |e, cx| {
                    e.set_text(&b.original, cx);
                    e.set_read_only(true, cx);
                });
            }
            cx.notify();
        }
    }

    /// Pretty-print the body as JSON, in the editor if open, else in the file.
    pub(crate) fn format_body(&mut self, response: &str, cx: &mut Context<Self>) {
        let editing = self
            .body_edit
            .as_ref()
            .is_some_and(|b| b.response == response);
        if editing && let Some(ed) = self.body_editors.get(response).cloned() {
            ed.update(cx, |e, cx| e.format_body(cx));
            if let Some(err) = ed.read(cx).last_error().cloned() {
                self.notify(format!("Not valid JSON: {err}"), cx);
            }
            cx.notify();
            return;
        }
        let Some(x) = self.response_by_id(response) else {
            return;
        };
        let res = format::validate_json(&x.body).and_then(|_| format::format_json(&x.body, "  "));
        match res {
            Ok(pretty) => {
                let id = response.to_string();
                self.store_op(cx, |s| {
                    s.set_str(
                        &fauxfield_store::Target::Response(id),
                        "body",
                        Some(&pretty),
                    )
                });
            }
            Err(e) => self.notify(format!("Not valid JSON: {e}"), cx),
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
                    ))
                    .child(field("Template", {
                        let (rid, on) = (resp.id.clone(), resp.template);
                        crate::dev_id!(
                            div()
                                .id("resp-template")
                                .w(px(70.))
                                .h(px(26.))
                                .px(px(8.))
                                .flex()
                                .items_center()
                                .rounded(px(4.))
                                .border_1()
                                .border_color(t.border_variant)
                                .cursor_pointer()
                                .hover(|s| s.border_color(t.border))
                                .text_size(px(12.))
                                .text_color(if on { t.text } else { t.text_muted })
                                .tooltip(crate::ui::tooltip(
                                    "Tera renders {{ }} and {% %} in the body and headers; Off sends them verbatim"
                                        .into()
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.toggle_response_template(&rid, cx)
                                }))
                                .child(if on { "Tera" } else { "Off" }),
                            "resp-template",
                            Button,
                            if on { "Tera" } else { "Off" },
                            Press(format!("resp-template:{}", resp.id))
                        )
                        .into_any_element()
                    }))
                    .child(field(
                        "Description",
                        self.inline_field(
                            t,
                            "resp-description".into(),
                            InlineTarget::Description(resp.id.clone()),
                            &resp.description,
                            "Only on the 3rd call",
                            260.,
                            false,
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
        let lang = self
            .body_editors
            .get(&resp.id)
            .map_or(Language::Plain, |e| e.read(cx).lang());
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
                            .child(lang.name()),
                    )
                    .when(!file_mode && lang == Language::Json, |d| {
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
                self.render_body_editor(t, resp, cx).into_any_element()
            })
            .child(self.render_preview_pane(t, &resp.id, cx))
    }

    fn render_body_file(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let path = resp.body_file.clone().unwrap_or_default();
        let preview = self.file_preview(&path, &resp.content_type);
        let pv = self.body_editors.get(&format!("{}#file", resp.id)).cloned();
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
                Ok(_) => div()
                    .id("body-file-preview")
                    .flex_1()
                    .min_h_0()
                    .children(pv)
                    .into_any_element(),
                Err(msg) => div()
                    .px(px(12.))
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child(msg)
                    .into_any_element(),
            })
    }

    fn render_body_editor(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let editing = self
            .body_edit
            .as_ref()
            .is_some_and(|b| b.response == resp.id);
        let ed = self.body_editors.get(&resp.id).cloned();
        let (text, lang) = ed
            .as_ref()
            .map(|e| (e.read(cx).text().to_string(), e.read(cx).lang()))
            .unwrap_or((String::new(), Language::Plain));
        // A templated JSON body is not JSON until it is rendered: the Preview judges it.
        let templated = resp.template && (text.contains("{{") || text.contains("{%"));
        let verdict: Option<(bool, String)> = format::validate(lang, &text)
            .filter(|_| !templated)
            .map(|r| match r {
                Ok(()) => (true, "Valid JSON".to_string()),
                Err(e) => (false, e.to_string()),
            });
        let tera_err = resp
            .template
            .then(|| {
                ed.as_ref()
                    .and_then(|e| e.read(cx).diagnostics().first().map(|d| d.1.clone()))
            })
            .flatten();
        let rid = resp.id.clone();
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
                    .cursor_text()
                    .on_action(cx.listener(
                        |this, _: &crate::code_editor::actions::Escape, _, cx| {
                            if this.body_edit.is_some() {
                                this.cancel_body_edit(cx);
                            } else {
                                cx.propagate();
                            }
                        }
                    ))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, w, cx| this.begin_body_edit(&rid, w, cx)),
                    )
                    .children(ed),
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
                    .children(tera_err.map(|m| {
                        crate::dev_id!(
                            div()
                                .id("body-tera-error")
                                .flex_1()
                                .truncate()
                                .text_color(t.deleted)
                                .child(format!("✗ Template: {m}")),
                            "body-tera-error",
                            Text,
                            m,
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
                                "Pretty-print valid JSON bodies when they are saved".into()
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
                    .child(if editing {
                        "Cmd/Ctrl+Enter or click outside to save · Esc to cancel"
                    } else {
                        "Click to edit"
                    }),
            )
    }
}

/// True when saving this into a Response with no content type should set `application/json`.
fn looks_like_json_document(body: &str) -> bool {
    let t = body.trim_start();
    (t.starts_with('{') || t.starts_with('[')) && format::validate_json(body).is_ok()
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
