//! The reusable ComboBox (`ComboBox`). Every drop-down choice in the app is this entity: rule
//! target and operator, Response mode and fault, the Route verb.
//!
//! * an entity with its own state (items, value, open, highlight), an option builder and
//!   events ([`ComboEvent::Change`], `Action`, `Clear`, `Open`, `Close`), so a screen never
//!   keeps menu state of its own;
//! * a popup drawn with `deferred` + `anchored` under the trigger, as wide as the trigger,
//!   flipped or shifted to stay inside the window, scrollable with a maximum height;
//! * keyboard: the trigger is a tab stop; Enter, Space, Up and Down open it; in the list
//!   Up/Down/Home/End/PageUp/PageDown move the highlight (scrolled into view), a letter jumps
//!   to the next entry starting with it (type-ahead), Enter or Tab picks, Esc closes and
//!   returns focus to the trigger;
//! * mouse: click toggles, hover highlights, a press outside closes;
//! * optional search box (`searchable`) filtering the entries, a clear button (`cleanable`),
//!   disabled state, placeholder, a fallback label for a value that is not in the list (a
//!   custom HTTP verb), per-entry colour, check mark on the current value, disabled entries,
//!   separators and action entries (like "Custom…") that emit instead of selecting;
//! * two trigger looks: a bordered field and a badge.
//!
//! Not included: multiple selection (nothing here needs it).

use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use gpui::{
    AnyElement, Bounds, Context, Entity, EventEmitter, FocusHandle, Focusable, Hsla,
    InteractiveElement, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, ParentElement,
    Pixels, Render, ScrollHandle, SharedString, StatefulInteractiveElement, Styled, Subscription,
    Window, anchored, canvas, deferred, div, point, prelude::*, px,
};

use crate::text_input::{InputEvent, TextInput};
use crate::theme::theme;
use crate::ui::icon;

const TYPE_AHEAD: Duration = Duration::from_millis(700);
const MENU_GAP: f32 = 2.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemKind {
    Value,
    Action,
}

/// One entry. `value` is what a selection reports; `label` is what is drawn.
#[derive(Clone, Debug, PartialEq)]
pub struct ComboItem {
    pub value: SharedString,
    pub label: SharedString,
    pub color: Option<Hsla>,
    pub detail: Option<SharedString>,
    pub disabled: bool,
    pub separator: bool,
    kind: ItemKind,
}

impl ComboItem {
    pub fn new(value: impl Into<SharedString>, label: impl Into<SharedString>) -> ComboItem {
        ComboItem {
            value: value.into(),
            label: label.into(),
            color: None,
            detail: None,
            disabled: false,
            separator: false,
            kind: ItemKind::Value,
        }
    }

    /// An entry whose label is its value.
    pub fn plain(value: impl Into<SharedString>) -> ComboItem {
        let v: SharedString = value.into();
        ComboItem::new(v.clone(), v)
    }

    /// An entry that emits [`ComboEvent::Action`] with `key` instead of selecting.
    pub fn action(key: impl Into<SharedString>, label: impl Into<SharedString>) -> ComboItem {
        ComboItem {
            kind: ItemKind::Action,
            ..ComboItem::new(key, label)
        }
    }

    pub fn color(mut self, c: Hsla) -> Self {
        self.color = Some(c);
        self
    }

    pub fn detail(mut self, d: impl Into<SharedString>) -> Self {
        self.detail = Some(d.into());
        self
    }

    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }

    /// A divider line above this entry.
    pub fn separator(mut self) -> Self {
        self.separator = true;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ComboEvent {
    /// The user picked a value.
    Change(SharedString),
    /// The user picked an action entry.
    Action(SharedString),
    /// The clear button was pressed (`cleanable`).
    Clear,
    Open,
    Close,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComboSize {
    Small,
    Medium,
    Large,
}

impl ComboSize {
    fn height(self) -> f32 {
        match self {
            ComboSize::Small => 22.0,
            ComboSize::Medium => 26.0,
            ComboSize::Large => 32.0,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComboLook {
    /// A bordered field with a chevron.
    Field,
    /// A filled badge with a chevron (the Route verb).
    Badge,
}

/// The ComboBox entity.
pub struct ComboBox {
    id: SharedString,
    focus: FocusHandle,
    items: Vec<ComboItem>,
    value: Option<SharedString>,
    fallback: Option<SharedString>,
    placeholder: SharedString,
    tooltip: Option<SharedString>,
    color: Option<Hsla>,
    look: ComboLook,
    size: ComboSize,
    mono: bool,
    disabled: bool,
    cleanable: bool,
    searchable: bool,
    menu_width: f32,
    menu_max_h: f32,
    empty: SharedString,
    // runtime
    open: bool,
    /// Highlighted entry, an index into `items`.
    hl: Option<usize>,
    query: String,
    search: Option<(Entity<TextInput>, Subscription)>,
    scroll: ScrollHandle,
    bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    ahead: String,
    ahead_at: Option<Instant>,
}

impl EventEmitter<ComboEvent> for ComboBox {}

impl Focusable for ComboBox {
    fn focus_handle(&self, _: &gpui::App) -> FocusHandle {
        self.focus.clone()
    }
}

// ---- construction and options ----------------------------------------------------------------

impl ComboBox {
    pub fn new(id: impl Into<SharedString>, cx: &mut Context<Self>) -> ComboBox {
        ComboBox {
            id: id.into(),
            focus: cx.focus_handle().tab_stop(true),
            items: Vec::new(),
            value: None,
            fallback: None,
            placeholder: "Select…".into(),
            tooltip: None,
            color: None,
            look: ComboLook::Field,
            size: ComboSize::Medium,
            mono: true,
            disabled: false,
            cleanable: false,
            searchable: false,
            menu_width: 0.0,
            menu_max_h: 280.0,
            empty: "No results".into(),
            open: false,
            hl: None,
            query: String::new(),
            search: None,
            scroll: ScrollHandle::new(),
            bounds: Rc::new(Cell::new(None)),
            ahead: String::new(),
            ahead_at: None,
        }
    }

    pub fn items(mut self, items: Vec<ComboItem>) -> Self {
        self.items = items;
        self
    }

    pub fn value(mut self, v: impl Into<SharedString>) -> Self {
        self.value = Some(v.into());
        self
    }

    pub fn placeholder(mut self, p: impl Into<SharedString>) -> Self {
        self.placeholder = p.into();
        self
    }

    pub fn look(mut self, l: ComboLook) -> Self {
        self.look = l;
        self
    }

    pub fn size(mut self, s: ComboSize) -> Self {
        self.size = s;
        self
    }

    /// Draw labels in the UI font instead of the buffer font.
    pub fn proportional(mut self) -> Self {
        self.mono = false;
        self
    }

    pub fn tooltip(mut self, t: impl Into<SharedString>) -> Self {
        self.tooltip = Some(t.into());
        self
    }

    pub fn cleanable(mut self, on: bool) -> Self {
        self.cleanable = on;
        self
    }

    /// A search box at the top of the list filters the entries.
    pub fn searchable(mut self, on: bool) -> Self {
        self.searchable = on;
        self
    }

    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }

    /// Minimum menu width; the menu is never narrower than the trigger.
    pub fn menu_width(mut self, w: f32) -> Self {
        self.menu_width = w;
        self
    }

    pub fn menu_max_h(mut self, h: f32) -> Self {
        self.menu_max_h = h;
        self
    }

    pub fn empty_text(mut self, t: impl Into<SharedString>) -> Self {
        self.empty = t.into();
        self
    }

    // ---- runtime API -------------------------------------------------------------------------

    pub fn selected(&self) -> Option<&SharedString> {
        self.value.as_ref()
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The text the trigger shows.
    #[cfg(feature = "dev-mcp")]
    pub fn display_label(&self) -> String {
        match (self.current_index(), &self.fallback) {
            (Some(i), _) => self.items[i].label.to_string(),
            (None, Some(f)) => f.to_string(),
            (None, None) => self.placeholder.to_string(),
        }
    }

    /// Replace entries, value and look from the model. Notifies only when something changed, so
    /// a screen can call it on every render.
    #[allow(clippy::too_many_arguments)]
    pub fn sync(
        &mut self,
        items: Vec<ComboItem>,
        value: Option<SharedString>,
        fallback: Option<SharedString>,
        color: Option<Hsla>,
        tooltip: Option<SharedString>,
        disabled: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = self.items != items
            || self.value != value
            || self.fallback != fallback
            || self.color != color
            || self.tooltip != tooltip
            || self.disabled != disabled;
        if changed {
            self.items = items;
            self.value = value;
            self.fallback = fallback;
            self.color = color;
            self.tooltip = tooltip;
            self.disabled = disabled;
            if self.disabled && self.open {
                self.open = false;
                cx.emit(ComboEvent::Close);
            }
            cx.notify();
        }
    }

    pub fn set_value(&mut self, v: Option<SharedString>, cx: &mut Context<Self>) {
        if self.value != v {
            self.value = v;
            cx.notify();
        }
    }

    pub fn focus(&self, window: &mut Window) {
        if !self.disabled {
            window.focus(&self.focus);
        }
    }

    /// Entries that pass the search query, as indices into `items`.
    fn visible(&self) -> Vec<usize> {
        let q = self.query.trim().to_lowercase();
        (0..self.items.len())
            .filter(|&i| q.is_empty() || self.items[i].label.to_lowercase().contains(&q))
            .collect()
    }

    fn current_index(&self) -> Option<usize> {
        let v = self.value.as_ref()?;
        self.items
            .iter()
            .position(|i| i.kind == ItemKind::Value && &i.value == v)
    }

    pub fn open(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open || self.disabled {
            return;
        }
        self.open = true;
        self.query.clear();
        self.ahead.clear();
        let vis = self.visible();
        self.hl = self
            .current_index()
            .filter(|i| vis.contains(i))
            .or_else(|| vis.iter().copied().find(|&i| !self.items[i].disabled));
        if self.searchable {
            let input = match &self.search {
                Some((i, _)) => i.clone(),
                None => {
                    let input = cx.new(|cx| {
                        TextInput::new(cx)
                            .placeholder("Search…")
                            .appearance(false)
                            .vertical_events()
                    });
                    let sub = cx.subscribe_in(
                        &input,
                        window,
                        |this: &mut ComboBox, input, ev: &InputEvent, window, cx| match ev {
                            InputEvent::Change => {
                                this.query = input.read(cx).value().to_string();
                                let vis = this.visible();
                                this.hl = vis.iter().copied().find(|&i| !this.items[i].disabled);
                                this.reveal();
                                cx.notify();
                            }
                            InputEvent::Up => this.step(-1, cx),
                            InputEvent::Down => this.step(1, cx),
                            InputEvent::PressEnter { .. } | InputEvent::Tab { .. } => {
                                this.pick_highlighted(window, cx)
                            }
                            InputEvent::Escape => this.close(true, window, cx),
                            _ => {}
                        },
                    );
                    self.search = Some((input.clone(), sub));
                    input
                }
            };
            input.update(cx, |t, cx| t.set_value("", cx));
            input.read(cx).focus(window);
        } else {
            window.focus(&self.focus);
        }
        self.reveal();
        cx.emit(ComboEvent::Open);
        cx.notify();
    }

    pub fn close(&mut self, refocus: bool, window: &mut Window, cx: &mut Context<Self>) {
        if !self.open {
            return;
        }
        self.open = false;
        self.query.clear();
        if refocus {
            window.focus(&self.focus);
        }
        cx.emit(ComboEvent::Close);
        cx.notify();
    }

    pub fn toggle(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open {
            self.close(true, window, cx);
        } else {
            self.open(window, cx);
        }
    }

    /// Scroll the highlighted entry into view.
    fn reveal(&self) {
        if let Some(h) = self.hl
            && let Some(pos) = self.visible().iter().position(|&i| i == h)
        {
            self.scroll.scroll_to_item(pos);
        }
    }

    /// Move the highlight by `delta` visible, enabled entries (wrapping).
    fn step(&mut self, delta: isize, cx: &mut Context<Self>) {
        let vis: Vec<usize> = self
            .visible()
            .into_iter()
            .filter(|&i| !self.items[i].disabled)
            .collect();
        if vis.is_empty() {
            return;
        }
        let at = self.hl.and_then(|h| vis.iter().position(|&i| i == h));
        let n = vis.len() as isize;
        let next = match at {
            Some(p) => (p as isize + delta).rem_euclid(n) as usize,
            None if delta < 0 => vis.len() - 1,
            None => 0,
        };
        self.hl = Some(vis[next]);
        self.reveal();
        cx.notify();
    }

    fn jump(&mut self, to_end: bool, cx: &mut Context<Self>) {
        let vis: Vec<usize> = self
            .visible()
            .into_iter()
            .filter(|&i| !self.items[i].disabled)
            .collect();
        self.hl = if to_end { vis.last() } else { vis.first() }.copied();
        self.reveal();
        cx.notify();
    }

    fn page(&mut self, delta: isize, cx: &mut Context<Self>) {
        let vis: Vec<usize> = self
            .visible()
            .into_iter()
            .filter(|&i| !self.items[i].disabled)
            .collect();
        if vis.is_empty() {
            return;
        }
        let at = self
            .hl
            .and_then(|h| vis.iter().position(|&i| i == h))
            .unwrap_or(0);
        let next = (at as isize + delta).clamp(0, vis.len() as isize - 1) as usize;
        self.hl = Some(vis[next]);
        self.reveal();
        cx.notify();
    }

    /// Type-ahead: jump to the next entry whose label starts with the typed prefix.
    fn type_ahead(&mut self, ch: char, cx: &mut Context<Self>) -> bool {
        let now = Instant::now();
        if self
            .ahead_at
            .is_none_or(|t| now.duration_since(t) > TYPE_AHEAD)
        {
            self.ahead.clear();
        }
        self.ahead_at = Some(now);
        self.ahead.extend(ch.to_lowercase());
        let vis: Vec<usize> = self
            .visible()
            .into_iter()
            .filter(|&i| !self.items[i].disabled)
            .collect();
        if vis.is_empty() {
            return false;
        }
        let from = self
            .hl
            .and_then(|h| vis.iter().position(|&i| i == h))
            .unwrap_or(0);
        // A repeated single letter cycles to the next match; a longer prefix may keep the
        // current entry.
        let start = if self.ahead.chars().count() == 1 {
            from + 1
        } else {
            from
        };
        let hit = (0..vis.len())
            .map(|d| vis[(start + d) % vis.len()])
            .find(|&i| self.items[i].label.to_lowercase().starts_with(&self.ahead));
        match hit {
            Some(i) => {
                self.hl = Some(i);
                self.reveal();
                cx.notify();
                true
            }
            None => false,
        }
    }

    fn pick_highlighted(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(i) = self.hl {
            self.pick(i, window, cx);
        }
    }

    /// Choose entry `ix` (an index into `items`).
    pub fn pick(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(item) = self.items.get(ix).cloned() else {
            return;
        };
        if item.disabled {
            return;
        }
        self.close(true, window, cx);
        match item.kind {
            ItemKind::Action => cx.emit(ComboEvent::Action(item.value)),
            ItemKind::Value => {
                self.value = Some(item.value.clone());
                cx.emit(ComboEvent::Change(item.value));
            }
        }
        cx.notify();
    }

    /// Choose the entry with this value or action key (the dev-mcp seam and tests).
    pub fn pick_value(&mut self, value: &str, window: &mut Window, cx: &mut Context<Self>) -> bool {
        match self.items.iter().position(|i| i.value.as_ref() == value) {
            Some(ix) if !self.items[ix].disabled => {
                self.pick(ix, window, cx);
                true
            }
            _ => false,
        }
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        if self.value.take().is_some() {
            cx.emit(ComboEvent::Clear);
            cx.notify();
        }
    }

    // ---- keys --------------------------------------------------------------------------------

    /// Returns whether the key was consumed.
    pub fn handle_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let k = &ev.keystroke;
        if k.modifiers.platform || k.modifiers.control || k.modifiers.alt {
            return false;
        }
        let key = k.key.as_str();
        if !self.open {
            return match key {
                "enter" | "space" | "down" | "up" => {
                    self.open(window, cx);
                    true
                }
                "tab" => {
                    if k.modifiers.shift {
                        window.focus_prev();
                    } else {
                        window.focus_next();
                    }
                    true
                }
                _ => false,
            };
        }
        match key {
            "down" => self.step(1, cx),
            "up" => self.step(-1, cx),
            "home" => self.jump(false, cx),
            "end" => self.jump(true, cx),
            "pagedown" => self.page(8, cx),
            "pageup" => self.page(-8, cx),
            "escape" => self.close(true, window, cx),
            "enter" | "tab" => self.pick_highlighted(window, cx),
            _ => {
                let Some(c) = k.key_char.as_deref().and_then(|s| s.chars().next()) else {
                    return false;
                };
                if self.searchable || !c.is_alphanumeric() {
                    return false;
                }
                return self.type_ahead(c, cx);
            }
        }
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

impl Render for ComboBox {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        // Focus that went elsewhere (code, another click target) closes the list.
        if self.open {
            let in_search = self
                .search
                .as_ref()
                .is_some_and(|(i, _)| i.read(cx).is_focused(window));
            if !self.focus.is_focused(window) && !in_search {
                self.open = false;
                self.query.clear();
                cx.emit(ComboEvent::Close);
            }
        }
        let focused = self.focus.is_focused(window) && !self.disabled;
        let current = self.current_index();
        let label_item = current.map(|i| &self.items[i]);
        let (label, label_color, empty_label) = match (label_item, &self.fallback) {
            (Some(i), _) => (
                i.label.clone(),
                i.color.or(self.color).unwrap_or(t.text),
                false,
            ),
            (None, Some(f)) => (f.clone(), self.color.unwrap_or(t.text), false),
            (None, None) => (self.placeholder.clone(), t.text_placeholder, true),
        };
        let h = self.size.height();
        let border = if self.open || focused {
            t.border_focused
        } else {
            t.border_variant
        };
        let id = self.id.clone();
        let bounds = self.bounds.clone();
        let can_clear = self.cleanable && !self.disabled && !empty_label;
        let clear = can_clear.then(|| {
            div()
                .id(SharedString::from(format!("{id}-clear")))
                .flex_none()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| this.clear(cx)))
                .child(icon("x", 12., t.icon_muted))
        });
        let trigger = div()
            .id(id.clone())
            .relative()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(4.))
            .rounded(px(4.))
            .when(self.mono, |d| d.font_family(crate::assets::BUFFER_FONT))
            .text_size(px(12.))
            .text_color(label_color)
            .track_focus(&self.focus)
            .key_context("ComboBox")
            .when(self.look == ComboLook::Field, |d| {
                d.h(px(h))
                    .pl(px(8.))
                    .pr(px(6.))
                    .border_1()
                    .border_color(border)
                    .when(!self.disabled, |d| d.hover(|s| s.border_color(t.border)))
            })
            .when(self.look == ComboLook::Badge, |d| {
                d.px(px(8.))
                    .py(px(2.))
                    .bg(t.element_selected)
                    .border_1()
                    .border_color(if self.open || focused {
                        t.border_focused
                    } else {
                        gpui::transparent_black()
                    })
            })
            .when(self.disabled, |d| d.opacity(0.5))
            .when(!self.disabled, |d| d.cursor_pointer())
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
                cx.listener(|this, _, window, cx| {
                    if !this.disabled {
                        this.toggle(window, cx);
                    }
                }),
            )
            .child(
                canvas(
                    move |b: Bounds<Pixels>, _, _| bounds.set(Some(b)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(div().min_w(px(0.)).truncate().child(label))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .flex_none()
                    .children(clear)
                    .child(icon("chevron-down", 12., t.icon_muted)),
            );
        let popup = self.open.then(|| self.render_popup(&t, cx));
        div()
            .id(SharedString::from(format!("{id}-combo")))
            .relative()
            .w_full()
            .child(trigger)
            .children(popup)
    }
}

impl ComboBox {
    fn render_popup(&self, t: &crate::theme::Theme, cx: &mut Context<Self>) -> AnyElement {
        let b = self.bounds.get();
        let trigger_w = b.map_or(0., |b| f32::from(b.size.width));
        let at = b.map_or(point(px(0.), px(0.)), |b| {
            point(b.left(), b.bottom() + px(MENU_GAP))
        });
        let width = trigger_w.max(self.menu_width).max(96.);
        let vis = self.visible();
        let current = self.current_index();
        let id = self.id.clone();
        let rows: Vec<AnyElement> = vis
            .iter()
            .map(|&ix| {
                let item = &self.items[ix];
                let hl = self.hl == Some(ix);
                let checked = current == Some(ix);
                let color = if item.disabled {
                    t.text_placeholder
                } else {
                    item.color.unwrap_or(t.text)
                };
                let (value, label) = (item.value.clone(), item.label.clone());
                let opt_id = format!("{id}-option-{value}");
                let row = div()
                    .id(SharedString::from(opt_id.clone()))
                    .h(px(26.))
                    .px(px(10.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .text_size(px(12.))
                    .text_color(color)
                    .when(self.mono, |d| d.font_family(crate::assets::BUFFER_FONT))
                    .when(hl && !item.disabled, |d| d.bg(t.element_hover))
                    .when(!item.disabled, |d| d.cursor_pointer())
                    .when(item.separator, |d| {
                        d.border_t_1().border_color(t.border_variant)
                    })
                    .on_mouse_move(cx.listener(move |this, _, _, cx| {
                        if this.hl != Some(ix) && !this.items[ix].disabled {
                            this.hl = Some(ix);
                            cx.notify();
                        }
                    }))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            this.pick(ix, window, cx);
                            cx.stop_propagation();
                        }),
                    )
                    .child(div().min_w(px(0.)).truncate().child(label.clone()))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .flex_none()
                            .children(item.detail.clone().map(|d| {
                                div().text_size(px(11.)).text_color(t.text_muted).child(d)
                            }))
                            .children(checked.then(|| icon("check", 12., t.created))),
                    );
                crate::dev_id_z!(
                    row,
                    1,
                    opt_id,
                    MenuItem,
                    label,
                    ComboItem(id.to_string(), value.to_string())
                )
                .into_any_element()
            })
            .collect();
        let search = self
            .search
            .as_ref()
            .filter(|_| self.searchable)
            .map(|(i, _)| {
                div()
                    .h(px(30.))
                    .px(px(8.))
                    .flex()
                    .items_center()
                    .border_b_1()
                    .border_color(t.border_variant)
                    .child(i.clone())
            });
        let list = div()
            .id(SharedString::from(format!("{id}-list")))
            .max_h(px(self.menu_max_h))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .flex()
            .flex_col()
            .when(rows.is_empty(), |d| {
                d.child(
                    div()
                        .h(px(26.))
                        .px(px(10.))
                        .flex()
                        .items_center()
                        .text_size(px(12.))
                        .text_color(t.text_muted)
                        .child(self.empty.clone()),
                )
            })
            .children(rows);
        deferred(
            anchored()
                .position(at)
                .snap_to_window_with_margin(px(8.))
                .child(
                    div()
                        .id(SharedString::from(format!("{id}-menu")))
                        .occlude()
                        .w(px(width))
                        .py(px(4.))
                        .rounded(px(6.))
                        .bg(t.elevated)
                        .border_1()
                        .border_color(t.border)
                        .shadow_md()
                        .on_mouse_down_out(cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                            this.mouse_out(ev, window, cx)
                        }))
                        .children(search)
                        .child(list),
                ),
        )
        .with_priority(5)
        .into_any_element()
    }
}

// ---- Shell integration -------------------------------------------------------------------------

use crate::shell::Shell;
use crate::theme::Theme;

#[derive(Clone, Copy, Debug)]
pub enum ComboWidth {
    /// Share the row (flex 1).
    Flex,
    Fixed(f32),
    Auto,
}

/// Handler that runs when a ComboBox of the Shell emits an event.
pub type ComboHandler = Rc<dyn Fn(&mut Shell, &ComboEvent, &mut Context<Shell>)>;

/// Everything a screen says about one ComboBox: a stable `key` (also its dev id), the current
/// model state, and what to do with its events. The entity itself lives in the Shell.
pub(crate) struct ComboSite {
    pub key: String,
    pub items: Vec<ComboItem>,
    pub value: Option<SharedString>,
    pub fallback: Option<SharedString>,
    pub color: Option<Hsla>,
    pub tip: Option<String>,
    pub width: ComboWidth,
    pub look: ComboLook,
    pub size: ComboSize,
    pub on_event: ComboHandler,
}

impl Shell {
    /// The ComboBox for `site.key`, created on first use and synced from the model on every
    /// render. This is the only way a screen draws a ComboBox.
    pub(crate) fn combo_field(&self, cx: &mut Context<Self>, site: ComboSite) -> AnyElement {
        let ComboSite {
            key,
            items,
            value,
            fallback,
            color,
            tip,
            width,
            look,
            size,
            on_event,
        } = site;
        let existing = self.combos.borrow().get(&key).cloned();
        let entity = match existing {
            Some(e) => e,
            None => {
                let e = cx.new(|cx| ComboBox::new(key.clone(), cx).look(look).size(size));
                let k = key.clone();
                let sub = cx.subscribe(&e, move |this: &mut Shell, _, ev: &ComboEvent, cx| {
                    match ev {
                        ComboEvent::Open => {
                            this.open_combos.insert(k.clone());
                        }
                        ComboEvent::Close => {
                            this.open_combos.remove(&k);
                        }
                        _ => {}
                    }
                    on_event(this, ev, cx);
                    cx.notify();
                });
                self.combo_subs.borrow_mut().push(sub);
                self.combos.borrow_mut().insert(key.clone(), e.clone());
                e
            }
        };
        let tip = tip.map(SharedString::from);
        entity.update(cx, |c, cx| {
            c.sync(items, value, fallback, color, tip, false, cx)
        });
        let wrap = div()
            .id(crate::ui::eid(format!("{key}-slot")))
            .map(|d| match width {
                ComboWidth::Flex => d.flex_1().min_w(px(0.)),
                ComboWidth::Fixed(w) => d.w(px(w)).flex_none(),
                ComboWidth::Auto => d.flex_none(),
            })
            .child(entity.clone());
        #[cfg(feature = "dev-mcp")]
        let label = entity.read(cx).display_label();
        crate::dev_id!(
            wrap,
            key.clone(),
            Button,
            label,
            Press(format!("combo:{key}"))
        )
        .into_any_element()
    }

    #[cfg(feature = "dev-mcp")]
    pub(crate) fn close_all_combos(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let all: Vec<_> = self.combos.borrow().values().cloned().collect();
        for e in all {
            e.update(cx, |c, cx| c.close(false, window, cx));
        }
    }

    pub(crate) fn any_combo_open(&self) -> bool {
        !self.open_combos.is_empty()
    }

    /// Open the ComboBox `key` (a screen drew it already).
    pub(crate) fn combo_open(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(e) = self.combos.borrow().get(key).cloned() else {
            return false;
        };
        e.update(cx, |c, cx| c.open(window, cx));
        true
    }

    /// Pick `value` in ComboBox `key` as a click on its entry would.
    #[cfg_attr(not(any(test, feature = "dev-mcp")), allow(dead_code))]
    pub(crate) fn combo_pick(
        &mut self,
        key: &str,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(e) = self.combos.borrow().get(key).cloned() else {
            return false;
        };
        e.update(cx, |c, cx| c.pick_value(value, window, cx))
    }

    /// Run the palette action `id` on the next render (a ComboBox event handler has no window
    /// of its own).
    pub(crate) fn defer_action(&mut self, id: &str, cx: &mut Context<Self>) {
        self.pending_action = Some(id.to_string());
        cx.notify();
    }
}

#[allow(dead_code)]
fn _theme_used(_: &Theme) {}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn items() -> Vec<ComboItem> {
        vec![
            ComboItem::plain("alpha"),
            ComboItem::plain("beta").disabled(true),
            ComboItem::plain("bravo"),
            ComboItem::plain("charlie"),
            ComboItem::action("custom", "Custom…").separator(),
        ]
    }

    fn make(
        cx: &mut TestAppContext,
        build: impl FnOnce(ComboBox) -> ComboBox + 'static,
    ) -> (Entity<ComboBox>, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(crate::theme::Theme::named("One Dark").unwrap()));
        #[cfg(feature = "dev-mcp")]
        cx.update(|cx| cx.set_global(crate::dev_mcp::tree::Registry::default()));
        let (v, vcx) = cx.add_window_view(|_, cx| build(ComboBox::new("t", cx).items(items())));
        vcx.update(|w, cx| v.read(cx).focus(w));
        (v, vcx)
    }

    fn log(
        v: &Entity<ComboBox>,
        cx: &mut VisualTestContext,
    ) -> Rc<std::cell::RefCell<Vec<ComboEvent>>> {
        let l = Rc::new(std::cell::RefCell::new(Vec::new()));
        let l2 = l.clone();
        let sub = cx.update(|_, cx| {
            cx.subscribe(v, move |_, ev: &ComboEvent, _| {
                l2.borrow_mut().push(ev.clone())
            })
        });
        std::mem::forget(sub);
        l
    }

    #[gpui::test]
    fn keyboard_opens_navigates_skips_disabled_and_picks(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |c| c.value("alpha"));
        let l = log(&v, cx);
        cx.simulate_keystrokes("enter");
        assert!(v.read_with(cx, |c, _| c.is_open()));
        cx.simulate_keystrokes("down");
        // `beta` is disabled: the highlight lands on `bravo`.
        cx.simulate_keystrokes("enter");
        assert!(!v.read_with(cx, |c, _| c.is_open()));
        assert_eq!(
            v.read_with(cx, |c, _| c.selected().cloned()),
            Some("bravo".into())
        );
        assert!(l.borrow().contains(&ComboEvent::Change("bravo".into())));
        // Esc closes without a change and focus stays on the trigger.
        cx.simulate_keystrokes("space escape");
        assert!(!v.read_with(cx, |c, _| c.is_open()));
        assert_eq!(
            v.read_with(cx, |c, _| c.selected().cloned()),
            Some("bravo".into())
        );
        // Home/End, wrapping and the action entry.
        cx.simulate_keystrokes("down end enter");
        assert!(l.borrow().contains(&ComboEvent::Action("custom".into())));
        assert_eq!(
            v.read_with(cx, |c, _| c.selected().cloned()),
            Some("bravo".into())
        );
    }

    #[gpui::test]
    fn type_ahead_jumps_and_cycles(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |c| c.value("alpha"));
        cx.simulate_keystrokes("enter");
        cx.simulate_input("c");
        cx.simulate_keystrokes("enter");
        assert_eq!(
            v.read_with(cx, |c, _| c.selected().cloned()),
            Some("charlie".into())
        );
    }

    #[gpui::test]
    fn search_filters_and_enter_picks(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |c| c.searchable(true).value("alpha"));
        cx.simulate_keystrokes("enter");
        cx.simulate_input("rav");
        assert_eq!(v.read_with(cx, |c, _| c.visible().len()), 1);
        cx.simulate_keystrokes("enter");
        assert_eq!(
            v.read_with(cx, |c, _| c.selected().cloned()),
            Some("bravo".into())
        );
        assert!(!v.read_with(cx, |c, _| c.is_open()));
    }

    #[gpui::test]
    fn fallback_label_clear_disabled_and_sync(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |c| c.cleanable(true));
        v.update(cx, |c, cx| {
            c.sync(
                items(),
                Some("zzz".into()),
                Some("zzz".into()),
                None,
                None,
                false,
                cx,
            )
        });
        assert!(v.read_with(cx, |c, _| c.current_index().is_none()));
        v.update(cx, |c, cx| {
            c.sync(items(), Some("alpha".into()), None, None, None, true, cx)
        });
        cx.simulate_keystrokes("enter");
        assert!(!v.read_with(cx, |c, _| c.is_open()), "disabled never opens");
        let l = log(&v, cx);
        v.update(cx, |c, cx| c.clear(cx));
        assert!(l.borrow().contains(&ComboEvent::Clear));
        assert!(v.read_with(cx, |c, _| c.selected().is_none()));
    }

    #[gpui::test]
    fn pick_value_reports_and_refuses_disabled(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |c| c);
        let ok = v.update_in(cx, |c, w, cx| c.pick_value("charlie", w, cx));
        assert!(ok);
        let bad = v.update_in(cx, |c, w, cx| c.pick_value("beta", w, cx));
        assert!(!bad);
        assert_eq!(
            v.read_with(cx, |c, _| c.selected().cloned()),
            Some("charlie".into())
        );
    }
}
