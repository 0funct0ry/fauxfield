//! The in-window menu bar (M3.2 Part D): drawn on Windows and Linux, and on macOS when the
//! dev override selects it. It renders the same [`MenuDef`] model as the native menu.

use gpui::{
    Context, Corner, InteractiveElement, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, Window, anchored, deferred, div, point, prelude::*, px,
};

use crate::menu::{self, MenuContext, MenuDef, MenuEntry, MenuStyle};
use crate::shell::Shell;
use crate::theme::Theme;
use crate::ui::{TITLE_BAR_H, icon};

/// Windows narrower than this collapse the titles into one hamburger button.
pub const COLLAPSE_W: f32 = 1000.;
const ROW_H: f32 = 24.;
const SUBMENU_DELAY_MS: u64 = 150;

/// Open state of the bar. `hi[level]` is the highlighted entry of each open level.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Bar {
    /// Open top-level menu (index into the in-window titles; 0 for the hamburger).
    pub open: Option<usize>,
    pub hi: [Option<usize>; 2],
    /// Entry of level 0 whose submenu is open (level 1).
    pub sub: Option<usize>,
    pub win_w: f32,
}

/// Next enabled, non-separator entry from `from` in direction `dir` (wrapping).
pub fn step(
    entries: &[MenuEntry],
    ctx: &MenuContext,
    from: Option<usize>,
    dir: i32,
) -> Option<usize> {
    let n = entries.len() as i32;
    if n == 0 {
        return None;
    }
    let mut i = match from {
        Some(i) => i as i32,
        None if dir > 0 => -1,
        None => n,
    };
    for _ in 0..n {
        i = (i + dir).rem_euclid(n);
        if menu::entry_enabled(&entries[i as usize], ctx) {
            return Some(i as usize);
        }
    }
    None
}

fn label_of(e: &MenuEntry) -> &str {
    match e {
        MenuEntry::Action { label, .. }
        | MenuEntry::Recent { label, .. }
        | MenuEntry::Disabled { label }
        | MenuEntry::Submenu { label, .. } => label,
        _ => "",
    }
}

/// First enabled entry after `from` whose label starts with `ch` (case-insensitive).
pub fn type_ahead(
    entries: &[MenuEntry],
    ctx: &MenuContext,
    from: Option<usize>,
    ch: char,
) -> Option<usize> {
    let n = entries.len();
    let start = from.map(|i| i + 1).unwrap_or(0);
    (0..n).map(|k| (start + k) % n).find(|&i| {
        menu::entry_enabled(&entries[i], ctx)
            && label_of(&entries[i])
                .chars()
                .next()
                .is_some_and(|c| c.to_lowercase().eq(ch.to_lowercase()))
    })
}

#[cfg(feature = "dev-mcp")]
macro_rules! tag_item {
    ($el:expr, $z:expr, $id:expr, $role:ident, $label:expr, $enabled:expr, $click:expr) => {
        crate::dev_mcp::tree::tag(
            $el,
            crate::dev_mcp::tree::Spec {
                id: ($id).to_string(),
                role: crate::dev_mcp::tree::Role::$role,
                label: ($label).to_string(),
                enabled: $enabled,
                z: $z,
                click: $click,
                meta: crate::dev_mcp::tree::Meta {
                    truncates: true,
                    ..Default::default()
                },
            },
        )
    };
}

#[cfg(not(feature = "dev-mcp"))]
macro_rules! tag_item {
    ($el:expr, $($rest:tt)*) => {
        $el
    };
}

impl Shell {
    pub(crate) fn bar_collapsed(&self) -> bool {
        self.bar.win_w > 0. && self.bar.win_w < COLLAPSE_W
    }

    pub(crate) fn bar_visible(&self) -> bool {
        self.menu_style == MenuStyle::InWindow
    }

    fn bar_ctx(&self) -> Option<&MenuContext> {
        self.menu_ctx.as_ref()
    }

    /// The in-window titles: every menu except the macOS app menu.
    pub(crate) fn bar_menus(&self) -> Vec<MenuDef> {
        match self.bar_ctx() {
            Some(c) => menu::build_menus(c)
                .into_iter()
                .filter(|m| m.title != "Fauxfield")
                .collect(),
            None => Vec::new(),
        }
    }

    /// Entries of the popup of top-level menu `top` (the hamburger lists the menus).
    pub(crate) fn bar_entries(&self, top: usize) -> Vec<MenuEntry> {
        let menus = self.bar_menus();
        if self.bar_collapsed() {
            menus
                .into_iter()
                .map(|m| MenuEntry::Submenu {
                    key: m.title.to_lowercase().leak(),
                    label: m.title.to_string(),
                    enabled: true,
                    entries: m.entries,
                })
                .collect()
        } else {
            menus
                .into_iter()
                .nth(top)
                .map(|m| m.entries)
                .unwrap_or_default()
        }
    }

    pub fn bar_open_menu(&mut self, top: usize, cx: &mut Context<Self>) {
        self.bar.open = Some(top);
        self.bar.hi = [None, None];
        self.bar.sub = None;
        cx.notify();
    }

    pub fn bar_close(&mut self, cx: &mut Context<Self>) {
        if self.bar.open.take().is_some() {
            self.bar.hi = [None, None];
            self.bar.sub = None;
            cx.notify();
        }
    }

    pub fn bar_toggle(&mut self, top: usize, cx: &mut Context<Self>) {
        if self.bar.open == Some(top) {
            self.bar_close(cx);
        } else {
            self.bar_open_menu(top, cx);
        }
    }

    pub fn bar_open_sub(&mut self, i: usize, cx: &mut Context<Self>) {
        let Some(top) = self.bar.open else { return };
        let entries = self.bar_entries(top);
        let Some(ctx) = self.bar_ctx().cloned() else {
            return;
        };
        if let Some(MenuEntry::Submenu {
            entries: sub,
            enabled: true,
            ..
        }) = entries.get(i)
        {
            self.bar.hi[0] = Some(i);
            self.bar.sub = Some(i);
            self.bar.hi[1] = step(sub, &ctx, None, 1);
            cx.notify();
        }
    }

    fn bar_hover(&mut self, level: usize, i: usize, cx: &mut Context<Self>) {
        let Some(top) = self.bar.open else { return };
        let entries = self.bar_entries(top);
        let list: Vec<MenuEntry> = if level == 0 {
            entries.clone()
        } else {
            match self.bar.sub.and_then(|s| entries.get(s)) {
                Some(MenuEntry::Submenu { entries, .. }) => entries.clone(),
                _ => return,
            }
        };
        let Some(ctx) = self.bar_ctx() else { return };
        if !list.get(i).is_some_and(|e| menu::entry_enabled(e, ctx)) {
            return;
        }
        self.bar.hi[level] = Some(i);
        if level == 0 {
            if matches!(list[i], MenuEntry::Submenu { .. }) {
                cx.spawn(async move |this, cx| {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(SUBMENU_DELAY_MS))
                        .await;
                    this.update(cx, |this, cx| {
                        if this.bar.hi[0] == Some(i) && this.bar.open.is_some() {
                            this.bar_open_sub(i, cx);
                        }
                    })
                    .ok();
                })
                .detach();
            } else {
                self.bar.sub = None;
            }
        }
        cx.notify();
    }

    /// Runs `id` as the menu does: refuses a disabled item, closes the bar, dispatches.
    pub fn run_menu_action(
        &mut self,
        id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        if let Some(ctx) = &self.menu_ctx
            && !menu::enabled(id, ctx)
        {
            let label = menu::build_menus(ctx)
                .iter()
                .find_map(|m| find_label(&m.entries, id))
                .unwrap_or_else(|| id.to_string());
            return Err(format!(
                "menu item disabled: {label} ({})",
                menu::why_disabled(id, ctx)
            ));
        }
        if let Some(ctx) = &self.menu_ctx
            && let Some(path) = menu::find_path(&menu::build_menus(ctx), id)
        {
            self.record_menu_invoke(path);
        }
        self.bar_close(cx);
        self.run_action_id(id, window, cx);
        Ok(())
    }

    pub(crate) fn record_menu_invoke(&mut self, path: Vec<String>) {
        let n = self.menu_invoked.as_ref().map_or(0, |(n, _)| *n) + 1;
        self.menu_invoked = Some((n, path));
    }

    /// Resolves `["File", "Open Recent", "<label>"]` in the live model and runs it.
    pub fn run_menu_path(
        &mut self,
        path: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let Some(ctx) = self.menu_ctx.clone() else {
            return Err("menus are not built yet".into());
        };
        let defs = menu::build_menus(&ctx);
        let joined = path.join("/");
        let missing = || format!("no menu item {joined}");
        let (first, rest) = path.split_first().ok_or_else(missing)?;
        let mut entries = &defs
            .iter()
            .find(|m| m.title == first)
            .ok_or_else(missing)?
            .entries;
        let mut hit: Option<&MenuEntry> = None;
        for (k, seg) in rest.iter().enumerate() {
            let e = entries
                .iter()
                // A Recent label is "<name> — <path>"; the bare workspace name also matches.
                .find(|e| label_of(e) == seg || label_of(e).starts_with(&format!("{seg} — ")))
                .ok_or_else(missing)?;
            hit = Some(e);
            if k + 1 < rest.len() {
                match e {
                    MenuEntry::Submenu { entries: sub, .. } => entries = sub,
                    _ => return Err(missing()),
                }
            }
        }
        match hit.ok_or_else(missing)? {
            MenuEntry::Action { id, .. } => self.run_menu_action(id, window, cx),
            MenuEntry::Recent {
                path: p, enabled, ..
            } => {
                if !enabled || ctx.modal_open {
                    return Err(format!("menu item disabled: {} (folder not found)", joined));
                }
                self.bar_close(cx);
                window
                    .dispatch_action(Box::new(crate::actions::OpenRecent { path: p.clone() }), cx);
                Ok(())
            }
            _ => Err(format!("menu item disabled: {joined} (not an action)")),
        }
    }

    /// Keyboard handling while a menu is open. Returns true when the key was consumed.
    pub(crate) fn bar_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(top) = self.bar.open else {
            return false;
        };
        let Some(ctx) = self.bar_ctx().cloned() else {
            return false;
        };
        let entries = self.bar_entries(top);
        let level = if self.bar.sub.is_some() { 1 } else { 0 };
        let list: Vec<MenuEntry> = if level == 1 {
            match self.bar.sub.and_then(|s| entries.get(s)) {
                Some(MenuEntry::Submenu { entries, .. }) => entries.clone(),
                _ => entries.clone(),
            }
        } else {
            entries.clone()
        };
        let titles = self.bar_menus().len();
        let hamburger = self.bar_collapsed();
        match ev.keystroke.key.as_str() {
            "escape" => {
                if level == 1 {
                    self.bar.sub = None;
                    cx.notify();
                } else {
                    self.bar_close(cx);
                    window.focus(&self.focus);
                }
            }
            "down" => {
                self.bar.hi[level] = step(&list, &ctx, self.bar.hi[level], 1);
                cx.notify();
            }
            "up" => {
                self.bar.hi[level] = step(&list, &ctx, self.bar.hi[level], -1);
                cx.notify();
            }
            "left" => {
                if level == 1 {
                    self.bar.sub = None;
                } else if !hamburger && titles > 0 {
                    self.bar_open_menu((top + titles - 1) % titles, cx);
                }
                cx.notify();
            }
            "right" => {
                let cur = self.bar.hi[level];
                if let (0, Some(i)) = (level, cur)
                    && matches!(list.get(i), Some(MenuEntry::Submenu { .. }))
                {
                    self.bar_open_sub(i, cx);
                } else if level == 0 && !hamburger && titles > 0 {
                    self.bar_open_menu((top + 1) % titles, cx);
                }
            }
            "enter" => {
                let Some(i) = self.bar.hi[level] else {
                    return true;
                };
                match list.get(i).cloned() {
                    Some(MenuEntry::Submenu { .. }) => self.bar_open_sub(i, cx),
                    Some(MenuEntry::Action { id, .. }) => {
                        let _ = self.run_menu_action(id, window, cx);
                    }
                    Some(MenuEntry::Recent {
                        path,
                        enabled: true,
                        ..
                    }) => {
                        self.bar_close(cx);
                        window.dispatch_action(Box::new(crate::actions::OpenRecent { path }), cx);
                    }
                    _ => {}
                }
            }
            _ => {
                let m = ev.keystroke.modifiers;
                if let Some(c) = ev
                    .keystroke
                    .key_char
                    .as_deref()
                    .and_then(|s| s.chars().next())
                    && !m.control
                    && !m.alt
                    && !m.platform
                    && let Some(i) = type_ahead(&list, &ctx, self.bar.hi[level], c)
                {
                    self.bar.hi[level] = Some(i);
                    cx.notify();
                }
            }
        }
        true
    }

    /// Alt+<mnemonic> opens a top-level menu. Returns true when consumed.
    pub(crate) fn bar_mnemonic(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let m = ev.keystroke.modifiers;
        if !self.bar_visible() || !m.alt || m.control || m.platform {
            return false;
        }
        let Some(c) = ev
            .keystroke
            .key
            .chars()
            .next()
            .filter(|_| ev.keystroke.key.len() == 1)
        else {
            return false;
        };
        if self.bar_collapsed() {
            return false;
        }
        let menus = self.bar_menus();
        match menus
            .iter()
            .position(|d| d.mnemonic.is_some_and(|x| x.eq_ignore_ascii_case(&c)))
        {
            Some(i) => {
                self.bar_open_menu(i, cx);
                true
            }
            None => false,
        }
    }

    // ---- rendering ----------------------------------------------------------------------

    /// Titles (or the hamburger), drawn at the left of the title bar.
    pub(crate) fn render_menubar(&self, t: &Theme, cx: &mut Context<Self>) -> gpui::AnyElement {
        let h = px(TITLE_BAR_H - 6.);
        let mut row = div()
            .id("menubar")
            .flex()
            .flex_row()
            .items_center()
            .gap(px(2.));
        if self.bar_collapsed() {
            let el = div()
                .relative()
                .child(
                    crate::ui::toolbar_button(t, "menubar-collapsed")
                        .child(icon("menu", 14., t.icon_muted))
                        .on_click(cx.listener(|this, _, _, cx| this.bar_toggle(0, cx))),
                )
                .when(self.bar.open == Some(0), |d| {
                    d.child(self.render_popup(t, 0, cx))
                });
            row = row.child(tag_item!(
                el,
                0,
                "menubar-collapsed",
                Button,
                "Menu",
                true,
                crate::dev_mcp::tree::Click::BarTitle(0)
            ));
        } else {
            for (i, d) in self.bar_menus().iter().enumerate() {
                let lower = d.title.to_lowercase();
                let id = format!("menubar-{lower}");
                let label = match d.mnemonic.and_then(|m| {
                    d.title
                        .char_indices()
                        .find(|(_, c)| c.eq_ignore_ascii_case(&m))
                }) {
                    Some((at, c)) => div()
                        .flex()
                        .flex_row()
                        .child(d.title[..at].to_string())
                        .child(div().underline().child(c.to_string()))
                        .child(d.title[at + c.len_utf8()..].to_string()),
                    None => div().child(d.title.to_string()),
                };
                let open = self.bar.open == Some(i);
                let el = div()
                    .relative()
                    .h(h)
                    .child(
                        crate::ui::toolbar_button(t, crate::ui::eid(id.clone()))
                            .when(open, |b| b.bg(t.element_hover))
                            .child(label)
                            .on_click(cx.listener(move |this, _, _, cx| this.bar_toggle(i, cx)))
                            .on_hover(cx.listener(move |this, hov: &bool, _, cx| {
                                if *hov && this.bar.open.is_some() && this.bar.open != Some(i) {
                                    this.bar_open_menu(i, cx);
                                }
                            })),
                    )
                    .when(open, |d| d.child(self.render_popup(t, i, cx)));
                row = row.child(tag_item!(
                    el,
                    0,
                    id,
                    Button,
                    d.title,
                    true,
                    crate::dev_mcp::tree::Click::BarTitle(i)
                ));
            }
        }
        row.child(div().w(px(1.)).h(px(14.)).mx(px(4.)).bg(t.border_variant))
            .into_any_element()
    }

    fn popup_width(entries: &[MenuEntry], ctx: &MenuContext) -> f32 {
        let widest =
            |f: &dyn Fn(&MenuEntry) -> usize| entries.iter().map(f).max().unwrap_or(0) as f32;
        let l = widest(&|e| label_of(e).chars().count());
        let b = widest(&|e| match e {
            MenuEntry::Action { id, .. } => ctx.binding(id).map_or(0, |k| k.chars().count()),
            _ => 0,
        });
        (l * 7.0 + b * 7.0 + 64.0).clamp(220., 420.)
    }

    fn render_popup(&self, t: &Theme, top: usize, cx: &mut Context<Self>) -> gpui::AnyElement {
        let entries = self.bar_entries(top);
        let popup = self.render_level(t, 0, &entries, cx);
        deferred(
            anchored()
                .anchor(Corner::TopLeft)
                .offset(point(px(0.), px(TITLE_BAR_H - 6.)))
                .snap_to_window_with_margin(px(4.))
                .child(popup),
        )
        .priority(2)
        .into_any_element()
    }

    fn render_level(
        &self,
        t: &Theme,
        level: usize,
        entries: &[MenuEntry],
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let Some(ctx) = self.menu_ctx.clone() else {
            return div().into_any_element();
        };
        let w = Self::popup_width(entries, &ctx);
        let mac = ctx.platform == crate::keymap::Platform::Macos;
        let _ = mac;
        let menu_key = self
            .bar_menus()
            .get(self.bar.open.unwrap_or(0))
            .map(|m| m.title.to_lowercase())
            .unwrap_or_default();
        let mut col = div()
            .id(crate::ui::eid(format!("menubar-popup-{level}")))
            .w(px(w))
            .py(px(4.))
            .flex()
            .flex_col()
            .rounded(px(6.))
            .border_1()
            .border_color(t.border)
            .bg(t.elevated)
            .occlude();
        let mut recent_n = 0usize;
        for (i, e) in entries.iter().enumerate() {
            if matches!(e, MenuEntry::Separator | MenuEntry::Services) {
                col = col.child(div().h(px(1.)).my(px(4.)).mx(px(8.)).bg(t.border_variant));
                continue;
            }
            let enabled = menu::entry_enabled(e, &ctx);
            let hi = self.bar.hi[level] == Some(i);
            let color = if enabled { t.text } else { t.text_placeholder };
            let binding = match e {
                MenuEntry::Action { id, .. } => ctx.binding(id).map(str::to_string),
                _ => None,
            };
            let is_sub = matches!(e, MenuEntry::Submenu { .. });
            let (row_id, role_click): (String, _) = match e {
                MenuEntry::Action { id, .. } => (format!("menu-{menu_key}-{id}"), Some(0)),
                MenuEntry::Recent { .. } => {
                    let r = format!("menu-file-recent-{recent_n}");
                    recent_n += 1;
                    (r, Some(1))
                }
                MenuEntry::Submenu { key, .. } => (format!("menu-{menu_key}-{key}"), Some(2)),
                _ => (format!("menu-{menu_key}-info-{i}"), None),
            };
            let _ = role_click;
            let mut row = div()
                .id(crate::ui::eid(row_id.clone()))
                .h(px(ROW_H))
                .mx(px(4.))
                .px(px(8.))
                .rounded(px(4.))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .text_size(px(12.))
                .text_color(color)
                .when(hi && enabled, |d| d.bg(t.element_hover))
                .when(enabled, |d| d.cursor_pointer())
                .child(div().flex_1().min_w_0().truncate().child(
                    if matches!(e, MenuEntry::Action { checked: true, .. }) {
                        format!("✓ {}", label_of(e))
                    } else {
                        label_of(e).to_string()
                    },
                ));
            if let Some(b) = &binding {
                row = row.child(div().text_color(t.text_muted).child(b.clone()));
            }
            if is_sub {
                row = row.child(icon("chevron-right", 12., t.icon_muted));
            }
            let lvl = level;
            row = row.on_hover(cx.listener(move |this, hov: &bool, _, cx| {
                if *hov {
                    this.bar_hover(lvl, i, cx);
                }
            }));
            if enabled {
                let ec = e.clone();
                row = row.on_click(cx.listener(move |this, _, w, cx| match &ec {
                    MenuEntry::Action { id, .. } => {
                        let _ = this.run_menu_action(id, w, cx);
                    }
                    MenuEntry::Recent { path, .. } => {
                        this.bar_close(cx);
                        w.dispatch_action(
                            Box::new(crate::actions::OpenRecent { path: path.clone() }),
                            cx,
                        );
                    }
                    MenuEntry::Submenu { .. } => this.bar_open_sub(i, cx),
                    _ => {}
                }));
            }
            let sub_open = level == 0 && self.bar.sub == Some(i);
            let mut cell = div().relative().child(row);
            if sub_open && let MenuEntry::Submenu { entries: sub, .. } = e {
                let child = self.render_level(t, 1, sub, cx);
                // No nested `deferred` (gpui forbids it); `anchored` flips to the left on its
                // own when the submenu would leave the window.
                cell = cell.child(
                    anchored()
                        .anchor(Corner::TopLeft)
                        .offset(point(px(w - 8.), px(-4.)))
                        .child(child),
                );
            }
            #[cfg(feature = "dev-mcp")]
            let label = label_of(e).to_string();
            #[cfg(feature = "dev-mcp")]
            let click = match e {
                MenuEntry::Action { id, .. } => Some(crate::dev_mcp::tree::Click::BarAction(id)),
                MenuEntry::Recent { path, .. } => {
                    Some(crate::dev_mcp::tree::Click::BarRecent(path.clone()))
                }
                MenuEntry::Submenu { .. } => Some(crate::dev_mcp::tree::Click::BarSubmenu(i)),
                _ => None,
            };
            #[cfg(feature = "dev-mcp")]
            let cell = match click {
                Some(c) => tag_item!(cell, 5 + level as u8, row_id, MenuItem, label, enabled, c)
                    .into_any_element(),
                None => cell.into_any_element(),
            };
            #[cfg(not(feature = "dev-mcp"))]
            let cell = cell.into_any_element();
            col = col.child(cell);
        }
        col.into_any_element()
    }

    /// Transparent layer under the popups that closes the menu on any outside click.
    pub(crate) fn render_bar_backdrop(&self, cx: &mut Context<Self>) -> gpui::AnyElement {
        div()
            .id("menubar-backdrop")
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .top(px(TITLE_BAR_H))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, _, cx| this.bar_close(cx)),
            )
            .into_any_element()
    }
}

fn find_label(entries: &[MenuEntry], id: &str) -> Option<String> {
    for e in entries {
        match e {
            MenuEntry::Action { id: i, label, .. } if *i == id => return Some(label.clone()),
            MenuEntry::Submenu { entries, .. } => {
                if let Some(l) = find_label(entries, id) {
                    return Some(l);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keymap::Platform;
    use crate::menu::build_menus;

    fn file_entries(ctx: &MenuContext) -> Vec<MenuEntry> {
        build_menus(ctx)
            .into_iter()
            .find(|m| m.title == "File")
            .unwrap()
            .entries
    }

    #[test]
    fn arrows_skip_separators_and_disabled() {
        let ctx = MenuContext::welcome(Platform::Linux);
        let e = file_entries(&ctx);
        // Welcome: Create Workspace (4), Open Folder (5), Open Recent (6, submenu), ...
        let first = step(&e, &ctx, None, 1).unwrap();
        assert_eq!(label_of(&e[first]), "Create Workspace…");
        let next = step(&e, &ctx, Some(first), 1).unwrap();
        assert_eq!(label_of(&e[next]), "Open Folder…");
        let up = step(&e, &ctx, Some(first), -1).unwrap();
        assert_eq!(label_of(&e[up]), "Quit", "wraps to the last enabled item");
    }

    #[test]
    fn type_ahead_picks_next_enabled_match() {
        let ctx = MenuContext::welcome(Platform::Linux);
        let e = file_entries(&ctx);
        let i = type_ahead(&e, &ctx, None, 'o').unwrap();
        assert_eq!(label_of(&e[i]), "Open Folder…");
        let j = type_ahead(&e, &ctx, Some(i), 'o').unwrap();
        assert_eq!(label_of(&e[j]), "Open Recent");
        assert!(
            type_ahead(&e, &ctx, None, 'n').is_none(),
            "New … items are disabled"
        );
    }
}
