//! Callbacks of a Response (M14, SPEC §7.3): the Callbacks sub-tab is a table with a `+` button;
//! a row's View, Edit and Delete buttons and its `...` menu act on one Callback, and View, Edit
//! and `+` open [`crate::callback_dialog`]. Also the Route-level Callbacks overview.

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::callback_dialog::CbMode;
use crate::dropdown_menu::{DropdownEvent, DropdownSite, MenuItem};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::{icon_button, icon_button_tinted};
use crate::view::{Response, Route};
use fauxfield_model::Callback;

const ROW_H: f32 = 34.0;
/// Column widths; `None` flexes.
const W_NUM: f32 = 30.0;
const W_METHOD: f32 = 70.0;
const W_NUM_COL: f32 = 84.0;
const W_STATE: f32 = 84.0;
const W_ACTIONS: f32 = 148.0;

/// Shortcuts of the row menu, active while the table has focus and a row is selected.
const KEY_DUPLICATE: &str = "secondary-d";
const KEY_UP: &str = "alt-up";
const KEY_DOWN: &str = "alt-down";
const KEY_DISABLE: &str = "alt-d";

impl Shell {
    pub(crate) fn edit_callback(
        &mut self,
        resp: &str,
        idx: usize,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Callback),
    ) {
        let Some((_, _, r)) = self.find_response(resp) else {
            return;
        };
        let Some(mut c) = r.callbacks.get(idx).cloned() else {
            return;
        };
        f(&mut c);
        let id = resp.to_string();
        self.store_op(cx, |s| s.set_callback(&id, Some(idx), &c));
    }

    /// An id `cb_<n>` that no Callback of the Response uses.
    pub(crate) fn next_callback_id(&self, resp: &str) -> String {
        let used: Vec<String> = self
            .find_response(resp)
            .map(|(_, _, r)| r.callbacks.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default();
        let mut n = used.len() + 1;
        while used.contains(&format!("cb_{n}")) {
            n += 1;
        }
        format!("cb_{n}")
    }

    pub(crate) fn duplicate_callback(&mut self, resp: &str, idx: usize, cx: &mut Context<Self>) {
        let Some((_, _, r)) = self.find_response(resp) else {
            return;
        };
        let Some(mut c) = r.callbacks.get(idx).cloned() else {
            return;
        };
        let mut n = 2;
        let base = c.id.clone();
        while r.callbacks.iter().any(|x| x.id == format!("{base}_{n}")) {
            n += 1;
        }
        c.id = format!("{base}_{n}");
        c.name = c.name.take().map(|x| format!("{x} (copy)"));
        let id = resp.to_string();
        let last = r.callbacks.len();
        self.store_op(cx, |s| {
            s.set_callback(&id, None, &c)?;
            if idx + 1 < last {
                s.move_callback(&id, last, idx + 1)?;
            }
            Ok(())
        });
        self.cb_selected.insert(resp.to_string(), idx + 1);
    }

    pub(crate) fn move_callback_row(
        &mut self,
        resp: &str,
        idx: usize,
        down: bool,
        cx: &mut Context<Self>,
    ) {
        let Some((_, _, r)) = self.find_response(resp) else {
            return;
        };
        let to = if down { idx + 1 } else { idx.wrapping_sub(1) };
        if to >= r.callbacks.len() {
            return;
        }
        let id = resp.to_string();
        self.store_op(cx, |s| s.move_callback(&id, idx, to));
        self.cb_selected.insert(resp.to_string(), to);
    }

    /// Ask before deleting (the confirmation modal calls [`Self::delete_callback_row`]).
    pub(crate) fn ask_delete_callback(&mut self, resp: &str, idx: usize, cx: &mut Context<Self>) {
        self.lc.callback_delete = Some((resp.to_string(), idx));
        cx.notify();
    }

    pub(crate) fn delete_callback_row(&mut self, resp: &str, idx: usize, cx: &mut Context<Self>) {
        let id = resp.to_string();
        self.store_op(cx, |s| s.delete_callback(&id, idx));
        let n = self
            .find_response(resp)
            .map_or(0, |(_, _, r)| r.callbacks.len());
        if let Some(sel) = self.cb_selected.get_mut(resp) {
            *sel = (*sel).min(n.saturating_sub(1));
        }
        cx.notify();
    }

    /// Keys of the table: Enter edits, Space views, Delete removes, arrows select, and the
    /// shortcuts of the row menu.
    fn on_callbacks_key(
        &mut self,
        resp: &str,
        ev: &KeyDownEvent,
        w: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((_, _, r)) = self.find_response(resp) else {
            return;
        };
        let n = r.callbacks.len();
        let sel = self.cb_selected.get(resp).copied().filter(|i| *i < n);
        let k = ev.keystroke.key.as_str();
        let m = &ev.keystroke.modifiers;
        let secondary = m.platform || m.control;
        let Some(i) = sel else {
            if n > 0 && matches!(k, "down" | "up") {
                self.cb_selected.insert(resp.to_string(), 0);
                cx.notify();
                cx.stop_propagation();
            }
            return;
        };
        match (k, secondary, m.alt) {
            ("down", false, false) => {
                self.cb_selected
                    .insert(resp.to_string(), (i + 1).min(n - 1));
            }
            ("up", false, false) => {
                self.cb_selected
                    .insert(resp.to_string(), i.saturating_sub(1));
            }
            ("down", false, true) => self.move_callback_row(resp, i, true, cx),
            ("up", false, true) => self.move_callback_row(resp, i, false, cx),
            ("d", true, false) => self.duplicate_callback(resp, i, cx),
            ("d", false, true) => self.edit_callback(resp, i, cx, |c| c.disabled = !c.disabled),
            ("enter", false, false) => {
                self.begin_callback_dialog(resp, Some(i), CbMode::Edit, w, cx)
            }
            ("space", false, false) => {
                self.begin_callback_dialog(resp, Some(i), CbMode::View, w, cx)
            }
            ("delete" | "backspace", false, false) => self.ask_delete_callback(resp, i, cx),
            _ => return,
        }
        cx.notify();
        cx.stop_propagation();
    }

    fn callback_menu(
        &self,
        resp: &Response,
        i: usize,
        c: &Callback,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mac = crate::keymap::Platform::current() == crate::keymap::Platform::Macos;
        let keys = |k: &str| crate::keymap::display_keys(k, mac);
        let n = resp.callbacks.len();
        let items = vec![
            MenuItem::item("duplicate", "Duplicate")
                .icon("copy")
                .shortcut(keys(KEY_DUPLICATE)),
            MenuItem::Separator,
            MenuItem::item("up", "Move up")
                .icon("arrow-up")
                .shortcut(keys(KEY_UP))
                .disabled((i == 0).then_some("First")),
            MenuItem::item("down", "Move down")
                .icon("arrow-down")
                .shortcut(keys(KEY_DOWN))
                .disabled((i + 1 >= n).then_some("Last")),
            MenuItem::Separator,
            MenuItem::item("toggle", if c.disabled { "Enable" } else { "Disable" })
                .icon(if c.disabled { "check" } else { "ban" })
                .shortcut(keys(KEY_DISABLE)),
        ];
        let rid = resp.id.clone();
        self.dropdown_field(
            cx,
            DropdownSite {
                key: format!("callback-menu-{}-{i}", resp.id),
                icon: "ellipsis-vertical",
                items,
                tip: Some("More actions".into()),
                min_width: 260.,
                on_event: std::rc::Rc::new(move |this, ev, cx| {
                    let DropdownEvent::Action(id) = ev else {
                        return;
                    };
                    match id.as_ref() {
                        "duplicate" => this.duplicate_callback(&rid, i, cx),
                        "up" => this.move_callback_row(&rid, i, false, cx),
                        "down" => this.move_callback_row(&rid, i, true, cx),
                        "toggle" => this.edit_callback(&rid, i, cx, |c| c.disabled = !c.disabled),
                        _ => {}
                    }
                }),
            },
        )
    }

    /// Callbacks sub-tab of a Response: a toolbar with `+` and the table.
    pub(crate) fn render_callbacks_editor(
        &self,
        t: &Theme,
        resp: &Response,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rid = resp.id.clone();
        let add = crate::dev_id!(
            icon_button(t, "callback-add", "plus", "Add callback", false, true).on_click(
                cx.listener(move |this, _, w, cx| {
                    this.begin_callback_dialog(&rid, None, CbMode::Create, w, cx)
                })
            ),
            "callback-add",
            Button,
            "Add callback",
            Press(format!("callback-add:{}", resp.id))
        );
        let head = |label: &'static str, w: Option<f32>| {
            let d = div()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(label);
            match w {
                Some(w) => d.w(px(w)).flex_none(),
                None => d.flex_1().min_w(px(0.)),
            }
        };
        let sel = self.cb_selected.get(&resp.id).copied();
        let mut rows = div().flex().flex_col();
        for (i, c) in resp.callbacks.iter().enumerate() {
            rows = rows.child(self.render_callback_row(t, resp, i, c, sel == Some(i), cx));
        }
        let rid2 = resp.id.clone();
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("callbacks-{}", resp.id)))
                .key_context("CallbacksTable")
                .track_focus(&self.cb_table_focus)
                .on_key_down(cx.listener(move |this, ev: &KeyDownEvent, w, cx| {
                    this.on_callbacks_key(&rid2, ev, w, cx)
                }))
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
                        .gap(px(8.))
                        .child(add)
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child(format!(
                                    "{} callback{} fire after this Response is sent",
                                    resp.callbacks.len(),
                                    if resp.callbacks.len() == 1 { "" } else { "s" }
                                )),
                        ),
                )
                .child(
                    div()
                        .h(px(26.))
                        .flex_none()
                        .px(px(12.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .border_b_1()
                        .border_color(t.border_variant)
                        .child(head("#", Some(W_NUM)))
                        .child(head("NAME", None))
                        .child(head("METHOD", Some(W_METHOD)))
                        .child(head("URL", None))
                        .child(head("DELAY", Some(W_NUM_COL)))
                        .child(head("RETRIES", Some(W_NUM_COL)))
                        .child(head("TIMEOUT", Some(W_NUM_COL)))
                        .child(head("STATE", Some(W_STATE)))
                        .child(head("", Some(W_ACTIONS))),
                )
                .child(
                    div()
                        .id(crate::ui::eid(format!("callbacks-rows-{}", resp.id)))
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .child(rows)
                        .when(resp.callbacks.is_empty(), |d| {
                            d.child(
                                div()
                                    .p(px(16.))
                                    .text_size(px(12.))
                                    .text_color(t.text_muted)
                                    .child(crate::copy::empty_line("callbacks")),
                            )
                        }),
                ),
            format!("callbacks-table-{}", resp.id),
            Text,
            format!("{} callbacks", resp.callbacks.len()),
            None
        )
        .into_any_element()
    }

    fn render_callback_row(
        &self,
        t: &Theme,
        resp: &Response,
        i: usize,
        c: &Callback,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (r_sel, r_view, r_edit, r_del) = (
            resp.id.clone(),
            resp.id.clone(),
            resp.id.clone(),
            resp.id.clone(),
        );
        let method = c
            .method
            .clone()
            .unwrap_or_else(|| "POST".into())
            .to_ascii_uppercase();
        let label = c
            .name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| c.id.clone());
        let ms = |v: Option<u64>| v.map_or("-".to_string(), |v| format!("{v} ms"));
        let cell = |w: Option<f32>| match w {
            Some(w) => div().w(px(w)).flex_none().truncate(),
            None => div().flex_1().min_w(px(0.)).truncate(),
        };
        let (hover, sel_bg) = (t.element_hover, t.element_selected);
        let muted = c.disabled;
        let actions = div()
            .w(px(W_ACTIONS))
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .justify_end()
            // A press on a row button or the `...` menu must not reach the row: it selects the
            // row and moves focus, which would close a freshly opened menu.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(crate::dev_id!(
                icon_button(
                    t,
                    crate::ui::eid(format!("callback-view-{}-{i}", resp.id)),
                    "eye",
                    "View",
                    false,
                    true
                )
                .on_click(cx.listener(move |this, _, w, cx| {
                    cx.stop_propagation();
                    this.begin_callback_dialog(&r_view, Some(i), CbMode::View, w, cx)
                })),
                format!("callback-view-{}-{i}", resp.id),
                Button,
                "View callback",
                Press(format!("callback-view:{}:{i}", resp.id))
            ))
            .child(crate::dev_id!(
                icon_button(
                    t,
                    crate::ui::eid(format!("callback-edit-{}-{i}", resp.id)),
                    "pencil",
                    "Edit",
                    false,
                    true
                )
                .on_click(cx.listener(move |this, _, w, cx| {
                    cx.stop_propagation();
                    this.begin_callback_dialog(&r_edit, Some(i), CbMode::Edit, w, cx)
                })),
                format!("callback-edit-{}-{i}", resp.id),
                Button,
                "Edit callback",
                Press(format!("callback-edit:{}:{i}", resp.id))
            ))
            .child(crate::dev_id!(
                icon_button_tinted(
                    t,
                    crate::ui::eid(format!("callback-delete-{}-{i}", resp.id)),
                    "trash-2",
                    "Delete",
                    t.deleted,
                    false,
                    true
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.ask_delete_callback(&r_del, i, cx)
                })),
                format!("callback-delete-{}-{i}", resp.id),
                Button,
                "Delete callback",
                Press(format!("callback-delete:{}:{i}", resp.id))
            ))
            .child(self.callback_menu(resp, i, c, cx));
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("callback-row-{}-{i}", resp.id)))
                .h(px(ROW_H))
                .flex_none()
                .px(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.))
                .border_b_1()
                .border_color(t.border_variant)
                .cursor_pointer()
                .when(muted, |d| d.text_color(t.text_muted))
                .when(selected, |d| d.bg(sel_bg))
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _, w, cx| {
                    this.cb_selected.insert(r_sel.clone(), i);
                    w.focus(&this.cb_table_focus);
                    cx.notify();
                }))
                .child(
                    cell(Some(W_NUM))
                        .text_color(t.text_muted)
                        .child(format!("{}", i + 1))
                )
                .child(cell(None).child(label))
                .child(
                    cell(Some(W_METHOD))
                        .font_family(BUFFER_FONT)
                        .text_size(px(11.))
                        .text_color(t.method_color(&method))
                        .child(method),
                )
                .child(
                    cell(None)
                        .font_family(BUFFER_FONT)
                        .text_size(px(11.))
                        .child(c.url.clone())
                )
                .child(cell(Some(W_NUM_COL)).child(ms(c.delay_ms)))
                .child(cell(Some(W_NUM_COL)).child(c.retries.map_or("0".into(), |r| r.to_string())))
                .child(cell(Some(W_NUM_COL)).child(ms(c.timeout_ms)))
                .child(
                    cell(Some(W_STATE))
                        .text_color(if c.disabled {
                            t.text_placeholder
                        } else {
                            t.created
                        })
                        .child(if c.disabled { "Disabled" } else { "Enabled" }),
                )
                .child(actions),
            format!("callback-row-{}-{i}", resp.id),
            Row,
            format!("{} {}", c.id, c.url),
            Press(format!("callback-row:{}:{i}", resp.id))
        )
        .into_any_element()
    }

    /// Route-level Callbacks tab: every Callback of every Response of the Route; a row opens it.
    pub(crate) fn render_route_callbacks(
        &self,
        t: &Theme,
        r: &Route,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut col = div()
            .id(crate::ui::eid(format!("route-callbacks-{}", r.id)))
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(div().text_size(px(12.)).text_color(t.text_muted).child(
                "Callbacks belong to a Response. Click a row to manage it in the Responses tab.",
            ));
        let mut any = false;
        for (ri, resp) in r.responses.iter().enumerate() {
            for (i, c) in resp.callbacks.iter().enumerate() {
                any = true;
                let (route, resp_id) = (r.id.clone(), resp.id.clone());
                let label = format!(
                    "{}  {}  {} {}{}",
                    resp.name,
                    c.id,
                    c.method.clone().unwrap_or_else(|| "POST".into()),
                    c.url,
                    c.delay_ms.map(|d| format!("  +{d} ms")).unwrap_or_default()
                );
                col = col.child(crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("route-callback-{}-{i}", resp.id)))
                        .h(px(26.))
                        .px(px(8.))
                        .flex()
                        .items_center()
                        .rounded(px(4.))
                        .border_1()
                        .border_color(t.border_variant)
                        .text_size(px(12.))
                        .cursor_pointer()
                        .hover(|s| s.bg(t.element_hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.route_sub.insert(route.clone(), 0);
                            this.resp_sub.insert(resp_id.clone(), 3);
                            this.resp_sel.insert(route.clone(), ri);
                            this.cb_selected.insert(resp_id.clone(), i);
                            cx.notify();
                        }))
                        .child(label.clone()),
                    format!("route-callback-{}-{i}", resp.id),
                    Row,
                    label,
                    Press(format!("route-callback:{}:{i}", resp.id))
                ));
            }
        }
        if !any {
            col = col.child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_muted)
                    .child(crate::copy::empty_line("callbacks")),
            );
        }
        col.into_any_element()
    }
}
