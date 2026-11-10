//! The `CodeEditor` GPUI entity: focus, scroll, caret, input handling, actions and events.

use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use gpui::{
    App, AppContext as _, Bounds, ClipboardItem, Context, Entity, EntityInputHandler, EventEmitter,
    FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, ScrollWheelEvent,
    Styled, UTF16Selection, Window, div, point, px,
};

use super::actions as act;
use super::buffer::{Buffer, EditKind, Plan};
use super::completion::{self, CompletionProvider, Item, Trigger};
use super::display_map::DisplayMap;
use super::element::EditorElement;
use super::format;
use super::highlight::Highlighter;
use super::language_config::Language;
use super::ops::{self, Indent};
use crate::theme::theme;

/// Reports whether a `{{name}}` reference resolves (accent) or not (error colour).
pub type VarCheck = Rc<dyn Fn(&str) -> bool>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorEvent {
    Changed,
    Focused,
    Blurred,
    CaretMoved,
}

#[derive(Clone)]
pub struct Options {
    pub line_numbers: bool,
    pub soft_wrap: bool,
    pub tab_size: usize,
    pub hard_tabs: bool,
    pub show_whitespace: bool,
    pub indent_guides: bool,
    pub auto_close: bool,
    pub read_only: bool,
    pub cursor_blink: bool,
    /// Pasting a valid JSON document over an empty or fully selected body pretty-prints it.
    pub format_paste: bool,
    pub placeholder: String,
}

impl Default for Options {
    fn default() -> Options {
        Options {
            line_numbers: true,
            soft_wrap: false,
            tab_size: 2,
            hard_tabs: false,
            show_whitespace: false,
            indent_guides: true,
            auto_close: true,
            read_only: false,
            cursor_blink: true,
            format_paste: false,
            placeholder: String::new(),
        }
    }
}

/// Geometry captured at paint time, used for hit-testing and scrolling.
#[derive(Clone)]
pub struct Layout {
    pub bounds: Bounds<Pixels>,
    pub cell_w: f32,
    pub line_h: f32,
    pub text_x: f32,
    pub rows_visible: usize,
    pub wrap_cols: Option<usize>,
    pub dm: Rc<DisplayMap>,
}

pub struct CompState {
    pub items: Vec<Item>,
    pub replace: Range<usize>,
    pub selected: usize,
}

pub struct CodeEditor {
    pub(crate) focus: FocusHandle,
    pub(crate) buf: Buffer,
    pub(crate) lang: Language,
    pub(crate) hl: Highlighter,
    pub(crate) hl_rev: u64,
    pub(crate) opts: Options,
    pub(crate) scroll_row: f32,
    pub(crate) scroll_x: f32,
    pub(crate) goal_col: Option<usize>,
    pub(crate) marked: Option<Range<usize>>,
    pub(crate) reveal: bool,
    pub(crate) layout: Option<Layout>,
    dm_cache: Option<(u64, usize, Option<usize>, Rc<DisplayMap>)>,
    pub(crate) caret_on: bool,
    blink_epoch: u64,
    dragging: bool,
    drag_pointer: Option<gpui::Point<Pixels>>,
    pub(crate) provider: Option<Box<dyn CompletionProvider>>,
    pub(crate) comp: Option<CompState>,
    pub(crate) var_check: Option<VarCheck>,
    /// Inline diagnostics (byte range, message), set by the owner (Tera syntax errors, M6).
    pub(crate) diagnostics: Vec<(Range<usize>, String)>,
    pub(crate) read_only_ranges: Vec<Range<usize>>,
    pub(crate) goto: Option<String>,
    pub(crate) last_error: Option<format::FormatError>,
    was_focused: bool,
    /// Set by `.line_numbers(..)`: the surface decides, settings do not override it.
    fixed_line_numbers: bool,
    wrap_toggled: bool,
}

impl EventEmitter<EditorEvent> for CodeEditor {}

impl Focusable for CodeEditor {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

const BLINK: Duration = Duration::from_millis(500);

fn utf16_len(s: &str) -> usize {
    s.chars().map(char::len_utf16).sum()
}

impl CodeEditor {
    pub fn new(lang: Language, cx: &mut Context<Self>) -> CodeEditor {
        CodeEditor {
            focus: cx.focus_handle(),
            buf: Buffer::new(""),
            lang,
            hl: Highlighter::new(lang),
            hl_rev: u64::MAX,
            opts: Options::default(),
            scroll_row: 0.0,
            scroll_x: 0.0,
            goal_col: None,
            marked: None,
            reveal: false,
            layout: None,
            dm_cache: None,
            caret_on: true,
            blink_epoch: 0,
            dragging: false,
            drag_pointer: None,
            provider: None,
            comp: None,
            var_check: None,
            diagnostics: Vec::new(),
            read_only_ranges: Vec::new(),
            goto: None,
            last_error: None,
            was_focused: false,
            fixed_line_numbers: false,
            wrap_toggled: false,
        }
    }

    // ---- builder -----------------------------------------------------------------------
    pub fn line_numbers(mut self, v: bool) -> Self {
        self.opts.line_numbers = v;
        self.fixed_line_numbers = true;
        self
    }
    pub fn soft_wrap(mut self, v: bool) -> Self {
        self.opts.soft_wrap = v;
        self
    }
    pub fn tab_size(mut self, v: usize) -> Self {
        self.opts.tab_size = v.clamp(1, 16);
        self
    }
    pub fn show_whitespace(mut self, v: bool) -> Self {
        self.opts.show_whitespace = v;
        self
    }
    pub fn indent_guides(mut self, v: bool) -> Self {
        self.opts.indent_guides = v;
        self
    }
    pub fn auto_close(mut self, v: bool) -> Self {
        self.opts.auto_close = v;
        self
    }
    pub fn read_only(mut self, v: bool) -> Self {
        self.opts.read_only = v;
        self
    }
    pub fn format_paste(mut self, v: bool) -> Self {
        self.opts.format_paste = v;
        self
    }
    pub fn placeholder(mut self, v: &str) -> Self {
        self.opts.placeholder = v.to_string();
        self
    }
    pub fn completion_provider(mut self, p: impl CompletionProvider + 'static) -> Self {
        self.provider = Some(Box::new(p));
        self
    }
    pub fn var_check(mut self, f: VarCheck) -> Self {
        self.var_check = Some(f);
        self
    }
    pub fn read_only_ranges(mut self, r: Vec<Range<usize>>) -> Self {
        self.read_only_ranges = r;
        self
    }
    pub fn options(mut self, o: Options) -> Self {
        self.opts = o;
        self
    }

    // ---- setters / getters -------------------------------------------------------------
    /// Replaces the text silently (no `Changed`), clearing history.
    pub fn set_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.buf.set_text(text);
        self.after_external(cx);
    }

    /// Replaces the text as one undo step and emits `Changed`.
    pub fn replace_all(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        if self.opts.read_only {
            return false;
        }
        let ok = self.buf.replace_all(text);
        if ok {
            self.after_edit(cx);
        }
        ok
    }

    fn after_external(&mut self, cx: &mut Context<Self>) {
        self.marked = None;
        self.comp = None;
        self.hl_rev = u64::MAX;
        cx.notify();
    }

    pub fn set_language(&mut self, lang: Language, cx: &mut Context<Self>) {
        if lang != self.lang {
            self.lang = lang;
            self.hl = Highlighter::new(lang);
            self.hl_rev = u64::MAX;
            cx.notify();
        }
    }
    pub fn set_read_only(&mut self, v: bool, cx: &mut Context<Self>) {
        if self.opts.read_only != v {
            self.opts.read_only = v;
            cx.notify();
        }
    }
    /// Applies `[editor]` settings for the current language, keeping the surface's own choices
    /// (read-only, placeholder, paste formatting, fixed line numbers).
    pub fn apply_settings(&mut self, st: &super::settings::EditorSettings, cx: &mut Context<Self>) {
        let mut o = st.options_for(self.lang);
        o.read_only = self.opts.read_only;
        o.format_paste = self.opts.format_paste;
        o.placeholder = self.opts.placeholder.clone();
        if self.fixed_line_numbers {
            o.line_numbers = self.opts.line_numbers;
        }
        // The soft-wrap toggle is a runtime choice until a settings writer exists.
        if self.wrap_toggled {
            o.soft_wrap = self.opts.soft_wrap;
        }
        if self.opts.tab_size != o.tab_size
            || self.opts.soft_wrap != o.soft_wrap
            || self.opts.show_whitespace != o.show_whitespace
            || self.opts.indent_guides != o.indent_guides
            || self.opts.line_numbers != o.line_numbers
            || self.opts.auto_close != o.auto_close
            || self.opts.hard_tabs != o.hard_tabs
            || self.opts.cursor_blink != o.cursor_blink
        {
            self.opts = o;
            cx.notify();
        }
    }

    pub fn set_options(&mut self, o: Options, cx: &mut Context<Self>) {
        self.opts = o;
        cx.notify();
    }
    pub fn set_cursor(&mut self, offset: usize, cx: &mut Context<Self>) {
        self.buf.set_selection(offset, offset);
        self.caret_moved(cx);
    }
    /// Moves the caret to a one-based line.
    pub fn go_to(&mut self, line: usize, cx: &mut Context<Self>) {
        let off = self.buf.offset_of_line(line.saturating_sub(1));
        self.set_cursor(off, cx);
    }
    pub fn toggle_soft_wrap(&mut self, cx: &mut Context<Self>) {
        self.opts.soft_wrap = !self.opts.soft_wrap;
        self.wrap_toggled = true;
        self.scroll_x = 0.0;
        self.reveal = true;
        cx.notify();
    }
    pub fn text(&self) -> &str {
        self.buf.text()
    }
    /// Replace the inline diagnostics; repaints only when they changed.
    pub fn set_diagnostics(&mut self, d: Vec<(Range<usize>, String)>, cx: &mut Context<Self>) {
        if self.diagnostics != d {
            self.diagnostics = d;
            cx.notify();
        }
    }
    pub fn diagnostics(&self) -> &[(Range<usize>, String)] {
        &self.diagnostics
    }
    pub fn revision(&self) -> u64 {
        self.buf.revision()
    }
    pub fn selection(&self) -> Range<usize> {
        self.buf.selection()
    }
    pub fn cursor(&self) -> usize {
        self.buf.head()
    }
    /// Zero-based (line, column).
    pub fn line_col(&self) -> (usize, usize) {
        self.buf.line_col(self.buf.head())
    }
    pub fn line_count(&self) -> usize {
        self.buf.line_count()
    }
    pub fn lang(&self) -> Language {
        self.lang
    }
    pub fn is_soft_wrap(&self) -> bool {
        self.opts.soft_wrap
    }
    pub fn is_read_only(&self) -> bool {
        self.opts.read_only
    }
    pub fn scroll_row(&self) -> f32 {
        self.scroll_row
    }
    pub fn last_error(&self) -> Option<&format::FormatError> {
        self.last_error.as_ref()
    }
    pub fn undo_depth(&self) -> usize {
        self.buf.undo_depth()
    }
    pub fn indent(&self) -> Indent {
        Indent {
            tab_size: self.opts.tab_size,
            hard_tabs: self.opts.hard_tabs,
        }
    }

    /// Validity verdict for the footer (`None` when the language is not validated).
    pub fn validity(&self) -> Option<Result<(), format::FormatError>> {
        format::validate(self.lang, self.buf.text())
    }

    // ---- display map and highlight -----------------------------------------------------
    pub(crate) fn display_map(&mut self, wrap: Option<usize>) -> Rc<DisplayMap> {
        let key = (self.buf.revision(), self.opts.tab_size, wrap);
        if let Some((r, t, w, dm)) = &self.dm_cache
            && (*r, *t, *w) == key
        {
            return dm.clone();
        }
        let dm = Rc::new(DisplayMap::new(self.buf.text(), self.opts.tab_size, wrap));
        self.dm_cache = Some((key.0, key.1, key.2, dm.clone()));
        dm
    }

    pub(crate) fn sync_highlight(&mut self) {
        if self.hl_rev != self.buf.revision() {
            self.hl.reparse(self.buf.text());
            self.hl_rev = self.buf.revision();
        }
    }

    // ---- editing core ------------------------------------------------------------------
    fn blocked_by_read_only(&self, r: &Range<usize>) -> bool {
        self.read_only_ranges.iter().any(|ro| {
            if r.is_empty() {
                r.start > ro.start && r.start < ro.end
            } else {
                r.start < ro.end && ro.start < r.end
            }
        })
    }

    /// Applies a plan if allowed; keeps the incremental highlighter in step.
    pub(crate) fn apply(&mut self, plan: Plan, kind: EditKind, cx: &mut Context<Self>) -> bool {
        if self.opts.read_only || self.blocked_by_read_only(&plan.range) {
            return false;
        }
        let old = (self.hl_rev == self.buf.revision()).then(|| self.buf.text().to_string());
        let range = plan.range.clone();
        let new_len = plan.text.len();
        if !self.buf.apply(plan, kind) {
            return false;
        }
        if let Some(old) = old {
            self.hl.edit(&old, self.buf.text(), range, new_len);
            self.hl_rev = self.buf.revision();
        }
        self.goal_col = None;
        self.marked = None;
        self.after_edit(cx);
        true
    }

    fn after_edit(&mut self, cx: &mut Context<Self>) {
        self.last_error = None;
        self.reveal = true;
        self.reset_blink(cx);
        self.update_completions(Trigger::Typing);
        cx.emit(EditorEvent::Changed);
        cx.emit(EditorEvent::CaretMoved);
        cx.notify();
    }

    fn caret_moved(&mut self, cx: &mut Context<Self>) {
        self.reveal = true;
        self.comp = None;
        self.reset_blink(cx);
        cx.emit(EditorEvent::CaretMoved);
        cx.notify();
    }

    fn undo_op(&mut self, redo: bool, cx: &mut Context<Self>) {
        if self.opts.read_only {
            return;
        }
        let ok = if redo {
            self.buf.redo()
        } else {
            self.buf.undo()
        };
        if ok {
            self.hl_rev = u64::MAX;
            self.goal_col = None;
            self.marked = None;
            self.after_edit(cx);
        }
    }

    fn set_sel(&mut self, anchor: usize, head: usize, keep_goal: bool, cx: &mut Context<Self>) {
        self.buf.set_selection(anchor, head);
        if !keep_goal {
            self.goal_col = None;
        }
        self.caret_moved(cx);
    }

    fn move_to(&mut self, off: usize, extend: bool, cx: &mut Context<Self>) {
        let a = if extend { self.buf.anchor() } else { off };
        self.set_sel(a, off, false, cx);
    }

    fn vertical(&mut self, down: bool, extend: bool, rows: usize, cx: &mut Context<Self>) {
        let wrap = self.layout.as_ref().and_then(|l| l.wrap_cols);
        let dm = self.display_map(wrap);
        let head = self.buf.head();
        let row = dm.row_of(head);
        let col = self
            .goal_col
            .unwrap_or_else(|| dm.col_of(self.buf.text(), head));
        let target = if down {
            (row + rows).min(dm.row_count() - 1)
        } else {
            row.saturating_sub(rows)
        };
        let off = if target == row && down {
            self.buf.len()
        } else if target == row {
            0
        } else {
            dm.offset_at(self.buf.text(), target, col)
        };
        let a = if extend { self.buf.anchor() } else { off };
        self.set_sel(a, off, true, cx);
        self.goal_col = Some(col);
    }

    fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        let mut chars = text.chars();
        let plan = match (chars.next(), chars.next()) {
            (Some(c), None) if !c.is_control() => {
                ops::type_char(&self.buf, self.lang, c, self.opts.auto_close)
            }
            _ => ops::insert_text(&self.buf, text),
        };
        let kind = if text.chars().count() == 1 {
            EditKind::Typing
        } else {
            EditKind::Other
        };
        self.apply(plan, kind, cx);
    }

    // ---- completion --------------------------------------------------------------------
    fn update_completions(&mut self, trigger: Trigger) {
        self.comp = None;
        let Some(p) = &self.provider else { return };
        let off = self.buf.head();
        if trigger == Trigger::Typing && self.buf.has_selection() {
            return;
        }
        let Some(c) = p.completions(self.buf.text(), off, trigger) else {
            return;
        };
        let query = &self.buf.text()[c.replace.clone()];
        if trigger == Trigger::Typing && query.is_empty() && !self.buf.text()[..off].ends_with("{{")
        {
            return;
        }
        let items = completion::rank(c.items, query);
        if items.is_empty() {
            return;
        }
        self.comp = Some(CompState {
            items,
            replace: c.replace,
            selected: 0,
        });
    }

    /// Moves the popup selection (wrapping); false when no popup is open.
    fn step_completion(&mut self, forward: bool, cx: &mut Context<Self>) -> bool {
        let Some(c) = self.comp.as_mut().filter(|c| !c.items.is_empty()) else {
            return false;
        };
        let n = c.items.len();
        c.selected = if forward {
            (c.selected + 1) % n
        } else {
            (c.selected + n - 1) % n
        };
        cx.notify();
        true
    }

    fn accept_completion(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(c) = self.comp.take() else {
            return false;
        };
        let Some(item) = c.items.get(c.selected) else {
            return false;
        };
        let (text, caret) = completion::expand_snippet(&item.insert);
        let at = c.replace.start;
        let plan = Plan {
            range: c.replace,
            text,
            sel: (at + caret, at + caret),
        };
        self.apply(plan, EditKind::Other, cx);
        self.comp = None;
        true
    }

    // ---- blink -------------------------------------------------------------------------
    fn reset_blink(&mut self, cx: &mut Context<Self>) {
        self.caret_on = true;
        self.blink_epoch += 1;
        if !self.opts.cursor_blink {
            return;
        }
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

    // ---- scrolling ---------------------------------------------------------------------
    pub(crate) fn clamp_scroll(&mut self) {
        if let Some(l) = &self.layout {
            let max = l.dm.row_count().saturating_sub(1) as f32;
            self.scroll_row = self.scroll_row.clamp(0.0, max);
        }
        self.scroll_x = self.scroll_x.max(0.0);
    }

    fn on_wheel(&mut self, ev: &ScrollWheelEvent, cx: &mut Context<Self>) {
        let Some(l) = &self.layout else { return };
        let d = ev.delta.pixel_delta(px(l.line_h));
        let (dy, dx) = (f32::from(d.y), f32::from(d.x));
        self.scroll_row -= dy / l.line_h;
        if !self.opts.soft_wrap {
            self.scroll_x -= dx;
        }
        self.clamp_scroll();
        cx.notify();
        cx.stop_propagation();
    }

    // ---- mouse -------------------------------------------------------------------------
    fn offset_at_point(&mut self, p: gpui::Point<Pixels>) -> Option<usize> {
        let l = self.layout.clone()?;
        let x = f32::from(p.x - l.bounds.origin.x) - l.text_x + self.scroll_x;
        let y = f32::from(p.y - l.bounds.origin.y) / l.line_h + self.scroll_row;
        let row = (y.floor().max(0.0) as usize).min(l.dm.row_count() - 1);
        let col = (x / l.cell_w).round().max(0.0) as usize;
        Some(l.dm.offset_at(self.buf.text(), row, col))
    }

    fn on_mouse_down(&mut self, ev: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus);
        let Some(off) = self.offset_at_point(ev.position) else {
            return;
        };
        self.dragging = true;
        match ev.click_count {
            2 => {
                let w = self.buf.word_at(off);
                self.set_sel(w.start, w.end, false, cx);
            }
            n if n >= 3 => {
                let s = self.buf.line_start(off);
                let e = self.buf.line_end(off);
                self.set_sel(s, (e + 1).min(self.buf.len()), false, cx);
            }
            _ => {
                let a = if ev.modifiers.shift {
                    self.buf.anchor()
                } else {
                    off
                };
                self.set_sel(a, off, false, cx);
            }
        }
    }

    fn on_mouse_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if !self.dragging || ev.pressed_button != Some(MouseButton::Left) {
            return;
        }
        self.drag_pointer = Some(ev.position);
        if let Some(l) = &self.layout {
            let (top, bottom) = (f32::from(l.bounds.top()), f32::from(l.bounds.bottom()));
            let y = f32::from(ev.position.y);
            if y < top {
                self.scroll_row -= ((top - y) / l.line_h).clamp(0.3, 3.0);
            } else if y > bottom {
                self.scroll_row += ((y - bottom) / l.line_h).clamp(0.3, 3.0);
            }
            self.clamp_scroll();
        }
        if let Some(off) = self.offset_at_point(ev.position) {
            let a = self.buf.anchor();
            self.buf.set_selection(a, off);
            self.reset_blink(cx);
            cx.emit(EditorEvent::CaretMoved);
            cx.notify();
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _cx: &mut Context<Self>) {
        self.dragging = false;
        self.drag_pointer = None;
    }

    // ---- keys --------------------------------------------------------------------------
    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let k = &ev.keystroke;
        let plain = !k.modifiers.control && !k.modifiers.alt && !k.modifiers.platform;
        if self.goto.is_some() && plain {
            self.goto_key(ev, cx);
            cx.stop_propagation();
            return;
        }
        match k.key.as_str() {
            "enter" if plain && !k.modifiers.shift => {
                if self.opts.read_only {
                    return;
                }
                // Enter, like Tab, accepts the open completion.
                if self.accept_completion(cx) {
                    cx.stop_propagation();
                    return;
                }
                let plan = ops::newline(&self.buf, self.lang, self.indent());
                self.comp = None;
                self.apply(plan, EditKind::Other, cx);
                cx.stop_propagation();
            }
            "tab" if !k.modifiers.control && !k.modifiers.platform && !k.modifiers.alt => {
                if k.modifiers.shift {
                    if let Some(p) = ops::outdent(&self.buf, self.indent()) {
                        self.apply(p, EditKind::Other, cx);
                    }
                } else if !self.accept_completion(cx) {
                    let p = ops::indent(&self.buf, self.indent());
                    self.apply(p, EditKind::Other, cx);
                }
                cx.stop_propagation();
            }
            "pageup" | "pagedown" => {
                let rows = self
                    .layout
                    .as_ref()
                    .map_or(10, |l| l.rows_visible.saturating_sub(1).max(1));
                self.vertical(k.key == "pagedown", k.modifiers.shift, rows, cx);
                self.scroll_row = (self.scroll_row
                    + if k.key == "pagedown" {
                        rows as f32
                    } else {
                        -(rows as f32)
                    })
                .max(0.0);
                self.clamp_scroll();
                cx.stop_propagation();
            }
            _ => {
                let _ = window;
            }
        }
    }

    fn goto_key(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        match key {
            "enter" => {
                if let Some(n) = self.goto.take().and_then(|s| s.parse::<usize>().ok()) {
                    self.go_to(n, cx);
                }
            }
            "escape" => self.goto = None,
            "backspace" => {
                if let Some(g) = &mut self.goto {
                    g.pop();
                }
            }
            d if d.len() == 1 && d.as_bytes()[0].is_ascii_digit() => {
                if let Some(g) = &mut self.goto
                    && g.len() < 9
                {
                    g.push_str(d);
                }
            }
            _ => {}
        }
        cx.notify();
    }

    // ---- clipboard ---------------------------------------------------------------------
    fn copy_sel(&mut self, cx: &mut Context<Self>) -> bool {
        if !self.buf.has_selection() {
            return false;
        }
        cx.write_to_clipboard(ClipboardItem::new_string(
            self.buf.selected_text().to_string(),
        ));
        true
    }

    fn do_format(&mut self, cx: &mut Context<Self>) {
        let unit = self.indent().unit();
        match format::format(self.lang, self.buf.text(), &unit) {
            Some(Ok(out)) => {
                if out != self.buf.text() {
                    self.replace_all(&out, cx);
                }
                self.last_error = None;
            }
            Some(Err(e)) => {
                self.last_error = Some(e);
                cx.notify();
            }
            None => {}
        }
    }

    /// Formats the document as one undo step; errors are kept in [`CodeEditor::last_error`].
    pub fn format_body(&mut self, cx: &mut Context<Self>) {
        self.do_format(cx);
    }
}

// ---- IME / text input -----------------------------------------------------------------
impl CodeEditor {
    fn off_from_utf16(&self, n: usize) -> usize {
        let mut u = 0;
        for (i, c) in self.buf.text().char_indices() {
            if u >= n {
                return i;
            }
            u += c.len_utf16();
        }
        self.buf.len()
    }
    fn off_to_utf16(&self, off: usize) -> usize {
        utf16_len(&self.buf.text()[..off.min(self.buf.len())])
    }
    fn range_from_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.off_from_utf16(r.start)..self.off_from_utf16(r.end)
    }
    fn range_to_utf16(&self, r: &Range<usize>) -> Range<usize> {
        self.off_to_utf16(r.start)..self.off_to_utf16(r.end)
    }
}

impl EntityInputHandler for CodeEditor {
    fn text_for_range(
        &mut self,
        range: Range<usize>,
        adjusted: &mut Option<Range<usize>>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<String> {
        let r = self.range_from_utf16(&range);
        *adjusted = Some(self.range_to_utf16(&r));
        Some(self.buf.text()[r].to_string())
    }

    fn selected_text_range(
        &mut self,
        _: bool,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.buf.selection()),
            reversed: self.buf.head() < self.buf.anchor(),
        })
    }

    fn marked_text_range(&self, _: &mut Window, _: &mut Context<Self>) -> Option<Range<usize>> {
        self.marked.as_ref().map(|m| self.range_to_utf16(m))
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
        let target = range
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked.clone());
        match target {
            Some(r) => {
                self.buf.set_selection(r.start, r.end);
                let plan = ops::insert_text(&self.buf, text);
                self.apply(plan, EditKind::Other, cx);
            }
            None => self.type_text(text, cx),
        }
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
        let target = range
            .map(|r| self.range_from_utf16(&r))
            .or_else(|| self.marked.clone())
            .unwrap_or_else(|| self.buf.selection());
        self.buf.set_selection(target.start, target.end);
        let plan = ops::insert_text(&self.buf, new_text);
        let start = target.start;
        if self.apply(plan, EditKind::Other, cx) {
            let clean = super::buffer::sanitize(new_text);
            self.marked = (!clean.is_empty()).then(|| start..start + clean.len());
            if let Some(sel) = new_selected {
                let base = self.off_to_utf16(start);
                let a = self.off_from_utf16(base + sel.start);
                let b = self.off_from_utf16(base + sel.end);
                self.buf.set_selection(a, b);
            }
        }
    }

    fn bounds_for_range(
        &mut self,
        range: Range<usize>,
        element_bounds: Bounds<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let l = self.layout.clone()?;
        let r = self.range_from_utf16(&range);
        let row = l.dm.row_of(r.start);
        let col = l.dm.col_of(self.buf.text(), r.start);
        let x = l.text_x + col as f32 * l.cell_w - self.scroll_x;
        let y = (row as f32 - self.scroll_row) * l.line_h;
        Some(Bounds::new(
            point(
                element_bounds.origin.x + px(x),
                element_bounds.origin.y + px(y),
            ),
            gpui::size(px(l.cell_w), px(l.line_h)),
        ))
    }

    fn character_index_for_point(
        &mut self,
        p: gpui::Point<Pixels>,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> Option<usize> {
        let off = self.offset_at_point(p)?;
        Some(self.off_to_utf16(off))
    }
}

// ---- render ---------------------------------------------------------------------------
impl Render for CodeEditor {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = theme(cx).clone();
        let focused = self.focus.is_focused(window);
        if focused != self.was_focused {
            self.was_focused = focused;
            cx.emit(if focused {
                EditorEvent::Focused
            } else {
                EditorEvent::Blurred
            });
            if focused {
                self.reset_blink(cx);
            }
        }
        macro_rules! on {
            ($d:expr, $ty:ty, |$s:ident, $cx:ident| $body:expr) => {
                $d.on_action(cx.listener(|$s, _: &$ty, _, $cx| $body))
            };
        }
        let mut d = div()
            .key_context("CodeEditor")
            .track_focus(&self.focus)
            .size_full()
            .relative()
            .bg(t.editor);
        d = on!(d, act::Left, |s, cx| {
            let o = if s.buf.has_selection() {
                s.buf.selection().start
            } else {
                s.buf.prev_boundary(s.buf.head())
            };
            s.move_to(o, false, cx)
        });
        d = on!(d, act::Right, |s, cx| {
            let o = if s.buf.has_selection() {
                s.buf.selection().end
            } else {
                s.buf.next_boundary(s.buf.head())
            };
            s.move_to(o, false, cx)
        });
        d = on!(d, act::SelectLeft, |s, cx| {
            let o = s.buf.prev_boundary(s.buf.head());
            s.move_to(o, true, cx)
        });
        d = on!(d, act::SelectRight, |s, cx| {
            let o = s.buf.next_boundary(s.buf.head());
            s.move_to(o, true, cx)
        });
        // Bound keys run as actions before key listeners, so the popup must claim Up/Down here
        // (this also covers Ctrl-P / Ctrl-N).
        d = on!(d, act::Up, |s, cx| if !s.step_completion(false, cx) {
            s.vertical(false, false, 1, cx)
        });
        d = on!(d, act::Down, |s, cx| if !s.step_completion(true, cx) {
            s.vertical(true, false, 1, cx)
        });
        d = on!(d, act::SelectUp, |s, cx| s.vertical(false, true, 1, cx));
        d = on!(d, act::SelectDown, |s, cx| s.vertical(true, true, 1, cx));
        d = on!(d, act::WordLeft, |s, cx| {
            let o = s.buf.prev_word(s.buf.head());
            s.move_to(o, false, cx)
        });
        d = on!(d, act::WordRight, |s, cx| {
            let o = s.buf.next_word(s.buf.head());
            s.move_to(o, false, cx)
        });
        d = on!(d, act::SelectWordLeft, |s, cx| {
            let o = s.buf.prev_word(s.buf.head());
            s.move_to(o, true, cx)
        });
        d = on!(d, act::SelectWordRight, |s, cx| {
            let o = s.buf.next_word(s.buf.head());
            s.move_to(o, true, cx)
        });
        d = on!(d, act::LineStart, |s, cx| {
            let o = s.smart_home();
            s.move_to(o, false, cx)
        });
        d = on!(d, act::SelectLineStart, |s, cx| {
            let o = s.smart_home();
            s.move_to(o, true, cx)
        });
        d = on!(d, act::LineEnd, |s, cx| {
            let o = s.buf.line_end(s.buf.head());
            s.move_to(o, false, cx)
        });
        d = on!(d, act::SelectLineEnd, |s, cx| {
            let o = s.buf.line_end(s.buf.head());
            s.move_to(o, true, cx)
        });
        d = on!(d, act::DocStart, |s, cx| s.move_to(0, false, cx));
        d = on!(d, act::SelectDocStart, |s, cx| s.move_to(0, true, cx));
        d = on!(d, act::DocEnd, |s, cx| {
            let o = s.buf.len();
            s.move_to(o, false, cx)
        });
        d = on!(d, act::SelectDocEnd, |s, cx| {
            let o = s.buf.len();
            s.move_to(o, true, cx)
        });
        d = on!(d, act::Backspace, |s, cx| {
            if let Some(p) = ops::backspace(&s.buf, s.lang, s.opts.auto_close) {
                s.apply(p, EditKind::Typing, cx);
            }
        });
        d = on!(d, act::Delete, |s, cx| {
            let sel = s.buf.selection();
            let p = if sel.is_empty() {
                ops::delete_range(sel.start, s.buf.next_boundary(sel.start))
            } else {
                ops::delete_range(sel.start, sel.end)
            };
            if p.range.start != p.range.end {
                s.apply(p, EditKind::Typing, cx);
            }
        });
        d = on!(d, act::DeleteWordLeft, |s, cx| s
            .delete_to(s.buf.prev_word(s.buf.head()), cx));
        d = on!(d, act::DeleteWordRight, |s, cx| s
            .delete_to(s.buf.next_word(s.buf.head()), cx));
        d = on!(d, act::DeleteToLineStart, |s, cx| s
            .delete_to(s.buf.line_start(s.buf.head()), cx));
        d = on!(d, act::DeleteToLineEnd, |s, cx| s
            .delete_to(s.buf.line_end(s.buf.head()), cx));
        d = on!(d, act::Copy, |s, cx| {
            s.copy_sel(cx);
        });
        d = on!(d, act::Cut, |s, cx| {
            if !s.opts.read_only && s.copy_sel(cx) {
                let r = s.buf.selection();
                s.apply(ops::delete_range(r.start, r.end), EditKind::Other, cx);
            }
        });
        d = on!(d, act::Paste, |s, cx| {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                let text = s.paste_text(text);
                let p = ops::insert_text(&s.buf, &text);
                s.apply(p, EditKind::Other, cx);
            }
        });
        d = on!(d, act::SelectAll, |s, cx| {
            let n = s.buf.len();
            s.set_sel(0, n, false, cx)
        });
        d = on!(d, crate::actions::EditCopy, |s, cx| {
            s.copy_sel(cx);
        });
        d = on!(d, crate::actions::EditCut, |s, cx| {
            if !s.opts.read_only && s.copy_sel(cx) {
                let r = s.buf.selection();
                s.apply(ops::delete_range(r.start, r.end), EditKind::Other, cx);
            }
        });
        d = on!(d, crate::actions::EditPaste, |s, cx| {
            if let Some(text) = cx.read_from_clipboard().and_then(|c| c.text()) {
                let text = s.paste_text(text);
                let p = ops::insert_text(&s.buf, &text);
                s.apply(p, EditKind::Other, cx);
            }
        });
        d = on!(d, crate::actions::EditSelectAll, |s, cx| {
            let n = s.buf.len();
            s.set_sel(0, n, false, cx)
        });
        d = on!(d, crate::actions::Undo, |s, cx| s.undo_op(false, cx));
        d = on!(d, crate::actions::Redo, |s, cx| s.undo_op(true, cx));
        d = on!(d, act::Undo, |s, cx| s.undo_op(false, cx));
        d = on!(d, act::Redo, |s, cx| s.undo_op(true, cx));
        d = on!(d, act::ToggleComment, |s, cx| {
            if let Some(p) = ops::toggle_comment(&s.buf, s.lang) {
                s.apply(p, EditKind::Other, cx);
            }
        });
        d = on!(d, act::MoveLineUp, |s, cx| {
            if let Some(p) = ops::move_lines(&s.buf, true) {
                s.apply(p, EditKind::Other, cx);
            }
        });
        d = on!(d, act::MoveLineDown, |s, cx| {
            if let Some(p) = ops::move_lines(&s.buf, false) {
                s.apply(p, EditKind::Other, cx);
            }
        });
        d = on!(d, act::DuplicateLine, |s, cx| {
            let p = ops::duplicate_lines(&s.buf);
            s.apply(p, EditKind::Other, cx);
        });
        d = on!(d, act::DeleteLine, |s, cx| {
            let p = ops::delete_lines(&s.buf);
            s.apply(p, EditKind::Other, cx);
        });
        d = on!(d, act::GoToLine, |s, cx| {
            s.goto = Some(String::new());
            cx.notify();
        });
        d = on!(d, act::MatchBracket, |s, cx| {
            if let Some((a, b)) = ops::matching_bracket(s.buf.text(), s.lang, s.buf.head()) {
                let target = if s.buf.head() == a + 1 || s.buf.head() == a {
                    b
                } else {
                    a
                };
                s.move_to(target, false, cx);
            }
        });
        d = on!(d, act::ShowCompletions, |s, cx| {
            s.update_completions(Trigger::Manual);
            cx.notify();
        });
        d = on!(d, act::ToggleSoftWrap, |s, cx| s.toggle_soft_wrap(cx));
        d = on!(d, act::FormatBody, |s, cx| s.do_format(cx));
        let esc = cx.listener(|s: &mut CodeEditor, _: &act::Escape, _, cx| {
            if s.comp.take().is_some() || s.goto.take().is_some() {
                cx.notify();
            } else {
                cx.propagate();
            }
        });
        let up_down = cx.listener(|s: &mut CodeEditor, ev: &KeyDownEvent, w, cx| {
            if let Some(c) = &mut s.comp {
                match ev.keystroke.key.as_str() {
                    "down" => {
                        c.selected = (c.selected + 1) % c.items.len();
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    "up" => {
                        c.selected = (c.selected + c.items.len() - 1) % c.items.len();
                        cx.stop_propagation();
                        cx.notify();
                        return;
                    }
                    _ => {}
                }
            }
            s.on_key_down(ev, w, cx);
        });
        let popup = self.comp.as_ref().and_then(|c| self.render_popup(c, &t));
        if self.comp.is_some() && popup.is_none() && self.layout.is_some() {
            cx.notify(); // stale layout: draw the popup again once this paint refreshed it
        }
        let goto = self.goto.as_ref().map(|g| {
            div()
                .absolute()
                .top(px(6.))
                .right(px(14.))
                .px(px(8.))
                .py(px(3.))
                .rounded(px(4.))
                .bg(t.elevated)
                .border_1()
                .border_color(t.border_focused)
                .text_size(px(12.))
                .text_color(t.text)
                .child(format!("Go to line: {g}"))
        });
        d.on_action(esc)
            .capture_key_down(up_down)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|s, ev, w, cx| s.on_mouse_down(ev, w, cx)),
            )
            .on_mouse_move(cx.listener(|s, ev, _, cx| s.on_mouse_move(ev, cx)))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|s, ev, _, cx| s.on_mouse_up(ev, cx)),
            )
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|s, ev, _, cx| s.on_mouse_up(ev, cx)),
            )
            .on_scroll_wheel(cx.listener(|s, ev, _, cx| s.on_wheel(ev, cx)))
            .child(EditorElement::new(cx.entity()))
            .children(popup)
            .children(goto)
    }
}

impl CodeEditor {
    /// Normalises pasted text; a valid JSON document replacing everything is pretty-printed.
    fn paste_text(&self, text: String) -> String {
        let text = super::buffer::sanitize(&text);
        let t = text.trim_start();
        let replaces_all =
            self.buf.text().trim().is_empty() || self.buf.selection() == (0..self.buf.len());
        if self.opts.format_paste
            && replaces_all
            && (t.starts_with('{') || t.starts_with('['))
            && format::validate_json(&text).is_ok()
        {
            return format::format_json(&text, &self.indent().unit()).unwrap_or(text);
        }
        text
    }

    fn smart_home(&self) -> usize {
        let ls = self.buf.line_start(self.buf.head());
        let le = self.buf.line_end(self.buf.head());
        let first = ls + self.buf.text()[ls..le].len() - self.buf.text()[ls..le].trim_start().len();
        if self.buf.head() == first { ls } else { first }
    }

    fn delete_to(&mut self, to: usize, cx: &mut Context<Self>) {
        let sel = self.buf.selection();
        let p = if sel.is_empty() {
            ops::delete_range(self.buf.head(), to)
        } else {
            ops::delete_range(sel.start, sel.end)
        };
        if p.range.start != p.range.end {
            self.apply(p, EditKind::Other, cx);
        }
    }

    fn render_popup(&self, c: &CompState, t: &crate::theme::Theme) -> Option<gpui::AnyElement> {
        let l = self.layout.as_ref()?;
        // The layout is rebuilt at paint; right after an edit it is one frame stale.
        if !l.dm.is_for_len(self.buf.len()) {
            return None;
        }
        let row = l.dm.row_of(self.buf.head());
        let col = l.dm.col_of(self.buf.text(), self.buf.head());
        let x = l.text_x + col as f32 * l.cell_w - self.scroll_x;
        let y = (row as f32 + 1.0 - self.scroll_row) * l.line_h;
        let rows = c.items.iter().enumerate().map(|(i, it)| {
            div()
                .px(px(8.))
                .h(px(20.))
                .flex()
                .items_center()
                .gap(px(8.))
                .text_size(px(12.))
                .text_color(t.text)
                .when_selected(i == c.selected, t.element_selected)
                .child(it.label.clone())
                .child(div().text_color(t.text_muted).child(it.group.clone()))
        });
        Some(
            div()
                .absolute()
                .left(px(x.max(0.)))
                .top(px(y))
                .min_w(px(180.))
                .py(px(2.))
                .rounded(px(4.))
                .bg(t.elevated)
                .border_1()
                .border_color(t.border)
                .shadow_md()
                .children(rows)
                .into_any_element(),
        )
    }
}

trait SelectedExt: Styled + Sized {
    fn when_selected(self, on: bool, bg: gpui::Hsla) -> Self {
        if on { self.bg(bg) } else { self }
    }
}
impl SelectedExt for gpui::Div {}

pub fn new_editor(
    cx: &mut App,
    lang: Language,
    f: impl FnOnce(CodeEditor) -> CodeEditor,
) -> Entity<CodeEditor> {
    cx.new(|cx| f(CodeEditor::new(lang, cx)))
}
