//! Status bar: dock toggles, running counter, network profile, environment, problems counter.

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    div, px,
};

use crate::layout::BottomTab;
use crate::shell::{PickerKind, Shell};
use crate::theme::Theme;
use crate::ui::{STATUS_BAR_H, icon, toolbar_button};

impl Shell {
    pub(crate) fn render_status_bar(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let on = |open: bool| if open { t.accent } else { t.icon_muted };
        let total = self.ws.servers.len();
        let problems = self.problems.len();
        div()
            .id("status-bar")
            .h(px(STATUS_BAR_H))
            .w_full()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .bg(t.status_bar)
            .border_t_1()
            .border_color(t.border)
            .text_size(px(11.))
            .text_color(t.text_muted)
            .child(
                toolbar_button(t, "status-left-dock")
                    .child(icon("columns-2", 14., on(self.layout.left_open)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_left(cx))),
            )
            .child(
                toolbar_button(t, "status-bottom-dock")
                    .child(icon("rows-2", 14., on(self.layout.bottom_open)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_bottom(cx))),
            )
            .child(
                toolbar_button(t, "status-right-dock")
                    .child(icon("columns-2", 14., on(self.layout.right_open)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_right(cx))),
            )
            .child(div().id("status-servers").child(format!(
                "{}/{} servers running",
                self.ws.running_count(),
                total
            )))
            .child(
                div()
                    .flex_1()
                    .truncate()
                    .px(px(8.))
                    .text_color(t.text)
                    .children(self.notice.as_ref().map(|(_, m)| m.clone())),
            )
            .child(div().id("status-network").child("Network: no throttling"))
            .child(
                toolbar_button(t, "status-environment")
                    .child(icon("layers", 12., t.icon_muted))
                    .child(self.active_env.clone())
                    .on_click(cx.listener(|this, _, w, cx| {
                        this.open_picker(PickerKind::Environment, w, cx)
                    })),
            )
            .child(
                toolbar_button(t, "status-problems")
                    .child(icon(
                        "triangle-alert",
                        12.,
                        if problems > 0 {
                            t.modified
                        } else {
                            t.icon_muted
                        },
                    ))
                    .child(problems.to_string())
                    .on_click(
                        cx.listener(|this, _, _, cx| this.show_bottom(BottomTab::Problems, cx)),
                    ),
            )
    }
}
