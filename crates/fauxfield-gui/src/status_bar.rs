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
        let total = self.ws.enabled_count();
        let problems = self.problems.len();
        div()
            .id("status-bar")
            .relative()
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
            .children((!self.toasts_older.is_empty()).then(|| {
                // Older toasts stack above the status bar, newest at the bottom.
                div()
                    .absolute()
                    .bottom(px(STATUS_BAR_H))
                    .left(px(6.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .children(self.toasts_older.iter().map(|(id, m)| {
                        div()
                            .id(crate::ui::eid(format!("toast-older-{id}")))
                            .px(px(8.))
                            .py(px(3.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(t.border)
                            .bg(t.elevated)
                            .text_color(t.text_muted)
                            .child(m.clone())
                    }))
            }))
            .child(crate::dev_id!(
                div()
                    .id("toast-text")
                    .flex_1()
                    .truncate()
                    .px(px(8.))
                    .text_color(if self.notice_is_error {
                        t.deleted
                    } else {
                        t.text
                    })
                    .cursor_pointer()
                    .on_click(cx.listener(|this, _, _, cx| this.dismiss_toast(cx)))
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
                        .on_click(cx.listener(|this, _, w, cx| this.run_toast_action(w, cx)),),
                    "toast-action",
                    Button,
                    a.label,
                    Action("toast_action")
                )
            }))
            .children(self.update_notice.as_ref().map(|i| {
                let label = format!("Update {} available", i.latest);
                crate::dev_id!(
                    toolbar_button(t, "status-update")
                        .text_color(t.accent)
                        .child(label.clone())
                        .on_click(cx.listener(|this, _, _, cx| this.open_update_page(cx))),
                    "status.update",
                    Button,
                    label,
                    Action("check_for_updates")
                )
            }))
            .children(self.render_agent_status(t, cx))
            .child(crate::dev_id!(
                toolbar_button(t, "status-settings")
                    .child(icon("settings", 12., t.icon_muted))
                    .tooltip(crate::ui::tooltip(crate::actions::tip(
                        "open_settings",
                        &self.keymap,
                        cfg!(target_os = "macos"),
                    )))
                    .on_click(cx.listener(|this, _, _, cx| this.open_settings(cx))),
                "status.settings",
                Button,
                "Settings",
                Action("open_settings")
            ))
            .child({
                let name = t.name.clone();
                let tip =
                    crate::actions::tip("select_theme", &self.keymap, cfg!(target_os = "macos"));
                crate::dev_id!(
                    toolbar_button(t, "status-theme")
                        .child(icon("palette", 12., t.icon_muted))
                        .child(name.clone())
                        .tooltip(crate::ui::tooltip(tip))
                        .on_click(cx.listener(|this, _, w, cx| {
                            this.open_picker(PickerKind::Theme, w, cx)
                        })),
                    "status-theme",
                    Button,
                    format!("Theme: {name}"),
                    Action("select_theme")
                )
            })
            .child({
                let p = self.network_profile;
                let col = if p == fauxfield_engine::NetworkProfile::None {
                    t.icon_muted
                } else {
                    t.modified
                };
                crate::dev_id!(
                    toolbar_button(t, "status-network")
                        .child(icon(
                            if p == fauxfield_engine::NetworkProfile::Offline {
                                "wifi-off"
                            } else {
                                "wifi"
                            },
                            12.,
                            col,
                        ))
                        .child(format!("Network: {}", p.label()))
                        .tooltip(crate::ui::tooltip(
                            "Network profile for every Server (runtime only)".into(),
                        ))
                        .on_click(cx.listener(|this, _, w, cx| {
                            this.open_picker(PickerKind::Network, w, cx)
                        })),
                    "status-network",
                    Button,
                    format!("Network: {}", p.label()),
                    Action("switch_network_profile")
                )
            })
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
