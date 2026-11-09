//! The Route Headers sub-tab: a key/value table for `Route.headers` (SPEC §5.5). Values are
//! Tera templates (checked inline); the headers are sent with every Response of the Route.
//! Same cell editing as the Response headers table (`header_table`): click a cell, Tab walks
//! Key → Value → next row, Enter ends the edit, a press elsewhere saves. A new row exists only
//! in the table until it has a Key and a Value.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, px,
};

use crate::code_editor::tera_ctx;
use crate::header_catalog::Col;
use crate::header_table::clean_key;
use crate::response_pane::{InlineTarget, Nav};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::icon;
use crate::view::Route;

const ROW_PX: f32 = 36.0;
const KEY_W: f32 = 280.0;
const VALUE_W: f32 = 460.0;

/// Rows of a Route's header table: its headers in file order, then the pending row.
pub fn route_header_rows(
    route: &Route,
    pending: Option<&(String, String)>,
) -> Vec<(String, String, bool)> {
    let mut rows: Vec<_> = route
        .headers
        .iter()
        .map(|(k, v)| (k.clone(), v.clone(), false))
        .collect();
    if let Some((r, k)) = pending
        && *r == route.id
    {
        rows.push((k.clone(), String::new(), true));
    }
    rows
}

/// First Tera syntax error of a header value, if any.
pub fn value_error(value: &str) -> Option<String> {
    tera_ctx::diagnostics(value)
        .into_iter()
        .next()
        .map(|(_, m)| m)
}

impl Shell {
    pub(crate) fn route_rows(&self, route: &str) -> Vec<(String, String, bool)> {
        self.ws
            .find_route(route)
            .map(|(_, r)| route_header_rows(r, self.pending_route_header.as_ref()))
            .unwrap_or_default()
    }

    fn route_row_of(&self, route: &str, key: &str) -> Option<usize> {
        self.route_rows(route)
            .iter()
            .position(|r| r.0.eq_ignore_ascii_case(key))
    }

    pub(crate) fn begin_route_header_cell(
        &mut self,
        route: &str,
        row: usize,
        col: Col,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.route_rows(route);
        let text = rows.get(row).map_or(String::new(), |r| match col {
            Col::Key => r.0.clone(),
            Col::Value => r.1.clone(),
        });
        let target = match col {
            Col::Key => InlineTarget::RouteHeaderKey {
                route: route.into(),
                row,
            },
            Col::Value => InlineTarget::RouteHeaderValue {
                route: route.into(),
                row,
            },
        };
        self.begin_inline(target, &text, true, window, cx);
    }

    pub(crate) fn commit_route_header_key(
        &mut self,
        route: &str,
        row: usize,
        text: &str,
        nav: Nav,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.route_rows(route);
        let new = clean_key(text);
        let is_new = row >= rows.len();
        let pending = rows.get(row).is_some_and(|r| r.2);
        if new.is_empty() {
            if pending {
                self.pending_route_header = None;
            }
            return;
        }
        let dup = rows
            .iter()
            .enumerate()
            .any(|(i, r)| i != row && r.0.eq_ignore_ascii_case(&new));
        if dup {
            self.notify(format!("`{new}` is already in this Route."), cx);
            if pending {
                self.pending_route_header = None;
            }
            return;
        }
        if is_new || pending {
            self.pending_route_header = Some((route.to_string(), new));
            if nav != Nav::Prev {
                let at = if is_new { rows.len() } else { row };
                self.begin_route_header_cell(route, at, Col::Value, window, cx);
            }
            return;
        }
        let (old_key, value) = (rows[row].0.clone(), rows[row].1.clone());
        if old_key != new {
            let (id, n) = (route.to_string(), new.clone());
            self.store_op(cx, |s| {
                s.set_route_header(&id, &old_key, None)?;
                s.set_route_header(&id, &n, Some(&value))
            });
        }
        match nav {
            Nav::Next => {
                let at = self.route_row_of(route, &new).unwrap_or(row);
                self.begin_route_header_cell(route, at, Col::Value, window, cx);
            }
            Nav::Prev => {
                if row > 0 {
                    self.begin_route_header_cell(route, row - 1, Col::Value, window, cx);
                }
            }
            Nav::Stay => {}
        }
    }

    pub(crate) fn commit_route_header_value(
        &mut self,
        route: &str,
        row: usize,
        text: &str,
        nav: Nav,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let rows = self.route_rows(route);
        let Some(r) = rows.get(row).cloned() else {
            return;
        };
        let value = text.trim().to_string();
        let id = route.to_string();
        if r.2 {
            self.pending_route_header = None;
        }
        if r.1 != value || r.2 {
            let key = r.0.clone();
            self.store_op(cx, |s| s.set_route_header(&id, &key, Some(&value)));
        }
        let n = self.route_rows(route).len();
        match nav {
            Nav::Next => {
                let at = self.route_row_of(route, &r.0).map_or(row, |i| i + 1);
                self.begin_route_header_cell(route, at.min(n), Col::Key, window, cx);
            }
            Nav::Prev => {
                let at = self.route_row_of(route, &r.0).unwrap_or(row);
                self.begin_route_header_cell(route, at, Col::Key, window, cx);
            }
            Nav::Stay => {}
        }
    }

    pub(crate) fn remove_route_header_row(
        &mut self,
        route: &str,
        row: usize,
        cx: &mut Context<Self>,
    ) {
        let rows = self.route_rows(route);
        let Some(r) = rows.get(row).cloned() else {
            return;
        };
        self.inline = None;
        if r.2 {
            self.pending_route_header = None;
            cx.notify();
            return;
        }
        let id = route.to_string();
        self.store_op(cx, |s| s.set_route_header(&id, &r.0, None));
    }

    pub(crate) fn render_route_headers(
        &self,
        t: &Theme,
        route: &Route,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rows = route_header_rows(route, self.pending_route_header.as_ref());
        let n = rows.len();
        let draft = self.inline.as_ref().is_some_and(|e| {
            matches!(&e.target, InlineTarget::RouteHeaderKey { route: r, row }
                if *r == route.id && *row >= n)
        });
        let rid = route.id.clone();
        let row_el = |i: usize, key: &str, value: &str, cx: &mut Context<Self>| -> AnyElement {
            let r_del = rid.clone();
            // The live text of the cell being edited is checked, else the stored value.
            let editing = self.inline.as_ref().and_then(|e| match &e.target {
                InlineTarget::RouteHeaderValue { route: r, row } if *r == rid && *row == i => {
                    Some(e.buf.text.clone())
                }
                _ => None,
            });
            let err = value_error(editing.as_deref().unwrap_or(value));
            div()
                .flex()
                .flex_col()
                .border_b_1()
                .border_color(t.border_variant)
                .child(
                    div()
                        .h(px(ROW_PX))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(self.inline_field(
                            t,
                            format!("route-header-key-{i}"),
                            InlineTarget::RouteHeaderKey {
                                route: rid.clone(),
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
                            format!("route-header-value-{i}"),
                            InlineTarget::RouteHeaderValue {
                                route: rid.clone(),
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
                                .id(crate::ui::eid(format!("route-header-remove-{i}")))
                                .size(px(22.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .rounded(px(3.))
                                .cursor_pointer()
                                .hover(|s| s.bg(t.element_hover))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_route_header_row(&r_del, i, cx)
                                }))
                                .child(icon("x", 12., t.icon_muted)),
                            format!("route-header-remove-{i}"),
                            Button,
                            format!("Remove header {key}"),
                            Press(format!("route-header-remove:{rid}:{i}"))
                        )),
                )
                .children(err.map(|m| {
                    crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("route-header-error-{i}")))
                            .pl(px(KEY_W + 16.))
                            .pb(px(4.))
                            .text_size(px(11.))
                            .text_color(t.deleted)
                            .child(format!("✗ {m}")),
                        format!("route-header-error-{i}"),
                        Text,
                        m,
                        None
                    )
                }))
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
        for (i, (k, v, _)) in rows.iter().enumerate() {
            table = table.child(row_el(i, k, v, cx));
        }
        if draft {
            table = table.child(row_el(n, "", "", cx));
        } else {
            let rid_add = route.id.clone();
            table = table.child(crate::dev_id!(
                div()
                    .id("route-header-add")
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
                        let at = this.route_rows(&rid_add).len();
                        this.begin_route_header_cell(&rid_add, at, Col::Key, w, cx)
                    }))
                    .child(icon("plus", 12., t.icon_muted))
                    .child("Add header"),
                "route-header-add",
                Button,
                "Add header",
                Press(format!("route-header-add:{}", route.id))
            ));
        }
        div()
            .id("route-headers-editor")
            .size_full()
            .overflow_y_scroll()
            .px(px(16.))
            .py(px(10.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(div().text_size(px(12.)).text_color(t.text_muted).child(
                "Sent with every Response of this Route; a Response header of the same name \
                 wins. Values are Tera templates.",
            ))
            .child(table)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_errors_only_for_bad_templates() {
        assert!(value_error("plain").is_none());
        assert!(value_error("{{ uuid() }}").is_none());
        assert!(value_error("{{ oops ").is_some());
    }
}
