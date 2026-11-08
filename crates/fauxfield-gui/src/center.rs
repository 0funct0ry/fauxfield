//! Centre pane: the Zed-style tab bar and the Route, Server and Environment tab bodies, plus the
//! right dock (Inspector placeholder). All on static mock data until M2.

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::mock::{Environment, Route, Server};
use crate::shell::{ROUTE_SUBTABS, SERVER_SECTIONS, Shell};
use crate::tabs::{Tab, TabKind};
use crate::theme::Theme;
use crate::ui::{ROW_H, TAB_BAR_H, icon, status_dot, toolbar_button};

/// Drag payload of a tab being reordered.
#[derive(Clone)]
pub struct DraggedTab {
    pub index: usize,
    pub title: String,
}

struct DragGhost(String);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = crate::theme::theme(cx).clone();
        div()
            .px_2()
            .py_1()
            .rounded(px(4.))
            .bg(t.elevated)
            .border_1()
            .border_color(t.border)
            .shadow_md()
            .text_size(px(12.))
            .text_color(t.text)
            .child(self.0.clone())
    }
}

/// Environment keys whose values are masked until revealed (SPEC §9.4).
pub fn is_secret_key(key: &str) -> bool {
    let k = key.to_lowercase();
    ["token", "secret", "password", "key"]
        .iter()
        .any(|w| k.contains(w))
}

pub fn masked(value: &str) -> String {
    "•".repeat(value.chars().count().clamp(6, 12))
}

fn tab_icon(kind: TabKind) -> &'static str {
    match kind {
        TabKind::Route => "file-text",
        TabKind::Server => "server",
        TabKind::Environment => "layers",
    }
}

impl Shell {
    pub(crate) fn render_center(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("center")
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .bg(t.editor)
            .child(self.render_tab_bar(t, cx))
            .child(
                div()
                    .id("tab-body")
                    .flex_1()
                    .min_h_0()
                    .child(self.render_tab_body(t, cx)),
            )
    }

    fn render_tab_bar(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.tabs.active_index();
        div()
            .id("tab-bar")
            .h(px(TAB_BAR_H))
            .w_full()
            .flex_none()
            .flex()
            .flex_row()
            .bg(t.tab_bar)
            .border_b_1()
            .border_color(t.border)
            .overflow_x_hidden()
            .children(
                self.tabs
                    .tabs()
                    .iter()
                    .enumerate()
                    .map(|(i, tab)| self.render_tab(t, i, tab, active == Some(i), cx)),
            )
    }

    fn render_tab(
        &self,
        t: &Theme,
        i: usize,
        tab: &Tab,
        active: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let hover = t.element_hover;
        let drag = DraggedTab {
            index: i,
            title: tab.title.clone(),
        };
        let accent = match tab.kind {
            TabKind::Route => t.syntax_function,
            TabKind::Server => t.created,
            TabKind::Environment => t.modified,
        };
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("tab-{}", tab.id)))
                .h_full()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .pl(px(10.))
                .pr(px(6.))
                .cursor_pointer()
                .border_r_1()
                .border_color(t.border_variant)
                .text_size(px(12.))
                .when(active, |d| d.bg(t.tab_active).text_color(t.text))
                .when(!active, |d| {
                    d.bg(t.tab_inactive)
                        .text_color(t.text_muted)
                        .hover(move |s| s.bg(hover))
                })
                .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                    this.activate_tab(i, cx);
                    if ev.click_count() >= 2 {
                        this.pin_tab_at(i, cx);
                    }
                }))
                .on_drag(drag, |d, _, _, cx| cx.new(|_| DragGhost(d.title.clone())))
                .drag_over::<DraggedTab>(move |s, _, _, _| s.bg(hover))
                .on_drop(
                    cx.listener(move |this, d: &DraggedTab, _, cx| this.move_tab(d.index, i, cx))
                )
                .child(icon(tab_icon(tab.kind), 13., accent))
                .child(
                    div()
                        .when(tab.preview, |d| d.italic())
                        .child(tab.title.clone()),
                )
                .child(crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("tab-close-{}", tab.id)))
                        .size(px(16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .hover(move |s| s.bg(hover))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _, _, cx| {
                                this.close_tab_at(i, cx);
                                cx.stop_propagation();
                            }),
                        )
                        .child(icon("x", 12., t.icon_muted)),
                    format!("tab-close-{}", tab.id),
                    Button,
                    format!("Close {}", tab.title),
                    CloseTab(tab.id.clone())
                ),),
            format!("tab-{}", tab.id),
            Tab,
            &tab.title,
            Tab(tab.id.clone())
        )
        .into_any_element()
    }
    fn render_tab_body(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let Some(tab) = self.tabs.active() else {
            return self.render_empty(t).into_any_element();
        };
        match tab.kind {
            TabKind::Route => match self.ws.find_route(&tab.id) {
                Some((s, r)) => self.render_route_tab(t, s, r, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
            TabKind::Server => match self.ws.find_server(&tab.id) {
                Some(s) => self.render_server_tab(t, s, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
            TabKind::Environment => match self.ws.find_environment(&tab.id) {
                Some(e) => self.render_environment_tab(t, e, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
        }
    }

    fn render_empty(&self, t: &Theme) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .text_color(t.text_muted)
            .child(icon("server", 28., t.icon_muted))
            .child("Open a route, server or environment from the tree.")
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(t.text_placeholder)
                    .child("Quick Search finds anything in the workspace."),
            )
    }

    // ---- Route tab -----------------------------------------------------------------------

    fn render_route_tab(
        &self,
        t: &Theme,
        s: &Server,
        r: &Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sub = self.route_sub.get(&r.id).copied().unwrap_or(0);
        let route_id = r.id.clone();
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
                    .flex_row()
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
                            .font_family(BUFFER_FONT)
                            .text_size(px(12.))
                            .text_color(t.method_color(&r.method))
                            .child(r.method.clone()),
                    )
                    .child(
                        div()
                            .font_family(BUFFER_FONT)
                            .text_size(px(14.))
                            .child(format!("{}{}", s.prefix, r.path)),
                    )
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(r.name.clone()),
                    )
                    .child(div().flex_1())
                    .child(crate::dev_id!(
                        toolbar_button(t, "route-send")
                            .child(icon("play", 12., t.created))
                            .child("Send test")
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.run_action_id("send_test", w, cx)
                            }),),
                        format!("route-send-{}", r.id),
                        Button,
                        "Send test",
                        Action("send_test")
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
                    .children(ROUTE_SUBTABS.iter().enumerate().map(|(i, name)| {
                        let rid = route_id.clone();
                        let active = i == sub;
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("subtab-{name}")))
                                .px(px(10.))
                                .h(px(28.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(12.))
                                .border_b_2()
                                .border_color(if active { t.accent } else { t.editor })
                                .text_color(if active { t.text } else { t.text_muted })
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.route_sub.insert(rid.clone(), i);
                                    cx.notify();
                                }))
                                .child(*name),
                            format!("subtab-{}-{name}", r.id),
                            Tab,
                            *name,
                            SubTab(r.id.clone(), i)
                        )
                    })),
            )
            .child(
                div().flex_1().min_h_0().child(match sub {
                    0 => self.render_responses(t, r).into_any_element(),
                    n => div()
                        .p(px(16.))
                        .text_color(t.text_muted)
                        .child(format!(
                            "{} arrive{} in {}.",
                            ROUTE_SUBTABS[n],
                            "s",
                            ["", "M5", "M2", "M14", "M15", "M2"][n]
                        ))
                        .into_any_element(),
                }),
            )
    }

    fn render_responses(&self, t: &Theme, r: &Route) -> impl IntoElement {
        let first = r.responses.first().cloned();
        div()
            .size_full()
            .flex()
            .flex_row()
            .child(
                div()
                    .w(px(200.))
                    .h_full()
                    .flex_none()
                    .border_r_1()
                    .border_color(t.border_variant)
                    .bg(t.panel)
                    .children(r.responses.iter().enumerate().map(|(i, resp)| {
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("resp-{}-{i}", r.id)))
                                .h(px(ROW_H + 4.))
                                .px(px(10.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .when(i == 0, |d| d.bg(t.element_selected))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(t.status_color(resp.status))
                                        .child(resp.status.to_string()),
                                )
                                .child(div().flex_1().truncate().child(resp.name.clone()))
                                .children(resp.default.then(|| icon("check", 12., t.created))),
                            format!("resp-{}-{i}", r.id),
                            Row,
                            &resp.name,
                            None
                        )
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(first.map(|resp| {
                        div()
                            .size_full()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .h(px(32.))
                                    .flex_none()
                                    .px(px(12.))
                                    .flex()
                                    .items_center()
                                    .gap(px(16.))
                                    .text_size(px(12.))
                                    .text_color(t.text_muted)
                                    .child(format!("Status {}", resp.status))
                                    .child(format!("Delay {} ms", resp.delay_ms))
                                    .child(resp.content_type.clone()),
                            )
                            .child(crate::dev_id!(
                                div()
                                    .id("body-editor")
                                    .flex_1()
                                    .min_h_0()
                                    .overflow_y_scroll()
                                    .p(px(12.))
                                    .font_family(BUFFER_FONT)
                                    .text_size(px(13.))
                                    .text_color(t.syntax_string)
                                    .child(if resp.body.is_empty() {
                                        "(empty body)".to_string()
                                    } else {
                                        resp.body.clone()
                                    }),
                                "body-editor",
                                Input,
                                "Response body",
                                None
                            ))
                    })),
            )
    }

    // ---- Server tab ----------------------------------------------------------------------

    fn render_server_tab(&self, t: &Theme, s: &Server, cx: &mut Context<Self>) -> impl IntoElement {
        let section = self.server_section.get(&s.id).copied().unwrap_or(0);
        let sid = s.id.clone();
        let index = self
            .ws
            .servers
            .iter()
            .position(|x| x.id == s.id)
            .unwrap_or(0);
        div()
            .id(crate::ui::eid(format!("server-tab-{}", s.id)))
            .size_full()
            .flex()
            .flex_row()
            .child(
                div()
                    .w(px(160.))
                    .h_full()
                    .flex_none()
                    .py(px(6.))
                    .bg(t.panel)
                    .border_r_1()
                    .border_color(t.border_variant)
                    .children(SERVER_SECTIONS.iter().enumerate().map(|(i, name)| {
                        let sid = sid.clone();
                        let sid_click = sid.clone();
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("section-{name}")))
                                .h(px(ROW_H))
                                .px(px(12.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(12.))
                                .when(i == section, |d| {
                                    d.bg(t.element_selected).text_color(t.text)
                                })
                                .when(i != section, |d| d.text_color(t.text_muted))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.server_section.insert(sid_click.clone(), i);
                                    cx.notify();
                                }))
                                .child(*name),
                            format!("section-{}-{name}", sid),
                            Tab,
                            *name,
                            Section(sid.clone(), i)
                        )
                    })),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child(status_dot(t, s.running))
                            .child(div().text_size(px(16.)).child(s.name.clone()))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(t.text_muted)
                                    .child(if s.running { "running" } else { "stopped" }),
                            )
                            .child(div().flex_1())
                            .child(crate::dev_id!(
                                toolbar_button(t, "server-toggle")
                                    .child(icon(
                                        if s.running { "square" } else { "play" },
                                        12.,
                                        t.created,
                                    ))
                                    .child(if s.running { "Stop" } else { "Start" })
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.toggle_server_at(index, cx)
                                    })),
                                format!("server-toggle-{}", s.id),
                                Button,
                                if s.running { "Stop" } else { "Start" },
                                Action("toggle_server")
                            )),
                    )
                    .child(if section == 0 {
                        self.server_general(t, s).into_any_element()
                    } else {
                        div()
                            .text_color(t.text_muted)
                            .child(format!(
                                "{} settings arrive in {}.",
                                SERVER_SECTIONS[section],
                                ["", "M10", "M10", "M9", "M9", "M11", "M8", "M7", "M15"][section]
                            ))
                            .into_any_element()
                    }),
            )
    }

    fn server_general(&self, t: &Theme, s: &Server) -> impl IntoElement {
        let kv = |k: &str, v: String| {
            div()
                .flex()
                .flex_row()
                .h(px(ROW_H + 4.))
                .items_center()
                .child(
                    div()
                        .w(px(120.))
                        .text_color(t.text_muted)
                        .child(k.to_string()),
                )
                .child(div().font_family(BUFFER_FONT).text_size(px(12.)).child(v))
        };
        div()
            .flex()
            .flex_col()
            .child(kv("Name", s.name.clone()))
            .child(kv("Host", s.host.clone()))
            .child(kv("Port", s.port.to_string()))
            .child(kv(
                "Prefix",
                if s.prefix.is_empty() {
                    "(none)".into()
                } else {
                    s.prefix.clone()
                },
            ))
            .child(kv("Routes", s.all_routes().count().to_string()))
    }

    // ---- Environment tab -----------------------------------------------------------------

    fn render_environment_tab(
        &self,
        t: &Theme,
        e: &Environment,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let reveal = self.reveal_secrets;
        let cell = |w: f32, s: String| div().w(px(w)).flex_none().truncate().child(s);
        div()
            .id(crate::ui::eid(format!("env-tab-{}", e.name)))
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
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(icon("layers", 14., t.modified))
                    .child(div().text_size(px(14.)).child(e.name.clone()))
                    .child(div().text_size(px(12.)).text_color(t.text_muted).child(
                        if e.name == self.active_env {
                            "active"
                        } else {
                            ""
                        },
                    ))
                    .child(div().flex_1())
                    .child(crate::dev_id!(
                        toolbar_button(t, "env-reveal")
                            .child(if reveal {
                                "Hide secrets"
                            } else {
                                "Show secrets"
                            })
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.reveal_secrets = !this.reveal_secrets;
                                cx.notify();
                            })),
                        "env-reveal",
                        Button,
                        if reveal {
                            "Hide secrets"
                        } else {
                            "Show secrets"
                        },
                        None
                    )),
            )
            .child(
                div()
                    .h(px(ROW_H))
                    .flex_none()
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(cell(200., "KEY".into()))
                    .child(cell(320., "VALUE".into()))
                    .child(cell(120., "SOURCE".into())),
            )
            .child(
                div()
                    .id("env-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .children(e.vars.iter().map(|v| {
                        let shown = if is_secret_key(&v.key) && !reveal {
                            masked(&v.value)
                        } else {
                            v.value.clone()
                        };
                        div()
                            .id(crate::ui::eid(format!("var-{}", v.key)))
                            .h(px(ROW_H + 2.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .child(cell(200., v.key.clone()))
                            .child(
                                div()
                                    .w(px(320.))
                                    .flex_none()
                                    .truncate()
                                    .font_family(BUFFER_FONT)
                                    .text_size(px(12.))
                                    .child(shown),
                            )
                            .child(
                                div()
                                    .w(px(120.))
                                    .flex_none()
                                    .text_size(px(12.))
                                    .text_color(t.text_muted)
                                    .child(v.source.label()),
                            )
                    })),
            )
    }

    // ---- Right dock ----------------------------------------------------------------------

    pub(crate) fn render_right_dock(&self, t: &Theme, _cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyElement = match self.traffic_selected.and_then(|i| self.traffic.get(i)) {
            Some(r) => {
                let kv = |k: &str, v: String| {
                    div()
                        .flex()
                        .flex_row()
                        .py(px(2.))
                        .child(div().w(px(70.)).text_color(t.text_muted).child(k.to_string()))
                        .child(div().flex_1().truncate().font_family(BUFFER_FONT).text_size(px(12.)).child(v))
                };
                div()
                    .p(px(10.))
                    .flex()
                    .flex_col()
                    .child(kv("Method", r.method.to_string()))
                    .child(kv("Path", r.path.clone()))
                    .child(kv("Status", r.status.to_string()))
                    .child(kv("Server", r.server.clone()))
                    .child(kv("Route", r.route.clone()))
                    .child(kv("Time", r.time.clone()))
                    .child(kv("Duration", format!("{:.1} ms", r.duration_ms)))
                    .child(kv("Size", format!("{} B", r.size)))
                    .into_any_element()
            }
            None => div()
                .p(px(10.))
                .text_color(t.text_muted)
                .child("Select a request in Traffic to inspect it. Variables in scope appear here from M7.")
                .into_any_element(),
        };
        div()
            .id("right-dock")
            .w(px(self.layout.right_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.panel)
            .child(
                div()
                    .h(px(ROW_H + 4.))
                    .px(px(10.))
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .child("INSPECTOR"),
            )
            .child(body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn secret_keys_are_detected() {
        for k in [
            "api_token",
            "API_SECRET",
            "db_password",
            "stripe_key",
            "Token",
        ] {
            assert!(is_secret_key(k), "{k}");
        }
        for k in ["base_url", "region", "HOME"] {
            assert!(!is_secret_key(k), "{k}");
        }
    }

    #[test]
    fn masks_have_bounded_length() {
        assert_eq!(masked("a").chars().count(), 6);
        assert_eq!(masked("abcdefghijklmnopqrstuvwxyz").chars().count(), 12);
    }
}
