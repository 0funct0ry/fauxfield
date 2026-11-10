//! The reusable single-line text input (`TextInput`). Every one-line input box in the app is
//! this entity; there is no other.
//!
//! * text entry through GPUI's input handler, so IME composition (marked text and the candidate
//!   window), dead keys, the emoji palette and the macOS accent popup work;
//! * grapheme-aware caret movement and deletion;
//! * mouse: click, Shift-click, drag (also outside the box), double-click word, triple-click all;
//! * keys: arrows with Shift, word (Alt, or Ctrl off macOS) and line (Cmd) steps, Home/End,
//!   Cmd+Up/Down, emacs Ctrl+A/E/B/F/D/H/K on macOS, forward and word deletion;
//! * Cut, Copy, Paste (first line, `on_paste` hook), Select All, Undo and Redo with coalescing,
//!   a right-click menu, a blinking caret, horizontal scrolling that follows the caret;
//! * placeholder, prefix/suffix slots, clear button, masked (password) mode with a reveal toggle
//!   (Copy and Cut are disabled while masked), disabled and read-only states, sizes, focus ring,
//!   an invalid style, `clean_on_escape`, character filters, `max_len` and a `validate` predicate;
//! * events: [`InputEvent::Change`], `PressEnter`, `Escape`, `Tab`, `Up`/`Down` (opt-in), `Focus`
//!   and `Blur`.
//!
//! Not included on purpose: number masks, inline tokens, touch handles, OTP/number inputs.

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    AnyElement, App, Bounds, ClipboardItem, Context, DispatchPhase, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, InspectorElementId, InteractiveElement, IntoElement, KeyDownEvent, LayoutId,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, ParentElement, Pixels,
    Point, Render, ShapedLine, SharedString, Style, Styled, TextRun, UTF16Selection,
    UnderlineStyle, Window, anchored, deferred, div, fill, point, prelude::*, px, relative, size,
};
use unicode_segmentation::UnicodeSegmentation;

use crate::editor::TextBuf;
use crate::theme::theme;
use crate::ui::icon;

const BLINK: Duration = Duration::from_millis(530);
const BULLET: &str = "•";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// The text changed through typing, paste, cut, delete, undo or redo (not `set_value`).
    Change,
    PressEnter {
        /// Cmd/Ctrl was held.
        secondary: bool,
        /// Alt/Option was held.
        alt: bool,
    },
    /// Esc on an input that did not consume it (`clean_on_escape` consumes it while non-empty).
    Escape,
    Tab {
        reverse: bool,
    },
    /// Only with [`TextInput::vertical_events`].
    Up,
    Down,
    Focus,
    Blur,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputSize {
    Small,
    Medium,
    Large,
}

impl InputSize {
    fn height(self) -> f32 {
        match self {
            InputSize::Small => 22.0,
            InputSize::Medium => 28.0,
            InputSize::Large => 34.0,
        }
    }
}

/// Which characters an input accepts (applied to typing, IME and paste).
#[derive(Clone)]
pub enum InputFilter {
    Any,
    Digits,
    NoWhitespace,
    Custom(Rc<dyn Fn(char) -> bool>),
}

impl InputFilter {
    fn allows(&self, c: char) -> bool {
        if c.is_control() {
            return false;
        }
        match self {
            InputFilter::Any => true,
            InputFilter::Digits => c.is_ascii_digit(),
            InputFilter::NoWhitespace => !c.is_whitespace(),
            InputFilter::Custom(f) => f(c),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Type,
    Other,
}

#[derive(Clone)]
struct Snap {
    text: String,
    cursor: usize,
}

struct Layout {
    bounds: Bounds<Pixels>,
    line: ShapedLine,
}

type Slot = Rc<dyn Fn(&mut Window, &mut App) -> AnyElement>;
type Validate = Rc<dyn Fn(&str) -> bool>;
type PasteHook = Rc<dyn Fn(&ClipboardItem) -> bool>;

pub struct TextInput {
    focus: FocusHandle,
    buf: TextBuf,
    marked: Option<Range<usize>>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last: Option<Kind>,
    layout: Option<Layout>,
    scroll_x: f32,
    selecting: bool,
    caret_on: bool,
    blink_epoch: u64,
    was_focused: bool,
    menu: Option<Point<Pixels>>,
    reveal: bool,
    // options
    id: SharedString,
    placeholder: SharedString,
    masked: bool,
    mask_toggle: bool,
    cleanable: bool,
    readonly: bool,
    disabled: bool,
    invalid: bool,
    appearance: bool,
    full_height: bool,
    pad: f32,
    blink: bool,
    clean_on_escape: bool,
    vertical_events: bool,
    size: InputSize,
    max_len: usize,
    filter: InputFilter,
    validate: Option<Validate>,
    on_paste: Option<PasteHook>,
    prefix: Option<Slot>,
    suffix: Option<Slot>,
}

impl EventEmitter<InputEvent> for TextInput {}

impl Focusable for TextInput {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

fn to_utf16(s: &str, off: usize) -> usize {
    s[..off.min(s.len())].encode_utf16().count()
}

fn from_utf16(s: &str, u: usize) -> usize {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        if n >= u {
            return i;
        }
        n += c.len_utf16();
    }
    s.len()
}

// ---- construction and options ----------------------------------------------------------------

impl TextInput {
    pub fn new(cx: &mut Context<Self>) -> TextInput {
        let n = cx.entity_id().as_u64();
        TextInput {
            focus: cx.focus_handle(),
            buf: TextBuf::default(),
            marked: None,
            undo: Vec::new(),
            redo: Vec::new(),
            last: None,
            layout: None,
            scroll_x: 0.0,
            selecting: false,
            caret_on: true,
            blink_epoch: 0,
            was_focused: false,
            menu: None,
            reveal: false,
            id: format!("text-input-{n}").into(),
            placeholder: SharedString::default(),
            masked: false,
            mask_toggle: false,
            cleanable: false,
            readonly: false,
            disabled: false,
            invalid: false,
            appearance: true,
            full_height: false,
            pad: 8.0,
            blink: true,
            clean_on_escape: false,
            vertical_events: false,
            size: InputSize::Medium,
            max_len: usize::MAX,
            filter: InputFilter::Any,
            validate: None,
            on_paste: None,
            prefix: None,
            suffix: None,
        }
    }

    /// Initial text; the caret goes to the end. No event, no undo history.
    pub fn text(mut self, text: &str) -> Self {
        self.buf = TextBuf::new(text);
        self
    }

    /// Like [`text`](Self::text), with everything selected so typing replaces it.
    pub fn text_selected(mut self, text: &str) -> Self {
        self.buf = TextBuf::new(text);
        self.buf.select_all();
        self
    }

    pub fn placeholder(mut self, p: impl Into<SharedString>) -> Self {
        self.placeholder = p.into();
        self
    }

    /// Show bullets instead of the text; Copy and Cut are disabled. Add `mask_toggle` for a
    /// reveal button.
    pub fn masked(mut self, on: bool) -> Self {
        self.masked = on;
        self
    }

    pub fn mask_toggle(mut self) -> Self {
        self.mask_toggle = true;
        self
    }

    /// A clear button while the input has text.
    pub fn cleanable(mut self, on: bool) -> Self {
        self.cleanable = on;
        self
    }

    pub fn readonly(mut self, on: bool) -> Self {
        self.readonly = on;
        self
    }

    pub fn disabled(mut self, on: bool) -> Self {
        self.disabled = on;
        self
    }

    /// No border or background (for inline fields drawn inside another element).
    pub fn appearance(mut self, on: bool) -> Self {
        self.appearance = on;
        self
    }

    /// For a field drawn inside another box: no chrome, no padding, as tall as its parent.
    pub fn flush(mut self) -> Self {
        self.appearance = false;
        self.full_height = true;
        self.pad = 0.0;
        self
    }

    pub fn size(mut self, s: InputSize) -> Self {
        self.size = s;
        self
    }

    pub fn cursor_blink(mut self, on: bool) -> Self {
        self.blink = on;
        self
    }

    /// Esc clears a non-empty input; only an empty input emits [`InputEvent::Escape`].
    pub fn clean_on_escape(mut self) -> Self {
        self.clean_on_escape = true;
        self
    }

    /// Up and Down emit events (for a list or completion popup) instead of moving to the
    /// start and end of the text.
    pub fn vertical_events(mut self) -> Self {
        self.vertical_events = true;
        self
    }

    pub fn max_len(mut self, chars: usize) -> Self {
        self.max_len = chars;
        self
    }

    pub fn filter(mut self, f: InputFilter) -> Self {
        self.filter = f;
        self
    }

    /// An edit is rejected when the resulting text fails the predicate.
    pub fn validate(mut self, f: impl Fn(&str) -> bool + 'static) -> Self {
        self.validate = Some(Rc::new(f));
        self
    }

    /// Runs before a paste inserts text; return `true` to consume it.
    pub fn on_paste(mut self, f: impl Fn(&ClipboardItem) -> bool + 'static) -> Self {
        self.on_paste = Some(Rc::new(f));
        self
    }

    pub fn prefix(mut self, f: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        self.prefix = Some(Rc::new(f));
        self
    }

    pub fn suffix(mut self, f: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        self.suffix = Some(Rc::new(f));
        self
    }

    // ---- runtime API -------------------------------------------------------------------------

    pub fn value(&self) -> &str {
        &self.buf.text
    }

    pub fn is_empty(&self) -> bool {
        self.buf.text.is_empty()
    }

    /// The selection as a byte range; the caret is an empty range.
    pub fn selection(&self) -> Range<usize> {
        self.buf
            .selection()
            .map_or(self.buf.cursor..self.buf.cursor, |(a, z)| a..z)
    }

    pub fn cursor(&self) -> usize {
        self.buf.cursor
    }

    /// Replace the text without emitting `Change`; clears the history, the caret goes to the end.
    pub fn set_value(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buf = TextBuf::new(text);
        self.undo.clear();
        self.redo.clear();
        self.last = None;
        self.marked = None;
        self.scroll_x = 0.0;
        self.touch(cx);
    }

    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.buf.select_all();
        self.last = None;
        self.touch(cx);
    }

    pub fn set_invalid(&mut self, on: bool, cx: &mut Context<Self>) {
        if self.invalid != on {
            self.invalid = on;
            cx.notify();
        }
    }

    pub fn set_disabled(&mut self, on: bool, cx: &mut Context<Self>) {
        self.disabled = on;
        cx.notify();
    }

    pub fn set_readonly(&mut self, on: bool, cx: &mut Context<Self>) {
        self.readonly = on;
        cx.notify();
    }

    pub fn set_placeholder(&mut self, p: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.placeholder = p.into();
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window) {
        if !self.disabled {
            window.focus(&self.focus);
        }
    }

    pub fn is_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    // ---- editing core ------------------------------------------------------------------------

    fn hides_text(&self) -> bool {
        self.masked && !self.reveal
    }

    fn clean(&self, s: &str) -> String {
        s.chars().filter(|c| self.filter.allows(*c)).collect()
    }

    fn accepts(&self, text: &str) -> bool {
        text.chars().count() <= self.max_len && self.validate.as_ref().is_none_or(|v| v(text))
    }

    /// Show the caret now and restart the blink timer.
    fn touch(&mut self, cx: &mut Context<Self>) {
        self.caret_on = true;
        self.blink_epoch += 1;
        if self.blink {
            let epoch = self.blink_epoch;
            cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(BLINK).await;
                    let go = this
                        .update(cx, |t, cx| {
                            if t.blink_epoch != epoch {
                                return false;
                            }
                            t.caret_on = !t.caret_on;
                            cx.notify();
                            true
                        })
                        .unwrap_or(false);
                    if !go {
                        break;
                    }
                }
            })
            .detach();
        }
        cx.notify();
    }

    /// Apply `f` to the buffer. Text changes go through the filters and the undo history;
    /// a rejected change is rolled back.
    fn edit(&mut self, kind: Kind, cx: &mut Context<Self>, f: impl FnOnce(&mut TextBuf)) {
        let before = self.buf.clone();
        let editing = !(self.readonly || self.disabled);
        f(&mut self.buf);
        if self.buf.text != before.text {
            if !editing || !self.accepts(&self.buf.text) {
                self.buf = before;
                return;
            }
            if !(kind == Kind::Type && self.last == Some(Kind::Type)) {
                self.undo.push(Snap {
                    text: before.text,
                    cursor: before.cursor,
                });
            }
            self.redo.clear();
            self.last = Some(kind);
            self.marked = None;
            cx.emit(InputEvent::Change);
        } else {
            self.last = None;
        }
        self.touch(cx);
    }

    /// Replace `range` (default: the selection) with `text` after filtering.
    fn insert_at(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        kind: Kind,
        cx: &mut Context<Self>,
    ) {
        let mut clean = self.clean(text);
        if clean.is_empty() && !text.is_empty() {
            return;
        }
        let (a, z) = range.map_or_else(
            || {
                self.buf
                    .selection()
                    .unwrap_or((self.buf.cursor, self.buf.cursor))
            },
            |r| (r.start, r.end),
        );
        let (a, z) = (
            a.min(z).min(self.buf.text.len()),
            z.min(self.buf.text.len()),
        );
        // Truncate a paste that would exceed `max_len`.
        let room = self
            .max_len
            .saturating_sub(self.buf.text.chars().count() - self.buf.text[a..z].chars().count());
        if clean.chars().count() > room {
            clean = clean.chars().take(room).collect();
        }
        self.edit(kind, cx, |b| {
            b.anchor = Some(a);
            b.cursor = z;
            b.insert(&clean);
        });
    }

    fn snapshot_swap(&mut self, snap: Snap, to_redo: bool) {
        let cur = Snap {
            text: std::mem::replace(&mut self.buf.text, snap.text),
            cursor: self.buf.cursor,
        };
        self.buf.cursor = snap.cursor.min(self.buf.text.len());
        self.buf.anchor = None;
        if to_redo {
            self.redo.push(cur);
        } else {
            self.undo.push(cur);
        }
        self.last = None;
        self.marked = None;
    }

    pub fn undo(&mut self, cx: &mut Context<Self>) {
        if self.readonly || self.disabled {
            return;
        }
        if let Some(s) = self.undo.pop() {
            self.snapshot_swap(s, true);
            cx.emit(InputEvent::Change);
            self.touch(cx);
        }
    }

    pub fn redo(&mut self, cx: &mut Context<Self>) {
        if self.readonly || self.disabled {
            return;
        }
        if let Some(s) = self.redo.pop() {
            self.snapshot_swap(s, false);
            cx.emit(InputEvent::Change);
            self.touch(cx);
        }
    }

    /// Type `text` at the caret as if the user did (filters apply; one coalesced undo step).
    pub fn insert_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.insert_at(None, text, Kind::Type, cx);
    }

    pub fn backspace(&mut self, cx: &mut Context<Self>) {
        self.edit(Kind::Other, cx, |b| b.backspace());
    }

    pub fn copy(&mut self, cx: &mut Context<Self>) {
        if self.hides_text() {
            return;
        }
        if let Some(t) = self.buf.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(t.to_string()));
        }
    }

    pub fn cut(&mut self, cx: &mut Context<Self>) {
        if self.hides_text() || self.readonly || self.disabled {
            return;
        }
        if let Some(t) = self.buf.selected_text() {
            cx.write_to_clipboard(ClipboardItem::new_string(t.to_string()));
            self.edit(Kind::Other, cx, |b| {
                b.delete_selection();
            });
        }
    }

    pub fn paste(&mut self, cx: &mut Context<Self>) {
        if self.readonly || self.disabled {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else {
            return;
        };
        if self.on_paste.clone().is_some_and(|h| h(&item)) {
            return;
        }
        if let Some(t) = item.text() {
            let line = t.lines().next().unwrap_or("").to_string();
            self.insert_at(None, &line, Kind::Other, cx);
        }
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.edit(Kind::Other, cx, |b| {
            b.select_all();
            b.delete_selection();
        });
    }

    // ---- keys --------------------------------------------------------------------------------

    /// Returns whether the key was consumed. Plain characters are not: GPUI then hands them to
    /// the input handler, which is what makes IME and dead keys work.
    pub fn handle_key(
        &mut self,
        ev: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.disabled {
            return false;
        }
        let m = ev.keystroke.modifiers;
        let key = ev.keystroke.key.as_str();
        let mac = cfg!(target_os = "macos");
        let cmd = m.secondary();
        let line = mac && m.platform;
        let hidden = self.hides_text();
        // Word steps over a masked value go to its ends, like the real thing.
        let word = !hidden && if mac { m.alt } else { m.control };
        let emacs = mac && m.control && !m.platform && !m.alt;
        let extend = m.shift;
        self.menu = None;

        match key {
            "escape" => {
                if self.clean_on_escape && !self.buf.text.is_empty() {
                    self.clear(cx);
                } else {
                    cx.emit(InputEvent::Escape);
                }
                return true;
            }
            "enter" => {
                cx.emit(InputEvent::PressEnter {
                    secondary: cmd,
                    alt: m.alt,
                });
                return true;
            }
            "tab" => {
                cx.emit(InputEvent::Tab { reverse: m.shift });
                return true;
            }
            "up" if self.vertical_events => {
                cx.emit(InputEvent::Up);
                return true;
            }
            "down" if self.vertical_events => {
                cx.emit(InputEvent::Down);
                return true;
            }
            "space" if mac && m.control && m.platform => {
                window.show_character_palette();
                return true;
            }
            "a" if cmd => self.select_all(cx),
            "c" if cmd => self.copy(cx),
            "x" if cmd => self.cut(cx),
            "v" if cmd => self.paste(cx),
            "z" if cmd && m.shift => self.redo(cx),
            "z" if cmd => self.undo(cx),
            "y" if cmd && !mac => self.redo(cx),
            "left" if line => self.nav(cx, |b| b.home(extend)),
            "right" if line => self.nav(cx, |b| b.end(extend)),
            "left" if word || (hidden && (mac && m.alt || !mac && m.control)) => {
                if word {
                    self.nav(cx, |b| b.word_left(extend))
                } else {
                    self.nav(cx, |b| b.home(extend))
                }
            }
            "right" if word || (hidden && (mac && m.alt || !mac && m.control)) => {
                if word {
                    self.nav(cx, |b| b.word_right(extend))
                } else {
                    self.nav(cx, |b| b.end(extend))
                }
            }
            "left" => self.nav(cx, |b| b.left(extend)),
            "right" => self.nav(cx, |b| b.right(extend)),
            "home" => self.nav(cx, |b| b.home(extend)),
            "end" => self.nav(cx, |b| b.end(extend)),
            "up" => self.nav(cx, |b| b.home(extend)),
            "down" => self.nav(cx, |b| b.end(extend)),
            "backspace" if line => self.edit(Kind::Other, cx, |b| b.delete_to_start()),
            "backspace" if word => self.edit(Kind::Other, cx, |b| b.delete_word_left()),
            "backspace" => self.edit(Kind::Other, cx, |b| b.backspace()),
            "delete" if line => self.edit(Kind::Other, cx, |b| b.delete_to_end()),
            "delete" if word => self.edit(Kind::Other, cx, |b| b.delete_word_right()),
            "delete" => self.edit(Kind::Other, cx, |b| b.delete()),
            "a" if emacs => self.nav(cx, |b| b.home(extend)),
            "e" if emacs => self.nav(cx, |b| b.end(extend)),
            "b" if emacs => self.nav(cx, |b| b.left(extend)),
            "f" if emacs => self.nav(cx, |b| b.right(extend)),
            "d" if emacs => self.edit(Kind::Other, cx, |b| b.delete()),
            "h" if emacs => self.edit(Kind::Other, cx, |b| b.backspace()),
            "k" if emacs => self.edit(Kind::Other, cx, |b| b.delete_to_end()),
            _ => return false,
        }
        true
    }

    fn nav(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut TextBuf)) {
        f(&mut self.buf);
        self.last = None;
        self.touch(cx);
    }

    // ---- mouse -------------------------------------------------------------------------------

    /// Byte offset of the character nearest window x.
    fn index_at(&self, x: Pixels) -> usize {
        let Some(l) = &self.layout else {
            return self.buf.text.len();
        };
        let d = l
            .line
            .closest_index_for_x(x - l.bounds.left() + px(self.scroll_x));
        self.real(d)
    }

    fn mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus);
        self.menu = None;
        let at = self.index_at(ev.position.x);
        self.last = None;
        match ev.click_count {
            0 | 1 => {
                self.buf.anchor = if ev.modifiers.shift {
                    self.buf.anchor.or(Some(self.buf.cursor))
                } else {
                    None
                };
                self.buf.cursor = at;
                self.selecting = true;
            }
            2 if !self.hides_text() => self.buf.select_word_at(at),
            _ => self.buf.select_all(),
        }
        self.touch(cx);
    }

    fn mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.selecting {
            return;
        }
        if ev.pressed_button != Some(MouseButton::Left) {
            self.selecting = false;
            return;
        }
        let at = self.index_at(ev.position.x);
        self.buf.anchor.get_or_insert(self.buf.cursor);
        self.buf.cursor = at;
        self.touch(cx);
    }

    // ---- masked display mapping ---------------------------------------------------------------

    /// Display byte offset of a text byte offset (bullets are one per grapheme).
    fn disp(&self, i: usize) -> usize {
        if !self.hides_text() {
            return i;
        }
        self.buf.text[..i.min(self.buf.text.len())]
            .graphemes(true)
            .count()
            * BULLET.len()
    }

    fn real(&self, d: usize) -> usize {
        if !self.hides_text() {
            return d.min(self.buf.text.len());
        }
        let n = d / BULLET.len();
        self.buf
            .text
            .grapheme_indices(true)
            .nth(n)
            .map_or(self.buf.text.len(), |(i, _)| i)
    }

    fn display_text(&self) -> String {
        if self.hides_text() {
            BULLET.repeat(self.buf.text.graphemes(true).count())
        } else {
            self.buf.text.clone()
        }
    }
}

// ---- IME ---------------------------------------------------------------------------------------

impl EntityInputHandler for TextInput {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let t = &self.buf.text;
        let r = from_utf16(t, range.start)..from_utf16(t, range.end);
        *adjusted = Some(to_utf16(t, r.start)..to_utf16(t, r.end));
        Some(t[r].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        let r = self.selection();
        let t = &self.buf.text;
        Some(UTF16Selection {
            range: to_utf16(t, r.start)..to_utf16(t, r.end),
            reversed: self.buf.anchor.is_some_and(|a| a > self.buf.cursor),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        let t = &self.buf.text;
        self.marked
            .as_ref()
            .map(|m| to_utf16(t, m.start)..to_utf16(t, m.end))
    }

    fn unmark_text(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        self.marked = None;
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        text: &str,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly || self.disabled {
            return;
        }
        let t = &self.buf.text;
        let target = range
            .map(|r| from_utf16(t, r.start)..from_utf16(t, r.end))
            .or_else(|| self.marked.clone());
        let was_marked = self.marked.is_some();
        // Typing runs coalesce; finishing a composition is its own undo step.
        let kind = if was_marked { Kind::Other } else { Kind::Type };
        self.insert_at(target, text, kind, cx);
        self.marked = None;
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range: Option<Range<usize>>,
        new_text: &str,
        new_selected: Option<Range<usize>>,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.readonly || self.disabled {
            return;
        }
        let t = &self.buf.text;
        let target = range
            .map(|r| from_utf16(t, r.start)..from_utf16(t, r.end))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.selection());
        let start = target.start.min(target.end);
        let clean = self.clean(new_text);
        let before_len = self.buf.text.len();
        self.insert_at(Some(target), new_text, Kind::Other, cx);
        if self.buf.text.len() == before_len && clean.is_empty() && !new_text.is_empty() {
            return;
        }
        let end = (start + clean.len()).min(self.buf.text.len());
        self.marked = (end > start).then_some(start..end);
        if let Some(sel) = new_selected {
            let base = to_utf16(&self.buf.text, start);
            let a = from_utf16(&self.buf.text, base + sel.start).min(self.buf.text.len());
            let b = from_utf16(&self.buf.text, base + sel.end).min(self.buf.text.len());
            self.buf.anchor = Some(a);
            self.buf.cursor = b;
        }
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let l = self.layout.as_ref()?;
        let t = &self.buf.text;
        let a = self.disp(from_utf16(t, range.start));
        let z = self.disp(from_utf16(t, range.end));
        let sx = px(self.scroll_x);
        Some(Bounds::from_corners(
            point(
                element_bounds.left() + l.line.x_for_index(a) - sx,
                element_bounds.top(),
            ),
            point(
                element_bounds.left() + l.line.x_for_index(z) - sx,
                element_bounds.bottom(),
            ),
        ))
    }

    fn character_index_for_point(
        &mut self,
        p: Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let l = self.layout.as_ref()?;
        let d = l
            .line
            .closest_index_for_x(p.x - l.bounds.left() + px(self.scroll_x));
        Some(to_utf16(&self.buf.text, self.real(d)))
    }
}

// ---- element -----------------------------------------------------------------------------------

struct TextElement {
    input: Entity<TextInput>,
}

struct Prepaint {
    line: ShapedLine,
    caret: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_x: f32,
}

impl IntoElement for TextElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TextElement {
    type RequestLayoutState = ();
    type PrepaintState = Prepaint;

    fn id(&self) -> Option<ElementId> {
        None
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = relative(1.).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Prepaint {
        let t = theme(cx).clone();
        let input = self.input.read(cx);
        let style = window.text_style();
        let focused = input.focus.is_focused(window);
        let empty = input.buf.text.is_empty();
        let display = if empty {
            input.placeholder.to_string()
        } else {
            input.display_text()
        };
        let color = if empty { t.text_placeholder } else { t.text };
        let base = TextRun {
            len: display.len(),
            font: style.font(),
            color,
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let runs = match input.marked.as_ref().filter(|_| !empty) {
            Some(m) => {
                let (a, z) = (input.disp(m.start), input.disp(m.end));
                [
                    TextRun {
                        len: a,
                        ..base.clone()
                    },
                    TextRun {
                        len: z - a,
                        underline: Some(UnderlineStyle {
                            color: Some(color),
                            thickness: px(1.),
                            wavy: false,
                        }),
                        ..base.clone()
                    },
                    TextRun {
                        len: display.len() - z,
                        ..base.clone()
                    },
                ]
                .into_iter()
                .filter(|r| r.len > 0)
                .collect()
            }
            None => vec![base],
        };
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line = window
            .text_system()
            .shape_line(display.into(), font_size, &runs, None);
        let (sel, cursor) = (input.selection(), input.buf.cursor);
        let (x0, x1) = (
            line.x_for_index(if empty { 0 } else { input.disp(sel.start) }),
            line.x_for_index(if empty { 0 } else { input.disp(sel.end) }),
        );
        let caret_x = line.x_for_index(if empty { 0 } else { input.disp(cursor) });
        let width = bounds.size.width;
        // Keep the caret in view; the empty placeholder never scrolls.
        let total = if empty { px(0.) } else { line.width };
        let mut sx = px(input.scroll_x);
        if caret_x - sx > width - px(2.) {
            sx = caret_x - width + px(2.);
        }
        if caret_x < sx {
            sx = caret_x;
        }
        sx = sx.min((total - width + px(2.)).max(px(0.))).max(px(0.));
        let lh = window.line_height();
        let top = bounds.top() + (bounds.size.height - lh) / 2.;
        let selection = (!empty && x1 > x0 && focused).then(|| {
            fill(
                Bounds::from_corners(
                    point(bounds.left() + x0 - sx, top),
                    point(bounds.left() + x1 - sx, top + lh),
                ),
                t.element_selected,
            )
        });
        let caret = (focused && input.caret_on && !input.disabled && (sel.is_empty() || empty))
            .then(|| {
                fill(
                    Bounds::new(point(bounds.left() + caret_x - sx, top), size(px(1.5), lh)),
                    t.accent,
                )
            });
        let scroll_x = f32::from(sx);
        Prepaint {
            line,
            caret,
            selection,
            scroll_x,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        pre: &mut Prepaint,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus = self.input.read(cx).focus.clone();
        window.handle_input(
            &focus,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );
        let lh = window.line_height();
        let top = bounds.top() + (bounds.size.height - lh) / 2.;
        window.with_content_mask(Some(gpui::ContentMask { bounds }), |window| {
            if let Some(q) = pre.selection.take() {
                window.paint_quad(q);
            }
            let _ = pre
                .line
                .paint(point(bounds.left() - px(pre.scroll_x), top), lh, window, cx);
            if let Some(q) = pre.caret.take() {
                window.paint_quad(q);
            }
        });
        // Remember the geometry for hit-testing and the IME candidate window.
        let line = std::mem::take(&mut pre.line);
        let sx = pre.scroll_x;
        self.input.update(cx, |i, _| {
            i.scroll_x = sx;
            i.layout = Some(Layout { bounds, line });
        });
        // A drag keeps selecting while the pointer leaves the box.
        let input = self.input.clone();
        window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
            if phase == DispatchPhase::Bubble {
                input.update(cx, |i, cx| i.mouse_move(ev, cx));
            }
        });
        let input = self.input.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, _, _, cx| {
            input.update(cx, |i, _| i.selecting = false);
        });
    }
}

// ---- render ------------------------------------------------------------------------------------

impl Render for TextInput {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let focused = self.focus.is_focused(window) && !self.disabled;
        if focused != self.was_focused {
            self.was_focused = focused;
            cx.emit(if focused {
                InputEvent::Focus
            } else {
                InputEvent::Blur
            });
            if focused {
                self.touch(cx);
            } else {
                self.menu = None;
                self.selecting = false;
            }
        }
        let this = cx.entity();
        let muted = t.icon_muted;
        let clear = (self.cleanable && !self.buf.text.is_empty() && !self.disabled).then(|| {
            div()
                .id(SharedString::from(format!("{}-clear", self.id)))
                .flex_none()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, window, cx| {
                    this.clear(cx);
                    window.focus(&this.focus);
                }))
                .child(icon("x", 12., muted))
        });
        let toggle = (self.masked && self.mask_toggle && !self.disabled).then(|| {
            div()
                .id(SharedString::from(format!("{}-reveal", self.id)))
                .flex_none()
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .on_click(cx.listener(|this, _, _, cx| {
                    this.reveal = !this.reveal;
                    cx.notify();
                }))
                .child(icon(
                    if self.reveal { "eye-off" } else { "eye" },
                    14.,
                    muted,
                ))
        });
        let border = if self.invalid {
            t.deleted
        } else if focused {
            t.border_focused
        } else {
            t.border
        };
        let menu = self.menu.map(|pos| self.render_menu(pos, &t, cx));
        div()
            .id(self.id.clone())
            .key_context("TextInput")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if this.handle_key(ev, window, cx) {
                    cx.stop_propagation();
                }
            }))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    this.mouse_down(ev, window, cx);
                    cx.stop_propagation();
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    if !this.disabled {
                        window.focus(&this.focus);
                        this.menu = Some(ev.position);
                        cx.notify();
                    }
                    cx.stop_propagation();
                }),
            )
            .relative()
            .w_full()
            .when(self.full_height, |d| d.h_full())
            .when(!self.full_height, |d| d.h(px(self.size.height())))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .px(px(self.pad))
            .cursor_text()
            .when(self.appearance, |d| {
                d.rounded(px(4.))
                    .border_1()
                    .border_color(border)
                    .bg(t.editor)
            })
            .when(self.disabled, |d| d.opacity(0.5))
            .when_some(self.prefix.clone(), |d, f| d.child(f(window, cx)))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .overflow_hidden()
                    .child(TextElement { input: this }),
            )
            .children(clear)
            .children(toggle)
            .when_some(self.suffix.clone(), |d, f| d.child(f(window, cx)))
            .children(menu)
    }
}

impl TextInput {
    fn render_menu(
        &self,
        pos: Point<Pixels>,
        t: &crate::theme::Theme,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let has_sel = self.buf.selection().is_some() && !self.hides_text();
        let editable = !(self.readonly || self.disabled);
        let can_paste = editable && cx.read_from_clipboard().is_some_and(|c| c.text().is_some());
        let item =
            |label: &'static str, on: bool, f: fn(&mut TextInput, &mut Context<TextInput>)| {
                div()
                    .id(SharedString::from(format!("{}-menu-{label}", self.id)))
                    .h(px(24.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(px(12.))
                    .text_color(if on { t.text } else { t.text_muted })
                    .when(on, |d| d.hover(|h| h.bg(t.element_hover)).cursor_pointer())
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _, window, cx| {
                            if on {
                                f(this, cx);
                            }
                            this.menu = None;
                            window.focus(&this.focus);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(label)
            };
        deferred(
            anchored().position(pos).child(
                div()
                    .occlude()
                    .w(px(140.))
                    .py(px(4.))
                    .rounded(px(6.))
                    .border_1()
                    .border_color(t.border)
                    .bg(t.elevated)
                    .shadow_lg()
                    .on_mouse_down_out(cx.listener(|this, _, _, cx| {
                        this.menu = None;
                        cx.notify();
                    }))
                    .child(item("Cut", has_sel && editable, |i, cx| i.cut(cx)))
                    .child(item("Copy", has_sel, |i, cx| i.copy(cx)))
                    .child(item("Paste", can_paste, |i, cx| i.paste(cx)))
                    .child(item("Select All", !self.buf.text.is_empty(), |i, cx| {
                        i.select_all(cx)
                    })),
            ),
        )
        .with_priority(10)
        .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{TestAppContext, VisualTestContext};

    fn make(
        cx: &mut TestAppContext,
        build: impl FnOnce(TextInput) -> TextInput + 'static,
    ) -> (Entity<TextInput>, &mut VisualTestContext) {
        cx.update(|cx| cx.set_global(crate::theme::Theme::named("One Dark").unwrap()));
        let (view, vcx) = cx.add_window_view(|_, cx| build(TextInput::new(cx)));
        vcx.update(|w, cx| view.read(cx).focus(w));
        (view, vcx)
    }

    fn val(v: &Entity<TextInput>, cx: &mut VisualTestContext) -> String {
        v.read_with(cx, |i, _| i.value().to_string())
    }

    #[gpui::test]
    fn typing_navigation_and_history(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |i| i);
        cx.simulate_input("hello world");
        assert_eq!(val(&v, cx), "hello world");
        let word = if cfg!(target_os = "macos") {
            "alt-backspace"
        } else {
            "ctrl-backspace"
        };
        cx.simulate_keystrokes(word);
        assert_eq!(val(&v, cx), "hello ");
        // One undo undoes the word delete, the next the whole typing run.
        cx.simulate_keystrokes("secondary-z");
        assert_eq!(val(&v, cx), "hello world");
        cx.simulate_keystrokes("secondary-z");
        assert_eq!(val(&v, cx), "");
        cx.simulate_keystrokes("secondary-shift-z");
        assert_eq!(val(&v, cx), "hello world");
        cx.simulate_keystrokes("home shift-right shift-right");
        v.read_with(cx, |i, _| assert_eq!(i.selection(), 0..2));
        cx.simulate_input("J");
        assert_eq!(val(&v, cx), "Jllo world");
        cx.simulate_keystrokes("secondary-a backspace");
        assert_eq!(val(&v, cx), "");
    }

    #[gpui::test]
    fn ime_composition_marks_then_commits(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |i| i.text("ab"));
        v.update_in(cx, |i, w, cx| {
            i.replace_and_mark_text_in_range(None, "に", Some(1..1), w, cx);
            assert_eq!(i.value(), "abに");
            assert_eq!(i.marked_text_range(w, cx), Some(2..3));
            i.replace_and_mark_text_in_range(None, "日本", None, w, cx);
            assert_eq!(i.value(), "ab日本");
            i.replace_text_in_range(None, "日本", w, cx);
            assert!(i.marked_text_range(w, cx).is_none());
            assert_eq!(i.value(), "ab日本");
        });
    }

    #[gpui::test]
    fn filter_max_len_validate_and_graphemes(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |i| i.filter(InputFilter::Digits).max_len(5));
        cx.simulate_input("80a8x0123");
        assert_eq!(val(&v, cx), "80801");
        let (v, cx) = make(cx, |i| {
            i.validate(|s| s.parse::<f32>().is_ok() || s.is_empty())
        });
        cx.simulate_input("1.5x");
        assert_eq!(val(&v, cx), "1.5");
        let (v, cx) = make(cx, |i| i.text("a👨\u{200d}👩\u{200d}👧"));
        cx.simulate_keystrokes("backspace");
        assert_eq!(val(&v, cx), "a");
    }

    #[gpui::test]
    fn events_readonly_masked_and_escape(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |i| i.text("secret").masked(true).clean_on_escape());
        let log = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let l2 = log.clone();
        let _sub = cx.update(|_, cx| {
            cx.subscribe(&v, move |_, ev: &InputEvent, _| {
                l2.borrow_mut().push(ev.clone())
            })
        });
        // Copy is disabled while masked.
        cx.write_to_clipboard(ClipboardItem::new_string("keep".into()));
        cx.simulate_keystrokes("secondary-a secondary-c");
        assert_eq!(
            cx.read_from_clipboard().and_then(|c| c.text()).as_deref(),
            Some("keep")
        );
        cx.simulate_keystrokes("escape");
        assert_eq!(val(&v, cx), "", "Esc clears first");
        cx.simulate_keystrokes("escape enter tab shift-tab");
        let log = log.borrow();
        assert!(log.contains(&InputEvent::Change));
        assert!(log.contains(&InputEvent::Escape));
        assert!(log.contains(&InputEvent::PressEnter {
            secondary: false,
            alt: false
        }));
        assert!(log.contains(&InputEvent::Tab { reverse: true }));

        let (v, cx) = make(cx, |i| i.text("fixed").readonly(true));
        cx.simulate_input("x");
        cx.simulate_keystrokes("backspace");
        assert_eq!(val(&v, cx), "fixed");
    }

    #[gpui::test]
    fn paste_takes_first_line_and_on_paste_can_consume(cx: &mut TestAppContext) {
        let (v, cx) = make(cx, |i| i);
        cx.write_to_clipboard(ClipboardItem::new_string("one\ntwo".into()));
        cx.simulate_keystrokes("secondary-v");
        assert_eq!(val(&v, cx), "one");
        let (v, cx) = make(cx, |i| i.on_paste(|_| true));
        cx.simulate_keystrokes("secondary-v");
        assert_eq!(val(&v, cx), "");
    }
}
