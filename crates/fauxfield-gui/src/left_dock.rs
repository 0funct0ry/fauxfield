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
            .on_drop(cx.listener(|this, p: &gpui::ExternalPaths, w, cx| {
                this.import_dropped(p.paths().to_vec(), w, cx)
            }))
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
                .when(tab == tree::SidebarTab::Environments, |d| {
                    d.child(crate::dev_id_meta!(
                        div()
                            .id("sidebar-new-environment")
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(sel))
                            .tooltip(crate::ui::tooltip("New environment…".to_string()))
                            .on_click(cx.listener(|this, _, w, cx| {
                                this.begin_prompt(
                                    crate::shell::PromptAction::NewEnvironment,
                                    "",
                                    w,
                                    cx,
                                )
                            }))
                            .child(icon("plus", 14., t.icon_muted)),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        "sidebar-new-environment",
                        Button,
                        "New environment",
                        Action("new_environment")
                    ))
                })
                .when(tab == tree::SidebarTab::Datasets, |d| {
                    d.child(crate::dev_id_meta!(
                        div()
                            .id("sidebar-new-dataset")
                            .size(px(20.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(sel))
                            .tooltip(crate::ui::tooltip("New Dataset…".to_string()))
                            .on_click(cx.listener(|this, _, w, cx| this.begin_new_dataset(w, cx)))
                            .child(icon("plus", 14., t.icon_muted)),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        "sidebar-new-dataset",
                        Button,
                        "New Dataset",
                        Action("new_dataset")
                    ))
                })
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
                .when(tab == tree::SidebarTab::Environments, |d| {
                    d.child(crate::dev_id!(
                        div()
                            .id("sidebar-empty-new-environment")
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
                                this.begin_prompt(
                                    crate::shell::PromptAction::NewEnvironment,
                                    "",
                                    w,
                                    cx,
                                )
                            }))
                            .child("New environment…"),
                        "sidebar-empty-new-environment",
                        Button,
                        "New environment…",
                        Action("new_environment")
                    ))
                })
                .when(tab == tree::SidebarTab::Datasets, |d| {
                    d.child(crate::dev_id!(
                        div()
                            .id("sidebar-empty-new-dataset")
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
                            .on_click(cx.listener(|this, _, w, cx| this.begin_new_dataset(w, cx)))
                            .child("New Dataset…"),
                        "sidebar-empty-new-dataset",
                        Button,
                        "New Dataset…",
                        Action("new_dataset")
                    ))
                })
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
        let draggable = matches!(r.kind, RowKind::Route { .. } | RowKind::Folder { .. });
        let drop_id = r.id.clone();
        let drag = tree::DraggedRow {
            id: r.id.clone(),
            label: r.label.clone(),
        };
        let disabled = matches!(
            r.kind,
            RowKind::Server { enabled: false, .. }
                | RowKind::Folder { enabled: false, .. }
                | RowKind::Route { disabled: true, .. }
        );
        let lead: gpui::AnyElement = match &r.kind {
            RowKind::Server { running, https, .. } => div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .child(status_dot(t, *running))
                .when(*https, |d| {
                    d.child(crate::dev_id!(
                        div().child(icon("lock", 11., t.icon_muted)),
                        format!("tree-lock-{}", r.id),
                        Text,
                        "HTTPS",
                        None
                    ))
                })
                .into_any_element(),
            RowKind::Folder { .. } => icon(
                if r.expanded { "folder-open" } else { "folder" },
                14.,
                t.icon_muted,
            )
            .into_any_element(),
            RowKind::Route { method, .. } => method_badge(t, method).into_any_element(),
            RowKind::Workspace => div().into_any_element(),
            RowKind::Environment { active } => icon(
                if *active { "check" } else { "layers" },
                13.,
                if *active { t.created } else { t.icon_muted },
            )
            .into_any_element(),
            RowKind::Dataset { .. } => icon("table", 13., t.icon_muted).into_any_element(),
            RowKind::CrudOp { method } => method_badge(t, method).into_any_element(),
            RowKind::Response { .. } | RowKind::ErrorPresets | RowKind::DatasetRow => {
                div().into_any_element()
            }
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
                .when(draggable, |d| {
                    let (hover_drop, drop_target) = (hover, drop_id.clone());
                    d.on_drag(drag, |d, _, _, cx| {
                        gpui::AppContext::new(cx, |_| crate::center::DragGhost(d.label.clone()))
                    })
                    .drag_over::<tree::DraggedRow>(move |s, _, _, _| s.bg(hover_drop))
                    .on_drop(cx.listener(
                        move |this, d: &tree::DraggedRow, _, cx| {
                            this.drop_row(&d.id, &drop_target, cx)
                        },
                    ))
                })
                .when(!draggable && is_server, |d| {
                    let target = drop_id.clone();
                    d.drag_over::<tree::DraggedRow>(move |s, _, _, _| s.bg(hover))
                        .on_drop(cx.listener(move |this, d: &tree::DraggedRow, _, cx| {
                            this.drop_row(&d.id, &target, cx)
                        }))
                })
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
                            .items_center()
                            .gap(px(4.))
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .when(
                                self.ws
                                    .servers
                                    .iter()
                                    .any(|s| s.id == r.id && s.random_output()),
                                |d| {
                                    d.child(crate::dev_id!(
                                        div()
                                            .id(crate::ui::eid(format!("dice-{}", r.id)))
                                            .flex_none()
                                            .tooltip(crate::ui::tooltip(
                                                crate::view::RANDOM_OUTPUT_TIP.into()
                                            ))
                                            .child(icon("dice", 12., t.modified)),
                                        format!("tree-dice-{}", r.id),
                                        Text,
                                        crate::view::RANDOM_OUTPUT_TIP,
                                        None
                                    ))
                                },
                            )
                            .when(
                                self.ws
                                    .servers
                                    .iter()
                                    .any(|s| s.id == r.id && s.chaos.is_some()),
                                |d| {
                                    let tip = self
                                        .ws
                                        .servers
                                        .iter()
                                        .find(|s| s.id == r.id)
                                        .and_then(|s| s.chaos)
                                        .map(|c| {
                                            format!(
                                                "Chaos on: {:.0} % of requests get a random fault",
                                                c * 100.0
                                            )
                                        })
                                        .unwrap_or_default();
                                    d.child(crate::dev_id!(
                                        div()
                                            .id(crate::ui::eid(format!("chaos-{}", r.id)))
                                            .flex_none()
                                            .tooltip(crate::ui::tooltip(tip.clone()))
                                            .child(icon("zap", 12., t.modified)),
                                        format!("tree-chaos-{}", r.id),
                                        Text,
                                        tip,
                                        None
                                    ))
                                },
                            )
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
                            .child(format!(":{port}"))
                            .into_any_element(),
                    ),
                    RowKind::Folder { routes, .. } => Some(
                        div()
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(routes.to_string())
                            .into_any_element(),
                    ),
                    RowKind::Route { warn: Some(w), .. } => Some(
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("warn-{}", r.id)))
                                .flex_none()
                                .tooltip(crate::ui::tooltip(w.message.clone()))
                                .child(icon(
                                    "triangle-alert",
                                    12.,
                                    if w.error { t.deleted } else { t.modified }
                                )),
                            format!("tree-shadow-icon-{}", r.id),
                            Text,
                            w.message.clone(),
                            None
                        )
                        .into_any_element()
                    ),
                    RowKind::Dataset { unused: true, .. } => Some(
                        crate::dev_id!(
                            div()
                                .id(crate::ui::eid(format!("unused-{}", r.id)))
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(t.text_muted)
                                .tooltip(crate::ui::tooltip("No Route uses this Dataset".into()))
                                .child("unused"),
                            format!("tag-unused-{}", r.id),
                            Text,
                            "unused",
                            None
                        )
                        .into_any_element()
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

    /// (label, dev id, action) of every entry of menu `m`.
    pub(crate) fn menu_item_list(
        &self,
        m: &crate::shell::RowMenu,
    ) -> Vec<(String, String, tree::MenuAct)> {
        let running = self.ws.find_server(&m.row_id).is_some_and(|s| s.running);
        let is_server = matches!(m.kind, RowKind::Server { .. });
        if matches!(m.kind, RowKind::Workspace) {
            return tree::workspace_menu_items(&self.recents)
                .into_iter()
                .map(|(l, a)| (l, format!("menu-workspace-{}", a.key()), a))
                .collect();
        }
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
    }

    pub(crate) fn render_row_menu(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let m = self.menu.as_ref().expect("menu is open");
        let hover = t.element_hover;
        let items = self.menu_item_list(m);
        let selected = self.menu_sel;
        let muted = t.text;
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
                    .children(
                        items
                            .into_iter()
                            .enumerate()
                            .map(|(i, (label, dev_id, act))| {
                                crate::dev_id_z!(
                                    div()
                                        .id(crate::ui::eid(dev_id.clone()))
                                        .h(px(26.))
                                        .px(px(12.))
                                        .flex()
                                        .items_center()
                                        .cursor_pointer()
                                        .text_size(px(12.))
                                        .when(i == selected, move |d| d.bg(hover))
                                        .on_mouse_move(cx.listener(move |this, _, _, cx| {
                                            if this.menu_sel != i {
                                                this.menu_sel = i;
                                                cx.notify();
                                            }
                                        }))
                                        .on_click(cx.listener(move |this, _, w, cx| {
                                            this.run_menu(act, w, cx)
                                        }))
                                        .text_color(muted)
                                        .child(label.clone()),
                                    1,
                                    dev_id,
                                    MenuItem,
                                    label,
                                    Menu(act)
                                )
                            }),
                    ),
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
