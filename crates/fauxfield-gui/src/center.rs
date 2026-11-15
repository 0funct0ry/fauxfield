//! Centre pane: the Zed-style tab bar and the Route, Server and Environment tab bodies, plus the
//! right dock (Inspector placeholder). All on static mock data until M2.

use gpui::{
    AnyElement, ClickEvent, Context, InteractiveElement, IntoElement, MouseButton, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, div, prelude::*, px,
};

use crate::assets::BUFFER_FONT;
use crate::shell::{ROUTE_SUBTABS, SERVER_SECTIONS, Shell};
use crate::tabs::{Tab, TabKind};
use crate::theme::Theme;
use crate::ui::{ROW_H, TAB_BAR_H, icon, status_dot, toolbar_button};
use crate::view::{Environment, Route, Server};
use fauxfield_model::vars::Scope;

/// Drag payload of a tab being reordered.
#[derive(Clone)]
pub struct DraggedTab {
    pub index: usize,
    pub title: String,
}

pub(crate) struct DragGhost(pub(crate) String);

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
        TabKind::Folder => "folder",
        TabKind::Environment => "layers",
        TabKind::Dataset => "table",
        TabKind::Settings => "settings",
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
            .children((!self.conflicts.is_empty()).then(|| self.render_merge_banner(t, cx)))
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
            TabKind::Folder => t.icon_muted,
            TabKind::Environment => t.modified,
            TabKind::Dataset => t.modified,
            TabKind::Settings => t.icon_muted,
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
                        .size(px(20.))
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
                Some((s, r)) if r.is_static() => {
                    self.render_static_route_tab(t, s, r, cx).into_any_element()
                }
                Some((s, r)) if r.is_websocket() => {
                    self.render_ws_route_tab(t, s, r, cx).into_any_element()
                }
                Some((s, r)) if r.is_crud() => {
                    self.render_crud_route_tab(t, s, r, cx).into_any_element()
                }
                Some((s, r)) => self.render_route_tab(t, s, r, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
            TabKind::Server => match self.ws.find_server(&tab.id) {
                Some(s) => self.render_server_tab(t, s, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
            TabKind::Folder => match self.ws.find_folder(&tab.id) {
                Some((s, f)) => self.render_folder_tab(t, s, f, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
            TabKind::Environment => match self.ws.find_environment(&tab.id) {
                Some(e) => self.render_environment_tab(t, e, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
            TabKind::Dataset => match self.ws.find_dataset(&tab.id).cloned() {
                Some(d) => self.render_dataset_tab(t, &d, cx).into_any_element(),
                None => self.render_empty(t).into_any_element(),
            },
            TabKind::Settings => self.render_settings_tab(t, cx).into_any_element(),
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
                    .child(self.route_method_combo(t, r, cx))
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
                    .children((!s.prefix.is_empty()).then(|| {
                        div()
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(format!("prefix {}", s.prefix))
                    }))
                    .child(crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("route-match-{}", r.id)))
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(4.))
                            .bg(t.element_selected)
                            .cursor_pointer()
                            .text_size(px(11.))
                            .text_color(if r.pattern_error.is_some() {
                                t.deleted
                            } else {
                                t.text_muted
                            })
                            .on_click(cx.listener({
                                let rid = r.id.clone();
                                move |this, _, _, cx| {
                                    this.route_sub.insert(rid.clone(), 5);
                                    cx.notify();
                                }
                            }))
                            .child(format!(
                                "Match: {}{}",
                                r.match_mode,
                                if r.match_explicit.is_none() {
                                    " auto"
                                } else {
                                    ""
                                }
                            )),
                        format!("route-match-dropdown-{}", r.id),
                        Button,
                        "Match mode",
                        Action("edit_route_match")
                    ))
                    .children(
                        // The view model falls back to "METHOD path" for unnamed Routes;
                        // showing that next to the path is redundant.
                        (r.name != format!("{} {}", r.method, r.path)).then(|| {
                            div()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child(r.name.clone())
                        }),
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
                    0 => self.render_responses(t, r, cx).into_any_element(),
                    1 => self.render_rules(t, r, cx),
                    2 => self.render_route_headers(t, r, cx),
                    3 => self.render_route_callbacks(t, r, cx),
                    5 => self.render_route_settings(t, s, r, cx).into_any_element(),
                    n => div()
                        .p(px(16.))
                        .text_color(t.text_muted)
                        .child(format!(
                            "{} {} in {}.",
                            ROUTE_SUBTABS[n],
                            // Rules and Callbacks are plural; the rest are singular.
                            if matches!(n, 1 | 3) {
                                "arrive"
                            } else {
                                "arrives"
                            },
                            // PROMPTS.md: Rules M5, Headers M6, Callbacks M14, Docs M15, Settings M4.
                            ["", "M5", "M6", "M14", "M15", "M4"][n]
                        ))
                        .into_any_element(),
                }),
            )
    }

    /// Folder tab (SPEC §20.17): header with the Enabled toggle, settings, routes table.
    fn render_folder_tab(
        &self,
        t: &Theme,
        s: &Server,
        f: &crate::view::Folder,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = |k: &str| {
            div()
                .w(px(190.))
                .flex_none()
                .text_color(t.text_muted)
                .child(k.to_string())
        };
        let row = || div().flex().flex_row().h(px(ROW_H + 4.)).items_center();
        let (fid_en, fid_col, fid_name) = (f.id.clone(), f.id.clone(), f.id.clone());
        let cell = |w: f32, text: String, color| {
            div()
                .w(px(w))
                .flex_none()
                .truncate()
                .font_family(BUFFER_FONT)
                .text_size(px(12.))
                .text_color(color)
                .child(text)
        };
        div()
            .id(crate::ui::eid(format!("folder-tab-{}", f.id)))
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .pb(px(8.))
                    .child(icon("folder", 16., t.icon_muted))
                    .child(div().text_size(px(14.)).child(f.name.clone()))
                    .child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(format!("{} · {} routes", s.name, f.routes.len())),
                    ),
            )
            .child(section_heading(t, "General"))
            .child(row().child(label("Name")).child(field_button_with(
                "folder-name",
                f.name.clone(),
                "rename",
                cx.listener(move |this, _, w, cx| this.rename_server_row(&fid_name, w, cx)),
            )))
            .child(row().child(label("Enabled")).child(toggle_button(
                t,
                "folder-enabled",
                f.enabled,
                "folder_toggle_enabled",
                cx.listener(move |this, _, _, cx| this.folder_toggle(&fid_en, true, cx)),
            )))
            .child(row().child(label("Start collapsed")).child(toggle_button(
                t,
                "folder-collapsed",
                f.collapsed,
                "folder_toggle_collapsed",
                cx.listener(move |this, _, _, cx| this.folder_toggle(&fid_col, false, cx)),
            )))
            .child(div().mt(px(12.)).child(section_heading(t, "Routes")))
            .child(crate::dev_id!(
                div()
                    .id("folder-routes-table")
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .h(px(ROW_H))
                            .items_center()
                            .text_size(px(11.))
                            .text_color(t.text_muted)
                            .child(div().w(px(70.)).child("Method"))
                            .child(div().w(px(260.)).child("Path"))
                            .child(div().w(px(80.)).child("Match"))
                            .child(div().w(px(70.)).child("Priority"))
                            .child(div().w(px(90.)).child("Status")),
                    )
                    .children(f.routes.iter().map(|r| {
                        let rid = r.id.clone();
                        let status = if r.pattern_error.is_some() {
                            ("invalid", t.deleted)
                        } else if r.shadowed_by.is_some() {
                            ("shadowed", t.modified)
                        } else if f.enabled {
                            ("enabled", t.created)
                        } else {
                            ("disabled", t.text_muted)
                        };
                        div()
                            .id(crate::ui::eid(format!("folder-route-{}", r.id)))
                            .flex()
                            .flex_row()
                            .h(px(ROW_H))
                            .items_center()
                            .cursor_pointer()
                            .hover(|s| s.bg(t.element_hover))
                            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, _, cx| {
                                this.click_row(&rid, ev.click_count(), cx)
                            }))
                            .child(cell(70., r.method.clone(), t.method_color(&r.method)))
                            .child(cell(260., r.path.clone(), t.text))
                            .child(cell(80., r.match_mode.clone(), t.text_muted))
                            .child(cell(70., r.priority.to_string(), t.text_muted))
                            .child(cell(90., status.0.into(), status.1))
                    }))
                    .children(f.routes.is_empty().then(|| {
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child("No routes in this folder. Drag routes here or use New route…")
                    })),
                "folder-routes-table",
                Text,
                "Folder routes",
                None
            ))
    }

    /// Route Settings sub-tab (SPEC §20.15, §20.17): matching, priority, test a path.
    fn render_route_settings(
        &self,
        t: &Theme,
        s: &Server,
        r: &Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let label = |k: &str| {
            div()
                .w(px(190.))
                .flex_none()
                .text_color(t.text_muted)
                .child(k.to_string())
        };
        let row = || div().flex().flex_row().h(px(ROW_H + 4.)).items_center();
        let mode_text = if r.match_explicit.is_some() {
            r.match_mode.clone()
        } else {
            format!("{} (inferred)", r.match_mode)
        };
        let input = self.test_path.get(&r.id).cloned().unwrap_or_default();
        let result = self.test_path_result(s, r, &input);
        let note = |text: String, color| {
            div()
                .ml(px(190.))
                .text_size(px(12.))
                .text_color(color)
                .child(text)
        };
        div()
            .id(crate::ui::eid(format!("route-settings-{}", r.id)))
            .size_full()
            .overflow_y_scroll()
            .p(px(16.))
            .flex()
            .flex_col()
            .child(section_heading(t, "Matching"))
            .child(row().child(label("Match mode")).child(field_button_with(
                "route-match-mode",
                mode_text,
                "edit_route_match",
                cx.listener(|this, _, w, cx| this.edit_route_setting(1, w, cx)),
            )))
            .child(row().child(label("Priority")).child(field_button_with(
                "route-priority-input",
                r.priority.to_string(),
                "edit_route_priority",
                cx.listener(|this, _, w, cx| this.edit_route_setting(0, w, cx)),
            )))
            .child({
                let route_id = r.id.clone();
                row()
                    .child(label("Response mode"))
                    .child(self.response_mode_combo(&route_id, &r.response_mode, cx))
            })
            .child(row().child(label("Test a path")).child(field_button_with(
                "route-test-path-input",
                if input.is_empty() {
                    format!("{}/users/42", s.prefix)
                } else {
                    input.clone()
                },
                "route_test_path",
                cx.listener(|this, _, w, cx| this.edit_route_setting(2, w, cx)),
            )))
            .children(result.map(|(ok, text)| {
                crate::dev_id!(
                    note(text.clone(), if ok { t.created } else { t.text_muted })
                        .id("route-test-path-result"),
                    "route-test-path-result",
                    Text,
                    text,
                    None
                )
            }))
            .children(r.pattern_error.clone().map(|e| {
                crate::dev_id!(
                    note(e.clone(), t.deleted).id("route-pattern-error"),
                    "route-pattern-error",
                    Text,
                    e,
                    None
                )
            }))
            .children(
                r.shadowed_by
                    .clone()
                    .map(|by| note(format!("Never reached: shadowed by {by}"), t.modified)),
            )
            .when(r.kind == "http", |d| {
                let proxy_on = s.proxy.as_ref().is_some_and(|p| p.enabled);
                let toggle = |this: &Self, flag: &'static str, on: bool, cx: &mut Context<Self>| {
                    let rid = r.id.clone();
                    let (idn, text) = (
                        if flag == "proxy" {
                            "route-forward"
                        } else {
                            "route-pinned"
                        },
                        if on { "On" } else { "Off" },
                    );
                    crate::dev_id!(
                        this.state_button(
                            t,
                            format!("{idn}-{}", r.id),
                            text.into(),
                            on,
                            70.,
                            move |this, cx| this.toggle_route_flag(&rid, flag, cx),
                            cx,
                        ),
                        idn,
                        Button,
                        text,
                        Press(format!("{idn}:{}", r.id))
                    )
                };
                d.child(div().mt(px(14.)).child(section_heading(t, "Proxy")))
                    .child(
                        row()
                            .child(label("Forward to upstream"))
                            .child(toggle(self, "proxy", r.forward, cx)),
                    )
                    .child(
                        row()
                            .child(label("Pin mock"))
                            .child(toggle(self, "pinned", r.pinned, cx)),
                    )
                    .child(note(
                        if !proxy_on {
                            "The proxy is off for this Server: enable it in the Server tab (Proxy)."
                                .to_string()
                        } else if r.pinned {
                            "Pinned: always answered by this mock, even in passthrough mode."
                                .to_string()
                        } else if r.forward {
                            "Forwarded to the upstream; the Responses below are not used."
                                .to_string()
                        } else {
                            "Mocked unless the proxy mode is passthrough.".to_string()
                        },
                        t.text_muted,
                    ))
            })
            .child(div().mt(px(14.)).child(self.render_var_section(
                t,
                Scope::Route,
                Some(r.id.clone()),
                self.file_vars(Scope::Route, &r.id),
                "route",
                cx,
            )))
    }

    /// Result line of "Test a path": `(matches, text)`; `None` while the input is empty.
    fn test_path_result(&self, s: &Server, r: &Route, input: &str) -> Option<(bool, String)> {
        if input.is_empty() {
            return None;
        }
        let snap = self.store.as_ref()?.snapshot();
        let (_, srv) = snap.servers.iter().find(|(_, x)| x.id == s.id)?;
        let table = fauxfield_engine::RouteTable::compile(srv);
        let method = if r.method.eq_ignore_ascii_case("ANY") {
            "GET"
        } else {
            r.method.as_str()
        };
        let c = table.candidates(method, input);
        let Some(me) = c.iter().find(|m| m.route.id == r.id) else {
            return Some((false, "✗ does not match".into()));
        };
        let params = me
            .params
            .iter()
            .map(|(k, v)| format!("{k} = {v}"))
            .collect::<Vec<_>>()
            .join(", ");
        let mut text = if params.is_empty() {
            "✓ matches".to_string()
        } else {
            format!("✓ matches · {params}")
        };
        if let Some(w) = c.first().filter(|w| w.route.id != r.id) {
            text.push_str(&format!(
                ", but {} (priority {}) wins",
                w.pattern, w.priority
            ));
        }
        Some((true, text))
    }

    fn render_responses(&self, t: &Theme, r: &Route, cx: &mut Context<Self>) -> impl IntoElement {
        let sel = self
            .resp_sel
            .get(&r.id)
            .copied()
            .unwrap_or(0)
            .min(r.responses.len().saturating_sub(1));
        let first = r.responses.get(sel).cloned();
        div()
            .size_full()
            .flex()
            .flex_row()
            .child(
                div()
                    .w(px(self.layout.resp_list_width))
                    .h_full()
                    .flex_none()
                    .bg(t.panel)
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .id(crate::ui::eid(format!("resp-list-{}", r.id)))
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .children(r.responses.iter().enumerate().map(|(i, resp)| {
                                self.render_response_row(t, r, i, i == sel, resp, cx)
                            })),
                    )
                    .child(self.render_response_footer(t, r, cx)),
            )
            .child(self.resize_handle_v(t, crate::shell::Resize::RespList, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .children(first.map(|resp| self.render_response_pane(t, &resp, cx))),
            )
    }

    fn render_response_row(
        &self,
        t: &Theme,
        r: &Route,
        i: usize,
        selected: bool,
        resp: &crate::view::Response,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rid = r.id.clone();
        let (resp_id, menu_id, dots_id) = (resp.id.clone(), resp.id.clone(), resp.id.clone());
        let group = format!("resp-grp-{}", resp.id);
        let drag = crate::tree::DraggedResponse {
            id: resp.id.clone(),
            label: resp.name.clone(),
        };
        let (drop_target, hover_drop) = (resp.id.clone(), t.element_hover);
        let rules = resp.rules.len();
        let dim = resp.disabled;
        let badge = |text: String| {
            div()
                .px(px(5.))
                .rounded(px(3.))
                .bg(t.element_hover)
                .text_size(px(10.))
                .text_color(t.text_muted)
                .child(text)
        };
        crate::dev_id!(
            div()
                .id(crate::ui::eid(format!("resp-{}-{i}", r.id)))
                .group(group.clone())
                .min_h(px(ROW_H + 4.))
                .py(px(3.))
                .px(px(10.))
                .flex()
                .items_center()
                .gap(px(6.))
                .cursor_pointer()
                .on_click(cx.listener(move |this, ev: &ClickEvent, w, cx| {
                    this.resp_sel.insert(rid.clone(), i);
                    if ev.click_count() >= 2 {
                        this.rename_row(&resp_id, w, cx);
                    }
                    cx.notify();
                }))
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                        this.open_response_menu(
                            &menu_id,
                            f32::from(ev.position.x),
                            f32::from(ev.position.y),
                            cx,
                        )
                    }),
                )
                .on_drag(drag, |d, _, _, cx| {
                    gpui::AppContext::new(cx, |_| DragGhost(d.label.clone()))
                })
                .drag_over::<crate::tree::DraggedResponse>(move |s, _, _, _| s.bg(hover_drop))
                .on_drop(
                    cx.listener(move |this, d: &crate::tree::DraggedResponse, _, cx| {
                        this.drop_response(&d.id, &drop_target, cx)
                    },)
                )
                .when(selected, |d| d.bg(t.element_selected))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.status_color(resp.status))
                        .child(resp.status.to_string()),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .text_color(if dim { t.text_muted } else { t.text })
                        .child(div().truncate().child(resp.name.clone()))
                        .children((!resp.description.is_empty()).then(|| {
                            crate::dev_id!(
                                div()
                                    .id(crate::ui::eid(format!("resp-desc-{}", resp.id)))
                                    .truncate()
                                    .text_size(px(11.))
                                    .text_color(t.text_muted)
                                    .child(resp.description.clone()),
                                format!("resp-desc-{}", resp.id),
                                Text,
                                resp.description.clone(),
                                None
                            )
                        })),
                )
                .children(dim.then(|| badge("off".into())))
                .children((rules > 0).then(|| {
                    crate::dev_id!(
                        badge(format!("{rules} rule{}", if rules == 1 { "" } else { "s" }))
                            .id(crate::ui::eid(format!("resp-rules-{}", resp.id))),
                        format!("resp-rules-{}", resp.id),
                        Text,
                        format!("{rules} rules"),
                        None
                    )
                }))
                .children((!resp.fault.is_empty()).then(|| {
                    let tip = format!(
                        "{}{}",
                        crate::network_ui::fault_tip(&resp.fault),
                        if resp.fault_rate.is_empty() {
                            String::new()
                        } else {
                            format!(" · rate {}", resp.fault_rate)
                        }
                    );
                    crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("resp-fault-icon-{}", resp.id)))
                            .flex_none()
                            .tooltip(crate::ui::tooltip(tip.clone()))
                            .child(icon("zap", 12., t.deleted)),
                        format!("resp-fault-icon-{}", resp.id),
                        Text,
                        tip,
                        None
                    )
                }))
                .children(resp.default.then(|| {
                    crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("resp-default-{}", resp.id)))
                            .child(icon("star", 12., t.modified)),
                        format!("resp-default-{}", resp.id),
                        Text,
                        "Default response",
                        None
                    )
                }))
                .child(crate::dev_id!(
                    div()
                        .id(crate::ui::eid(format!("resp-dots-{}", resp.id)))
                        .size(px(20.))
                        .flex_none()
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(px(3.))
                        .invisible()
                        .group_hover(group, |s| s.visible())
                        .hover(|s| s.bg(t.element_selected))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                                this.open_response_menu(
                                    &dots_id,
                                    f32::from(ev.position.x),
                                    f32::from(ev.position.y),
                                    cx,
                                );
                                cx.stop_propagation();
                            }),
                        )
                        .child(icon("ellipsis", 14., t.icon_muted)),
                    format!("resp-dots-{}", resp.id),
                    Button,
                    format!("Menu for {}", resp.name),
                    None
                )),
            format!("resp-{}-{i}", r.id),
            Row,
            &resp.name,
            Response(r.id.clone(), i)
        )
        .into_any_element()
    }

    /// "New response" and "Add error preset" under the response list.
    fn render_response_footer(&self, t: &Theme, r: &Route, cx: &mut Context<Self>) -> AnyElement {
        let (new_id, preset_id) = (r.id.clone(), r.id.clone());
        let btn = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .h(px(26.))
                .px(px(10.))
                .flex()
                .items_center()
                .text_size(px(12.))
                .text_color(t.text_muted)
                .cursor_pointer()
                .hover(|s| s.bg(t.element_hover).text_color(t.text))
                .child(label)
        };
        div()
            .flex_none()
            .border_t_1()
            .border_color(t.border_variant)
            .flex()
            .flex_col()
            .child(crate::dev_id!(
                btn("resp-add", "+ New response").on_click(
                    cx.listener(move |this, _, _, cx| { this.new_response_for(&new_id, cx) })
                ),
                "resp-add",
                Button,
                "New response",
                Press(format!("resp-add:{}", r.id))
            ))
            .child(crate::dev_id!(
                btn("resp-add-preset", "+ Add error preset…").on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &gpui::MouseDownEvent, _, cx| {
                        this.open_picker_menu(
                            &preset_id,
                            crate::tree::RowKind::ErrorPresets,
                            f32::from(ev.position.x),
                            f32::from(ev.position.y) - 300.,
                            cx,
                        );
                    })
                ),
                "resp-add-preset",
                Button,
                "Add error preset",
                Press(format!("resp-add-preset:{}", r.id))
            ))
            .into_any_element()
    }

    // ---- Server tab ----------------------------------------------------------------------

    /// Header plus one scrolling form of sections (MOCKUP `serverView`). Only General has
    /// content before M7 to M11; the other headings say when theirs arrive.
    fn render_server_tab(&self, t: &Theme, s: &Server, cx: &mut Context<Self>) -> impl IntoElement {
        let index = self
            .ws
            .servers
            .iter()
            .position(|x| x.id == s.id)
            .unwrap_or(0);
        let addr = s
            .listen
            .clone()
            .unwrap_or_else(|| format!("{}:{}", s.host, s.port));
        let url = format!("{}://{addr}", if s.https() { "https" } else { "http" });
        let url_copy = url.clone();
        div()
            .id(crate::ui::eid(format!("server-tab-{}", s.id)))
            .size_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .h(px(40.))
                    .flex_none()
                    .px(px(16.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(status_dot(t, s.running))
                    .child(div().text_size(px(14.)).child(s.name.clone()))
                    .when(s.random_output(), |d| {
                        d.child(crate::dev_id!(
                            div()
                                .id("server-dice")
                                .flex_none()
                                .tooltip(crate::ui::tooltip(crate::view::RANDOM_OUTPUT_TIP.into()))
                                .child(icon("dice", 14., t.modified)),
                            "server-dice",
                            Text,
                            crate::view::RANDOM_OUTPUT_TIP,
                            None
                        ))
                    })
                    .when(!s.enabled, |d| {
                        d.child(
                            div()
                                .px(px(6.))
                                .rounded(px(3.))
                                .border_1()
                                .border_color(t.border_variant)
                                .text_size(px(11.))
                                .text_color(t.text_muted)
                                .child("disabled"),
                        )
                    })
                    .child(crate::dev_id!(
                        div()
                            .id("server-address")
                            .font_family(BUFFER_FONT)
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .cursor_pointer()
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.write_to_clipboard(gpui::ClipboardItem::new_string(
                                    url_copy.clone(),
                                ));
                                this.notify("Copied address".into(), cx);
                            }))
                            .child(url.clone()),
                        "server-address",
                        Button,
                        &url,
                        None
                    ))
                    .child(div().flex_1())
                    .child(crate::dev_id!(
                        toolbar_button(t, "server-toggle")
                            .child(icon(
                                if s.running { "square" } else { "play" },
                                12.,
                                t.created,
                            ))
                            .child(if s.running { "Stop" } else { "Start" })
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.toggle_server_at(index, cx)),
                            ),
                        format!("server-toggle-{}", s.id),
                        Button,
                        if s.running { "Stop" } else { "Start" },
                        Action("toggle_server")
                    )),
            )
            .child(
                div()
                    .id(crate::ui::eid(format!("server-form-{}", s.id)))
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(self.server_general(t, s, cx))
                    .children(SERVER_SECTIONS.iter().enumerate().skip(1).map(|(i, name)| {
                        if i == 1 {
                            return self.server_tls_section(t, s, cx);
                        }
                        if i == 2 {
                            return self.server_cors_section(t, s, cx);
                        }
                        if i == 3 {
                            return self.server_network_section(t, s, cx);
                        }
                        if i == 4 {
                            return self.server_rate_limit_section(t, s, cx);
                        }
                        if i == 5 {
                            return self.server_proxy_section(t, s, cx);
                        }
                        if i == 6 {
                            return self.server_datasets_section(t, s, cx);
                        }
                        if i == 7 {
                            return div().mt(px(14.)).flex().flex_col().child(
                                self.render_var_section(
                                    t,
                                    Scope::Server,
                                    Some(s.id.clone()),
                                    self.file_vars(Scope::Server, &s.id),
                                    "server",
                                    cx,
                                ),
                            );
                        }
                        div()
                            .mt(px(14.))
                            .flex()
                            .flex_col()
                            .child(section_heading(t, name))
                            .child(div().text_size(px(12.)).text_color(t.text_muted).child(
                                format!(
                                    "Settings arrive in {}.",
                                    ["", "M10", "M10", "M9", "M9", "M11", "M8", "", "M15"][i]
                                ),
                            ))
                    })),
            )
    }

    /// Server tab "Datasets" section: this Server's Datasets and the New CRUD resource button.
    fn server_datasets_section(&self, t: &Theme, s: &Server, cx: &mut Context<Self>) -> gpui::Div {
        let hover = t.element_hover;
        let border = t.border;
        let sid = s.id.clone();
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(section_heading(t, "Datasets"))
            .children(
                self.ws
                    .datasets
                    .iter()
                    .filter(|d| d.server_id == s.id)
                    .map(|d| {
                        let key = d.key();
                        div()
                            .id(crate::ui::eid(format!("server-ds-{}", d.name)))
                            .h(px(ROW_H + 2.))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .text_size(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .child(icon("table", 13., t.icon_muted))
                            .child(d.label())
                            .child(div().text_color(t.text_muted).child(format!(
                                "key {} · {}{}{}",
                                d.key,
                                d.id_strategy,
                                if d.persist { " · persisted" } else { "" },
                                if d.unused() { " · unused" } else { "" }
                            )))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.click_row(&crate::tree::dataset_row_id(&key), 2, cx)
                            }))
                    }),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("server-new-dataset-{sid}")))
                            .h(px(24.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border)
                            .text_size(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, w, cx| this.begin_new_dataset(w, cx)))
                            .child("New Dataset…"),
                        format!("server-new-dataset-{sid}"),
                        Button,
                        "New Dataset…",
                        Action("new_dataset")
                    ))
                    .child(crate::dev_id!(
                        div()
                            .id(crate::ui::eid(format!("server-new-crud-{sid}")))
                            .h(px(24.))
                            .px(px(10.))
                            .flex()
                            .items_center()
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border)
                            .text_size(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .on_click(cx.listener(|this, _, w, cx| this.begin_new_crud(w, cx)))
                            .child("New CRUD resource…"),
                        format!("server-new-crud-{sid}"),
                        Button,
                        "New CRUD resource…",
                        Action("new_crud_resource")
                    )),
            )
    }

    /// Route tab of a `kind = "crud"` Route: its Dataset and the six generated operations.
    fn render_crud_route_tab(
        &self,
        t: &Theme,
        s: &Server,
        r: &Route,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let key = r.dataset.as_ref().map(|d| format!("{}:{d}", s.id));
        let base = r.path.trim_end_matches('/').to_string();
        let prefix = s.prefix.clone();
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
                    .child(crate::ui::method_badge(t, "CRUD"))
                    .child(div().text_size(px(14.)).child(r.path.clone()))
                    .child(div().flex_1())
                    .when_some(key, |d, key| {
                        d.child(crate::dev_id!(
                            toolbar_button(t, "crud-open-dataset")
                                .child(format!(
                                    "Open dataset {}",
                                    r.dataset.clone().unwrap_or_default()
                                ))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.click_row(&crate::tree::dataset_row_id(&key), 2, cx)
                                })),
                            "crud-open-dataset",
                            Button,
                            "Open dataset",
                            None
                        ))
                    }),
            )
            .child(
                div()
                    .id("crud-tab-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_size(px(12.))
                    .child(section_heading(t, "Operations"))
                    .children(crate::tree::CRUD_OPS.iter().map(|(m, suffix)| {
                        div()
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .h(px(ROW_H))
                            .child(crate::ui::method_badge(t, m))
                            .child(format!("{}{}{}", prefix, base, suffix))
                    }))
                    .child(section_heading(t, "Settings"))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .child("Dataset:")
                            .child(self.route_dataset_combo(
                                &s.id,
                                &r.id,
                                r.dataset.as_deref(),
                                cx,
                            ))
                            .child(format!(
                                "· schema validation: {}",
                                r.validate.clone().unwrap_or_else(|| "off".into())
                            )),
                    )
                    .child(div().text_color(t.text_muted).child(
                        "List supports _page, _limit, _sort, _order, q and field filters (age_gte, age_lte, role_ne, name_like). \
                         An http route with a higher priority overrides one operation.",
                    )),
            )
    }

    fn server_general(&self, t: &Theme, s: &Server, cx: &mut Context<Self>) -> impl IntoElement {
        let label = |k: &str| {
            div()
                .w(px(190.))
                .flex_none()
                .text_color(t.text_muted)
                .child(k.to_string())
        };
        let row = || div().flex().flex_row().h(px(ROW_H + 4.)).items_center();
        let sid = s.id.clone();
        let (id_name, id_host, id_port, id_prefix) =
            (sid.clone(), sid.clone(), sid.clone(), sid.clone());
        let (id_en, id_hint, id_strict) = (sid.clone(), sid.clone(), sid.clone());
        let prefix = if s.prefix.is_empty() {
            "(none)".to_string()
        } else {
            s.prefix.clone()
        };
        div()
            .flex()
            .flex_col()
            .child(section_heading(t, "General"))
            .child(row().child(label("Name")).child(field_button_with(
                "server-name",
                s.name.clone(),
                "rename",
                cx.listener(move |this, _, w, cx| this.rename_server_row(&id_name, w, cx)),
            )))
            .child(row().child(label("Host")).child(field_button_with(
                "server-host",
                s.host.clone(),
                "edit_server_host",
                cx.listener(move |this, _, w, cx| this.change_host(&id_host, w, cx)),
            )))
            .child(row().child(label("Port")).child(field_button_with(
                "server-port",
                s.port.to_string(),
                "edit_server_port",
                cx.listener(move |this, _, w, cx| this.change_port(&id_port, w, cx)),
            )))
            .child(row().child(label("Prefix")).child(field_button_with(
                "server-prefix",
                prefix,
                "edit_server_prefix",
                cx.listener(move |this, _, w, cx| this.change_prefix(&id_prefix, w, cx)),
            )))
            .child(
                row()
                    .child(label("Enabled"))
                    .child(toggle_button(
                        t,
                        "server-enabled",
                        s.enabled,
                        "toggle_server_enabled",
                        cx.listener(move |this, _, _, cx| this.toggle_enabled(&id_en, cx)),
                    ))
                    .child(
                        div()
                            .ml(px(8.))
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(if s.enabled {
                                "Started by Run all"
                            } else {
                                "Skipped by Run all; start it explicitly after enabling"
                            }),
                    ),
            )
            .child(
                row()
                    .child(label("Suggest closest routes on 404"))
                    .child(toggle_button(
                        t,
                        "server-unmatched-hint",
                        s.unmatched_hint,
                        "toggle_unmatched_hint",
                        cx.listener(move |this, _, _, cx| this.toggle_unmatched_hint(&id_hint, cx)),
                    )),
            )
            .child(
                row()
                    .child(label("Strict trailing slash"))
                    .child(toggle_button(
                        t,
                        "server-strict-slash",
                        s.strict_slash,
                        "toggle_strict_slash",
                        cx.listener(move |this, _, _, cx| this.toggle_strict_slash(&id_strict, cx)),
                    ))
                    .child(
                        div()
                            .ml(px(8.))
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child("/users and /users/ are different paths"),
                    ),
            )
            .child(
                row().child(label("Routes")).child(
                    div()
                        .font_family(BUFFER_FONT)
                        .text_size(px(12.))
                        .child(s.all_routes().count().to_string()),
                ),
            )
            .children(s.error.clone().map(|e| {
                crate::dev_id!(
                    div()
                        .id("server-error")
                        .mt(px(6.))
                        .text_size(px(12.))
                        .text_color(t.deleted)
                        .child(format!("Last start failed: {e}")),
                    "server-error",
                    Text,
                    format!("Last start failed: {e}"),
                    None
                )
            }))
    }

    // ---- Environment tab -----------------------------------------------------------------

    fn render_environment_tab(
        &self,
        t: &Theme,
        e: &Environment,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let reveal = self.reveal_secrets;
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
                    .id("env-rows")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .p(px(12.))
                    .child(
                        self.render_var_section(
                            t,
                            Scope::Environment,
                            Some(e.name.clone()),
                            e.vars
                                .iter()
                                .map(|v| VarRow {
                                    key: v.key.clone(),
                                    raw: v.raw.clone(),
                                    shown: v.value.clone(),
                                    note: v.source.label().to_string(),
                                    // Dotenv-only keys (raw == value) are read-only.
                                    editable: !(v.source == crate::view::VarSource::Dotenv
                                        && v.raw == v.value),
                                })
                                .collect(),
                            "env",
                            cx,
                        ),
                    ),
            )
    }

    // ---- Right dock ----------------------------------------------------------------------

    /// Keeps the two read-only body viewers of the Traffic detail in step with the selection.
    pub(crate) fn ensure_traffic_editors(&mut self, cx: &mut Context<Self>) {
        use crate::code_editor::{CodeEditor, Language};
        let rec = self
            .traffic_selected
            .and_then(|i| self.traffic.get(i))
            .and_then(|r| r.rec.clone());
        let Some(rec) = rec else {
            return;
        };
        let st = self.editor_settings.clone();
        let eds = self
            .traffic_editors
            .get_or_insert_with(|| {
                [0, 1].map(|_| {
                    cx.new(|cx| {
                        CodeEditor::new(Language::Plain, cx)
                            .read_only(true)
                            .line_numbers(false)
                            .placeholder("(empty body)")
                    })
                })
            })
            .clone();
        let ct = |h: &[(String, String)]| {
            h.iter()
                .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                .map(|(_, v)| v.clone())
                .unwrap_or_default()
        };
        let items = [
            (&rec.request_body, ct(&rec.request_headers)),
            (&rec.response_body, ct(&rec.response_headers)),
        ];
        for (ed, (body, ct)) in eds.iter().zip(items) {
            ed.update(cx, |e, cx| {
                if e.text() != body.as_str() {
                    e.set_text(body, cx);
                }
                e.set_language(Language::detect(&ct, body), cx);
                e.apply_settings(&st, cx);
            });
        }
    }

    /// Request, response and timing of a recorded request (SPEC §9.3 detail split).
    fn render_inspector_detail(
        &self,
        t: &Theme,
        row: &crate::traffic::TrafficRow,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(rec) = row.rec.as_deref() else {
            return div().into_any_element();
        };
        let has_callbacks = self.traffic.iter().any(|r| r.parent == Some(rec.id));
        let section = |title: &str| {
            div()
                .pt(px(8.))
                .pb(px(2.))
                .text_size(px(11.))
                .text_color(t.text_muted)
                .child(title.to_string())
        };
        let line = |k: String, v: String| {
            div()
                .flex()
                .flex_row()
                .gap(px(6.))
                .py(px(1.))
                .font_family(BUFFER_FONT)
                .text_size(px(11.))
                .child(div().flex_none().text_color(t.text_muted).child(k))
                .child(div().flex_1().min_w_0().child(v))
        };
        let body = |i: usize| {
            let ed = self.traffic_editors.as_ref().map(|e| e[i].clone());
            div()
                .h(px(160.))
                .flex_none()
                .border_1()
                .border_color(t.border_variant)
                .rounded(px(3.))
                .children(ed)
        };
        let mut col = div()
            .id("inspector-detail")
            .size_full()
            .p(px(10.))
            .flex()
            .flex_col()
            .overflow_y_scroll()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_color(t.method_color(&rec.method))
                            .child(rec.method.clone()),
                    )
                    .child(div().flex_1().truncate().child(rec.url.clone()))
                    .child(
                        div()
                            .text_color(t.status_color(rec.status))
                            .child(rec.status.to_string()),
                    ),
            )
            .child(line("Server".into(), rec.server.clone()))
            .child(line("Route".into(), rec.route.clone()))
            .child(line("Source".into(), rec.source.clone()));
        if !rec.route_pattern.is_empty() {
            col = col
                .child(
                    div()
                        .id("traffic-matched-route")
                        .child(section("MATCHED ROUTE")),
                )
                .child(line("Pattern".into(), rec.route_pattern.clone()));
            for (k, v) in &rec.params {
                col = col.child(line(format!("{k} ="), v.clone()));
            }
        }
        if !rec.dataset_changes.is_empty() {
            col = col.child(
                div()
                    .id("traffic-dataset-changes")
                    .child(section("DATASET CHANGES")),
            );
            for c in &rec.dataset_changes {
                let keys = if c.keys.is_empty() {
                    String::new()
                } else {
                    format!(" [{}]", c.keys.join(", "))
                };
                col = col.child(line(
                    c.op.clone(),
                    format!("{}: {} row(s){keys}", c.dataset, c.rows),
                ));
            }
        }
        if let Some(tr) = &rec.trace {
            col = col
                .child(
                    div()
                        .id("traffic-trace")
                        .child(section("RESPONSE SELECTION")),
                )
                .child(line(
                    "Mode".into(),
                    format!("{} (call {})", tr.mode, tr.call),
                ))
                .child(line(
                    "Chosen".into(),
                    if rec.response_name.is_empty() {
                        rec.response_id.clone()
                    } else {
                        format!("{} ({})", rec.response_name, rec.response_id)
                    },
                ))
                .child(line("Because".into(), tr.reason.clone()));
            for (n, resp) in tr.evaluated.iter().enumerate() {
                let mark = |ok: bool| if ok { "✓" } else { "✗" };
                let colour = |ok: bool| if ok { t.created } else { t.text_muted };
                let head = format!(
                    "{} {}{}",
                    mark(resp.passed),
                    if resp.name.is_empty() {
                        &resp.id
                    } else {
                        &resp.name
                    },
                    if resp.note.is_empty() {
                        format!("  [{}]", resp.logic)
                    } else {
                        format!("  ({})", resp.note)
                    }
                );
                col = col.child(
                    div()
                        .id(crate::ui::eid(format!("trace-resp-{n}")))
                        .pt(px(3.))
                        .font_family(BUFFER_FONT)
                        .text_size(px(11.))
                        .text_color(colour(resp.passed))
                        .child(head),
                );
                for (k, rule) in resp.rules.iter().enumerate() {
                    let subject = if rule.key.is_empty() {
                        rule.target.clone()
                    } else {
                        format!("{} {}", rule.target, rule.key)
                    };
                    let actual = rule
                        .actual
                        .clone()
                        .map_or("(none)".to_string(), |a| format!("{a:?}"));
                    col = col.child(
                        div()
                            .id(crate::ui::eid(format!("trace-rule-{n}-{k}")))
                            .pl(px(14.))
                            .font_family(BUFFER_FONT)
                            .text_size(px(11.))
                            .text_color(colour(rule.passed))
                            .child(format!(
                                "{} {subject} {}{} {} — got {actual}",
                                mark(rule.passed),
                                if rule.negate { "not " } else { "" },
                                rule.op,
                                rule.expected,
                            )),
                    );
                }
            }
        }
        let mut col = col
            .child(section("TIMING"))
            .child(line("Match".into(), format!("{} µs", rec.timing.match_us)))
            .child(line(
                "Render".into(),
                format!("{} µs", rec.timing.render_us),
            ))
            .child(line(
                "Delay".into(),
                format!(
                    "{} ms ({} network, {} response)",
                    rec.timing.delay_ms,
                    rec.timing.network_ms,
                    rec.timing.delay_ms.saturating_sub(rec.timing.network_ms)
                ),
            ))
            .child(line(
                "Transfer".into(),
                format!("{} ms", rec.timing.transfer_ms),
            ))
            .child(line("Total".into(), format!("{} ms", rec.timing.total_ms)))
            .children(rec.parent_id.map(|p| {
                let target = self.traffic.iter().position(|r| r.id == p);
                crate::dev_id!(
                    div()
                        .id("inspector-parent")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .pt(px(2.))
                        .text_size(px(11.))
                        .text_color(t.accent)
                        .when(target.is_some(), |d| {
                            d.cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.traffic_selected = target;
                                    cx.notify();
                                }))
                        })
                        .child(icon("arrow-up", 11., t.accent))
                        .child(format!(
                            "Triggered by request {p}{}",
                            rec.callback_id
                                .as_deref()
                                .map(|c| format!(" (callback {c})"))
                                .unwrap_or_default()
                        )),
                    "inspector-parent",
                    Button,
                    "Triggered by",
                    Press(format!("inspector-parent:{p}"))
                )
            }))
            .children(
                rec.error
                    .as_ref()
                    .filter(|_| rec.source == "callback")
                    .map(|e| {
                        crate::dev_id!(
                            div()
                                .id("inspector-callback-error")
                                .pt(px(2.))
                                .text_size(px(11.))
                                .text_color(t.deleted)
                                .child(format!("Callback failed: {e}")),
                            "inspector-callback-error",
                            Text,
                            format!("Callback failed: {e}"),
                            None
                        )
                    }),
            )
            .children(rec.fault.as_ref().map(|f| {
                crate::dev_id!(
                    div()
                        .id("inspector-fault")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .pt(px(2.))
                        .text_size(px(11.))
                        .text_color(t.deleted)
                        .child(icon("zap", 11., t.deleted))
                        .child(format!("Injected fault: {f}")),
                    "inspector-fault",
                    Text,
                    format!("Injected fault: {f}"),
                    None
                )
            }))
            .child(section("REQUEST"));
        for (k, v) in &rec.request_headers {
            col = col.child(line(format!("{k}:"), v.clone()));
        }
        col = col.child(body(0)).child(section("RESPONSE"));
        for (k, v) in &rec.response_headers {
            col = col.child(line(format!("{k}:"), v.clone()));
        }
        col = col.child(body(1));
        if rec.truncated {
            col = col.child(
                div()
                    .text_color(t.text_muted)
                    .child("Body truncated at 1 MiB."),
            );
        }
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(10.))
                    .py(px(4.))
                    .child(crate::dev_id!(
                        toolbar_button(t, "copy-curl")
                            .child(icon("copy", 12., t.icon_muted))
                            .child("Copy as cURL")
                            .on_click(cx.listener(|this, _, w, cx| this.run_action_id(
                                "copy_as_curl",
                                w,
                                cx
                            ))),
                        "copy-curl",
                        Button,
                        "Copy as cURL",
                        Action("copy_as_curl")
                    ))
                    .when(rec.source == "callback" || has_callbacks, |d| {
                        d.child(crate::dev_id!(
                            toolbar_button(t, "refire-callback")
                                .child(if rec.source == "callback" {
                                    "Re-fire callback"
                                } else {
                                    "Re-fire callbacks"
                                })
                                .on_click(cx.listener(|this, _, w, cx| this.run_action_id(
                                    "refire_callback",
                                    w,
                                    cx
                                ))),
                            "refire-callback",
                            Button,
                            "Re-fire callback",
                            Action("refire_callback")
                        ))
                    })
                    .when(rec.route == "proxy" || rec.route == "unmatched", |d| {
                        d.child(crate::dev_id!(
                            toolbar_button(t, "mock-this-route")
                                .child("Mock this route")
                                .on_click(cx.listener(|this, _, w, cx| this.run_action_id(
                                    "mock_this_route",
                                    w,
                                    cx
                                ))),
                            "mock-this-route",
                            Button,
                            "Mock this route",
                            Action("mock_this_route")
                        ))
                    }),
            )
            .child(crate::dev_id!(
                col,
                "inspector-detail",
                Text,
                "Request detail",
                None
            ))
            .into_any_element()
    }

    pub(crate) fn render_right_dock(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let body: AnyElement = match self.traffic_selected.and_then(|i| self.traffic.get(i)) {
            Some(r) if r.rec.is_some() => self.render_inspector_detail(t, r, cx),
            Some(r) => {
                let kv = |k: &str, v: String| {
                    div()
                        .flex()
                        .flex_row()
                        .py(px(2.))
                        .child(
                            div()
                                .w(px(70.))
                                .text_color(t.text_muted)
                                .child(k.to_string()),
                        )
                        .child(
                            div()
                                .flex_1()
                                .truncate()
                                .font_family(BUFFER_FONT)
                                .text_size(px(12.))
                                .child(v),
                        )
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
                .child("Select a request in Traffic to inspect it.")
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
            .children(self.render_scope_section(t))
            .children(self.render_seed_section(t, cx))
    }

    /// Inspector Seed section for the selected Server (SPEC §8.3): `fake_seed` input and the
    /// Deterministic / Random toggle.
    fn render_seed_section(&self, t: &Theme, cx: &mut Context<Self>) -> Option<AnyElement> {
        let s = &self.ws.servers[self.current_server()?];
        let (sid_d, sid_r) = (s.id.clone(), s.id.clone());
        let seed = s.fake_seed;
        let seg = |id: &'static str, label: &'static str, on: bool| {
            div()
                .id(id)
                .h(px(22.))
                .px(px(10.))
                .flex()
                .items_center()
                .rounded(px(4.))
                .cursor_pointer()
                .text_size(px(12.))
                .when(on, |d| d.bg(t.element_selected).text_color(t.text))
                .when(!on, |d| {
                    d.text_color(t.text_muted).hover(|s| s.bg(t.element_hover))
                })
                .child(label)
        };
        Some(
            div()
                .p(px(10.))
                .flex()
                .flex_col()
                .gap(px(6.))
                .border_t_1()
                .border_color(t.border_variant)
                .child(section_heading(t, "Seed"))
                .child(crate::dev_id!(
                    div()
                        .id("seed-mode")
                        .flex()
                        .flex_row()
                        .gap(px(2.))
                        .child(crate::dev_id!(
                            seg("seed-mode-deterministic", "Deterministic", seed.is_some())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_seed_mode(&sid_d, true, cx)
                                })),
                            "seed-mode-deterministic",
                            Button,
                            "Deterministic",
                            Press(format!("seed-mode:{}:1", s.id))
                        ))
                        .child(crate::dev_id!(
                            seg("seed-mode-random", "Random", seed.is_none()).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    this.set_seed_mode(&sid_r, false, cx)
                                })
                            ),
                            "seed-mode-random",
                            Button,
                            "Random",
                            Press(format!("seed-mode:{}:0", s.id))
                        )),
                    "seed-mode",
                    Panel,
                    "Seed mode",
                    None
                ))
                .child(self.inline_field(
                    t,
                    "seed-value".into(),
                    crate::response_pane::InlineTarget::Seed(s.id.clone()),
                    &seed.map(|n| n.to_string()).unwrap_or_default(),
                    "fake_seed (random)",
                    160.,
                    true,
                    cx,
                ))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.text_muted)
                        .child("Same seed, same fake data on every request."),
                )
                .into_any_element(),
        )
    }
}

/// One row of a Variables table.
pub struct VarRow {
    pub key: String,
    /// Text as written in the file (what editing starts from).
    pub raw: String,
    /// Effective text shown (before masking).
    pub shown: String,
    /// Source label shown in the last column (`file`, `dotenv`, `process env`).
    pub note: String,
    pub editable: bool,
}

impl Shell {
    /// Variables written in a file at `scope`, as table rows (`owner` is a Server or Route ID).
    fn file_vars(&self, scope: Scope, owner: &str) -> Vec<VarRow> {
        let Some(store) = &self.store else {
            return vec![];
        };
        let snap = store.snapshot();
        let map = match scope {
            Scope::Server => snap
                .servers
                .iter()
                .map(|(_, s)| s)
                .find(|s| s.id == owner)
                .map(|s| s.variables.clone()),
            Scope::Route => snap
                .servers
                .iter()
                .flat_map(|(_, s)| s.routes.iter())
                .find(|r| r.id == owner)
                .map(|r| r.variables.clone()),
            _ => None,
        };
        map.unwrap_or_default()
            .iter()
            .map(|(k, v)| {
                let raw = fauxfield_model::vars::value_text(v);
                VarRow {
                    key: k.clone(),
                    shown: raw.clone(),
                    raw,
                    note: "file".into(),
                    editable: true,
                }
            })
            .collect()
    }

    /// Editable Variables table with an "Add variable" button; secret-looking keys are masked
    /// unless "Show secrets" is on. Edits save through the store (comments are kept).
    fn render_var_section(
        &self,
        t: &Theme,
        scope: Scope,
        owner: Option<String>,
        rows: Vec<VarRow>,
        id: &'static str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let reveal = self.reveal_secrets;
        let add_owner = owner.clone();
        div()
            .id(crate::ui::eid(format!("vars-{id}")))
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .child(div().flex_1().child(section_heading(t, "Variables")))
                    .child(crate::dev_id!(
                        toolbar_button(t, crate::ui::eid(format!("var-add-{id}")))
                            .child("+ Add variable")
                            .on_click(cx.listener(move |this, _, w, cx| {
                                this.begin_prompt(
                                    crate::shell::PromptAction::SetVariable {
                                        scope,
                                        owner: add_owner.clone(),
                                        key: None,
                                    },
                                    "",
                                    w,
                                    cx,
                                )
                            })),
                        format!("var-add-{id}"),
                        Button,
                        "Add variable",
                        Press(format!(
                            "var-add:{}|{}",
                            scope.as_str(),
                            owner.clone().unwrap_or_default()
                        ))
                    )),
            )
            .child(
                div()
                    .h(px(ROW_H))
                    .flex()
                    .items_center()
                    .text_size(px(11.))
                    .text_color(t.text_muted)
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(div().w(px(200.)).flex_none().child("KEY"))
                    .child(div().w(px(320.)).flex_none().child("VALUE"))
                    .child(div().w(px(120.)).flex_none().child("SOURCE")),
            )
            .when(rows.is_empty(), |d| {
                d.child(
                    div()
                        .py(px(8.))
                        .text_size(px(12.))
                        .text_color(t.text_muted)
                        .child("No variables."),
                )
            })
            .children(rows.into_iter().map(|r| {
                let shown = if is_secret_key(&r.key) && !reveal {
                    masked(&r.shown)
                } else {
                    r.shown.clone()
                };
                let (k_edit, o_edit, raw) = (r.key.clone(), owner.clone(), r.raw.clone());
                let (k_del, o_del) = (r.key.clone(), owner.clone());
                let row_id = format!("var-{id}-{}", r.key);
                crate::dev_id!(
                    div()
                        .id(crate::ui::eid(row_id.clone()))
                        .h(px(ROW_H + 2.))
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .w(px(200.))
                                .flex_none()
                                .truncate()
                                .child(r.key.clone())
                        )
                        .child(
                            div()
                                .id(crate::ui::eid(format!("{row_id}-value")))
                                .w(px(320.))
                                .flex_none()
                                .truncate()
                                .font_family(BUFFER_FONT)
                                .text_size(px(12.))
                                .when(r.editable, |d| {
                                    d.cursor_pointer().on_click(cx.listener(
                                        move |this, _, w, cx| {
                                            this.begin_prompt(
                                                crate::shell::PromptAction::SetVariable {
                                                    scope,
                                                    owner: o_edit.clone(),
                                                    key: Some(k_edit.clone()),
                                                },
                                                &raw,
                                                w,
                                                cx,
                                            )
                                        },
                                    ))
                                })
                                .child(shown),
                        )
                        .child(
                            div()
                                .w(px(100.))
                                .flex_none()
                                .text_size(px(12.))
                                .text_color(t.text_muted)
                                .child(r.note.clone()),
                        )
                        .when(r.editable, |d| {
                            d.child(
                                div()
                                    .id(crate::ui::eid(format!("{row_id}-del")))
                                    .size(px(20.))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .rounded(px(3.))
                                    .cursor_pointer()
                                    .hover(|s| s.bg(t.element_hover))
                                    .tooltip(crate::ui::tooltip("Delete variable".to_string()))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        let (k, o) = (k_del.clone(), o_del.clone());
                                        this.store_op(cx, |s| {
                                            s.set_variable(scope, o.as_deref(), &k, None)
                                        });
                                    }))
                                    .child(icon("x", 12., t.icon_muted)),
                            )
                        }),
                    row_id,
                    Row,
                    r.key,
                    None
                )
            }))
            .into_any_element()
    }

    /// Variables in scope for the current Server and Route, with scope and source (M7).
    fn scope_vars(&self) -> Vec<fauxfield_model::vars::ResolvedVar> {
        let Some(si) = self.current_server() else {
            return vec![];
        };
        let sid = self.ws.servers[si].id.clone();
        self.scope_vars_in(&sid, self.current_route_id())
    }

    /// Effective variables of a Server (no Route scope), for previews in its tab.
    pub(crate) fn scope_vars_of(&self, sid: &str) -> Vec<fauxfield_model::vars::ResolvedVar> {
        self.scope_vars_in(sid, None)
    }

    fn scope_vars_in(
        &self,
        sid: &str,
        rid: Option<String>,
    ) -> Vec<fauxfield_model::vars::ResolvedVar> {
        let sid = sid.to_string();
        if let Some(h) = &self.host
            && let Ok(v) = h.engine().vars_in_scope(&sid, rid.as_deref())
        {
            return v;
        }
        self.store
            .as_ref()
            .map(|st| {
                st.snapshot().vars_in_scope(
                    Some(&self.active_env),
                    Some(&sid),
                    rid.as_deref(),
                    None,
                )
            })
            .unwrap_or_default()
    }

    /// Inspector "Variables in scope": effective value, scope and source of each variable.
    fn render_scope_section(&self, t: &Theme) -> Option<AnyElement> {
        self.current_server()?;
        let vars = self.scope_vars();
        let reveal = self.reveal_secrets;
        Some(
            div()
                .id("inspector-vars")
                .p(px(10.))
                .flex()
                .flex_col()
                .border_t_1()
                .border_color(t.border_variant)
                .child(section_heading(t, "Variables in scope"))
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(t.text_muted)
                        .pb(px(4.))
                        .child(format!(
                            "Environment: {}",
                            if self.active_env.is_empty() {
                                "none"
                            } else {
                                &self.active_env
                            }
                        )),
                )
                .when(vars.is_empty(), |d| {
                    d.child(
                        div()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child("No variables."),
                    )
                })
                .children(vars.into_iter().map(|v| {
                    let shown = if is_secret_key(&v.key) && !reveal {
                        masked(&v.value)
                    } else {
                        v.value.clone()
                    };
                    let source = match v.source {
                        fauxfield_model::vars::Source::File => String::new(),
                        fauxfield_model::vars::Source::Dotenv => " · dotenv".into(),
                        fauxfield_model::vars::Source::Process => " · process env".into(),
                    };
                    let row_id = format!("scope-var-{}", v.key);
                    crate::dev_id!(
                        div()
                            .id(crate::ui::eid(row_id.clone()))
                            .py(px(2.))
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .child(div().flex_1().truncate().child(v.key.clone()))
                                    .child(
                                        div()
                                            .text_size(px(11.))
                                            .text_color(t.text_muted)
                                            .child(format!("{}{source}", v.scope.as_str())),
                                    ),
                            )
                            .child(
                                div()
                                    .truncate()
                                    .font_family(BUFFER_FONT)
                                    .text_size(px(12.))
                                    .text_color(t.text_muted)
                                    .child(shown),
                            ),
                        row_id,
                        Row,
                        format!("{} = {} ({})", v.key, v.value, v.scope.as_str()),
                        None
                    )
                }))
                .into_any_element(),
        )
    }
}

fn section_heading(t: &Theme, name: &str) -> impl IntoElement {
    div()
        .mt(px(4.))
        .pb(px(4.))
        .text_size(px(11.))
        .text_color(t.text_muted)
        .child(name.to_uppercase())
}

/// A field button whose click is a custom closure; the dev id names the action it mirrors.
fn field_button_with(
    id: &'static str,
    label: String,
    action: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let _ = action;
    crate::dev_id!(
        div()
            .id(id)
            .h(px(22.))
            .px(px(4.))
            .flex()
            .items_center()
            .font_family(BUFFER_FONT)
            .text_size(px(12.))
            .cursor_pointer()
            .on_click(on_click)
            .child(label.clone()),
        id,
        Button,
        label,
        Action(action)
    )
}

/// A small on/off switch.
fn toggle_button(
    t: &Theme,
    id: &'static str,
    on: bool,
    action: &'static str,
    on_click: impl Fn(&gpui::ClickEvent, &mut gpui::Window, &mut gpui::App) + 'static,
) -> impl IntoElement {
    let _ = action;
    crate::dev_id!(
        div()
            .id(id)
            // MOCKUP .toggle: 26x14 track, 10 pt knob inset 2. The element is 26x20 so the hit
            // area stays >= 20 pt tall (UX103); the track is drawn centred inside it.
            .w(px(26.))
            .h(px(20.))
            .flex_none()
            .flex()
            .items_center()
            .cursor_pointer()
            .on_click(on_click)
            .child(
                div()
                    .w(px(26.))
                    .h(px(14.))
                    .rounded_full()
                    .bg(if on {
                        t.border_focused
                    } else {
                        t.element_selected
                    })
                    .flex()
                    .items_center()
                    .px(px(2.))
                    .when(on, |d| d.justify_end())
                    .child(div().size(px(10.)).rounded_full().bg(if on {
                        t.text
                    } else {
                        t.text_muted
                    }),),
            ),
        id,
        Button,
        if on { "On" } else { "Off" },
        Action(action)
    )
}

impl Shell {
    /// Shown when an external edit collides with unsaved GUI edits (SPEC §4.7).
    fn render_merge_banner(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let files = self.conflicts.join(", ");
        crate::dev_id!(
            div()
                .id("merge-banner")
                .h(px(32.))
                .flex_none()
                .px(px(12.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(10.))
                .bg(t.element_selected)
                .border_b_1()
                .border_color(t.border)
                .text_size(px(12.))
                .child(icon("triangle-alert", 13., t.modified))
                .child(
                    div()
                        .flex_1()
                        .truncate()
                        .child(format!("{files} changed on disk while you were editing."))
                )
                .child(crate::dev_id!(
                    toolbar_button(t, "merge-keep-mine")
                        .child("Keep mine")
                        .on_click(cx.listener(|this, _, w, cx| this.run_action_id(
                            "merge_keep_mine",
                            w,
                            cx
                        ))),
                    "merge-keep-mine",
                    Button,
                    "Keep mine",
                    Action("merge_keep_mine")
                ))
                .child(crate::dev_id!(
                    toolbar_button(t, "merge-load-disk")
                        .child("Load from disk")
                        .on_click(cx.listener(|this, _, w, cx| this.run_action_id(
                            "merge_load_disk",
                            w,
                            cx
                        ))),
                    "merge-load-disk",
                    Button,
                    "Load from disk",
                    Action("merge_load_disk")
                )),
            "merge-banner",
            Modal,
            "Merge banner",
            None
        )
    }

    /// The Welcome view: shown when no workspace is open.
    pub(crate) fn render_welcome(&self, t: &Theme, cx: &mut Context<Self>) -> impl IntoElement {
        let recents = self.recent_entries();
        let n = recents.len();
        let (accent, on_accent, hover, border) =
            (t.accent, t.background, t.element_hover, t.border);
        let primary = crate::dev_id!(
            div()
                .id("welcome-open")
                .h(px(32.))
                .px(px(16.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(6.))
                .bg(accent)
                .text_color(on_accent)
                .text_size(px(13.))
                .cursor_pointer()
                .hover(|s| s.opacity(0.9))
                .on_click(cx.listener(|this, _, w, cx| this.run_action_id("open_folder", w, cx)))
                .child(icon("folder-open", 14., on_accent))
                .child("Open folder…"),
            "welcome-open-folder",
            Button,
            "Open folder",
            Action("open_folder")
        );
        let secondary = crate::dev_id!(
            div()
                .id("welcome-create")
                .h(px(32.))
                .px(px(16.))
                .flex()
                .items_center()
                .gap(px(8.))
                .rounded(px(6.))
                .border_1()
                .border_color(border)
                .text_size(px(13.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(|this, _, w, cx| this.run_action_id(
                    "create_workspace",
                    w,
                    cx
                )))
                .child(icon("plus", 14., t.icon_muted))
                .child("Create workspace…"),
            "welcome-create-workspace",
            Button,
            "Create workspace",
            Action("create_workspace")
        );
        let rows = recents.iter().enumerate().map(|(i, r)| {
            let (open_path, rm_path) = (r.path.clone(), r.path.clone());
            let (exists, path_s) = (r.exists, r.path.display().to_string());
            let muted = t.text_muted;
            let color = if r.exists { t.text } else { t.text_muted };
            div()
                .flex()
                .flex_row()
                .items_center()
                .when(i + 1 < n, |d| d.border_b_1().border_color(t.border_variant))
                .child(crate::dev_id_meta!(
                    div()
                        .id(crate::ui::eid(format!("recent-{i}")))
                        .flex_1()
                        .min_w_0()
                        .px(px(12.))
                        .py(px(8.))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.))
                        .text_color(color)
                        .when(r.exists, |d| d.cursor_pointer().hover(move |s| s.bg(hover)))
                        .on_click(
                            cx.listener(move |this, _, _, cx| this.open_recent(&open_path, cx))
                        )
                        .tooltip(crate::ui::tooltip(path_s.clone()))
                        .child(icon("folder", 16., t.icon_muted))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .flex()
                                .flex_col()
                                .child(div().truncate().text_size(px(13.)).child(r.name.clone()))
                                .child(
                                    div()
                                        .truncate()
                                        .text_size(px(11.))
                                        .text_color(muted)
                                        .child(path_s.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_none()
                                .text_size(px(11.))
                                .text_color(muted)
                                .child(if exists {
                                    format!("Opened {}", r.opened)
                                } else {
                                    "Not found".to_string()
                                },)
                        ),
                    crate::dev_mcp::tree::Meta {
                        tooltip: true,
                        ..Default::default()
                    },
                    format!("recent-{i}"),
                    Row,
                    &r.name,
                    Recent(r.path.to_string_lossy().into_owned())
                ))
                .child(div().px(px(8.)).child(crate::dev_id_meta!(
                        div()
                            .id(crate::ui::eid(format!("recent-remove-{i}")))
                            .size(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(4.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover))
                            .tooltip(crate::ui::tooltip("Remove from recent".to_string()))
                            .on_click(
                                cx.listener(move |this, _, _, cx| this.remove_recent(&rm_path, cx))
                            )
                            .child(icon("x", 14., t.icon_muted)),
                        crate::dev_mcp::tree::Meta {
                            tooltip: true,
                            ..Default::default()
                        },
                        format!("recent-remove-{i}"),
                        Button,
                        "Remove from recent",
                        RemoveRecent(r.path.to_string_lossy().into_owned())
                    )))
        });
        crate::dev_id!(
            div()
                .id("welcome")
                .flex_1()
                .min_h_0()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .bg(t.editor)
                .child(
                    div()
                        .w(px(520.))
                        .flex()
                        .flex_col()
                        .gap(px(28.))
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(10.))
                                .child(
                                    div()
                                        .size(px(56.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .rounded(px(14.))
                                        .border_1()
                                        .border_color(t.border)
                                        .bg(t.panel)
                                        .child(icon("server", 28., t.accent)),
                                )
                                .child(div().text_size(px(22.)).child("Fauxfield"))
                                .child(
                                    div()
                                        .text_size(px(13.))
                                        .text_color(t.text_muted)
                                        .child("Local-first mock APIs, driven by plain files."),
                                )
                                .child(
                                    div()
                                        .mt(px(8.))
                                        .flex()
                                        .flex_row()
                                        .gap(px(10.))
                                        .child(primary)
                                        .child(secondary),
                                ),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(11.))
                                        .text_color(t.text_muted)
                                        .child("RECENT WORKSPACES"),
                                )
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .rounded(px(8.))
                                        .border_1()
                                        .border_color(t.border)
                                        .bg(t.panel)
                                        .overflow_hidden()
                                        .children(rows)
                                        .when(recents.is_empty(), |d| {
                                            d.child(
                                                div()
                                                    .py(px(20.))
                                                    .flex()
                                                    .justify_center()
                                                    .text_size(px(12.))
                                                    .text_color(t.text_placeholder)
                                                    .child("No recent workspaces yet."),
                                            )
                                        }),
                                ),
                        ),
                ),
            "welcome",
            Panel,
            "Welcome",
            None
        )
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
