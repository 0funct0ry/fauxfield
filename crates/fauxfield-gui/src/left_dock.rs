//! Left dock: the workspace tree with status dots, ports and method badges (SPEC §9.3).

use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, MouseButton, MouseDownEvent,
    ParentElement, StatefulInteractiveElement, Styled, div, px,
};

use crate::shell::Shell;
use crate::theme::Theme;
use crate::tree::{self, RowKind, TreeRow};
use crate::ui::{ROW_H, icon, method_badge, status_dot};

pub(crate) const SIDEBAR_TAB_H: f32 = 30.;

impl Shell {
    pub(crate) fn render_left_dock(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let tab = self.sidebar_tab;
        let rows = tree::flatten(&self.ws, &self.collapsed, &self.active_env, tab);
        let count = self.sidebar_count(tab);
        let body: gpui::AnyElement = if rows.is_empty() {
            self.render_sidebar_empty(t, tab, cx).into_any_element()
        } else {
            crate::dev_id!(
                div()
                    // One id per tab keeps each tab's scroll position for the session.
                    .id(crate::ui::eid(format!("tree-{}", tab.id())))
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
            )
            .into_any_element()
        };
        div()
            .id("left-dock")
            .w(px(self.layout.left_width))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(t.panel)
            .child(self.render_workspace_header(t, cx))
            .child(self.render_sidebar_tabs(t, cx))
            .child(self.render_sidebar_title(t, tab, count, cx))
            .child(body)
    }

    /// Item count shown in the panel title: servers, environments or datasets.
    pub(crate) fn sidebar_count(&self, tab: tree::SidebarTab) -> usize {
        match tab {
            tree::SidebarTab::Servers => self.ws.servers.len(),
            tree::SidebarTab::Environments => self.ws.environments.len(),
            tree::SidebarTab::Datasets => self.ws.datasets.len(),
        }
    }

    fn tab_tooltip(&self, tab: tree::SidebarTab) -> String {
        let mac = crate::keymap::Platform::current() == crate::keymap::Platform::Macos;
        match self.keymap.binding_for(tab.action_id()) {
            Some(keys) => format!(
                "{} ({})",
                tab.label(),
                crate::keymap::display_keys(keys, mac)
            ),
            None => tab.label().to_string(),
        }
    }

    fn render_sidebar_tabs(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, text, muted, accent) = (t.element_hover, t.text, t.icon_muted, t.accent);
        let buttons = tree::SIDEBAR_TABS.into_iter().map(|tab| {
            let active = tab == self.sidebar_tab;
            let tip = self.tab_tooltip(tab);
            crate::dev_id_meta!(
                div()
                    .id(crate::ui::eid(format!("sidebar-tab-{}", tab.id())))
                    .w(px(40.))
                    .h(px(SIDEBAR_TAB_H))
                    .flex_none()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .border_b_2()
                    .border_color(if active {
                        accent
                    } else {
                        gpui::transparent_black()
                    })
                    .when(!active, |d| d.hover(move |s| s.bg(hover)))
                    .tooltip(crate::ui::tooltip(tip.clone()))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_sidebar_tab(tab, cx)))
                    .child(icon(tab.icon(), 16., if active { text } else { muted })),
                crate::dev_mcp::tree::Meta {
                    tooltip: true,
                    ..Default::default()
                },
                format!("sidebar-tab-{}", tab.id()),
                Tab,
                tab.label(),
                Action(tab.action_id())
            )
        });
        crate::dev_id!(
            div()
                .id("sidebar-tabs")
                .h(px(SIDEBAR_TAB_H))
                .w_full()
                .flex_none()
                .flex()
                .flex_row()
                .border_b_1()
                .border_color(t.border_variant)
                .children(buttons),
            "sidebar-tabs",
            TabList,
            "Sidebar tabs",
            None
        )
    }

    fn render_sidebar_title(
        &self,
        t: &Theme,
        tab: tree::SidebarTab,
        count: usize,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let sel = t.element_selected;
        let mac = crate::keymap::Platform::current() == crate::keymap::Platform::Macos;
        let tip = match self.keymap.binding_for("new_server") {
            Some(k) => format!("New server… ({})", crate::keymap::display_keys(k, mac)),
            None => "New server…".to_string(),
        };
        crate::dev_id!(
            div()
                .id("sidebar-title")
                .h(px(ROW_H))
                .w_full()
                .flex_none()
                .flex()
                .flex_row()
                .items_center()
                .pl(px(10.))
                .pr(px(6.))
                .gap(px(4.))
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(
                    div()
                        .flex_1()
                        .child(format!("{} {count}", tab.label().to_uppercase()))
                )
                .when(tab == tree::SidebarTab::Servers, |d| {
                    d.child(crate::dev_id_meta!(
                        div()
                            .id("sidebar-new-server")
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(sel))
                            .tooltip(crate::ui::tooltip(tip.clone()))
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.begin_prompt(crate::shell::PromptAction::NewServer, "", w, cx)
                            }))
                            .child(icon("plus", 14., t.icon_muted)),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        "sidebar-new-server",
                        Button,
                        "New server",
                        Action("new_server")
                    ))
                }),
            "sidebar-title",
            Row,
            tab.label(),
            None
        )
    }

    fn render_sidebar_empty(
        &self,
        t: &Theme,
        tab: tree::SidebarTab,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let (hover, border) = (t.element_hover, t.border);
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("sidebar-empty-{}", tab.id())))
                .flex_1()
                .min_h_0()
                .w_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .px(px(12.))
                .text_size(px(12.))
                .text_color(t.text_muted)
                .child(div().text_center().child(tab.empty_text()))
                .when(tab == tree::SidebarTab::Servers, |d| {
                    d.child(crate::dev_id!(
                        div()
                            .id("sidebar-empty-new-server")
                            .h(px(24.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border)
                            .text_color(t.text)
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.begin_prompt(crate::shell::PromptAction::NewServer, "", w, cx)
                            }))
                            .child("New server…"),
                        "sidebar-empty-new-server",
                        Button,
                        "New server…",
                        Action("new_server")
                    ))
                }),
            format!("sidebar-empty-{}", tab.id()),
            Text,
            tab.empty_text(),
            None
        )
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
        let is_server = matches!(r.kind, RowKind::Server { .. });
        let disabled = matches!(r.kind, RowKind::Server { enabled: false, .. });
        let lead: gpui::AnyElement = match &r.kind {
            RowKind::Server { running, .. } => status_dot(t, *running).into_any_element(),
            RowKind::Folder => icon(
                if r.expanded { "folder-open" } else { "folder" },
                14.,
                t.icon_muted,
            )
            .into_any_element(),
            RowKind::Route { method } => method_badge(t, method).into_any_element(),
            RowKind::Workspace => div().into_any_element(),
            RowKind::Environment { active } => icon(
                if *active { "check" } else { "layers" },
                13.,
                if *active { t.created } else { t.icon_muted },
            )
            .into_any_element(),
            RowKind::Dataset => icon("table", 13., t.icon_muted).into_any_element(),
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
                .when(!is_server, |d| {
                    d.invisible()
                        .group_hover(group_name.clone(), |s| s.visible())
                })
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
                .child(icon(
                    if is_server {
                        "ellipsis-vertical"
                    } else {
                        "ellipsis"
                    },
                    14.,
                    t.icon_muted
                )),
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
                .text_size(px(13.))
                .text_color(if disabled { t.text_muted } else { t.text })
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
                .child(div().flex_1().truncate().child(r.label.clone()))
                .children(match &r.kind {
                    RowKind::Server { port, enabled, .. } => Some(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(4.))
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .when(!enabled, |d| {
                                d.child(crate::dev_id!(
                                    div()
                                        .id(crate::ui::eid(format!("disabled-{}", r.id)))
                                        .child("disabled"),
                                    format!("tag-disabled-{}", r.id),
                                    Text,
                                    "disabled",
                                    None
                                ))
                            })
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
    /// The workspace node above the tree: name (tooltip: full path) and its hamburger menu.
    fn render_workspace_header(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let (hover, sel) = (t.element_hover, t.element_selected);
        let name = self.ws.name.clone();
        let path = self.ws.root.display().to_string();
        crate::dev_id!(
            div()
                .id("tree-workspace")
                .h(px(ROW_H + 4.))
                .pl(px(10.))
                .pr(px(6.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .border_b_1()
                .border_color(t.border_variant)
                .hover(move |s| s.bg(hover))
                .child(icon("folder-open", 14., t.icon_muted))
                .child(crate::dev_id_meta!(
                    div()
                        .id("tree-workspace-name")
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_size(px(12.))
                        .tooltip(crate::ui::tooltip(path))
                        .child(name.clone()),
                    crate::dev_mcp::tree::Meta {
                        tooltip: true,
                        ..Default::default()
                    },
                    "tree-workspace-name",
                    Text,
                    &name,
                    None
                ))
                .child(crate::dev_id!(
                    div()
                        .id("workspace-menu")
                        .size(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .cursor_pointer()
                        .hover(move |s| s.bg(sel))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                this.open_menu(
                                    tree::WORKSPACE_ROW,
                                    f32::from(ev.position.x),
                                    f32::from(ev.position.y),
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        )
                        .child(icon("ellipsis-vertical", 14., t.icon_muted)),
                    "workspace-menu",
                    Button,
                    "Workspace menu",
                    RowMenu(tree::WORKSPACE_ROW.to_string())
                )),
            "tree-workspace",
            Row,
            &self.ws.name,
            None
        )
    }

    pub(crate) fn render_row_menu(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.menu.as_ref().expect("menu is open");
        let running = self.ws.find_server(&m.row_id).is_some_and(|s| s.running);
        let hover = t.element_hover;
        let (is_ws, is_server) = (
            matches!(m.kind, RowKind::Workspace),
            matches!(m.kind, RowKind::Server { .. }),
        );
        // (label, dev id, action)
        let items: Vec<(String, String, tree::MenuAct)> = if is_ws {
            tree::workspace_menu_items(&self.recents)
                .into_iter()
                .map(|(l, a)| (l, format!("menu-workspace-{}", a.key()), a))
                .collect()
        } else {
            tree::menu_items(&m.kind, running)
                .into_iter()
                .map(|(l, a)| {
                    let id = if is_server {
                        format!("menu-server-{}", slug(l))
                    } else {
                        format!("menu-{l}")
                    };
                    (l.to_string(), id, a)
                })
                .collect()
        };
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
                    .children(items.into_iter().map(|(label, dev_id, act)| {
                        crate::dev_id_z!(
                            div()
                                .id(crate::ui::eid(dev_id.clone()))
                                .h(px(26.))
                                .px(px(12.))
                                .flex()
                                .items_center()
                                .cursor_pointer()
                                .text_size(px(12.))
                                .hover(move |s| s.bg(hover))
                                .on_click(
                                    cx.listener(move |this, _, w, cx| this.run_menu(act, w, cx))
                                )
                                .child(label.clone()),
                            1,
                            dev_id,
                            MenuItem,
                            label,
                            Menu(act)
                        )
                    })),
            )
    }
}

/// `Change Port…` -> `change-port`.
fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}
