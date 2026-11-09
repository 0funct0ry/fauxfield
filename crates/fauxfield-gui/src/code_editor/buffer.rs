//! GPUI-free text buffer: grapheme-aware editing, sanitising, size cap and snapshot undo.

use std::ops::Range;
use std::time::{Duration, Instant};

use unicode_segmentation::UnicodeSegmentation;

/// Largest single insert accepted (paste or `replace_all`), in bytes.
pub const MAX_INSERT: usize = 1024 * 1024;
const MAX_UNDO_STEPS: usize = 200;
const MAX_UNDO_BYTES: usize = 1024 * 1024;
const COALESCE: Duration = Duration::from_secs(1);

/// Normalises line endings to `\n` and drops control characters other than `\n` and `\t`.
pub fn sanitize(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push('\n');
            }
            '\n' | '\t' => out.push(c),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// One edit: replace `range` with `text`, then select `sel` (anchor, head).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub range: Range<usize>,
    pub text: String,
    pub sel: (usize, usize),
}

impl Plan {
    pub fn insert_at(at: usize, text: &str, caret: usize) -> Plan {
        Plan {
            range: at..at,
            text: text.to_string(),
            sel: (caret, caret),
        }
    }
}

/// How an edit groups in the undo history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditKind {
    /// Single typed characters; coalesce within a 1 s pause.
    Typing,
    /// Anything else (paste, line ops, format); always its own step.
    Other,
}

#[derive(Clone)]
struct Snapshot {
    text: String,
    sel: (usize, usize),
}

pub struct Buffer {
    text: String,
    /// Selection anchor and caret (head), both byte offsets on grapheme boundaries.
    anchor: usize,
    head: usize,
    revision: u64,
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    last_typing: Option<Instant>,
}

impl Buffer {
    pub fn new(text: &str) -> Buffer {
        Buffer {
            text: sanitize(text),
            anchor: 0,
            head: 0,
            revision: 0,
            undo: Vec::new(),
            redo: Vec::new(),
            last_typing: None,
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }
    pub fn len(&self) -> usize {
        self.text.len()
    }
    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn head(&self) -> usize {
        self.head
    }
    pub fn anchor(&self) -> usize {
        self.anchor
    }
    pub fn selection(&self) -> Range<usize> {
        self.anchor.min(self.head)..self.anchor.max(self.head)
    }
    pub fn has_selection(&self) -> bool {
        self.anchor != self.head
    }
    pub fn selected_text(&self) -> &str {
        &self.text[self.selection()]
    }

    /// Replaces the whole text silently: clears history, selection to the start.
    pub fn set_text(&mut self, text: &str) {
        self.text = sanitize(text);
        self.anchor = 0;
        self.head = 0;
        self.undo.clear();
        self.redo.clear();
        self.last_typing = None;
        self.revision += 1;
    }

    pub fn set_selection(&mut self, anchor: usize, head: usize) {
        self.anchor = self.clamp(anchor);
        self.head = self.clamp(head);
    }

    pub fn clamp(&self, offset: usize) -> usize {
        let mut o = offset.min(self.text.len());
        while !self.text.is_char_boundary(o) {
            o -= 1;
        }
        o
    }

    // ---- grapheme and word boundaries -------------------------------------------------

    pub fn prev_boundary(&self, offset: usize) -> usize {
        self.text[..offset]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    pub fn next_boundary(&self, offset: usize) -> usize {
        self.text[offset..]
            .graphemes(true)
            .next()
            .map_or(self.text.len(), |g| offset + g.len())
    }

    pub fn prev_word(&self, offset: usize) -> usize {
        let mut o = offset;
        let mut seen_word = false;
        while o > 0 {
            let p = self.prev_boundary(o);
            let g = &self.text[p..o];
            let is_word = g.chars().any(|c| c.is_alphanumeric() || c == '_');
            if g == "\n" && !seen_word {
                return p;
            }
            if seen_word && !is_word {
                break;
            }
            if is_word {
                seen_word = true;
            }
            o = p;
        }
        o
    }

    pub fn next_word(&self, offset: usize) -> usize {
        let mut o = offset;
        let mut seen_word = false;
        while o < self.text.len() {
            let n = self.next_boundary(o);
            let g = &self.text[o..n];
            let is_word = g.chars().any(|c| c.is_alphanumeric() || c == '_');
            if g == "\n" && !seen_word {
                return n;
            }
            if seen_word && !is_word {
                break;
            }
            if is_word {
                seen_word = true;
            }
            o = n;
        }
        o
    }

    /// The word (or run of one kind of character) around `offset`, for double-click.
    pub fn word_at(&self, offset: usize) -> Range<usize> {
        let is_word = |g: &str| g.chars().any(|c| c.is_alphanumeric() || c == '_');
        let mut start = offset;
        let mut end = offset;
        let kind = self.text[offset..].graphemes(true).next().map(is_word);
        let Some(kind) =
            kind.or_else(|| self.text[..offset].graphemes(true).next_back().map(is_word))
        else {
            return offset..offset;
        };
        while start > 0 {
            let p = self.prev_boundary(start);
            if self.text[p..start] == *"\n" || is_word(&self.text[p..start]) != kind {
                break;
            }
            start = p;
        }
        while end < self.text.len() {
            let n = self.next_boundary(end);
            if self.text[end..n] == *"\n" || is_word(&self.text[end..n]) != kind {
                break;
            }
            end = n;
        }
        start..end
    }

    // ---- lines ------------------------------------------------------------------------

    pub fn line_start(&self, offset: usize) -> usize {
        self.text[..offset].rfind('\n').map_or(0, |i| i + 1)
    }
    pub fn line_end(&self, offset: usize) -> usize {
        self.text[offset..]
            .find('\n')
            .map_or(self.text.len(), |i| offset + i)
    }
    pub fn line_count(&self) -> usize {
        self.text.bytes().filter(|&b| b == b'\n').count() + 1
    }
    /// Zero-based (line, column in graphemes).
    pub fn line_col(&self, offset: usize) -> (usize, usize) {
        let ls = self.line_start(offset);
        let line = self.text[..ls].bytes().filter(|&b| b == b'\n').count();
        (line, self.text[ls..offset].graphemes(true).count())
    }
    /// Offset of the start of zero-based `line` (clamped to the last line).
    pub fn offset_of_line(&self, line: usize) -> usize {
        let mut off = 0;
        for _ in 0..line {
            match self.text[off..].find('\n') {
                Some(i) => off += i + 1,
                None => break,
            }
        }
        off
    }
    pub fn offset_of(&self, line: usize, col: usize) -> usize {
        let ls = self.offset_of_line(line);
        let le = self.line_end(ls);
        self.text[ls..le]
            .grapheme_indices(true)
            .nth(col)
            .map_or(le, |(i, _)| ls + i)
    }

    // ---- editing ----------------------------------------------------------------------

    /// Applies a plan as one undo step (typing may coalesce). Returns false when rejected.
    pub fn apply(&mut self, plan: Plan, kind: EditKind) -> bool {
        if plan.text.len() > MAX_INSERT
            || plan.range.end > self.text.len()
            || plan.range.start > plan.range.end
            || !self.text.is_char_boundary(plan.range.start)
            || !self.text.is_char_boundary(plan.range.end)
        {
            return false;
        }
        let text = sanitize(&plan.text);
        let now = Instant::now();
        let coalesce = kind == EditKind::Typing
            && self
                .last_typing
                .is_some_and(|t| now.duration_since(t) < COALESCE)
            && !self.undo.is_empty();
        if !coalesce {
            self.push_undo();
        }
        self.last_typing = (kind == EditKind::Typing).then_some(now);
        self.redo.clear();
        self.text.replace_range(plan.range.clone(), &text);
        // Selection offsets refer to the text after the edit; sanitising can shorten it.
        self.anchor = self.clamp(plan.sel.0);
        self.head = self.clamp(plan.sel.1);
        self.revision += 1;
        true
    }

    /// Replaces the whole text as one undo step (selection to the start).
    pub fn replace_all(&mut self, text: &str) -> bool {
        let len = self.text.len();
        self.apply(
            Plan {
                range: 0..len,
                text: text.to_string(),
                sel: (0, 0),
            },
            EditKind::Other,
        )
    }

    fn push_undo(&mut self) {
        self.undo.push(Snapshot {
            text: self.text.clone(),
            sel: (self.anchor, self.head),
        });
        let mut bytes: usize = self.undo.iter().map(|s| s.text.len()).sum();
        while self.undo.len() > MAX_UNDO_STEPS || (bytes > MAX_UNDO_BYTES && self.undo.len() > 1) {
            bytes -= self.undo.remove(0).text.len();
        }
    }

    pub fn undo(&mut self) -> bool {
        let Some(s) = self.undo.pop() else {
            return false;
        };
        self.redo.push(Snapshot {
            text: std::mem::take(&mut self.text),
            sel: (self.anchor, self.head),
        });
        self.restore(s);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(s) = self.redo.pop() else {
            return false;
        };
        self.undo.push(Snapshot {
            text: std::mem::take(&mut self.text),
            sel: (self.anchor, self.head),
        });
        self.restore(s);
        true
    }

    fn restore(&mut self, s: Snapshot) {
        self.text = s.text;
        self.anchor = self.clamp(s.sel.0);
        self.head = self.clamp(s.sel.1);
        self.last_typing = None;
        self.revision += 1;
    }

    pub fn undo_depth(&self) -> usize {
        self.undo.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(b: &mut Buffer, s: &str) {
        let at = b.head();
        b.apply(Plan::insert_at(at, s, at + s.len()), EditKind::Typing);
    }

    #[test]
    fn sanitize_crlf_and_controls() {
        assert_eq!(sanitize("a\r\nb\rc\u{0}d\te"), "a\nb\ncd\te");
    }

    #[test]
    fn graphemes_move_whole_clusters() {
        let b = Buffer::new("a👨‍👩‍👧b");
        let n = b.next_boundary(1);
        assert_eq!(&b.text()[1..n], "👨‍👩‍👧");
        assert_eq!(b.prev_boundary(n), 1);
    }

    #[test]
    fn typing_coalesces_and_undoes_in_one_step() {
        let mut b = Buffer::new("");
        typed(&mut b, "a");
        typed(&mut b, "b");
        typed(&mut b, "c");
        assert_eq!(b.undo_depth(), 1);
        assert!(b.undo());
        assert_eq!(b.text(), "");
        assert!(b.redo());
        assert_eq!(b.text(), "abc");
    }

    #[test]
    fn other_edits_break_groups() {
        let mut b = Buffer::new("");
        typed(&mut b, "a");
        b.apply(Plan::insert_at(1, "PASTE", 6), EditKind::Other);
        typed(&mut b, "z");
        assert_eq!(b.undo_depth(), 3);
    }

    #[test]
    fn insert_cap_rejected() {
        let mut b = Buffer::new("x");
        let big = "a".repeat(MAX_INSERT + 1);
        assert!(!b.replace_all(&big));
        assert_eq!(b.text(), "x");
    }

    #[test]
    fn set_text_clears_history() {
        let mut b = Buffer::new("");
        typed(&mut b, "a");
        b.set_text("new");
        assert!(!b.undo());
    }

    #[test]
    fn undo_step_limit() {
        let mut b = Buffer::new("");
        for i in 0..300 {
            b.apply(Plan::insert_at(b.head(), "x", i + 1), EditKind::Other);
        }
        assert_eq!(b.undo_depth(), 200);
    }

    #[test]
    fn line_col_and_offsets() {
        let b = Buffer::new("ab\ncé\n");
        assert_eq!(b.line_col(4), (1, 1));
        assert_eq!(b.offset_of(1, 1), 4);
        assert_eq!(b.offset_of(1, 99), 6);
        assert_eq!(b.line_count(), 3);
    }

    #[test]
    fn word_motion() {
        let b = Buffer::new("foo bar_baz  qux");
        assert_eq!(b.next_word(0), 3);
        assert_eq!(b.next_word(3), 11);
        assert_eq!(b.prev_word(11), 4);
        assert_eq!(b.word_at(5), 4..11);
    }
}
