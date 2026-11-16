//! The Response headers table: a standard key/value table editor.
//!
//! - Every Key and Value cell edits in place (click, then caret/selection with keys or mouse).
//! - A completion popup offers standard header names while typing a Key and the common values
//!   of the header while typing a Value (Up/Down, Enter/Tab or click to accept, Esc to close).
//! - Tab / Shift+Tab walk Key → Value → next row's Key (Tab past the last row opens a new row);
//!   Enter ends the edit (on a Key it moves on to the Value); a press elsewhere saves.
//! - Rows keep the order they have in the file; renaming a header keeps its position.
//! - `Content-Type` is shown as the first row and edits the Response's `content_type` field.
//! - A new row exists only in the table until it has a Key (and, for Content-Type, a Value).

use gpui::{
    AnyElement, Bounds, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Pixels, StatefulInteractiveElement, Styled, Window, canvas, deferred, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::header_catalog::{self, Col, Suggestion};
use crate::response_pane::{InlineEdit, InlineTarget, Nav};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::icon;
use crate::view::Response;

/// Height of one table row.
const ROW_PX: f32 = 36.0;
const KEY_W: f32 = 280.0;
const VALUE_W: f32 = 460.0;

/// One row of the table. `content_type` rows edit the Response's `content_type` field (the
/// engine lets it win over a `content-type` header).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HeaderRow {
    pub key: String,
    pub value: String,
    pub content_type: bool,
    /// A key that was typed but not written yet.
    pub pending: bool,
}

/// Rows of a Response: Content-Type first (when set), its headers in file order, then the
/// pending row (if it belongs to this Response).
pub fn header_rows(resp: &Response, pending: Option<&(String, String)>) -> Vec<HeaderRow> {
    let mut rows = Vec::new();
    if !resp.content_type.is_empty() {
        rows.push(HeaderRow {
            key: "Content-Type".into(),
            value: resp.content_type.clone(),
            content_type: true,
            pending: false,
        });
    }
    rows.extend(resp.headers.iter().map(|(k, v)| HeaderRow {
        key: k.clone(),
        value: v.clone(),
        content_type: false,
        pending: false,
    }));
    if let Some((r, k)) = pending
        && *r == resp.id
    {
        rows.push(HeaderRow {
            key: k.clone(),
            value: String::new(),
            content_type: false,
            pending: true,
        });
    }
    rows
}

pub fn is_content_type(k: &str) -> bool {
    k.eq_ignore_ascii_case("content-type")
}

/// A header name as it is stored: no whitespace, canonical casing for a known header.
pub fn clean_key(text: &str) -> String {
    let k: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    header_catalog::canonical(&k).map_or(k, str::to_string)
}

impl Shell {
    fn rows_of(&self, response: &str) -> Vec<HeaderRow> {
        self.response_by_id(response)
            .map(|r| header_rows(&r, self.pending_header.as_ref()))
            .unwrap_or_default()
    }

    /// Start editing a cell. `row == rows.len()` starts a new row.
    pub(crate) fn begin_header_cell(
        &mut self,
        response: &str,
        row: usize,
        col: Col,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.rows_of(response);
        let text = rows.get(row).map_or(String::new(), |r| match col {
            Col::Key => r.key.clone(),
            Col::Value => r.value.clone(),
        });
        let target = match col {
            Col::Key => InlineTarget::HeaderKey {
                response: response.into(),
                row,
            },
            Col::Value => InlineTarget::HeaderValue {
                response: response.into(),
                row,
            },
        };
        self.begin_inline(target, &text, true, window, cx);
    }

    /// Completions for the active header cell, empty while the popup is closed.
    pub(crate) fn inline_suggestions(&self) -> Vec<Suggestion> {
        let Some(InlineEdit {
            target,
            text,
            ac_open: true,
            ..
        }) = &self.inline
        else {
            return Vec::new();
        };
        match target {
            InlineTarget::HeaderKey { .. } => header_catalog::suggest(Col::Key, "", text),
            InlineTarget::HeaderValue { response, row } => {
                let rows = self.rows_of(response);
                let name = rows.get(*row).map_or("", |r| r.key.as_str());
                header_catalog::suggest(Col::Value, name, text)
            }
            _ => Vec::new(),
        }
    }

    /// Accept completion `i` (popup click): the text becomes the label and the cell commits.
    pub(crate) fn accept_suggestion(
        &mut self,
        i: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let sugg = self.inline_suggestions();
        let Some(sg) = sugg.get(i) else { return };
        let is_key = matches!(
            self.inline.as_ref().map(|e| &e.target),
            Some(InlineTarget::HeaderKey { .. })
        );
        let label = sg.label;
        self.set_inline_text(label, cx);
        self.commit_inline(if is_key { Nav::Next } else { Nav::Stay }, window, cx);
    }

    /// Row index of the header called `key` after an edit.
    fn row_of(&self, response: &str, key: &str) -> Option<usize> {
        self.rows_of(response)
            .iter()
            .position(|r| r.key.eq_ignore_ascii_case(key))
    }

    pub(crate) fn commit_header_key(
        &mut self,
        response: &str,
        row: usize,
        text: &str,
        nav: Nav,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use fauxfield_store::Target as T;
        let rows = self.rows_of(response);
        let new = clean_key(text);
        let is_new = row >= rows.len();
        let pending = rows.get(row).is_some_and(|r| r.pending);
        if new.is_empty() {
            // An empty key never creates a row; an existing row keeps its old key.
            if pending {
                self.pending_header = None;
            }
            return;
        }
        let dup = rows
            .iter()
            .enumerate()
            .any(|(i, r)| i != row && r.key.eq_ignore_ascii_case(&new));
        if dup {
            self.notify(format!("`{new}` is already in this Response."), cx);
            if pending {
                self.pending_header = None;
            }
            return;
        }
        if is_new || pending {
            // The row stays pending until its value is entered, so go straight to it.
            self.pending_header = Some((response.to_string(), new));
            if nav != Nav::Prev {
                let at = if is_new { rows.len() } else { row };
                self.begin_header_cell(response, at, Col::Value, window, cx);
            }
            return;
        }
        let old = rows[row].clone();
        if old.key != new {
            let (id, value) = (response.to_string(), old.value.clone());
            let (new_s, old_key) = (new.clone(), old.key.clone());
            self.store_op(cx, |s| match (old.content_type, is_content_type(&new_s)) {
                (true, true) => Ok(()),
                (false, false) => s.rename_header(&id, &old_key, &new_s),
                (true, false) => {
                    s.set_str(&T::Response(id.clone()), "content_type", None)?;
                    s.set_header(&id, &new_s, Some(&value))
                }
                (false, true) => {
                    s.set_header(&id, &old_key, None)?;
                    s.set_str(&T::Response(id), "content_type", Some(&value))
                }
            });
        }
        match nav {
            Nav::Next => {
                let at = self.row_of(response, &new).unwrap_or(row);
                self.begin_header_cell(response, at, Col::Value, window, cx);
            }
            Nav::Prev => {
                if row > 0 {
                    self.begin_header_cell(response, row - 1, Col::Value, window, cx);
                }
            }
            Nav::Stay => {}
        }
    }

    pub(crate) fn commit_header_value(
        &mut self,
        response: &str,
        row: usize,
        text: &str,
        nav: Nav,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        use fauxfield_store::Target as T;
        let rows = self.rows_of(response);
        let Some(r) = rows.get(row).cloned() else {
            return;
        };
        let value = text.trim().to_string();
        let id = response.to_string();
        if r.pending {
            self.pending_header = None;
        }
        if r.content_type || is_content_type(&r.key) {
            // An empty Content-Type is no Content-Type: the row goes away.
            self.store_op(cx, |s| {
                s.set_str(
                    &T::Response(id),
                    "content_type",
                    (!value.is_empty()).then_some(value.as_str()),
                )
            });
        } else if r.value != value || r.pending {
            let key = r.key.clone();
            self.store_op(cx, |s| s.set_header(&id, &key, Some(&value)));
        }
        let n = self.rows_of(response).len();
        match nav {
            Nav::Next => {
                // Next row's key; after the last row a new row opens.
                let at = self.row_of(response, &r.key).map_or(row, |i| i + 1);
                self.begin_header_cell(response, at.min(n), Col::Key, window, cx);
            }
            Nav::Prev => {
                let at = self.row_of(response, &r.key).unwrap_or(row);
                self.begin_header_cell(response, at, Col::Key, window, cx);
            }
            Nav::Stay => {}
        }
    }

    pub(crate) fn remove_header_row(&mut self, response: &str, row: usize, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let rows = self.rows_of(response);
        let Some(r) = rows.get(row).cloned() else {
            return;
        };
        self.inline = None;
        if r.pending {
            self.pending_header = None;
            cx.notify();
            return;
        }
        let id = response.to_string();
        self.store_op(cx, |s| {
            if r.content_type {
                s.set_str(&T::Response(id), "content_type", None)
            } else {
                s.set_header(&id, &r.key, None)
            }
        });
    }

    // ---- rendering ----------------------------------------------------------------------------

    /// The completion popup under the active cell: names with descriptions, or values.
    fn header_popup(
        &self,
        t: &Theme,
        sugg: &[Suggestion],
        left: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let sel = self
            .inline
            .as_ref()
            .and_then(|e| crate::response_pane::chosen(sugg, e.ac_sel, e.ac_explicit));
        let key_popup = sugg.iter().any(|s| !s.detail.is_empty());
        let popup_bounds = self.popup_bounds.clone();
        deferred(
            div()
                .id("hdr-autocomplete")
                .absolute()
                .top(px(ROW_PX - 4.))
                .left(px(left))
                .w(px(if key_popup { 600. } else { 480. }))
                .max_h(px(280.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(t.elevated)
                .border_1()
                .border_color(t.border)
                .shadow_lg()
                .occlude()
                .overflow_y_scroll()
                .child(
                    canvas(
                        move |b: Bounds<Pixels>, _, _| popup_bounds.set(Some(b)),
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
                .children(sugg.iter().enumerate().map(|(i, sg)| {
                    let (a, z) = sg.hit;
                    crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("hdr-ac-{i}")))
                            .h(px(26.))
                            .px(px(10.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.))
                            .cursor_pointer()
                            .font_family(BUFFER_FONT)
                            .text_size(px(12.))
                            .when(Some(i) == sel, |d| d.bg(t.element_hover))
                            .hover(|s| s.bg(t.element_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _, w, cx| {
                                    this.accept_suggestion(i, w, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .whitespace_nowrap()
                                    .child(div().child(sg.label[..a].to_string()))
                                    .child(
                                        div()
                                            .text_color(t.accent)
                                            .child(sg.label[a..z].to_string())
                                    )
                                    .child(div().child(sg.label[z..].to_string())),
                            )
                            .child(div().flex_1())
                            .child(
                                div()
                                    .truncate()
                                    .text_color(t.text_muted)
                                    .child(sg.detail.to_string()),
                            ),
                        format!("hdr-ac-{i}"),
                        Row,
                        sg.label.to_string(),
                        None
                    )
                })),
        )
        .with_priority(10)
        .into_any_element()
    }

    pub(crate) fn render_headers_editor(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = header_rows(resp, self.pending_header.as_ref());
        let n = rows.len();
        let sugg = self.inline_suggestions();
        // The row being edited and its column, if a header cell is active.
        let active = self.inline.as_ref().and_then(|e| match &e.target {
            InlineTarget::HeaderKey { response, row } if *response == resp.id => {
                Some((*row, Col::Key))
            }
            InlineTarget::HeaderValue { response, row } if *response == resp.id => {
                Some((*row, Col::Value))
            }
            _ => None,
        });
        let rid = resp.id.clone();
        let draft = active.is_some_and(|(r, c)| r >= n && c == Col::Key);
        let row_el = |i: usize, key: &str, value: &str, cx: &mut Context<Self>| -> AnyElement {
            let popup = (active.map(|(r, _)| r) == Some(i) && !sugg.is_empty()).then(|| {
                let left = if active.map(|(_, c)| c) == Some(Col::Key) {
                    0.0
                } else {
                    KEY_W + 8.0
                };
                self.header_popup(t, &sugg, left, cx)
            });
            let (r_del, i_del) = (rid.clone(), i);
            div()
                .relative()
                .h(px(ROW_PX))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .border_b_1()
                .border_color(t.border_variant)
                .child(self.inline_field(
                    t,
                    format!("hdr-key-{i}"),
                    InlineTarget::HeaderKey {
                        response: rid.clone(),
                        row: i,
                    },
                    key,
                    "Header name",
                    KEY_W,
                    true,
                    cx,
                ))
                .child(self.inline_field(
                    t,
                    format!("hdr-value-{i}"),
                    InlineTarget::HeaderValue {
                        response: rid.clone(),
                        row: i,
                    },
                    value,
                    "Value",
                    VALUE_W,
                    true,
                    cx,
                ))
                .child(crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("hdr-remove-{i}")))
                        .size(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .cursor_pointer()
                        .hover(|s| s.bg(t.element_hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_header_row(&r_del, i_del, cx)
                        }))
                        .child(icon("x", 12., t.icon_muted)),
                    format!("hdr-remove-{i}"),
                    Button,
                    format!("Remove header {key}"),
                    None
                ))
                .children(popup)
                .into_any_element()
        };
        let mut table = div().flex().flex_col().child(
            div()
                .h(px(28.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .text_size(px(11.))
                .text_color(t.text_muted)
                .border_b_1()
                .border_color(t.border_variant)
                .child(div().w(px(KEY_W)).pl(px(8.)).child("Key"))
                .child(div().w(px(VALUE_W)).pl(px(8.)).child("Value")),
        );
        for (i, r) in rows.iter().enumerate() {
            table = table.child(row_el(i, &r.key, &r.value, cx));
        }
        if draft {
            table = table.child(row_el(n, "", "", cx));
        } else {
            let rid_add = resp.id.clone();
            table = table.child(crate::dev_id!(
                div()
                    .id("hdr-add")
                    .h(px(ROW_PX))
                    .px(px(8.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .hover(|s| s.bg(t.element_hover).text_color(t.text))
                    .on_click(cx.listener(move |this, _, w, cx| {
                        let at = this.rows_of(&rid_add).len();
                        this.begin_header_cell(&rid_add, at, Col::Key, w, cx)
                    }))
                    .child(icon("plus", 12., t.icon_muted))
                    .child("Add header"),
                "hdr-add",
                Button,
                "Add header",
                None
            ));
        }
        div()
            .id("headers-editor")
            .size_full()
            .overflow_y_scroll()
            .px(px(16.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_size(px(12.)).text_color(t.text_muted).child(
                "Sent with this Response only. Tab moves between cells; completions offer \
                 standard names and values. Content-Type sets the Response's content type.",
            ))
            .child(table)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resp() -> Response {
        Response {
            template: true,
            id: "rsp_1".into(),
            name: "OK".into(),
            description: String::new(),
            status: 200,
            default: true,
            disabled: false,
            rules_logic: "and".into(),
            rules: vec![],
            actions: vec![],
            callbacks: vec![],
            action_error_status: String::new(),
            content_type: "application/json".into(),
            delay: String::new(),
            fault: String::new(),
            fault_rate: String::new(),
            body: String::new(),
            body_file: None,
            schema: None,
            headers: vec![("x-a".into(), "1".into())],
        }
    }

    #[test]
    fn rows_put_content_type_first_and_pending_last() {
        let p = ("rsp_1".to_string(), "x-new".to_string());
        let rows = header_rows(&resp(), Some(&p));
        assert_eq!(
            rows.iter().map(|r| r.key.as_str()).collect::<Vec<_>>(),
            ["Content-Type", "x-a", "x-new"]
        );
        assert!(rows[0].content_type && rows[2].pending);
        let other = ("rsp_2".to_string(), "x".to_string());
        assert_eq!(header_rows(&resp(), Some(&other)).len(), 2);
    }

    #[test]
    fn keys_are_cleaned_and_canonical() {
        assert_eq!(clean_key(" content-type "), "Content-Type");
        assert_eq!(clean_key("x my header"), "xmyheader");
        assert_eq!(clean_key("X-Custom"), "X-Custom");
        assert!(is_content_type("CONTENT-TYPE"));
    }
}
