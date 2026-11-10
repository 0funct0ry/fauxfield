//! The New Server dialog: a modal with Name, Host and Port. The three inputs are
//! [`TextInput`] entities; this module only lays them out and validates them.

use gpui::{
    Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::*, px,
};

use crate::shell::Shell;
use crate::text_input::{InputEvent, InputFilter, TextInput};
use crate::theme::Theme;
use crate::ui::{eid, toolbar_button};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldKind {
    Name,
    Host,
    Port,
}

const FIELDS: [(FieldKind, &str, &str); 3] = [
    (FieldKind::Name, "name", "Name"),
    (FieldKind::Host, "host", "Host"),
    (FieldKind::Port, "port", "Port"),
];

pub struct ServerDialog {
    pub inputs: Vec<Entity<TextInput>>,
    pub focus: usize,
    touched: [bool; 3],
    _subs: Vec<Subscription>,
}

/// Format problems only; clashes with other servers are checked by the Shell.
fn format_error(kind: FieldKind, text: &str) -> Option<String> {
    let v = text.trim();
    match kind {
        FieldKind::Name => v.is_empty().then(|| "A server needs a name.".to_string()),
        FieldKind::Host => fauxfield_model::validate_host(v).err(),
        FieldKind::Port => match v.parse::<u16>() {
            Ok(p) if p > 0 => None,
            _ => Some("A port is a number from 1 to 65535.".to_string()),
        },
    }
}

impl Shell {
    pub(crate) fn begin_server_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.picker.is_some() {
            self.close_picker(true, window, cx);
        }
        let port = self
            .ws
            .servers
            .iter()
            .map(|s| s.port)
            .max()
            .and_then(|p| p.checked_add(1))
            .unwrap_or(3001);
        let inputs: Vec<Entity<TextInput>> = vec![
            cx.new(|cx| TextInput::new(cx).placeholder("e.g. users-api")),
            cx.new(|cx| {
                TextInput::new(cx)
                    .placeholder("127.0.0.1")
                    .filter(InputFilter::NoWhitespace)
                    .max_len(253)
                    .text_selected("127.0.0.1")
            }),
            cx.new(|cx| {
                TextInput::new(cx)
                    .placeholder("3001")
                    .filter(InputFilter::Digits)
                    .max_len(5)
                    .text_selected(&port.to_string())
            }),
        ];
        let mut subs = Vec::new();
        for (i, input) in inputs.iter().enumerate() {
            subs.push(cx.subscribe_in(
                input,
                window,
                move |this: &mut Shell, _, ev: &InputEvent, window, cx| match ev {
                    InputEvent::Tab { reverse } => {
                        this.dialog_move_focus(if *reverse { -1 } else { 1 }, window, cx)
                    }
                    InputEvent::PressEnter { .. } => this.submit_server_dialog(window, cx),
                    InputEvent::Escape => this.close_server_dialog(window, cx),
                    InputEvent::Focus => {
                        if let Some(d) = this.server_dialog.as_mut() {
                            d.focus = i;
                        }
                    }
                    InputEvent::Blur => {
                        if let Some(d) = this.server_dialog.as_mut() {
                            d.touched[i] = true;
                        }
                        this.refresh_dialog_validity(cx);
                    }
                    InputEvent::Change => this.refresh_dialog_validity(cx),
                    _ => {}
                },
            ));
        }
        inputs[0].read(cx).focus(window);
        self.server_dialog = Some(ServerDialog {
            inputs,
            focus: 0,
            touched: [false; 3],
            _subs: subs,
        });
        cx.notify();
    }

    pub(crate) fn close_server_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.server_dialog.take().is_some() {
            window.focus(&self.focus);
            cx.notify();
        }
    }

    fn dialog_text(&self, i: usize, cx: &gpui::App) -> String {
        self.server_dialog
            .as_ref()
            .map(|d| d.inputs[i].read(cx).value().to_string())
            .unwrap_or_default()
    }

    /// Error for field `i`, including a port already used by another server on this host.
    fn dialog_field_error(&self, i: usize, cx: &gpui::App) -> Option<String> {
        if let Some(e) = format_error(FIELDS[i].0, &self.dialog_text(i, cx)) {
            return Some(e);
        }
        if FIELDS[i].0 == FieldKind::Port {
            let port: u16 = self.dialog_text(2, cx).trim().parse().ok()?;
            let host = self.dialog_text(1, cx);
            if let Some(s) = self
                .ws
                .servers
                .iter()
                .find(|s| s.port == port && s.host == host.trim())
            {
                return Some(format!("{}:{port} is already used by {}.", s.host, s.name));
            }
        }
        None
    }

    /// Red border on the inputs that were visited and are invalid.
    fn refresh_dialog_validity(&mut self, cx: &mut Context<Self>) {
        let Some(d) = self.server_dialog.as_ref() else {
            return;
        };
        let inputs = d.inputs.clone();
        let touched = d.touched;
        for (i, input) in inputs.iter().enumerate() {
            let bad = touched[i] && self.dialog_field_error(i, cx).is_some();
            input.update(cx, |t, cx| t.set_invalid(bad, cx));
        }
        cx.notify();
    }

    pub(crate) fn submit_server_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.server_dialog.as_mut() else {
            return;
        };
        d.touched = [true; 3];
        // Host changes can clash with the port, so every field is re-checked.
        let bad = (0..3).find(|&i| self.dialog_field_error(i, cx).is_some());
        self.refresh_dialog_validity(cx);
        if let Some(i) = bad {
            let input = self.server_dialog.as_ref().expect("dialog is open").inputs[i].clone();
            input.read(cx).focus(window);
            return;
        }
        let name = self.dialog_text(0, cx).trim().to_string();
        let host = self.dialog_text(1, cx).trim().to_string();
        let port: u16 = self.dialog_text(2, cx).trim().parse().unwrap_or(3001);
        if let Some(id) = self.store_op(cx, |s| s.create_server_at(&name, &host, port)) {
            self.close_server_dialog(window, cx);
            self.reveal_row(&id);
        }
    }

    fn dialog_move_focus(&mut self, delta: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(d) = self.server_dialog.as_ref() else {
            return;
        };
        let n = d.inputs.len() as isize;
        let next = d.inputs[(d.focus as isize + delta).rem_euclid(n) as usize].clone();
        next.update(cx, |t, cx| {
            t.focus(window);
            t.select_all(cx);
        });
    }

    pub(crate) fn render_server_dialog(
        &self,
        t: &Theme,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let d = self.server_dialog.as_ref().expect("dialog is open");
        let fields: Vec<_> = (0..FIELDS.len())
            .map(|i| {
                #[cfg_attr(not(feature = "dev-mcp"), allow(unused_variables))]
                let (_, key, label) = FIELDS[i];
                let err = d.touched[i]
                    .then(|| self.dialog_field_error(i, cx))
                    .flatten();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(label),
                    )
                    .child(crate::dev_id!(
                        div().child(d.inputs[i].clone()),
                        format!("new-server-{key}"),
                        Input,
                        label,
                        None
                    ))
                    .when_some(err, |c, e| {
                        c.child(div().text_size(px(11.)).text_color(t.deleted).child(e))
                    })
            })
            .collect();
        let button = |id: &'static str, label: &'static str| {
            toolbar_button(t, eid(id))
                .h(px(26.))
                .px(px(12.))
                .child(label)
        };
        let create = button("new-server-create", "Create server")
            .bg(t.element_selected)
            .on_click(cx.listener(|this, _, w, cx| this.submit_server_dialog(w, cx)));
        let cancel = button("new-server-cancel", "Cancel")
            .on_click(cx.listener(|this, _, w, cx| this.close_server_dialog(w, cx)));
        crate::dev_id_z!(
            div()
                .id("new-server-backdrop")
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
                        .id("new-server-dialog")
                        .key_context("Dialog")
                        .track_focus(&self.server_dialog_focus)
                        // Keys that reach the dialog itself (focus on its padding).
                        .on_key_down(cx.listener(|this, ev: &KeyDownEvent, w, cx| {
                            match ev.keystroke.key.as_str() {
                                "escape" => this.close_server_dialog(w, cx),
                                "enter" => this.submit_server_dialog(w, cx),
                                "tab" => this.dialog_move_focus(0, w, cx),
                                _ => return,
                            }
                            cx.stop_propagation();
                        }))
                        .w(px(420.))
                        .p(px(16.))
                        .flex()
                        .flex_col()
                        .gap(px(12.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(t.border)
                        .bg(t.elevated)
                        .shadow_lg()
                        .child(div().text_size(px(14.)).child("New server"))
                        .children(fields)
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .child(
                                    div()
                                        .flex_1()
                                        .text_size(px(11.))
                                        .text_color(t.text_muted)
                                        .child("Tab next field · Enter create · Esc cancel"),
                                )
                                .child(div().flex().flex_row().gap(px(8.)).child(cancel).child(
                                    crate::dev_id!(
                                        create,
                                        "new-server-create",
                                        Button,
                                        "Create server",
                                        None
                                    ),
                                )),
                        ),
                ),
            2,
            "new-server-dialog",
            Modal,
            "New server",
            None
        )
    }
}
