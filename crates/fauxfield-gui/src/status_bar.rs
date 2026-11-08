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
            .child(crate::dev_id!(
                toolbar_button(t, "status-left-dock")
                    .child(icon("columns-2", 14., on(self.layout.left_open)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_left(cx))),
                "status-left-dock",
                Button,
                "Toggle left dock",
                Action("toggle_left_dock")
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "status-bottom-dock")
                    .child(icon("rows-2", 14., on(self.layout.bottom_open)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_bottom(cx))),
                "status-bottom-dock",
                Button,
                "Toggle bottom dock",
                Action("toggle_bottom_dock")
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "status-right-dock")
                    .child(icon("columns-2", 14., on(self.layout.right_open)))
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_right(cx))),
                "status-right-dock",
                Button,
                "Toggle right dock",
                Action("toggle_right_dock")
            ))
            .child(div().id("status-servers").child(format!(
                "{}/{} servers running",
                self.ws.running_count(),
                total
            )))
            .child(crate::dev_id!(
                div()
                    .flex_1()
                    .truncate()
                    .px(px(8.))
                    .text_color(t.text)
                    .children(self.notice.as_ref().map(|(_, m)| m.clone())),
                "toast",
                Toast,
                self.notice.as_ref().map(|(_, m)| m.as_str()).unwrap_or(""),
                None
            ))
            .children(self.notice_action.clone().map(|a| {
                crate::dev_id!(
                    toolbar_button(t, "toast-action")
                        .child(a.label)
                        .on_click(cx.listener(|this, _, w, cx| {
                            this.run_action_id("edit_server_port", w, cx)
                        })),
                    "toast-action",
                    Button,
                    a.label,
                    Action("edit_server_port")
                )
            }))
            .child(div().id("status-network").child("Network: no throttling"))
            .child(crate::dev_id!(
                toolbar_button(t, "status-environment")
                    .child(icon("layers", 12., t.icon_muted))
                    .child(self.active_env.clone())
                    .on_click(cx.listener(|this, _, w, cx| {
                        this.open_picker(PickerKind::Environment, w, cx)
                    })),
                "status-environment",
                Button,
                &self.active_env,
                Action("switch_environment")
            ))
            .child(crate::dev_id!(
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
                "status-problems",
                Button,
                format!("Problems: {problems}"),
                Action("show_problems")
            ))
    }
}
