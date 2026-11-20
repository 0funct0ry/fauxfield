//! Shell side of the application menu (M3.2): building the [`MenuContext`], syncing the native
//! menu bar, and the handlers of the new window, help, edit and about actions.

use std::collections::HashMap;
use std::path::Path;
use std::sync::OnceLock;

use gpui::{App, ClipboardItem, Context, Menu, MenuItem, OsAction, SystemMenuType, Window};

use crate::actions;
use crate::keymap::{Platform, display_keys};
use crate::menu::{self, MenuContext, MenuDef, MenuEntry, MenuStyle, Recent, SelectionKind, links};
use crate::shell::Shell;

/// Opens URLs. A trait so tests record instead of launching a browser.
pub trait UrlOpener {
    fn open(&self, url: &str, cx: &mut App);
}

pub struct SystemOpener;

impl UrlOpener for SystemOpener {
    fn open(&self, url: &str, cx: &mut App) {
        cx.open_url(url);
    }
}

/// Applies a built menu model to the OS. A trait so tests can count calls.
pub trait MenuSink {
    fn apply(&mut self, defs: &[MenuDef], cx: &mut App);
}

pub struct NativeSink;

impl MenuSink for NativeSink {
    fn apply(&mut self, defs: &[MenuDef], cx: &mut App) {
        cx.set_menus(to_gpui(defs));
    }
}

fn os_action(id: &str) -> Option<OsAction> {
    Some(match id {
        "undo" => OsAction::Undo,
        "redo" => OsAction::Redo,
        "edit_cut" => OsAction::Cut,
        "edit_copy" => OsAction::Copy,
        "edit_paste" => OsAction::Paste,
        "edit_select_all" => OsAction::SelectAll,
        _ => return None,
    })
}

fn to_items(entries: &[MenuEntry]) -> Vec<MenuItem> {
    let mut out = Vec::new();
    for e in entries {
        match e {
            MenuEntry::Separator => out.push(MenuItem::Separator),
            MenuEntry::Services => {
                out.push(MenuItem::os_submenu("Services", SystemMenuType::Services))
            }
            MenuEntry::Action { id, label, checked } => {
                if let Some(info) = actions::find(id) {
                    // gpui 0.2.2 native items have no check state: prefix a check mark.
                    let name = if *checked {
                        format!("✓ {label}")
                    } else {
                        label.clone()
                    };
                    out.push(MenuItem::Action {
                        name: name.into(),
                        action: (info.build)(),
                        os_action: os_action(id),
                    });
                }
            }
            MenuEntry::Recent { path, label, .. } => out.push(MenuItem::action(
                label.clone(),
                actions::OpenRecent { path: path.clone() },
            )),
            // A native item with no action is shown greyed.
            MenuEntry::Disabled { label } => out.push(MenuItem::Action {
                name: label.clone().into(),
                action: Box::new(gpui::NoAction),
                os_action: None,
            }),
            MenuEntry::Submenu { label, entries, .. } => out.push(MenuItem::Submenu(Menu {
                name: label.clone().into(),
                items: to_items(entries),
            })),
        }
    }
    out
}

pub fn to_gpui(defs: &[MenuDef]) -> Vec<Menu> {
    defs.iter()
        .map(|d| Menu {
            name: d.title.into(),
            items: to_items(&d.entries),
        })
        .collect()
}

/// Maps an action type name (`QuickSearch`) to its `ACTIONS` id, built once.
fn type_ids() -> &'static HashMap<String, &'static str> {
    static MAP: OnceLock<HashMap<String, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| {
        actions::ACTIONS
            .iter()
            .map(|a| {
                let full = (a.build)().name().to_string();
                let short = full.rsplit("::").next().unwrap_or(&full).to_string();
                (short, a.id)
            })
            .collect()
    })
}

impl Shell {
    /// Plain-data snapshot of everything the menu depends on.
    pub(crate) fn menu_context(&self, window: &Window) -> MenuContext {
        let mut c = MenuContext::welcome(Platform::current());
        c.workspace_open = self.store.is_some();
        c.modal_open = self.any_modal_open();
        c.text_input_focused = self.picker.is_some()
            || self.server_dialog.is_some()
            || self.schema_dialog.is_some()
            || self.crud_dialog.is_some()
            || self.column_dialog.is_some()
            || self.callback_dialog.is_some()
            || self.export_dialog.is_some()
            || self.seed_dialog.is_some()
            || self.body_edit.is_some()
            || self.inline.is_some();
        let sidx = self.current_server();
        c.current_server = sidx.map(|i| self.ws.servers[i].id.clone());
        c.current_server_enabled = sidx.is_some_and(|i| self.ws.servers[i].enabled);
        c.current_server_running = sidx.is_some_and(|i| self.ws.servers[i].running);
        c.current_route = self.current_route_id();
        c.current_response = self.current_response_id();
        c.selection_kind = match self.selected.as_deref() {
            None => SelectionKind::None,
            Some(id) if self.ws.find_route(id).is_some() => SelectionKind::Route,
            Some(id) if self.ws.servers.iter().any(|s| s.id == id) => SelectionKind::Server,
            Some(_) => SelectionKind::Folder,
        };
        c.servers_total = self.ws.servers.len();
        c.servers_enabled = self.ws.enabled_count();
        c.servers_running = self.ws.running_count();
        if let (Some(store), Some(file)) = (&self.store, self.undo_target()) {
            c.can_undo = store.can_undo(&file);
            c.can_redo = store.can_redo(&file);
        }
        c.active_tab_is_preview = self.tabs.active().is_some_and(|t| t.preview);
        c.tab_titles = self.tabs.iter().map(|t| t.title.clone()).collect();
        c.left_dock = self.layout.left_open;
        c.sidebar_tab = self.sidebar_tab.id();
        c.bottom_dock = self.layout.bottom_open;
        c.right_dock = self.layout.right_open;
        c.traffic_rows = self.traffic.len();
        c.traffic_row_selected = self.traffic_selected.is_some();
        c.dataset_row_target = self.dataset_row_target_exists();
        c.recents = self
            .recents
            .iter()
            .take(10)
            .map(|p| Recent {
                path: p.display().to_string(),
                name: p
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string()),
                exists: p.join("fauxfield.toml").is_file(),
            })
            .collect();
        c.fullscreen = window.is_fullscreen();
        c.maximized = window.is_maximized();
        let mac = c.platform == Platform::Macos;
        c.bindings = self
            .keymap
            .bindings
            .iter()
            .map(|b| (b.action.clone(), display_keys(&b.keys, mac)))
            .collect();
        c
    }

    /// Any modal, prompt, picker or confirmation is open (the menu greys almost everything).
    pub(crate) fn any_modal_open(&self) -> bool {
        self.picker.is_some()
            || self.server_dialog.is_some()
            || self.schema_dialog.is_some()
            || self.crud_dialog.is_some()
            || self.column_dialog.is_some()
            || self.callback_dialog.is_some()
            || self.export_dialog.is_some()
            || self.seed_dialog.is_some()
            || self.any_dialog_open()
            || self.quit_confirm
            || self.about_open
            || self.agents_dialog.is_some()
            || self.any_combo_open()
            || self.any_dropdown_open()
    }

    /// Whether the handler of the action type `ty` (e.g. "QuickSearch") is currently enabled.
    /// Actions outside the menu are always enabled. A disabled handler is not registered, so
    /// a native menu item greys and its key does nothing (SPEC §20.14).
    pub(crate) fn gated(&self, ty: &str) -> bool {
        let Some(id) = type_ids().get(ty) else {
            return true;
        };
        if menu::NOT_IN_MENU.iter().any(|(n, _)| n == id) {
            return true;
        }
        match &self.menu_ctx {
            Some(ctx) => menu::enabled(id, ctx),
            None => true,
        }
    }

    /// Whether action `id` is enabled under the current state (not the last painted frame).
    #[cfg(feature = "dev-mcp")]
    pub(crate) fn enabled_now(&self, id: &str, window: &Window) -> bool {
        if menu::NOT_IN_MENU.iter().any(|(n, _)| *n == id) {
            return true;
        }
        menu::enabled(id, &self.menu_context(window))
    }

    /// Dispatches `id` once frames have re-registered the handlers (dev-mcp tool calls).
    #[cfg(feature = "dev-mcp")]
    pub(crate) fn dispatch_next_frames(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.run_action_id_next_frame(id, window, cx);
    }

    /// The only place that builds menus. Rebuilds the model, and only when it differs from the
    /// last one hands it to the OS (native) or re-renders (in-window).
    pub fn sync_menus(&mut self, window: &Window, cx: &mut Context<Self>) {
        let ctx = self.menu_context(window);
        let defs = menu::build_menus(&ctx);
        self.menu_ctx = Some(ctx);
        if self.menu_style == MenuStyle::Native && self.menu_last.as_ref() != Some(&defs) {
            self.menu_sink.apply(&defs, cx);
            self.menu_last = Some(defs);
        }
    }

    pub fn set_menu_sink(&mut self, sink: Box<dyn MenuSink>) {
        self.menu_sink = sink;
        self.menu_last = None;
    }

    pub fn set_url_opener(&mut self, o: Box<dyn UrlOpener>) {
        self.url_opener = o;
    }

    pub fn open_link(&mut self, url: &str, cx: &mut Context<Self>) {
        self.url_opener.open(url, cx);
    }

    fn undo_target(&self) -> Option<String> {
        self.lc.undo_file.clone().or_else(|| self.file_for_edit())
    }

    // ---- edit_* on the focused single-line input (the picker query) -------------------------

    /// The query input of the open picker, else the New Server dialog's focused input.
    fn focused_input(&self) -> Option<gpui::Entity<crate::text_input::TextInput>> {
        if let Some(d) = &self.server_dialog {
            return d.inputs.get(d.focus).cloned();
        }
        if let Some(d) = &self.schema_dialog {
            return d.focused.clone();
        }
        if let Some(d) = &self.crud_dialog {
            return Some(d.path.clone());
        }
        if let Some(d) = &self.column_dialog {
            return d.focused.clone();
        }
        if let Some(d) = &self.callback_dialog {
            return d.focused.clone();
        }
        if let Some(d) = &self.export_dialog {
            return Some(d.path.clone());
        }
        if let Some(d) = &self.seed_dialog {
            return Some(d.rows.clone());
        }
        if let Some(p) = &self.picker {
            return Some(p.input.clone());
        }
        self.inline.as_ref().map(|i| i.input.clone())
    }

    pub(crate) fn edit_copy(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.focused_input() {
            i.update(cx, |t, cx| t.copy(cx));
        }
    }

    pub(crate) fn edit_cut(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.focused_input() {
            i.update(cx, |t, cx| t.cut(cx));
        }
    }

    pub(crate) fn edit_paste(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.focused_input() {
            i.update(cx, |t, cx| t.paste(cx));
        }
    }

    pub(crate) fn edit_select_all(&mut self, cx: &mut Context<Self>) {
        if let Some(i) = self.focused_input() {
            i.update(cx, |t, cx| t.select_all(cx));
        }
    }

    // ---- window, help, workspace -----------------------------------------------------------

    pub(crate) fn reveal_workspace(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self.store.as_ref().map(|s| s.snapshot().root.clone()) else {
            return;
        };
        cx.reveal_path(Path::new(&root));
    }

    pub(crate) fn open_documentation(&mut self, cx: &mut Context<Self>) {
        self.open_link(&links::documentation(), cx);
    }

    pub(crate) fn open_release_notes(&mut self, cx: &mut Context<Self>) {
        self.open_link(&links::release_notes(), cx);
    }

    pub(crate) fn report_issue(&mut self, cx: &mut Context<Self>) {
        self.open_link(&links::report_issue(), cx);
    }

    /// Build metadata for the About box, the clipboard and the window.
    pub fn build_info() -> fauxfield_control::build_info::BuildInfo {
        let features = if cfg!(feature = "dev-mcp") {
            vec!["dev-mcp"]
        } else {
            vec![]
        };
        fauxfield_control::build_info::BuildInfo::current(features)
    }

    pub fn copy_version_info(&mut self, cx: &mut Context<Self>) {
        cx.write_to_clipboard(ClipboardItem::new_string(Self::build_info().copy_text()));
        self.notify("Version info copied".into(), cx);
    }

    pub fn show_about(&mut self, cx: &mut Context<Self>) {
        // A second About just refocuses the open one.
        self.about_open = true;
        cx.notify();
    }

    pub fn close_about(&mut self, cx: &mut Context<Self>) {
        self.about_open = false;
        cx.notify();
    }
}

impl Shell {
    pub(crate) fn render_about(
        &self,
        t: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        use gpui::{InteractiveElement, IntoElement, ParentElement, Styled, div, prelude::*, px};
        let b = Self::build_info();
        let version = if b.is_debug() {
            format!("Version {} · Development build", b.version)
        } else {
            format!("Version {}", b.version)
        };
        let link = |id: &'static str, label: &'static str, url: String| {
            crate::ui::toolbar_button(t, crate::ui::eid(id))
                .h(px(24.))
                .px(px(8.))
                .text_color(t.accent)
                .child(label)
                .on_click(cx.listener(move |this, _, _, cx| this.open_link(&url, cx)))
        };
        let button = |id: &'static str, label: &'static str| {
            crate::ui::toolbar_button(t, crate::ui::eid(id))
                .h(px(26.))
                .px(px(12.))
                .child(label)
        };
        let muted = |s: String| div().text_color(t.text_muted).text_size(px(12.)).child(s);
        let mut card = div()
            .id("about-card")
            .w(px(380.))
            .p(px(20.))
            .flex()
            .flex_col()
            .items_center()
            .gap(px(8.))
            .rounded(px(8.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            // A click inside the card must not reach the backdrop that closes the box.
            .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(crate::ui::app_mark(t, 56.))
            .child(div().text_size(px(20.)).child("Fauxfield"))
            .child(muted("Local-first mock API tool".into()))
            .child(div().child(version))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .child(muted(format!(
                        "Commit {} · {}",
                        b.commit_label(),
                        b.commit_date
                    )))
                    .child(muted(format!("{} · gpui {}", b.target, b.gpui))),
            );
        if !b.features.is_empty() {
            card = card.child(muted("Agent testing (dev-mcp) enabled".into()));
        }
        card = card
            .child(div().child("Released under the MIT License."))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(4.))
                    .child(crate::dev_id!(
                        link(
                            "about-repository",
                            "Repository",
                            links::REPOSITORY.to_string()
                        ),
                        "about-repository",
                        Button,
                        "Repository",
                        AboutLink(links::REPOSITORY.to_string())
                    ))
                    .child(crate::dev_id!(
                        link("about-license", "License", links::license()),
                        "about-license",
                        Button,
                        "License",
                        AboutLink(links::license())
                    ))
                    .child(crate::dev_id!(
                        link(
                            "about-documentation",
                            "Documentation",
                            links::documentation()
                        ),
                        "about-documentation",
                        Button,
                        "Documentation",
                        AboutLink(links::documentation())
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .gap(px(8.))
                    .mt(px(8.))
                    .child(crate::dev_id!(
                        button("about-copy", "Copy Version Info")
                            .on_click(cx.listener(|this, _, _, cx| this.copy_version_info(cx))),
                        "about-copy",
                        Button,
                        "Copy Version Info",
                        AboutCopy
                    ))
                    .child(crate::dev_id!(
                        button("about-close", "Close")
                            .bg(t.accent_bg())
                            .on_click(cx.listener(|this, _, _, cx| this.close_about(cx))),
                        "about-close",
                        Button,
                        "Close",
                        AboutClose
                    )),
            );
        crate::dev_id_z!(
            div()
                .id("about")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.background.opacity(0.6))
                .on_mouse_down(
                    gpui::MouseButton::Left,
                    cx.listener(|this, _, _, cx| this.close_about(cx)),
                )
                .child(card),
            10,
            "about",
            Modal,
            "About Fauxfield",
            None
        )
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_type_maps_to_an_id() {
        let m = type_ids();
        assert_eq!(m.len(), actions::ACTIONS.len());
        assert_eq!(m.get("QuickSearch"), Some(&"quick_search"));
        assert_eq!(m.get("EditCut"), Some(&"edit_cut"));
    }

    #[test]
    fn model_converts_to_native_menus() {
        let ctx = MenuContext::welcome(Platform::Macos);
        let defs = menu::build_menus(&ctx);
        let native = to_gpui(&defs);
        assert_eq!(native.len(), defs.len());
        assert_eq!(native[0].name.as_ref(), "Fauxfield");
    }
}
