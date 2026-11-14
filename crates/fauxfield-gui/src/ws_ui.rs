//! WebSocket Route tab (M13, SPEC §7.4): a mode switch, the scripted messages as a table with a
//! detail editor, live connections, the frame timeline as a table, and the composer.

use gpui::{
    AnyElement, Context, Entity, InteractiveElement, IntoElement, KeyDownEvent, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::code_editor::{CodeEditor, Language};
use crate::response_pane::InlineTarget;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::toolbar_button;
use crate::view::{Route, Server};

/// Timeline frames kept in the GUI.
pub const WS_FRAME_CAP: usize = 500;

/// Text fields of the WebSocket tab edited inline.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WsField {
    Every,
    WhenKey,
    WhenValue,
    CloseCode,
    /// The timeline's payload filter (message index unused).
    Search,
}

impl WsField {
    fn id(self) -> &'static str {
        match self {
            Self::Every => "ws-every",
            Self::WhenKey => "ws-when-key",
            Self::WhenValue => "ws-when-value",
            Self::CloseCode => "ws-close",
            Self::Search => "ws-search",
        }
    }
}

const TRIGGERS: &[&str] = &["connect", "interval", "message"];
const MODES: &[(&str, &str)] = &[
    ("echo", "Echo"),
    ("script", "Script"),
    ("broadcast", "Broadcast"),
];

/// A text editor for one-off WebSocket text (composer, message body, frame viewer).
pub(crate) fn new_editor(
    st: &crate::code_editor::EditorSettings,
    lang: Language,
    placeholder: &str,
    read_only: bool,
    cx: &mut Context<Shell>,
) -> Entity<CodeEditor> {
    cx.new(|cx| {
        let mut e = CodeEditor::new(lang, cx)
            .line_numbers(false)
            .read_only(read_only)
            .placeholder(placeholder);
        e.apply_settings(st, cx);
        e
    })
}

fn fmt_time(ts: &str) -> String {
    // RFC 3339 `2026-11-13T10:11:12.345Z` -> `10:11:12.345`
    ts.split('T')
        .nth(1)
        .map(|t| t.trim_end_matches('Z').to_string())
        .unwrap_or_else(|| ts.to_string())
}

/// One line for a table cell: every run of whitespace (indentation, newlines) becomes one space.
fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn when_label(m: &crate::view::WsMsg) -> String {
    match m.trigger.as_str() {
        "connect" => "On connect".into(),
        "interval" => format!("Every {}", m.every),
        _ => "On message".into(),
    }
}

fn condition_label(m: &crate::view::WsMsg) -> String {
    match m.trigger.as_str() {
        "interval" => "timer".into(),
        "message" if !m.when_key.is_empty() => format!("{} = {}", m.when_key, m.when_value),
        "message" => "any message".into(),
        _ => "any".into(),
    }
}

/// A fixed-width table cell (`w > 0`) or a flexible one (`w == 0`).
fn cell(w: f32, el: gpui::Div) -> gpui::Div {
    if w > 0. {
        el.w(px(w)).flex_none()
    } else {
        el.flex_1().min_w_0()
    }
}

impl Shell {
    fn ws_label(&self, t: &Theme, l: &str, el: AnyElement) -> gpui::Div {
        self.https_label(t, l, el)
    }

    #[allow(clippy::too_many_arguments)]
    fn ws_field(
        &self,
        t: &Theme,
        route: &str,
        idx: usize,
        f: WsField,
        value: &str,
        placeholder: &str,
        width: f32,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.inline_field(
            t,
            format!("{}-{route}-{idx}", f.id()),
            InlineTarget::Ws {
                route: route.to_string(),
                idx,
                field: f,
            },
            value,
            placeholder,
            width,
            true,
            cx,
        )
    }

    fn ws_button(
        &self,
        t: &Theme,
        id: String,
        label: &str,
        op: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let op2 = op.clone();
        crate::dev_id!(
            toolbar_button(t, crate::ui::eid(id.clone()))
                .child(label.to_string())
                .on_click(cx.listener(move |this, _, _, cx| this.ws_op_from_ui(&op2, cx))),
            id,
            Button,
            label,
            Press(op)
        )
        .into_any_element()
    }

    /// One segment of a segmented control.
    fn ws_segment(
        &self,
        t: &Theme,
        id: String,
        label: String,
        selected: bool,
        op: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let op2 = op.clone();
        crate::dev_id!(
            div()
                .id(crate::ui::eid(id.clone()))
                .h(px(24.))
                .px(px(12.))
                .flex()
                .items_center()
                .cursor_pointer()
                .text_size(px(12.))
                .when(selected, |d| d.bg(t.element_selected))
                .hover(|d| d.bg(t.element_hover))
                .on_click(cx.listener(move |this, _, _, cx| this.ws_op_from_ui(&op2, cx)))
                .child(label.clone()),
            id,
            Button,
            label,
            Press(op)
        )
        .into_any_element()
    }

    fn ws_segmented(&self, t: &Theme, segs: Vec<AnyElement>) -> gpui::Div {
        div()
            .flex()
            .flex_row()
            .rounded(px(5.))
            .border_1()
            .border_color(t.border_variant)
            .overflow_hidden()
            .children(segs)
    }

    pub(crate) fn render_ws_route_tab(
        &self,
        t: &Theme,
        s: &Server,
        r: &Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let rid = r.id.clone();
        let mode = r.ws_mode.clone();
        let scripted = mode != "echo";

        // Mode switch (segmented control) with a one-line explanation.
        let mode_switch = self.ws_segmented(
            t,
            MODES
                .iter()
                .map(|(m, label)| {
                    self.ws_segment(
                        t,
                        format!("ws-mode-{m}-{}", r.id),
                        (*label).to_string(),
                        mode == *m,
                        format!("ws-mode-{m}:{}", r.id),
                        cx,
                    )
                })
                .collect(),
        );
        let mode_hint = match mode.as_str() {
            "echo" => "Sends every incoming message back to its sender.",
            "broadcast" => {
                "Sends the messages below. Timers reach every client; messages with no match go to the other clients."
            }
            _ => "Sends the messages below. Replies go to the sender only.",
        };

        let sel_msg = self
            .ws_msg_sel
            .get(&r.id)
            .copied()
            .filter(|i| *i < r.ws_messages.len());

        // Messages table.
        let header_cell = |w: f32, label: &str| {
            cell(
                w,
                div()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child(label.to_string()),
            )
        };
        let table = div()
            .rounded(px(5.))
            .border_1()
            .border_color(t.border_variant)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(header_cell(110., "When"))
                    .child(header_cell(160., "Condition"))
                    .child(header_cell(0., "Body"))
                    .child(header_cell(50., "Close"))
                    .child(header_cell(24., "")),
            )
            .when(r.ws_messages.is_empty(), |d| {
                d.child(
                    div()
                        .p(px(10.))
                        .text_color(t.text_muted)
                        .child("No messages yet. Add one below."),
                )
            })
            .children(r.ws_messages.iter().enumerate().map(|(i, m)| {
                let selected = sel_msg == Some(i);
                let rid2 = rid.clone();
                crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("ws-message-{}-{i}", r.id)))
                        .h(px(26.))
                        .px(px(8.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.))
                        .cursor_pointer()
                        .border_b_1()
                        .border_color(t.border_variant)
                        .when(selected, |d| d.bg(t.element_selected))
                        .hover(|d| d.bg(t.element_hover))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.ws_open_message(&rid2, i, cx))
                        )
                        .child(cell(110., div().child(when_label(m))))
                        .child(cell(
                            160.,
                            div()
                                .text_color(t.text_muted)
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .font_family(BUFFER_FONT)
                                .child(condition_label(m)),
                        ))
                        .child(cell(
                            0.,
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .font_family(BUFFER_FONT)
                                .child(one_line(&m.body)),
                        ))
                        .child(cell(
                            50.,
                            div().text_color(t.text_muted).child(
                                m.close_code
                                    .map(|c| c.to_string())
                                    .unwrap_or_else(|| "none".into())
                            ),
                        ))
                        .child(cell(
                            24.,
                            div().child(
                                div()
                                    .id(crate::ui::eid(format!("ws-del-{}-{i}", r.id)))
                                    .cursor_pointer()
                                    .text_color(t.text_muted)
                                    .hover(|d| d.text_color(t.deleted))
                                    .on_click({
                                        let op = format!("ws-del:{}:{i}", r.id);
                                        cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            this.ws_op_from_ui(&op, cx)
                                        })
                                    })
                                    .child("✕"),
                            ),
                        )),
                    format!("ws-message-{}-{i}", r.id),
                    Button,
                    format!("{} {}", when_label(m), condition_label(m)),
                    Press(format!("ws-open:{}:{i}", r.id))
                )
            }))
            .child(
                div()
                    .h(px(28.))
                    .px(px(8.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(div().text_color(t.text_muted).child("Add message:"))
                    .child(self.ws_button(
                        t,
                        format!("ws-add-connect-{}", r.id),
                        "On connect",
                        format!("ws-add-connect:{}", r.id),
                        cx,
                    ))
                    .child(self.ws_button(
                        t,
                        format!("ws-add-interval-{}", r.id),
                        "Every…",
                        format!("ws-add-interval:{}", r.id),
                        cx,
                    ))
                    .child(self.ws_button(
                        t,
                        format!("ws-add-message-{}", r.id),
                        "On message",
                        format!("ws-add-message:{}", r.id),
                        cx,
                    )),
            );

        let detail = sel_msg.and_then(|i| {
            let m = r.ws_messages.get(i)?;
            let ed = self
                .ws_body_ed
                .as_ref()
                .filter(|(rr, ii, _)| rr == &r.id && *ii == i)?;
            Some(self.render_ws_message_detail(t, s, r, i, m, &ed.2, cx))
        });

        // Connections.
        let conns: Vec<_> = self
            .ws_conns
            .iter()
            .filter(|c| c.route_id == r.id)
            .cloned()
            .collect();
        let selected_conn = self
            .ws_sel
            .clone()
            .filter(|id| conns.iter().any(|c| &c.id == id));

        let conn_rows: Vec<_> = conns
            .iter()
            .map(|c| {
                let sel = selected_conn.as_deref() == Some(c.id.as_str());
                let cid = c.id.clone();
                div()
                    .id(crate::ui::eid(format!("ws-conn-{}", c.id)))
                    .h(px(24.))
                    .px(px(6.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .rounded(px(3.))
                    .cursor_pointer()
                    .when(sel, |d| d.bg(t.element_selected))
                    .hover(|d| d.bg(t.element_hover))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.ws_sel = if this.ws_sel.as_deref() == Some(cid.as_str()) {
                            None
                        } else {
                            Some(cid.clone())
                        };
                        cx.notify();
                    }))
                    .child(div().font_family(BUFFER_FONT).child(c.id.clone()))
                    .child(div().text_color(t.text_muted).child(c.peer.clone()))
                    .child(
                        div()
                            .text_color(t.text_muted)
                            .child(format!("in {} / out {}", c.messages_in, c.messages_out)),
                    )
                    .child(div().flex_1())
                    .child(self.ws_button(
                        t,
                        format!("ws-disconnect-{}", c.id),
                        "Disconnect",
                        format!("ws-disconnect:{}", c.id),
                        cx,
                    ))
            })
            .collect();

        // Timeline.
        let mine: Vec<_> = self
            .ws_frames
            .iter()
            .filter(|f| f.route_id == r.id)
            .filter(|f| selected_conn.as_ref().is_none_or(|c| &f.connection == c))
            .collect();
        let n_in = mine.iter().filter(|f| f.dir == "in").count();
        let n_out = mine.iter().filter(|f| f.dir == "out").count();
        let needle = self.ws_search.to_lowercase();
        let rows: Vec<_> = mine
            .iter()
            .rev()
            .filter(|f| match self.ws_dir {
                1 => f.dir == "in",
                2 => f.dir == "out",
                _ => true,
            })
            .filter(|f| needle.is_empty() || f.body.to_lowercase().contains(&needle))
            .take(300)
            .map(|f| (*f).clone())
            .collect();
        let frame_sel = self
            .ws_frame_sel
            .and_then(|id| self.ws_frames.iter().find(|f| f.id == id))
            .cloned();
        let chips = self.ws_segmented(
            t,
            [
                (0u8, format!("All {}", mine.len()), "all"),
                (1, format!("In {n_in}"), "in"),
                (2, format!("Out {n_out}"), "out"),
            ]
            .into_iter()
            .map(|(k, label, name)| {
                self.ws_segment(
                    t,
                    format!("ws-dir-{name}-{}", r.id),
                    label,
                    self.ws_dir == k,
                    format!("ws-dir-{name}:{}", r.id),
                    cx,
                )
            })
            .collect(),
        );
        let th = |w: f32, label: &str, right: bool| {
            let d = div()
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(label.to_string());
            cell(w, if right { d.text_right() } else { d })
        };
        let timeline = div()
            .rounded(px(5.))
            .border_1()
            .border_color(t.border_variant)
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(24.))
                    .px(px(8.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(th(96., "Time", false))
                    .child(th(46., "Dir", false))
                    .child(th(56., "Type", false))
                    .child(th(60., "Size", true))
                    .child(th(0., "Payload", false)),
            )
            .when(rows.is_empty(), |d| {
                d.child(
                    div()
                        .p(px(10.))
                        .text_color(t.text_muted)
                        .child("No frames yet. Connect a client or change the filter."),
                )
            })
            .children(rows.into_iter().map(|f| {
                let out = f.dir == "out";
                let sel = frame_sel.as_ref().is_some_and(|s| s.id == f.id);
                let (rid2, fid) = (rid.clone(), f.id);
                crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("ws-frame-{}", f.id)))
                        .h(px(24.))
                        .px(px(8.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.))
                        .cursor_pointer()
                        .border_b_1()
                        .border_color(t.border_variant)
                        .when(sel, |d| d.bg(t.element_selected))
                        .hover(|d| d.bg(t.element_hover))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.ws_select_frame(&rid2, fid, cx))
                        )
                        .child(cell(
                            96.,
                            div()
                                .font_family(BUFFER_FONT)
                                .text_color(t.text_muted)
                                .child(fmt_time(&f.ts)),
                        ))
                        .child(cell(
                            46.,
                            div()
                                .text_size(px(11.))
                                .text_color(if out { t.modified } else { t.created })
                                .child(if out { "OUT" } else { "IN" }),
                        ))
                        .child(cell(
                            56.,
                            div().text_color(t.text_muted).child(f.kind.clone()),
                        ))
                        .child(cell(
                            60.,
                            div()
                                .text_right()
                                .text_color(t.text_muted)
                                .child(format!("{} B", f.size)),
                        ))
                        .child(cell(
                            0.,
                            div()
                                .overflow_hidden()
                                .whitespace_nowrap()
                                .font_family(BUFFER_FONT)
                                .child(one_line(&f.body)),
                        )),
                    format!("ws-frame-{}", f.id),
                    Button,
                    format!("{} {} {}", f.dir, f.kind, f.body),
                    Press(format!("ws-frame:{}:{}", r.id, f.id))
                )
            }))
            .when_some(frame_sel, |d, f| {
                d.child(
                    div()
                        .p(px(8.))
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(div().text_size(px(11.)).text_color(t.text_muted).child(
                                    format!("Frame {} · {} {} · {} B", f.id, f.dir, f.kind, f.size),
                                ))
                                .child(div().flex_1())
                                .child(self.ws_button(
                                    t,
                                    format!("ws-frame-copy-{}", r.id),
                                    "Copy",
                                    format!("ws-frame-copy:{}", r.id),
                                    cx,
                                )),
                        )
                        .child(crate::dev_id!(
                            div()
                                .id("ws-frame-view")
                                .h(px(110.))
                                .rounded(px(3.))
                                .border_1()
                                .border_color(t.border_variant)
                                .child(self.ws_frame_view.clone()),
                            "ws-frame-view",
                            Text,
                            f.body.chars().take(80).collect::<String>(),
                            None
                        )),
                )
            });

        // Composer.
        let all_label = format!("All connections ({})", conns.len());
        let target_combo = {
            use crate::combo_box::*;
            let mut items = vec![ComboItem::plain(all_label.clone())];
            items.extend(conns.iter().map(|c| ComboItem::plain(c.id.clone())));
            let cur = selected_conn.clone().unwrap_or_else(|| all_label.clone());
            let ids: Vec<String> = conns.iter().map(|c| c.id.clone()).collect();
            self.combo_field(
                cx,
                ComboSite {
                    key: format!("ws-target-{}", r.id),
                    items,
                    value: Some(cur.clone().into()),
                    fallback: Some(cur.into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Fixed(220.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(move |this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            this.ws_sel = ids
                                .iter()
                                .find(|i| i.as_str() == v.as_ref() as &str)
                                .cloned();
                            cx.notify();
                        }
                    }),
                },
            )
        };
        let format_combo = {
            use crate::combo_box::*;
            let cur = if self.ws_send_binary {
                "Binary (base64)"
            } else {
                "Text"
            };
            self.combo_field(
                cx,
                ComboSite {
                    key: format!("ws-format-{}", r.id),
                    items: vec![
                        ComboItem::plain("Text"),
                        ComboItem::plain("Binary (base64)"),
                    ],
                    value: Some(cur.into()),
                    fallback: Some(cur.into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Fixed(140.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(move |this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            this.ws_send_binary = v.starts_with("Binary");
                            cx.notify();
                        }
                    }),
                },
            )
        };
        let send_as_combo = {
            use crate::combo_box::*;
            let label = |f: &str| match f {
                "pretty" => "Pretty (2 spaces)",
                "raw" => "As written",
                _ => "Compact",
            };
            self.combo_field(
                cx,
                ComboSite {
                    key: format!("ws-send-as-{}", r.id),
                    items: fauxfield_model::WS_FORMATS
                        .iter()
                        .map(|f| ComboItem::plain(label(f)))
                        .collect(),
                    value: Some(label(&self.ws_send_format).into()),
                    fallback: Some(label(&self.ws_send_format).into()),
                    color: None,
                    tip: Some("How valid JSON is sent. Other text is sent as it is.".into()),
                    width: ComboWidth::Fixed(140.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(move |this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            this.ws_send_format = match v.as_ref() as &str {
                                "Pretty (2 spaces)" => "pretty",
                                "As written" => "raw",
                                _ => "compact",
                            }
                            .to_string();
                            cx.notify();
                        }
                    }),
                },
            )
        };
        let composer_text = self.ws_composer.read(cx).text().to_string();
        let composer_preview = self.ws_preview(
            s,
            &format!("composer:{}", r.id),
            &composer_text,
            &self.ws_send_format,
            self.ws_send_binary,
        );
        let composer = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                self.ws_split(
                    t,
                    "Message (templated)",
                    crate::dev_id!(
                        div()
                            .id("ws-composer")
                            .size_full()
                            .on_key_down(cx.listener({
                                let rid = rid.clone();
                                move |this, ev: &KeyDownEvent, _, cx| {
                                    if ev.keystroke.key == "enter"
                                        && ev.keystroke.modifiers.secondary()
                                    {
                                        cx.stop_propagation();
                                        let sel = this.ws_sel.clone();
                                        this.ws_send(&rid, sel, cx);
                                    }
                                }
                            }))
                            .child(self.ws_composer.clone()),
                        "ws-composer",
                        Input,
                        "Message",
                        None
                    )
                    .into_any_element(),
                    &format!("ws-composer-preview-{}", r.id),
                    &composer_preview,
                    format!("ws-preview-refresh:{}", r.id),
                    cx,
                ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(format_combo)
                    .when(!self.ws_send_binary, |d| d.child(send_as_combo))
                    .child(target_combo)
                    .child(self.ws_button(
                        t,
                        format!("ws-format-composer-{}", r.id),
                        "Format JSON",
                        format!("ws-format-composer:{}", r.id),
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(div().text_size(px(11.)).text_color(t.text_muted).child(
                        if cfg!(target_os = "macos") {
                            "⌘Enter"
                        } else {
                            "Ctrl+Enter"
                        },
                    ))
                    .child(self.ws_button(
                        t,
                        format!("ws-send-{}", r.id),
                        "Send",
                        format!("ws-send:{}", r.id),
                        cx,
                    )),
            );

        let section = |label: String| {
            div()
                .mt(px(10.))
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(label)
        };

        div()
            .id(crate::ui::eid(format!("route-tab-{}", r.id)))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(
                        div()
                            .px(px(8.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .bg(t.element_selected)
                            .text_size(px(13.))
                            .text_color(t.method_color("WS"))
                            .child("WS"),
                    )
                    .child(crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("route-path-{}", r.id)))
                            .cursor_pointer()
                            .font_family(BUFFER_FONT)
                            .text_size(px(14.))
                            .on_click(cx.listener(|this, _, w, cx| this.run_action_id(
                                "edit_route_path",
                                w,
                                cx
                            )))
                            .child(r.path.clone()),
                        format!("route-path-{}", r.id),
                        Button,
                        "Route path",
                        Action("edit_route_path")
                    ))
                    .child(crate::dev_id!(
                        toolbar_button(t, crate::ui::eid(format!("ws-copy-url-{}", r.id)))
                            .child(crate::ui::icon("copy", 12., t.icon_muted))
                            .tooltip(crate::ui::tooltip("Copy WebSocket URL".to_string()))
                            .on_click({
                                let op = format!("ws-copy-url:{}", r.id);
                                cx.listener(move |this, _, _, cx| this.ws_op_from_ui(&op, cx))
                            }),
                        format!("ws-copy-url-{}", r.id),
                        Button,
                        "Copy WebSocket URL",
                        Press(format!("ws-copy-url:{}", r.id))
                    )),
            )
            .child(
                div()
                    .id("ws-tab-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.))
                            .child(mode_switch)
                            .child(div().text_color(t.text_muted).child(mode_hint)),
                    )
                    .when(scripted, |d| {
                        d.child(section(format!("MESSAGES ({})", r.ws_messages.len())))
                            .child(table)
                            .when_some(detail, |d, el| d.child(el))
                    })
                    .child(section(format!("CONNECTIONS ({})", conns.len())))
                    .when(conns.is_empty(), |d| {
                        d.child(div().text_color(t.text_muted).child(
                            "No clients connected. Connect with new WebSocket(url) to see it here.",
                        ))
                    })
                    .children(conn_rows)
                    .child(section("SEND".to_string()))
                    .child(composer)
                    .child(
                        div()
                            .mt(px(10.))
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(10.))
                            .child(
                                div()
                                    .text_size(px(11.))
                                    .text_color(t.text_muted)
                                    .child("TIMELINE"),
                            )
                            .child(chips)
                            .child(self.ws_field(
                                t,
                                &rid,
                                0,
                                WsField::Search,
                                &self.ws_search,
                                "Filter payload",
                                180.,
                                cx,
                            ))
                            .child(div().flex_1())
                            .child(self.ws_button(
                                t,
                                format!("ws-clear-{}", r.id),
                                "Clear",
                                format!("ws-clear:{}", r.id),
                                cx,
                            )),
                    )
                    .child(timeline),
            )
    }

    /// The editor under the messages table for the selected message.
    #[allow(clippy::too_many_arguments)]
    fn render_ws_message_detail(
        &self,
        t: &Theme,
        s: &Server,
        r: &Route,
        i: usize,
        m: &crate::view::WsMsg,
        ed: &Entity<CodeEditor>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let rid = r.id.clone();
        let trig_combo = {
            use crate::combo_box::*;
            let rid = rid.clone();
            self.combo_field(
                cx,
                ComboSite {
                    key: format!("ws-trigger-{}-{i}", r.id),
                    items: TRIGGERS.iter().map(|m| ComboItem::plain(*m)).collect(),
                    value: Some(m.trigger.clone().into()),
                    fallback: Some(m.trigger.clone().into()),
                    color: None,
                    tip: None,
                    width: ComboWidth::Fixed(110.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(move |this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            this.set_ws_trigger(&rid, i, v, cx);
                        }
                    }),
                },
            )
        };
        let format_combo = {
            use crate::combo_box::*;
            let rid = rid.clone();
            let label = |f: &str| match f {
                "pretty" => "Pretty (2 spaces)",
                "raw" => "As written",
                _ => "Compact",
            };
            self.combo_field(
                cx,
                ComboSite {
                    key: format!("ws-format-msg-{}-{i}", r.id),
                    items: fauxfield_model::WS_FORMATS
                        .iter()
                        .map(|f| ComboItem::plain(label(f)))
                        .collect(),
                    value: Some(label(&m.format).into()),
                    fallback: Some(label(&m.format).into()),
                    color: None,
                    tip: Some("How valid JSON is sent. Other text is sent as it is.".into()),
                    width: ComboWidth::Fixed(140.),
                    look: ComboLook::Field,
                    size: ComboSize::Small,
                    on_event: std::rc::Rc::new(move |this, ev, cx| {
                        if let ComboEvent::Change(v) = ev {
                            let f = match v.as_ref() as &str {
                                "Pretty (2 spaces)" => "pretty",
                                "As written" => "raw",
                                _ => "compact",
                            };
                            this.set_ws_format(&rid, i, f, cx);
                        }
                    }),
                },
            )
        };
        let text = ed.read(cx).text().to_string();
        let preview = self.ws_preview(s, &format!("{}:{i}", r.id), &text, &m.format, m.binary);
        div()
            .p(px(10.))
            .rounded(px(5.))
            .border_1()
            .border_color(t.border_variant)
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_end()
                    .gap(px(10.))
                    .child(self.ws_label(t, "Trigger", trig_combo))
                    .when(m.trigger == "interval", |d| {
                        d.child(self.ws_label(
                            t,
                            "Every",
                            self.ws_field(t, &rid, i, WsField::Every, &m.every, "5s", 90., cx),
                        ))
                    })
                    .when(m.trigger == "message", |d| {
                        d.child(self.ws_label(
                            t,
                            "When (JSONPath)",
                            self.ws_field(
                                t,
                                &rid,
                                i,
                                WsField::WhenKey,
                                &m.when_key,
                                "$.type",
                                140.,
                                cx,
                            ),
                        ))
                        .child(self.ws_label(
                            t,
                            "equals",
                            self.ws_field(
                                t,
                                &rid,
                                i,
                                WsField::WhenValue,
                                &m.when_value,
                                "ping",
                                120.,
                                cx,
                            ),
                        ))
                    })
                    .child(self.ws_label(
                        t,
                        "Close code",
                        self.ws_field(
                            t,
                            &rid,
                            i,
                            WsField::CloseCode,
                            &m.close_code.map(|c| c.to_string()).unwrap_or_default(),
                            "none",
                            80.,
                            cx,
                        ),
                    ))
                    .when(!m.binary, |d| {
                        d.child(self.ws_label(t, "Send as", format_combo))
                    })
                    .child(self.ws_button(
                        t,
                        format!("ws-binary-{}-{i}", r.id),
                        if m.binary { "Binary ✓" } else { "Binary" },
                        format!("ws-binary:{}:{i}", r.id),
                        cx,
                    ))
                    .child(div().flex_1())
                    .child(self.ws_button(
                        t,
                        format!("ws-format-body-{}-{i}", r.id),
                        "Format JSON",
                        format!("ws-format-body:{}:{i}", r.id),
                        cx,
                    ))
                    .child(self.ws_button(
                        t,
                        format!("ws-apply-{}-{i}", r.id),
                        "Apply body",
                        format!("ws-apply:{}:{i}", r.id),
                        cx,
                    )),
            )
            .child(
                self.ws_split(
                    t,
                    if m.binary {
                        "Body (base64, templated)"
                    } else {
                        "Body (templated)"
                    },
                    crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("ws-body-{}-{i}", r.id)))
                            .size_full()
                            .child(ed.clone()),
                        format!("ws-body-{}-{i}", r.id),
                        Input,
                        "Message body",
                        None
                    )
                    .into_any_element(),
                    &format!("ws-preview-{}-{i}", r.id),
                    &preview,
                    format!("ws-preview-refresh:{}", r.id),
                    cx,
                ),
            )
    }

    /// The compact horizontal pair used by the message body and the composer: the editor on the
    /// left, what a client will receive on the right.
    #[allow(clippy::too_many_arguments)]
    fn ws_split(
        &self,
        t: &Theme,
        editor_label: &str,
        editor: AnyElement,
        preview_id: &str,
        preview: &(String, bool),
        refresh_op: String,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let head = |label: &str, extra: Option<AnyElement>| {
            div()
                .h(px(24.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.text_muted)
                        .child(label.to_string()),
                )
                .children(extra)
        };
        let boxed = |el: AnyElement| {
            div()
                .h(px(112.))
                .rounded(px(3.))
                .border_1()
                .border_color(t.border_variant)
                .overflow_hidden()
                .child(el)
        };
        div()
            .flex()
            .flex_row()
            .gap(px(10.))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(head(editor_label, None))
                    .child(boxed(editor)),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(head(
                        "Preview (what a client receives)",
                        Some(self.ws_button(
                            t,
                            format!("{preview_id}-refresh"),
                            "Render again",
                            refresh_op,
                            cx,
                        )),
                    ))
                    .child(boxed(
                        crate::dev_id!(
                            div()
                                .size_full()
                                .p(px(6.))
                                .overflow_hidden()
                                .text_size(px(11.))
                                .font_family(BUFFER_FONT)
                                .text_color(if preview.1 { t.deleted } else { t.text_muted })
                                .child(preview.0.clone()),
                            preview_id.to_string(),
                            Text,
                            preview.0.clone(),
                            None
                        )
                        .into_any_element(),
                    )),
            )
    }

    /// Render `body` with a sample connection so the preview shows what a client would get.
    fn ws_preview(
        &self,
        s: &Server,
        key: &str,
        body: &str,
        format: &str,
        binary: bool,
    ) -> (String, bool) {
        let key = format!("{key}:{format}");
        let key = key.as_str();
        if body.is_empty() {
            return ("(empty)".into(), false);
        }
        if let Some((b, out, err)) = self.ws_preview_cache.borrow().get(key)
            && b == body
        {
            return (out.clone(), *err);
        }
        let root = self.store.as_ref().map(|st| st.root()).unwrap_or_default();
        let ctx = serde_json::json!({
            "connection": { "id": "wsc_preview", "messages_in": 0, "messages_out": 0 },
            "message": { "text": "", "json": null, "binary": false, "size": 0 },
            "vars": {},
            "env": self.active_env,
            "server": { "name": s.name, "port": s.port },
        });
        let input = fauxfield_engine::template::Input {
            context: ctx,
            root: &root,
            seed: s.fake_seed,
            route_id: "",
            counter: 1,
            seq: None,
        };
        let out = match fauxfield_engine::template::render(body, &input) {
            Ok(v) => {
                let v = if binary {
                    v
                } else {
                    fauxfield_engine::ws_apply_format(&v, Some(format))
                };
                (v.chars().take(1000).collect::<String>(), false)
            }
            Err(e) => (format!("Template error: {e}"), true),
        };
        let mut cache = self.ws_preview_cache.borrow_mut();
        if cache.len() > 64 {
            cache.clear();
        }
        cache.insert(key.to_string(), (body.to_string(), out.0.clone(), out.1));
        out
    }

    // ---- edits --------------------------------------------------------------------------------

    pub(crate) fn set_ws_mode(&mut self, route: &str, mode: &str, cx: &mut Context<Self>) {
        use fauxfield_store::Target as T;
        let (id, m) = (route.to_string(), mode.to_string());
        self.store_op(cx, |s| {
            s.set_str(&T::Route(id), "mode", (m != "echo").then_some(m.as_str()))
        });
    }

    pub(crate) fn set_ws_format(
        &mut self,
        route: &str,
        idx: usize,
        format: &str,
        cx: &mut Context<Self>,
    ) {
        let (id, f) = (route.to_string(), format.to_string());
        self.store_op(cx, |s| {
            s.set_ws_message_str(&id, idx, "format", (f != "compact").then_some(f.as_str()))
        });
    }

    /// Re-indent the JSON in `ed`; a body that is not JSON shows why in a toast.
    fn ws_format_editor(&mut self, ed: &Entity<CodeEditor>, cx: &mut Context<Self>) {
        ed.update(cx, |e, cx| e.format_body(cx));
        if let Some(err) = ed.read(cx).last_error() {
            let msg = format!("Cannot format: {err}");
            self.notify(msg, cx);
        }
    }

    pub(crate) fn set_ws_trigger(
        &mut self,
        route: &str,
        idx: usize,
        trigger: &str,
        cx: &mut Context<Self>,
    ) {
        let (id, tr) = (route.to_string(), trigger.to_string());
        self.store_op(cx, |s| {
            s.set_ws_message_str(&id, idx, "trigger", Some(&tr))?;
            if tr == "interval" {
                s.set_ws_message_str(&id, idx, "every", Some("5s"))?;
            } else {
                s.set_ws_message_str(&id, idx, "every", None)?;
            }
            if tr != "message" {
                s.set_ws_when(&id, idx, None)?;
            }
            Ok(())
        });
    }

    pub(crate) fn commit_ws_field(
        &mut self,
        route: &str,
        idx: usize,
        f: WsField,
        text: &str,
        cx: &mut Context<Self>,
    ) {
        let v = text.trim().to_string();
        let id = route.to_string();
        match f {
            WsField::Every => {
                if fauxfield_model::parse_duration_ms(&v).is_none() {
                    return self.notify(
                        "Use a duration like 500ms, 5s or 1m (at least 10ms).".into(),
                        cx,
                    );
                }
                self.store_op(cx, |s| s.set_ws_message_str(&id, idx, "every", Some(&v)));
            }
            WsField::WhenKey | WsField::WhenValue => {
                let Some((_, _, m)) = self.ws_message(route, idx) else {
                    return;
                };
                let (mut key, mut val) = (m.when_key.clone(), m.when_value.clone());
                if f == WsField::WhenKey {
                    key = v;
                } else {
                    val = v;
                }
                self.store_op(cx, |s| {
                    s.set_ws_when(
                        &id,
                        idx,
                        (!key.is_empty()).then_some((key.as_str(), val.as_str())),
                    )
                });
            }
            WsField::CloseCode => {
                if v.is_empty() {
                    self.store_op(cx, |s| s.set_ws_message_close(&id, idx, None));
                } else {
                    match v.parse::<i64>() {
                        Ok(c) if (1000..=4999).contains(&c) => {
                            self.store_op(cx, |s| s.set_ws_message_close(&id, idx, Some(c)));
                        }
                        _ => self.notify("A close code is a number from 1000 to 4999.".into(), cx),
                    }
                }
            }
            WsField::Search => {
                self.ws_search = text.to_string();
                cx.notify();
            }
        }
    }

    fn ws_message(
        &self,
        route: &str,
        idx: usize,
    ) -> Option<(&Server, &Route, &crate::view::WsMsg)> {
        let (s, r) = self.ws.find_route(route)?;
        r.ws_messages.get(idx).map(|m| (s, r, m))
    }

    /// Select message `idx` and load its body into the editor. A body edited in the previous
    /// selection is saved first.
    pub(crate) fn ws_open_message(&mut self, route: &str, idx: usize, cx: &mut Context<Self>) {
        self.ws_apply_body(cx);
        let Some((_, _, m)) = self.ws_message(route, idx) else {
            return;
        };
        let body = m.body.clone();
        let st = self.editor_settings.clone();
        let ed = new_editor(&st, Language::Json, "{\"type\":\"tick\"}", false, cx);
        ed.update(cx, |e, cx| e.set_text(&body, cx));
        cx.observe(&ed, |_, _, cx| cx.notify()).detach();
        self.ws_body_ed = Some((route.to_string(), idx, ed));
        self.ws_msg_sel.insert(route.to_string(), idx);
        cx.notify();
    }

    /// Save the open message body to the file when it changed.
    pub(crate) fn ws_apply_body(&mut self, cx: &mut Context<Self>) {
        let Some((route, idx, ed)) = self.ws_body_ed.clone() else {
            return;
        };
        let text = ed.read(cx).text().to_string();
        let same = self
            .ws_message(&route, idx)
            .is_none_or(|(_, _, m)| m.body == text);
        if !same {
            self.store_op(cx, |s| {
                s.set_ws_message_str(&route, idx, "body", Some(&text))
            });
        }
    }

    pub(crate) fn ws_select_frame(&mut self, _route: &str, id: u64, cx: &mut Context<Self>) {
        self.ws_frame_sel = Some(id);
        if let Some(f) = self.ws_frames.iter().find(|f| f.id == id) {
            let text = if f.kind == "text" {
                serde_json::from_str::<serde_json::Value>(&f.body)
                    .ok()
                    .and_then(|v| serde_json::to_string_pretty(&v).ok())
                    .unwrap_or_else(|| f.body.clone())
            } else {
                f.body.clone()
            };
            self.ws_frame_view.update(cx, |e, cx| e.set_text(&text, cx));
        }
        cx.notify();
    }

    /// Send the composer text to one connection (`Some`) or to every connection of the Route.
    pub(crate) fn ws_send(&mut self, route: &str, conn: Option<String>, cx: &mut Context<Self>) {
        let Some((s, _)) = self.ws.find_route(route) else {
            return;
        };
        let sid = s.id.clone();
        let text = self.ws_composer.read(cx).text().to_string();
        let Some(h) = &self.host else {
            return self.notify("Start the server first.".into(), cx);
        };
        if text.is_empty() {
            return self.notify("Type a message first.".into(), cx);
        }
        let msg = match h.engine().ws_send_rendered(
            &sid,
            Some(route),
            conn.as_deref(),
            &text,
            self.ws_send_binary,
            Some(&self.ws_send_format),
        ) {
            Ok(0) => "No clients connected.".to_string(),
            Ok(n) => format!("Sent to {n} client(s)."),
            Err(e) => e.to_string(),
        };
        self.notify(msg, cx);
    }

    pub(crate) fn ws_disconnect(&mut self, conn: &str, cx: &mut Context<Self>) {
        let Some(info) = self.ws_conns.iter().find(|c| c.id == conn).cloned() else {
            return;
        };
        let Some(h) = &self.host else { return };
        match h.engine().ws_disconnect(&info.server_id, conn, Some(1000)) {
            Ok(()) => self.notify(format!("Disconnected {conn}."), cx),
            Err(e) => self.notify(e.to_string(), cx),
        }
    }

    /// Button and dev-mcp entry point: `ws-<op>:<id>[:<index>]`.
    pub(crate) fn ws_op_from_ui(&mut self, op: &str, cx: &mut Context<Self>) {
        let mut it = op.splitn(3, ':');
        let name = it
            .next()
            .unwrap_or("")
            .trim_start_matches("ws-")
            .to_string();
        let id = it.next().unwrap_or("").to_string();
        let idx = it.next().and_then(|s| s.parse::<usize>().ok());
        self.ws_op(&name, &id, idx, cx);
    }

    pub(crate) fn ws_op(
        &mut self,
        name: &str,
        id: &str,
        idx: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        match name {
            "add-connect" | "add-interval" | "add-message" => {
                let rid = id.to_string();
                let trigger = name.trim_start_matches("add-").to_string();
                let n = self
                    .ws
                    .find_route(id)
                    .map_or(0, |(_, r)| r.ws_messages.len());
                // A message added to an echo Route needs script mode to run.
                if self
                    .ws
                    .find_route(id)
                    .is_some_and(|(_, r)| r.ws_mode == "echo")
                {
                    self.set_ws_mode(id, "script", cx);
                }
                if self
                    .store_op(cx, |s| s.add_ws_message(&rid, &trigger))
                    .is_some()
                {
                    self.ws_open_message(id, n, cx);
                }
            }
            "del" => {
                let rid = id.to_string();
                if let Some(i) = idx {
                    self.ws_body_ed = None;
                    self.ws_msg_sel.remove(id);
                    self.store_op(cx, |s| s.delete_ws_message(&rid, i));
                }
            }
            "open" => {
                if let Some(i) = idx {
                    self.ws_open_message(id, i, cx);
                }
            }
            "apply" => {
                self.ws_apply_body(cx);
                self.notify("Message body saved.".into(), cx);
            }
            "binary" => {
                let rid = id.to_string();
                if let Some(i) = idx
                    && let Some((_, _, m)) = self.ws_message(id, i)
                {
                    let on = !m.binary;
                    self.store_op(cx, |s| s.set_ws_message_binary(&rid, i, on));
                }
            }
            "mode-echo" | "mode-script" | "mode-broadcast" => {
                self.ws_apply_body(cx);
                self.set_ws_mode(id, name.trim_start_matches("mode-"), cx);
            }
            "dir-all" | "dir-in" | "dir-out" => {
                self.ws_dir = match name {
                    "dir-in" => 1,
                    "dir-out" => 2,
                    _ => 0,
                };
                cx.notify();
            }
            "clear" => {
                self.ws_frames.retain(|f| f.route_id != id);
                self.ws_frame_sel = None;
                cx.notify();
            }
            "frame" => {
                if let Some(f) = idx {
                    self.ws_select_frame(id, f as u64, cx);
                }
            }
            "frame-copy" => {
                if let Some(fid) = self.ws_frame_sel
                    && let Some(f) = self.ws_frames.iter().find(|f| f.id == fid)
                {
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(f.body.clone()));
                    self.notify("Frame copied.".into(), cx);
                }
            }
            "send" => {
                let sel = self.ws_sel.clone();
                self.ws_send(id, sel, cx);
            }
            "copy-url" => {
                if let Some((s, r)) = self.ws.find_route(id) {
                    let host = if s.host == "0.0.0.0" {
                        "localhost"
                    } else {
                        &s.host
                    };
                    let url = format!("ws://{host}:{}{}{}", s.port, s.prefix, r.path);
                    cx.write_to_clipboard(gpui::ClipboardItem::new_string(url));
                    self.notify("WebSocket URL copied.".into(), cx);
                }
            }
            "format-body" => {
                if let Some((_, _, ed)) = self.ws_body_ed.clone() {
                    self.ws_format_editor(&ed, cx);
                }
            }
            "format-composer" => {
                let ed = self.ws_composer.clone();
                self.ws_format_editor(&ed, cx);
            }
            "preview-refresh" => {
                self.ws_preview_cache.borrow_mut().clear();
                cx.notify();
            }
            "disconnect" => self.ws_disconnect(id, cx),
            "sel" => {
                self.ws_sel = Some(id.to_string());
                cx.notify();
            }
            _ => {}
        }
    }

    /// Action `new_websocket_route`: add `/ws/<n>` to the current Server and open it.
    pub(crate) fn begin_new_websocket(&mut self, cx: &mut Context<Self>) {
        // No selection: a workspace with exactly one Server uses it.
        let server = self
            .current_server_id()
            .or_else(|| (self.ws.servers.len() == 1).then(|| self.ws.servers[0].id.clone()));
        let Some(server) = server else {
            return self.notify("Select a server first.".into(), cx);
        };
        self.new_websocket_in(&server, cx);
    }

    /// Add a WebSocket Route to Server `server` and open it.
    pub(crate) fn new_websocket_in(&mut self, server: &str, cx: &mut Context<Self>) {
        let server = server.to_string();
        let taken: Vec<String> = self
            .ws
            .find_server(&server)
            .map(|s| s.all_routes().map(|r| r.path.clone()).collect())
            .unwrap_or_default();
        let mut n = 1;
        let path = loop {
            let p = if n == 1 {
                "/ws".to_string()
            } else {
                format!("/ws/{n}")
            };
            if !taken.contains(&p) {
                break p;
            }
            n += 1;
        };
        if let Some(id) = self.store_op(cx, |s| s.create_websocket_route(&server, None, &path)) {
            self.click_row(&id, 2, cx);
        }
    }

    /// Mirror one engine event into the GUI's WebSocket state.
    pub(crate) fn on_ws_connection(
        &mut self,
        info: fauxfield_engine::WsConnectionInfo,
        open: bool,
    ) {
        self.ws_conns.retain(|c| c.id != info.id);
        if open {
            self.ws_conns.push(info);
        } else if self.ws_sel.as_deref() == Some(info.id.as_str()) {
            self.ws_sel = None;
        }
    }

    pub(crate) fn on_ws_frame(&mut self, f: fauxfield_engine::WsFrame) {
        // Keep per-connection counters fresh for the list.
        if let Some(c) = self.ws_conns.iter_mut().find(|c| c.id == f.connection)
            && (f.kind == "text" || f.kind == "binary")
        {
            if f.dir == "in" {
                c.messages_in += 1;
            } else {
                c.messages_out += 1;
            }
        }
        self.ws_frames.push_back(f);
        while self.ws_frames.len() > WS_FRAME_CAP {
            self.ws_frames.pop_front();
        }
    }
}
