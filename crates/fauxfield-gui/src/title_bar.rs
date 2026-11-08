//! Title bar: workspace menu, environment dropdown, Run all, search hint (SPEC §9.3).

use gpui::{
    Context, InteractiveElement, IntoElement, ParentElement, StatefulInteractiveElement, Styled,
    WindowControlArea, div, px,
};

use crate::keymap::display_keys;
use crate::shell::{PickerKind, Shell};
use crate::theme::Theme;
use crate::ui::{TITLE_BAR_H, icon, keycap, toolbar_button};

impl Shell {
    pub(crate) fn render_title_bar(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let mac = cfg!(target_os = "macos");
        let all_running = self.ws.running_count() == self.ws.servers.len();
        let hint = self
            .keymap
            .binding_for("quick_search")
            .map(|k| display_keys(k, mac))
            .unwrap_or_default();
        div()
            .id("title-bar")
            .window_control_area(WindowControlArea::Drag)
            .h(px(TITLE_BAR_H))
            .w_full()
            .flex_none()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            // Room for the inline macOS traffic lights.
            .pl(px(if mac { 78. } else { 8. }))
            .pr(px(8.))
            .bg(t.title_bar)
            .border_b_1()
            .border_color(t.border)
            .child(crate::dev_id!(
                toolbar_button(t, "title-workspace")
                    .child(icon("folder-open", 14., t.icon_muted))
                    .child(self.ws.name.clone())
                    .child(icon("chevron-down", 12., t.icon_muted))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.notify("Workspace menu arrives in M2.".into(), cx)
                    })),
                "title-workspace",
                Button,
                &self.ws.name,
                None
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "title-environment")
                    .child(icon("layers", 14., t.icon_muted))
                    .child(self.active_env.clone())
                    .child(icon("chevron-down", 12., t.icon_muted))
                    .on_click(cx.listener(|this, _, w, cx| {
                        this.open_picker(PickerKind::Environment, w, cx)
                    })),
                "title-environment",
                Button,
                &self.active_env,
                Action("switch_environment")
            ))
            .child(crate::dev_id!(
                toolbar_button(t, "title-run-all")
                    .child(icon(
                        if all_running { "square" } else { "play" },
                        12.,
                        t.created,
                    ))
                    .child(if all_running { "Stop all" } else { "Run all" })
                    .on_click(cx.listener(|this, _, _, cx| this.run_all(cx))),
                "title-run-all",
                Button,
                if all_running { "Stop all" } else { "Run all" },
                Action("run_all")
            ))
            // Double-clicking empty title-bar space zooms the window (macOS convention).
            .child(
                div()
                    .id("title-drag")
                    .flex_1()
                    .h_full()
                    .on_click(|ev, w, _| {
                        if ev.click_count() == 2 {
                            w.titlebar_double_click();
                        }
                    }),
            )
            .child(crate::dev_id!(
                toolbar_button(t, "title-search")
                    .w(px(220.))
                    .justify_between()
                    .border_1()
                    .border_color(t.border_variant)
                    .text_color(t.text_placeholder)
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(4.))
                            .child(icon("search", 13., t.icon_muted))
                            .child("Search workspace"),
                    )
                    .child(keycap(t, &hint))
                    .on_click(cx.listener(|this, _, w, cx| this.open_picker(
                        PickerKind::Search,
                        w,
                        cx
                    )),),
                "title-search",
                Button,
                "Search workspace",
                Action("quick_search")
            ))
    }
}
