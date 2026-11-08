//! Left dock: the workspace tree with status dots, ports and method badges (SPEC §9.3).

use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::tree::{self, RowKind, TreeRow};
use crate::ui::{ROW_H, icon, method_badge, status_dot};

impl Shell {
    pub(crate) fn render_left_dock(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let rows = tree::flatten(&self.ws, &self.collapsed, &self.active_env);
        div()
            .id("left-dock")
            .w(px(self.layout.left_width))
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
                    .child("WORKSPACE"),
            )
            .child(crate::dev_id!(
                div()
                    .id("tree")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .children(rows.iter().map(|r| self.render_tree_row(t, r, cx))),
                "tree",
                List,
                "Workspace tree",
                None
            ))
    }

    fn render_tree_row(&self, t: &Theme, r: &TreeRow, cx: &mut Context<Self>) -> gpui::AnyElement {
        let selected = self.selected.as_deref() == Some(r.id.as_str());
        let (hover, sel) = (t.element_hover, t.element_selected);
        let id = r.id.clone();
        let toggle_id = r.id.clone();
        let menu_id = r.id.clone();
        let dots_id = r.id.clone();
        let group = crate::ui::eid(format!("grp-{}", r.id));
        let group_name = format!("grp-{}", r.id);
        let is_section = matches!(r.kind, RowKind::Section);
        let lead: gpui::AnyElement = match &r.kind {
            RowKind::Server { running, .. } => status_dot(t, *running).into_any_element(),
            RowKind::Folder => icon(
                if r.expanded { "folder-open" } else { "folder" },
                14.,
                t.icon_muted,
            )
            .into_any_element(),
            RowKind::Route { method } => method_badge(t, method).into_any_element(),
            RowKind::Section => div().into_any_element(),
            RowKind::Environment { active } => icon(
                if *active { "check" } else { "layers" },
                13.,
                if *active { t.created } else { t.icon_muted },
            )
            .into_any_element(),
            RowKind::Dataset => icon("server", 13., t.icon_muted).into_any_element(),
        };
        let chevron: gpui::AnyElement = if r.expandable {
            crate::dev_id!(
                div()
                    .id(crate::ui::eid(format!("chev-{}", r.id)))
                    .w(px(20.))
                    .h(px(20.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_row(&toggle_id);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(icon(
                        if r.expanded {
                            "chevron-down"
                        } else {
                            "chevron-right"
                        },
                        12.,
                        t.icon_muted,
                    )),
                format!("chev-{}", r.id),
                Button,
                format!("Toggle {}", r.label),
                ToggleRow(r.id.clone())
            )
            .into_any_element()
        } else {
            div().w(px(20.)).flex_none().into_any_element()
        };
        let dots = crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("dots-{}", r.id)))
                .size(px(20.))
                .flex_none()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(3.))
                .invisible()
                .group_hover(group_name.clone(), |s| s.visible())
                .hover(move |s| s.bg(sel))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        this.open_menu(
                            &dots_id,
                            f32::from(ev.position.x),
                            f32::from(ev.position.y),
                            cx,
                        );
                        cx.stop_propagation();
                    }),
                )
                .child(icon("ellipsis", 14., t.icon_muted)),
            format!("dots-{}", r.id),
            Button,
            format!("Menu for {}", r.label),
            RowMenu(r.id.clone())
        );
        let _ = group;
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("row-{}", r.id)))
                .group(group_name)
                .h(px(ROW_H))
                .w_full()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.))
                .pl(px(6. + f32::from(r.depth) * 12.))
                .pr(px(8.))
                .cursor_pointer()
                .text_size(px(if is_section { 11. } else { 13. }))
                .text_color(if is_section { t.text_muted } else { t.text })
                .when(selected, |d| d.bg(sel))
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                    this.click_row(&id, ev.click_count(), cx)
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        this.open_menu(
                            &menu_id,
                            f32::from(ev.position.x),
                            f32::from(ev.position.y),
                            cx,
                        )
                    }),
                )
                .child(chevron)
                .child(lead)
                .child(div().flex_1().truncate().child(if is_section {
                    r.label.to_uppercase()
                } else {
                    r.label.clone()
                }))
                .children(match &r.kind {
                    RowKind::Server { port, .. } => Some(
                        div()
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(format!(":{port}")),
                    ),
                    _ => None,
                })
                .child(dots),
            format!("row-{}", r.id),
            Row,
            &r.label,
            Row(r.id.clone())
        )
        .into_any_element()
    }
}

use gpui::prelude::FluentBuilder as _;

impl Shell {
    pub(crate) fn render_row_menu(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.menu.as_ref().expect("menu is open");
        let running = self.ws.find_server(&m.row_id).is_some_and(|s| s.running);
        let hover = t.element_hover;
        let items = tree::menu_items(&m.kind, running);
        div()
            .id("row-menu-backdrop")
            .absolute()
            .size_full()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.close_menu(cx)),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, _, _, cx| this.close_menu(cx)),
            )
            .child(
                div()
                    .id("row-menu")
                    .absolute()
                    .left(px(m.x))
                    .top(px(m.y))
                    .min_w(px(160.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .bg(t.elevated)
                    .border_1()
                    .border_color(t.border)
                    .shadow_md()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .children(items.into_iter().map(|(label, act)| {
                        crate::dev_id_z!(
                            div()
                                .id(crate::ui::eid(format!("menu-{label}")))
                                .h(px(26.))
                                .px(px(12.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(12.))
                                .hover(move |s| s.bg(hover))
                                .on_click(cx.listener(move |this, _, _, cx| this.run_menu(act, cx)))
                                .child(label),
                            1,
                            format!("menu-{label}"),
                            MenuItem,
                            label,
                            Menu(act)
                        )
                    })),
            )
    }
}
