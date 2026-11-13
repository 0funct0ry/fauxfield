//! The reusable DropdownMenu: a trigger button that opens an action menu, modelled on the
//! gpui-kit `DropdownMenu` / `PopupMenu` (crates/component/src/menu). It is not a ComboBox: it
//! holds no value, it runs actions.
//!
//! * entries: [`MenuItem::Item`] (label, optional icon, shortcut, checked, disabled, danger),
//!   [`MenuItem::Separator`], [`MenuItem::Label`] (a non-clickable heading),
//!   [`MenuItem::Link`] (opens a URL, external-link icon) and [`MenuItem::Submenu`];
//! * look: popup 4 px padding and 2 px between rows, width 128 to 500 px (intrinsic, at least
//!   `min_width`), rounded, bordered, shadowed; rows 26 px with 8 px side padding, a menu-wide
//!   icon column (reserved when any row has an icon or a check), the label in the UI font,
//!   the shortcut right-aligned and muted 12 px away; separators are 1 px lines that bleed
//!   into the padding with 2 px above and below; disabled rows are muted with no hover;
//!   hover and the keyboard highlight share one background;
//! * submenus open on hover or Right, to the right of the row overlapping it by 8 px and
//!   4 px higher, flipped by the window snap when there is no room; Left closes them;
//! * keyboard: the trigger is a tab stop; Enter, Space or Down opens the menu with the first
//!   row highlighted (Up: the last); Up and Down move over clickable rows and wrap, Enter
//!   runs the row (a submenu row opens it), Esc closes the whole menu and returns focus to
//!   the trigger; a press outside closes it;
//! * clicking an item emits [`DropdownEvent::Action`] with its id and closes the menu; a link
//!   opens its URL and closes; a submenu row does not close.

use std::cell::Cell;
use std::rc::Rc;

use gpui::{
    AnyElement, Bounds, Context, EventEmitter, FocusHandle, Focusable, Hsla, InteractiveElement,
    IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement, Pixels, Render,
    SharedString, StatefulInteractiveElement, Styled, Window, anchored, canvas, deferred, div,
    point, prelude::*, px,
};

use crate::theme::{Theme, theme};
use crate::ui::icon;

const ROW_H: f32 = 26.0;
const GAP: f32 = 4.0;

/// One entry of a DropdownMenu.
#[derive(Clone, Debug, PartialEq)]
pub enum MenuItem {
    Item {
        /// What [`DropdownEvent::Action`] reports.
        id: SharedString,
        label: SharedString,
        icon: Option<SharedString>,
        /// Display text, e.g. `⌘⇧E`.
        shortcut: Option<SharedString>,
        checked: bool,
        disabled: bool,
        /// Why the item is disabled, shown in place of the shortcut.
        reason: Option<SharedString>,
        /// Destructive: drawn in the delete colour.
        danger: bool,
    },
    Separator,
    Label(SharedString),
    Link {
        label: SharedString,
        url: SharedString,
    },
    Submenu {
        label: SharedString,
        icon: Option<SharedString>,
        items: Vec<MenuItem>,
    },
}

impl MenuItem {
    pub fn item(id: impl Into<SharedString>, label: impl Into<SharedString>) -> MenuItem {
        MenuItem::Item {
            id: id.into(),
            label: label.into(),
            icon: None,
            shortcut: None,
            checked: false,
            disabled: false,
            reason: None,
            danger: false,
        }
    }

    pub fn icon(mut self, name: impl Into<SharedString>) -> Self {
        match &mut self {
            MenuItem::Item { icon, .. } | MenuItem::Submenu { icon, .. } => {
                *icon = Some(name.into())
            }
            _ => {}
        }
        self
    }

    pub fn shortcut(mut self, keys: impl Into<SharedString>) -> Self {
        if let MenuItem::Item { shortcut, .. } = &mut self {
            *shortcut = Some(keys.into());
        }
        self
    }

    pub fn checked(mut self, on: bool) -> Self {
        if let MenuItem::Item { checked, .. } = &mut self {
            *checked = on;
        }
        self
    }

    /// Disable the item; `reason` is shown in place of the shortcut.
    pub fn disabled(mut self, why: Option<impl Into<SharedString>>) -> Self {
        if let MenuItem::Item {
            disabled, reason, ..
        } = &mut self
        {
            *reason = why.map(Into::into);
            *disabled = reason.is_some();
        }
        self
    }

    pub fn danger(mut self, on: bool) -> Self {
        if let MenuItem::Item { danger, .. } = &mut self {
            *danger = on;
        }
        self
    }

    pub fn link(label: impl Into<SharedString>, url: impl Into<SharedString>) -> MenuItem {
        MenuItem::Link {
            label: label.into(),
            url: url.into(),
        }
    }

    pub fn submenu(label: impl Into<SharedString>, items: Vec<MenuItem>) -> MenuItem {
        MenuItem::Submenu {
            label: label.into(),
            icon: None,
            items,
        }
    }

    /// Rows the keyboard can land on.
    pub fn clickable(&self) -> bool {
        match self {
            MenuItem::Item { disabled, .. } => !disabled,
            MenuItem::Link { .. } | MenuItem::Submenu { .. } => true,
            MenuItem::Separator | MenuItem::Label(_) => false,
        }
    }

    fn has_left_slot(&self) -> bool {
        match self {
            MenuItem::Item { icon, checked, .. } => icon.is_some() || *checked,
            MenuItem::Submenu { icon, .. } => icon.is_some(),
            _ => false,
        }
    }
}

/// Drop leading, repeated and trailing separators (as the gpui-kit builder does).
pub fn tidy(items: Vec<MenuItem>) -> Vec<MenuItem> {
    let mut out: Vec<MenuItem> = Vec::with_capacity(items.len());
    for it in items {
        if it == MenuItem::Separator && out.last().is_none_or(|l| *l == MenuItem::Separator) {
            continue;
        }
        out.push(it);
    }
    while out.last() == Some(&MenuItem::Separator) {
        out.pop();
    }
    out
}

/// The next clickable row after `from` (`step` 1) or before it (`-1`), wrapping; with no
/// current row, the first (or last) clickable one.
pub fn next_clickable(items: &[MenuItem], from: Option<usize>, step: isize) -> Option<usize> {
    let n = items.len() as isize;
    if n == 0 {
        return None;
    }
    let mut i = match from {
        Some(f) => f as isize,
        None if step > 0 => -1,
        None => n,
    };
    for _ in 0..n {
        i = (i + step).rem_euclid(n);
        if items[i as usize].clickable() {
            return Some(i as usize);
        }
    }
    None
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropdownEvent {
    /// An item was clicked or confirmed: its id.
    Action(SharedString),
    Open,
    Close,
}

pub struct DropdownMenu {
    id: SharedString,
    focus: FocusHandle,
    items: Vec<MenuItem>,
    open: bool,
    /// Highlighted row of the root menu.
    hl: Option<usize>,
    /// Open submenu (root row) and its highlighted row.
    sub: Option<(usize, Option<usize>)>,
    /// The keyboard drives the highlight (hover-out then keeps it).
    keyboard: bool,
    trigger_icon: SharedString,
    tooltip: Option<SharedString>,
    min_width: f32,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Bounds of each root row (to place a submenu).
    row_bounds: Rc<std::cell::RefCell<Vec<Option<Bounds<Pixels>>>>>,
}

impl EventEmitter<DropdownEvent> for DropdownMenu {}

impl Focusable for DropdownMenu {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl DropdownMenu {
    pub fn new(id: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        DropdownMenu {
            id: id.into(),
            focus: cx.focus_handle(),
            items: vec![],
            open: false,
            hl: None,
            sub: None,
            keyboard: false,
            trigger_icon: "menu".into(),
            tooltip: None,
            min_width: 128.0,
            bounds: Rc::new(Cell::new(None)),
            row_bounds: Default::default(),
        }
    }

    pub fn trigger_icon(mut self, name: impl Into<SharedString>) -> Self {
        self.trigger_icon = name.into();
        self
    }

    pub fn min_width(mut self, w: f32) -> Self {
        self.min_width = w.clamp(128.0, 500.0);
        self
    }

    /// Replace the entries (called on every render by the screen that draws the menu).
    pub fn sync(
        &mut self,
        items: Vec<MenuItem>,
        tooltip: Option<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let items = tidy(items);
        if items != self.items {
            self.items = items;
            if self.hl.is_some_and(|h| h >= self.items.len()) {
                self.hl = None;
            }
            cx.notify();
        }
        self.tooltip = tooltip;
    }

    pub fn items(&self) -> &[MenuItem] {
        &self.items
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn highlighted(&self) -> Option<usize> {
        self.hl
    }

    pub fn open(&mut self, highlight_last: bool, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            return;
        }
        self.open = true;
        self.sub = None;
        self.hl = next_clickable(&self.items, None, if highlight_last { -1 } else { 1 });
        self.focus.focus(window);
        cx.emit(DropdownEvent::Open);
        cx.notify();
    }

    pub fn close(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.sub = None;
        if refocus {
            self.focus.focus(window);
        }
        cx.emit(DropdownEvent::Close);
        cx.notify();
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(true, window, cx);
        } else {
            self.keyboard = false;
            self.open(false, window, cx);
        }
    }

    /// Run root row `ix` (or row `sub_ix` of its submenu): an item emits and closes, a link
    /// opens its URL and closes, a submenu row opens its submenu.
    pub fn activate(
        &mut self,
        ix: usize,
        sub_ix: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let item = match (self.items.get(ix), sub_ix) {
            (Some(MenuItem::Submenu { items, .. }), Some(s)) => items.get(s).cloned(),
            (it, None) => it.cloned(),
            _ => None,
        };
        match item {
            Some(MenuItem::Item {
                id,
                disabled: false,
                ..
            }) => {
                self.close(true, window, cx);
                cx.emit(DropdownEvent::Action(id));
            }
            Some(MenuItem::Link { url, .. }) => {
                self.close(true, window, cx);
                cx.open_url(&url);
            }
            Some(MenuItem::Submenu { items, .. }) if sub_ix.is_none() => {
                self.hl = Some(ix);
                self.sub = Some((ix, next_clickable(&items, None, 1)));
                cx.notify();
            }
            _ => {}
        }
    }

    fn sub_items(&self) -> Option<&[MenuItem]> {
        let (ix, _) = self.sub?;
        match self.items.get(ix) {
            Some(MenuItem::Submenu { items, .. }) => Some(items),
            _ => None,
        }
    }

    fn handle_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let k = &ev.keystroke;
        if k.modifiers.platform || k.modifiers.control || k.modifiers.alt {
            return false;
        }
        let key = k.key.as_str();
        if !self.open {
            match key {
                "enter" | "space" | "down" => {
                    self.keyboard = true;
                    self.open(false, window, cx)
                }
                "up" => {
                    self.keyboard = true;
                    self.open(true, window, cx)
                }
                _ => return false,
            }
            return true;
        }
        self.keyboard = true;
        let in_sub = self.sub.is_some_and(|(_, s)| s.is_some());
        match key {
            "down" | "up" => {
                let step = if key == "down" { 1 } else { -1 };
                if in_sub {
                    let items = self.sub_items().unwrap_or(&[]).to_vec();
                    if let Some((_, s)) = &mut self.sub {
                        *s = next_clickable(&items, *s, step);
                    }
                } else {
                    self.sub = None;
                    self.hl = next_clickable(&self.items, self.hl, step);
                }
            }
            "right" => {
                if let Some(h) = self.hl
                    && matches!(self.items.get(h), Some(MenuItem::Submenu { .. }))
                {
                    self.activate(h, None, window, cx);
                }
            }
            "left" => self.sub = None,
            "enter" | "space" => {
                if let Some((ix, Some(s))) = self.sub {
                    self.activate(ix, Some(s), window, cx);
                } else if let Some(h) = self.hl {
                    self.activate(h, None, window, cx);
                }
            }
            "escape" => self.close(true, window, cx),
            "tab" => self.close(false, window, cx),
            _ => return false,
        }
        cx.notify();
        true
    }

    fn mouse_out(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        // A press on the trigger is its own toggle.
        if self.bounds.get().is_some_and(|b| b.contains(&ev.position)) {
            return;
        }
        self.close(false, window, cx);
    }
}

// ---- render ------------------------------------------------------------------------------------

impl Render for DropdownMenu {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        if self.open && !self.focus.contains_focused(window, cx) {
            self.open = false;
            self.sub = None;
            cx.emit(DropdownEvent::Close);
        }
        let focused = self.focus.is_focused(window);
        let id = self.id.clone();
        let bounds = self.bounds.clone();
        let trigger = div()
            .id(id.clone())
            .relative()
            .h(px(22.))
            .px(px(6.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .rounded(px(4.))
            .border_1()
            .border_color(if self.open || focused {
                t.border_focused
            } else {
                t.border_variant
            })
            .when(self.open, |d| d.bg(t.element_selected))
            .hover(|d| d.bg(t.element_hover))
            .cursor_pointer()
            .track_focus(&self.focus)
            .key_context("DropdownMenu")
            .when_some(self.tooltip.clone(), |d, tip| {
                d.tooltip(crate::ui::tooltip(tip.to_string()))
            })
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if this.handle_key(ev, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.toggle(window, cx)),
            )
            .child(
                canvas(
                    move |b: Bounds<Pixels>, _, _| bounds.set(Some(b)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(icon(&self.trigger_icon, 14., t.text).into_any_element())
            .child(icon("chevron-down", 12., t.icon_muted));
        let popup = self.open.then(|| self.render_popup(&t, cx));
        #[cfg(feature = "dev-mcp")]
        let label = self
            .tooltip
            .clone()
            .unwrap_or_else(|| "Menu".into())
            .to_string();
        div()
            .id(SharedString::from(format!("{id}-dropdown")))
            .relative()
            .flex_none()
            .child(crate::dev_id_meta!(
                trigger,
                crate::dev_mcp::tree::Meta {
                    tooltip: true,
                    ..Default::default()
                },
                id.to_string(),
                Button,
                label,
                Press(format!("dropdown:{id}"))
            ))
            .children(popup)
    }
}

impl DropdownMenu {
    /// One row. `path` is `(root, None)` for a root row and `(root, Some(i))` for a submenu row.
    fn row(
        &self,
        t: &Theme,
        item: &MenuItem,
        path: (usize, Option<usize>),
        hl: bool,
        left_slot: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (ix, sub_ix) = path;
        let row_id = match sub_ix {
            Some(s) => format!("{}-item-{ix}-{s}", self.id),
            None => format!("{}-item-{ix}", self.id),
        };
        if *item == MenuItem::Separator {
            return div()
                .my(px(2.))
                .mx(px(-4.))
                .h(px(1.))
                .bg(t.border_variant)
                .into_any_element();
        }
        let clickable = item.clickable();
        let (label, icon_name, right, color, checked, chevron, link) = match item {
            MenuItem::Item {
                label,
                icon,
                shortcut,
                checked,
                disabled,
                reason,
                danger,
                ..
            } => (
                label.clone(),
                icon.clone(),
                if *disabled {
                    reason.clone()
                } else {
                    shortcut.clone()
                },
                if *disabled {
                    t.text_placeholder
                } else if *danger {
                    t.deleted
                } else {
                    t.text
                },
                *checked,
                false,
                false,
            ),
            MenuItem::Label(l) => (l.clone(), None, None, t.text_muted, false, false, false),
            MenuItem::Link { label, .. } => (label.clone(), None, None, t.text, false, false, true),
            MenuItem::Submenu { label, icon, .. } => (
                label.clone(),
                icon.clone(),
                None,
                t.text,
                false,
                true,
                false,
            ),
            MenuItem::Separator => unreachable!(),
        };
        let icon_color: Hsla = if color == t.text { t.icon_muted } else { color };
        let rb = self.row_bounds.clone();
        let el = div()
            .id(SharedString::from(row_id.clone()))
            .relative()
            .h(px(ROW_H))
            .px(px(8.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(GAP))
            .rounded(px(4.))
            .text_size(px(13.))
            .text_color(color)
            .when(hl && clickable, |d| d.bg(t.element_hover))
            .when(clickable, |d| d.cursor_pointer())
            .when(sub_ix.is_none(), |d| {
                d.child(
                    canvas(
                        move |b: Bounds<Pixels>, _, _| {
                            let mut v = rb.borrow_mut();
                            if v.len() <= ix {
                                v.resize(ix + 1, None);
                            }
                            v[ix] = Some(b);
                        },
                        |_, _, _, _| {},
                    )
                    .absolute()
                    .size_full(),
                )
            })
            .when(clickable, |d| {
                d.on_mouse_move(cx.listener(move |this, _, _, cx| {
                    let changed = match sub_ix {
                        None => {
                            let was = this.hl;
                            this.hl = Some(ix);
                            // Hovering a submenu row opens it; any other row closes it.
                            let is_sub =
                                matches!(this.items.get(ix), Some(MenuItem::Submenu { .. }));
                            let sub = if is_sub {
                                Some((
                                    ix,
                                    this.sub.and_then(|(r, s)| (r == ix).then_some(s)).flatten(),
                                ))
                            } else {
                                None
                            };
                            let c = was != this.hl || this.sub != sub;
                            this.sub = sub;
                            c
                        }
                        Some(s) => {
                            let c = this.sub != Some((ix, Some(s)));
                            this.sub = Some((ix, Some(s)));
                            c
                        }
                    };
                    this.keyboard = false;
                    if changed {
                        cx.notify();
                    }
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _, window, cx| {
                        this.activate(ix, sub_ix, window, cx);
                        cx.stop_propagation();
                    }),
                )
            })
            .when(left_slot, |d| {
                let slot = if checked && icon_name.is_none() {
                    Some(icon("check", 14., icon_color).into_any_element())
                } else {
                    icon_name.map(|n| icon(&n, 14., icon_color).into_any_element())
                };
                d.child(div().size(px(14.)).flex_none().children(slot))
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(div().min_w(px(0.)).truncate().child(label.clone()))
                    .children(right.map(|r| {
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .text_color(t.text_muted)
                            .child(r)
                    }))
                    .when(chevron, |d| {
                        d.child(icon("chevron-right", 12., t.icon_muted))
                    })
                    .when(link, |d| d.child(icon("external-link", 12., t.icon_muted))),
            );
        let press = match sub_ix {
            Some(s) => format!("dropdown-sub:{}:{}", self.id, ix * 1000 + s),
            None => format!("dropdown-item:{}:{ix}", self.id),
        };
        let _ = &press;
        crate::dev_id_z!(el, 2, row_id, MenuItem, label, Press(press)).into_any_element()
    }

    fn panel(
        &self,
        t: &Theme,
        id: String,
        rows: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> gpui::Stateful<gpui::Div> {
        div()
            .id(SharedString::from(id))
            .occlude()
            .min_w(px(self.min_width))
            .max_w(px(500.))
            .p(px(4.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .rounded(px(6.))
            .bg(t.elevated)
            .border_1()
            .border_color(t.border)
            .shadow_md()
            .text_color(t.text)
            .on_mouse_down_out(
                cx.listener(|this, ev: &MouseDownEvent, window, cx| this.mouse_out(ev, window, cx)),
            )
            .children(rows)
    }

    fn render_popup(&self, t: &Theme, cx: &mut Context<Self>) -> AnyElement {
        let at = self.bounds.get().map_or(point(px(0.), px(0.)), |b| {
            point(b.left(), b.bottom() + px(GAP))
        });
        let left_slot = self.items.iter().any(MenuItem::has_left_slot);
        let rows: Vec<AnyElement> = self
            .items
            .iter()
            .enumerate()
            .map(|(i, it)| {
                let hl = self.hl == Some(i);
                self.row(t, it, (i, None), hl, left_slot, cx)
            })
            .collect();
        let id = self.id.to_string();
        let root = crate::dev_id_z!(
            self.panel(t, format!("{id}-menu"), rows, cx),
            1,
            format!("{id}-menu"),
            Menu,
            "Menu",
            None
        );
        let submenu = self.sub.and_then(|(ix, s_hl)| {
            let items = self.sub_items()?.to_vec();
            let row = self.row_bounds.borrow().get(ix).copied().flatten()?;
            let at = point(row.right() - px(8.), row.top() - px(4.));
            let left = items.iter().any(MenuItem::has_left_slot);
            let rows: Vec<AnyElement> = items
                .iter()
                .enumerate()
                .map(|(j, it)| self.row(t, it, (ix, Some(j)), s_hl == Some(j), left, cx))
                .collect();
            Some(
                deferred(
                    anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(4.))
                        .child(crate::dev_id_z!(
                            self.panel(t, format!("{id}-submenu"), rows, cx),
                            2,
                            format!("{id}-submenu"),
                            Menu,
                            "Submenu",
                            None
                        )),
                )
                .with_priority(6)
                .into_any_element(),
            )
        });
        div()
            .child(
                deferred(
                    anchored()
                        .position(at)
                        .snap_to_window_with_margin(px(8.))
                        .child(root),
                )
                .with_priority(5),
            )
            .children(submenu)
            .into_any_element()
    }
}

// ---- Shell integration -------------------------------------------------------------------------

use crate::shell::Shell;

/// Handler that runs when a DropdownMenu of the Shell emits an event.
pub type DropdownHandler = Rc<dyn Fn(&mut Shell, &DropdownEvent, &mut Context<Shell>)>;

/// Everything a screen says about one DropdownMenu: a stable `key` (also its dev id), the
/// trigger icon, the entries, a tooltip, and what to do with its events.
pub(crate) struct DropdownSite {
    pub key: String,
    pub icon: &'static str,
    pub items: Vec<MenuItem>,
    pub tip: Option<String>,
    pub min_width: f32,
    pub on_event: DropdownHandler,
}

impl Shell {
    /// The DropdownMenu for `site.key`, created on first use and synced on every render.
    pub(crate) fn dropdown_field(&self, cx: &mut Context<Self>, site: DropdownSite) -> AnyElement {
        let DropdownSite {
            key,
            icon,
            items,
            tip,
            min_width,
            on_event,
        } = site;
        let existing = self.dropdowns.borrow().get(&key).cloned();
        let entity = match existing {
            Some(e) => e,
            None => {
                let e = cx.new(|cx| {
                    DropdownMenu::new(key.clone(), cx)
                        .trigger_icon(icon)
                        .min_width(min_width)
                });
                let k = key.clone();
                let sub = cx.subscribe(&e, move |this: &mut Shell, _, ev: &DropdownEvent, cx| {
                    match ev {
                        DropdownEvent::Open => {
                            this.open_dropdowns.insert(k.clone());
                        }
                        DropdownEvent::Close => {
                            this.open_dropdowns.remove(&k);
                        }
                        DropdownEvent::Action(_) => {}
                    }
                    on_event(this, ev, cx);
                    cx.notify();
                });
                self.dropdown_subs.borrow_mut().push(sub);
                self.dropdowns.borrow_mut().insert(key.clone(), e.clone());
                e
            }
        };
        entity.update(cx, |d, cx| d.sync(items, tip.map(SharedString::from), cx));
        entity.into_any_element()
    }

    pub(crate) fn any_dropdown_open(&self) -> bool {
        !self.open_dropdowns.is_empty()
    }

    /// dev-mcp: `dropdown:<key>` toggles, `dropdown-item:<key>:<row>` runs a root row and
    /// `dropdown-sub:<key>:<row*1000+sub>` a submenu row.
    #[cfg_attr(not(any(test, feature = "dev-mcp")), allow(dead_code))]
    pub(crate) fn dropdown_press(
        &mut self,
        op: &str,
        key: &str,
        idx: Option<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let e = self
            .dropdowns
            .borrow()
            .get(key)
            .cloned()
            .ok_or_else(|| format!("no dropdown menu {key:?}"))?;
        e.update(cx, |d, cx| match (op, idx) {
            ("dropdown", _) => d.toggle(window, cx),
            ("dropdown-item", Some(i)) => d.activate(i, None, window, cx),
            ("dropdown-sub", Some(n)) => {
                d.activate(n / 1000, None, window, cx);
                d.activate(n / 1000, Some(n % 1000), window, cx)
            }
            _ => {}
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items() -> Vec<MenuItem> {
        vec![
            MenuItem::Label("View".into()),
            MenuItem::item("a", "A"),
            MenuItem::Separator,
            MenuItem::item("b", "B").disabled(Some("no")),
            MenuItem::submenu("More", vec![MenuItem::item("c", "C")]),
            MenuItem::Separator,
        ]
    }

    #[test]
    fn separators_are_tidied() {
        let t = tidy(vec![
            MenuItem::Separator,
            MenuItem::item("a", "A"),
            MenuItem::Separator,
            MenuItem::Separator,
            MenuItem::item("b", "B"),
            MenuItem::Separator,
        ]);
        assert_eq!(t.len(), 3);
        assert_eq!(t[1], MenuItem::Separator);
    }

    #[test]
    fn keyboard_skips_labels_separators_and_disabled_rows_and_wraps() {
        let it = tidy(items());
        assert_eq!(next_clickable(&it, None, 1), Some(1));
        assert_eq!(next_clickable(&it, None, -1), Some(4));
        assert_eq!(next_clickable(&it, Some(1), 1), Some(4));
        assert_eq!(next_clickable(&it, Some(4), 1), Some(1));
        assert_eq!(next_clickable(&it, Some(1), -1), Some(4));
        assert!(!it[3].clickable() && !it[0].clickable());
    }
}
