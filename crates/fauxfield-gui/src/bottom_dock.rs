//! Bottom dock: Traffic (virtualised, 10k mock rows), Problems and Logs tabs (SPEC §9.3, §9.9).

use gpui::{
    AnyElement, Context, InteractiveElement, IntoElement, ParentElement,
    StatefulInteractiveElement, Styled, div, prelude::*, px, uniform_list,
};

use crate::assets::BUFFER_FONT;
use crate::layout::BottomTab;
use crate::shell::Shell;
use crate::theme::Theme;
use crate::traffic::TrafficRow;
use crate::ui::{ROW_H, TAB_BAR_H, icon};

pub const TRAFFIC_ROW_H: f32 = 22.0;
/// Column widths: time, server, method, status, route, duration, size, source. Path takes the rest.
const COLS: [f32; 8] = [96., 100., 56., 52., 120., 72., 64., 56.];

impl Shell {
    pub(crate) fn render_bottom_dock(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.layout.bottom_tab;
        let hover = t.element_hover;
        div()
            .id("bottom-dock")
            .h(px(self.layout.bottom_height))
            .w_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.panel)
            .child(
                div()
                    .h(px(TAB_BAR_H))
                    .flex_none()
                    .px(px(6.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(2.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .children(BottomTab::ALL.iter().map(|tab| {
                        let tab = *tab;
                        let on = tab == active;
                        let count = match tab {
                            BottomTab::Traffic => format!("{}", self.traffic.len()),
                            BottomTab::Problems => format!("{}", self.problems.len()),
                            BottomTab::Logs => format!("{}", self.logs.len()),
                        };
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("bottom-tab-{}", tab.label())))
                                .px(px(10.))
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .gap(px(6.))
                                .rounded(px(4.))
                                .cursor_pointer()
                                .text_size(px(12.))
                                .when(on, |d| d.bg(t.element_selected).text_color(t.text))
                                .when(!on, |d| {
                                    d.text_color(t.text_muted).hover(move |s| s.bg(hover))
                                })
                                .on_click(
                                    cx.listener(move |this, _, _, cx| this.show_bottom(tab, cx))
                                )
                                .child(tab.label())
                                .child(
                                    div()
                                        .text_size(px(10.))
                                        .text_color(t.text_placeholder)
                                        .child(count),
                                ),
                            format!("bottom-tab-{}", tab.label()),
                            Tab,
                            tab.label(),
                            BottomTab(tab)
                        )
                    }))
                    .child(div().flex_1())
                    .child(crate::dev_id!(
                        div()
                            .id("bottom-close")
                            .size(px(20.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, _, cx| this.toggle_bottom(cx)))
                            .child(icon("x", 12., t.icon_muted)),
                        "bottom-close",
                        Button,
                        "Close bottom dock",
                        Action("toggle_bottom_dock")
                    )),
            )
            .child(match active {
                BottomTab::Traffic => self.render_traffic(t, cx),
                BottomTab::Problems => self.render_problems(t, cx),
                BottomTab::Logs => self.render_logs(t),
            })
    }

    fn render_traffic(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        #[cfg(feature = "dev-mcp")]
        crate::dev_mcp::tree::Registry::set_total(cx, "traffic", self.traffic.len());
        let head = |w: f32, s: &str| div().w(px(w)).flex_none().child(s.to_string());
        crate::dev_id!(
            div()
                .id("traffic")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .h(px(ROW_H))
                        .flex_none()
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .text_size(px(11.))
                        .text_color(t.text_muted)
                        .border_b_1()
                        .border_color(t.border_variant)
                        .child(head(COLS[0], "TIME"))
                        .child(head(COLS[1], "SERVER"))
                        .child(head(COLS[2], "METHOD"))
                        .child(div().flex_1().child("PATH"))
                        .child(head(COLS[3], "STATUS"))
                        .child(head(COLS[4], "ROUTE"))
                        .child(head(COLS[5], "DURATION"))
                        .child(head(COLS[6], "SIZE"))
                        .child(head(COLS[7], "SOURCE")),
                )
                .child(if self.traffic.is_empty() {
                    div()
                        .id("traffic-empty")
                        .flex_1()
                        .p(px(12.))
                        .text_color(t.text_muted)
                        .child(if self.ws.servers.iter().any(|s| s.running) {
                            "No requests yet. Send one with curl, or press Cmd/Ctrl+Enter on a route."
                        } else {
                            "No requests yet. Start a server with Cmd/Ctrl+R, then send a request."
                        })
                        .into_any_element()
                } else {
                    uniform_list(
                        "traffic-rows",
                        self.traffic.len(),
                        cx.processor(|this: &mut Shell, range: std::ops::Range<usize>, _w, cx| {
                            let t = crate::theme::theme(cx).clone();
                            range
                                .filter_map(|i| this.traffic.get(i).map(|r| (i, r.clone())))
                                .map(|(i, r)| this.render_traffic_row(&t, i, &r, cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .track_scroll(self.traffic_scroll.clone())
                    .flex_1()
                    .min_h_0()
                    .into_any_element()
                }),
            "traffic",
            Table,
            "Traffic",
            None
        )
        .into_any_element()
    }

    fn render_traffic_row(
        &self,
        t: &Theme,
        i: usize,
        r: &TrafficRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.traffic_selected == Some(i);
        #[cfg(feature = "dev-mcp")]
        let this_mask = self.mask_volatile;
        #[cfg(not(feature = "dev-mcp"))]
        let this_mask = false;
        let (hover, sel) = (t.element_hover, t.element_selected);
        let cell = |w: f32, s: String| div().w(px(w)).flex_none().truncate().child(s);
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("traffic-row-{i}")))
                .h(px(TRAFFIC_ROW_H))
                .w_full()
                .px(px(10.))
                .flex()
                .flex_row()
                .items_center()
                .font_family(BUFFER_FONT)
                .text_size(px(11.))
                .cursor_pointer()
                .when(r.upstream_error() && !selected, |d| d
                    .bg(t.deleted.opacity(0.12)))
                .when(selected, |d| d.bg(sel))
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.traffic_selected = Some(i);
                    this.layout.right_open = true;
                    cx.notify();
                }))
                .child(cell(
                    COLS[0],
                    volatile(this_mask, r.time.clone(), "--:--:--.---")
                ))
                .child(cell(COLS[1], r.server.clone()))
                .child(
                    div()
                        .w(px(COLS[2]))
                        .flex_none()
                        .text_color(t.method_color(&r.method))
                        .child(r.method.clone()),
                )
                .child(div().flex_1().truncate().child(if r.is_callback() {
                    format!("  ↳ {}", r.path)
                } else {
                    r.path.clone()
                }))
                .child(
                    div()
                        .w(px(COLS[3]))
                        .flex_none()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(3.))
                        .text_color(if r.status == 0 {
                            t.deleted
                        } else {
                            t.status_color(r.status)
                        })
                        .children(
                            r.rec
                                .as_ref()
                                .filter(|x| !x.dataset_changes.is_empty())
                                .map(|x| {
                                    let n: usize = x.dataset_changes.iter().map(|c| c.rows).sum();
                                    let tip = format!(
                                        "Changed Dataset: {} ({n} row{})",
                                        x.dataset_changes
                                            .iter()
                                            .map(|c| format!("{} {}", c.op, c.dataset))
                                            .collect::<Vec<_>>()
                                            .join(", "),
                                        if n == 1 { "" } else { "s" }
                                    );
                                    crate::dev_id!(
                                        div()
                                            .id(crate::ui::eid(format!("dataset-change-{i}")))
                                            .flex_none()
                                            .tooltip(crate::ui::tooltip(tip.clone()))
                                            .child(crate::ui::icon("database", 11., t.modified)),
                                        format!("traffic-dataset-{i}"),
                                        Text,
                                        tip,
                                        None
                                    )
                                })
                        )
                        .children(r.fault.as_ref().map(|f| {
                            crate::dev_id!(
                                div()
                                    .id(crate::ui::eid(format!("fault-{i}")))
                                    .flex_none()
                                    .tooltip(crate::ui::tooltip(format!("Injected: {f}")))
                                    .child(crate::ui::icon("zap", 11., t.deleted)),
                                format!("traffic-fault-{i}"),
                                Text,
                                format!("Injected: {f}"),
                                None
                            )
                        }))
                        .child(if r.status == 0 {
                            "—".to_string()
                        } else {
                            r.status.to_string()
                        }),
                )
                .child(cell(COLS[4], r.route.clone()))
                .child(cell(
                    COLS[5],
                    volatile(this_mask, format!("{:.1} ms", r.duration_ms), "-- ms")
                ))
                .child(cell(COLS[6], format!("{} B", r.size)))
                .child(
                    div()
                        .w(px(COLS[7]))
                        .flex_none()
                        .text_color(if r.proxied || r.is_callback() {
                            t.modified
                        } else {
                            t.text_muted
                        })
                        .child(if r.is_callback() {
                            "callback"
                        } else if r.proxied {
                            "proxy"
                        } else {
                            "mock"
                        }),
                ),
            format!("traffic-row-{i}"),
            Row,
            format!("{} {}", r.method, r.path),
            TrafficRow(i)
        )
        .into_any_element()
    }

    fn render_problems(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let hover = t.element_hover;
        div()
            .id("problems")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .children(self.problems.iter().enumerate().map(|(i, p)| {
                let rid = p.target.clone().unwrap_or_default();
                let key = format!(
                    "problem-{}-{i}",
                    if p.target.is_some() {
                        rid.as_str()
                    } else {
                        p.file.as_str()
                    }
                );
                crate::dev_id!(
                    div()
                        .id(crate::ui::eid(key.clone()))
                        .h(px(ROW_H + 2.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some((kind, title)) = this.tab_for_row(&rid) {
                                this.reveal_row(&rid);
                                this.open_tab(&rid, kind, &title, false);
                            }
                            cx.notify();
                        }))
                        .child(icon(
                            if p.warning {
                                "triangle-alert"
                            } else {
                                "circle-x"
                            },
                            13.,
                            if p.warning { t.modified } else { t.deleted },
                        ))
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .text_size(px(12.))
                                .child(p.message.clone()),
                        ),
                    key,
                    Row,
                    &p.message,
                    None
                )
            }))
            .when(self.problems.is_empty(), |d| {
                d.child(
                    div()
                        .p(px(12.))
                        .text_color(t.text_muted)
                        .child("No problems."),
                )
            })
            .into_any_element()
    }

    fn render_logs(&self, t: &Theme) -> AnyElement {
        div()
            .id("logs")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .p(px(10.))
            .font_family(BUFFER_FONT)
            .text_size(px(11.))
            .text_color(t.text_muted)
            .when(self.logs.is_empty(), |d| {
                d.child("No log lines yet. Servers log when they start, stop and handle requests.")
            })
            .children(self.logs.iter().map(|l| div().child(l.clone())))
            .into_any_element()
    }
}

/// Volatile text (timestamps, durations) shows a fixed placeholder while screenshots are taken.
fn volatile(mask: bool, real: String, placeholder: &str) -> String {
    if mask { placeholder.to_owned() } else { real }
}
