//! Single-line text buffer for the inline fields and the status reason phrases used by the
//! Response pane. Multi-line editing lives in `code_editor`. Pure, no GPUI.

/// A text buffer with a byte-offset cursor and an optional selection anchor (both always on a
/// char boundary). The selection is the range between `anchor` and `cursor`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TextBuf {
    pub text: String,
    pub cursor: usize,
    pub anchor: Option<usize>,
}

impl TextBuf {
    pub fn new(text: &str) -> TextBuf {
        TextBuf {
            text: text.to_string(),
            cursor: text.len(),
            anchor: None,
        }
    }

    /// The selected byte range, ordered; `None` when nothing is selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        (a != self.cursor).then(|| (a.min(self.cursor), a.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<&str> {
        self.selection().map(|(a, b)| &self.text[a..b])
    }

    /// Delete the selection, if any. Returns whether something was deleted.
    pub fn delete_selection(&mut self) -> bool {
        let sel = self.selection();
        self.anchor = None;
        match sel {
            Some((a, b)) => {
                self.text.replace_range(a..b, "");
                self.cursor = a;
                true
            }
            None => false,
        }
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(0);
        self.cursor = self.text.len();
    }

    /// The grapheme-cluster boundary before `at` (an emoji or a letter with combining marks
    /// is one step).
    pub fn prev_boundary(&self, at: usize) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        self.text[..at]
            .grapheme_indices(true)
            .next_back()
            .map_or(0, |(i, _)| i)
    }

    /// The grapheme-cluster boundary after `at`.
    pub fn next_boundary(&self, at: usize) -> usize {
        use unicode_segmentation::UnicodeSegmentation;
        self.text[at..]
            .graphemes(true)
            .next()
            .map_or(at, |g| at + g.len())
    }

    /// Start (`extend`) or drop the selection before a cursor move.
    fn begin_move(&mut self, extend: bool) {
        if extend {
            self.anchor.get_or_insert(self.cursor);
        } else {
            self.anchor = None;
        }
    }

    pub fn insert(&mut self, s: &str) {
        self.delete_selection();
        self.text.insert_str(self.cursor, s);
        self.cursor += s.len();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        let i = self.prev_boundary(self.cursor);
        if i < self.cursor {
            self.text.replace_range(i..self.cursor, "");
            self.cursor = i;
        }
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        let z = self.next_boundary(self.cursor);
        if z > self.cursor {
            self.text.replace_range(self.cursor..z, "");
        }
    }

    pub fn left(&mut self, extend: bool) {
        if !extend && let Some((a, _)) = self.selection() {
            self.anchor = None;
            self.cursor = a;
            return;
        }
        self.begin_move(extend);
        self.cursor = self.prev_boundary(self.cursor);
    }

    pub fn right(&mut self, extend: bool) {
        if !extend && let Some((_, b)) = self.selection() {
            self.anchor = None;
            self.cursor = b;
            return;
        }
        self.begin_move(extend);
        self.cursor = self.next_boundary(self.cursor);
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// Byte offset of the previous word start before `from` (skips separators first).
    pub fn word_start_before(&self, from: usize) -> usize {
        let mut i = from;
        let mut seen = false;
        for (j, c) in self.text[..from].char_indices().rev() {
            if Self::is_word(c) {
                seen = true;
            } else if seen {
                return i;
            }
            i = j;
        }
        if seen || from > 0 { i } else { 0 }
    }

    /// Byte offset of the next word end after `from` (skips separators first).
    pub fn word_end_after(&self, from: usize) -> usize {
        let mut seen = false;
        for (j, c) in self.text[from..].char_indices() {
            if Self::is_word(c) {
                seen = true;
            } else if seen {
                return from + j;
            }
        }
        self.text.len()
    }

    pub fn word_left(&mut self, extend: bool) {
        self.begin_move(extend);
        self.cursor = self.word_start_before(self.cursor);
    }

    pub fn word_right(&mut self, extend: bool) {
        self.begin_move(extend);
        self.cursor = self.word_end_after(self.cursor);
    }

    pub fn delete_word_left(&mut self) {
        if self.delete_selection() {
            return;
        }
        let a = self.word_start_before(self.cursor);
        self.text.replace_range(a..self.cursor, "");
        self.cursor = a;
    }

    pub fn delete_word_right(&mut self) {
        if self.delete_selection() {
            return;
        }
        let z = self.word_end_after(self.cursor);
        self.text.replace_range(self.cursor..z, "");
    }

    pub fn delete_to_end(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.text.truncate(self.cursor);
    }

    pub fn delete_to_start(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.text.replace_range(..self.cursor, "");
        self.cursor = 0;
    }

    /// Select the word (or separator run) around byte offset `at` (double-click).
    pub fn select_word_at(&mut self, at: usize) {
        let at = at.min(self.text.len());
        let class = |c: char| Self::is_word(c);
        let before = self.text[..at].chars().next_back().map(class);
        let after = self.text[at..].chars().next().map(class);
        let want = after.or(before).unwrap_or(true);
        let mut a = at;
        for (j, c) in self.text[..at].char_indices().rev() {
            if class(c) != want {
                break;
            }
            a = j;
        }
        let mut z = at;
        for (j, c) in self.text[at..].char_indices() {
            if class(c) != want {
                break;
            }
            z = at + j + c.len_utf8();
        }
        self.anchor = Some(a);
        self.cursor = z;
    }

    /// `(line, column in chars)` of a byte offset, both 0-based.
    pub fn line_col_of(&self, offset: usize) -> (usize, usize) {
        let before = &self.text[..offset];
        let line = before.matches('\n').count();
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        (line, before[start..].chars().count())
    }

    /// `(line, column in chars)` of the cursor.
    pub fn line_col(&self) -> (usize, usize) {
        self.line_col_of(self.cursor)
    }

    pub fn line_count(&self) -> usize {
        self.text.split('\n').count()
    }

    fn line_start(&self, line: usize) -> usize {
        if line == 0 {
            return 0;
        }
        self.text
            .match_indices('\n')
            .nth(line - 1)
            .map_or(self.text.len(), |(i, _)| i + 1)
    }

    /// Byte offset of `(line, col)`, clamped to the text and to the line's end.
    pub fn offset_of(&self, line: usize, col: usize) -> usize {
        let line = line.min(self.line_count().saturating_sub(1));
        let start = self.line_start(line);
        let rest = &self.text[start..];
        let end = rest.find('\n').unwrap_or(rest.len());
        start + rest[..end].char_indices().nth(col).map_or(end, |(i, _)| i)
    }

    /// Put the cursor at `(line, col)` (mouse click or drag).
    pub fn set_line_col(&mut self, line: usize, col: usize, extend: bool) {
        self.begin_move(extend);
        self.cursor = self.offset_of(line, col);
    }

    pub fn up(&mut self, extend: bool) {
        let (l, c) = self.line_col();
        self.begin_move(extend);
        self.cursor = if l > 0 { self.offset_of(l - 1, c) } else { 0 };
    }

    pub fn down(&mut self, extend: bool) {
        let (l, c) = self.line_col();
        self.begin_move(extend);
        self.cursor = if l + 1 < self.line_count() {
            self.offset_of(l + 1, c)
        } else {
            self.text.len()
        };
    }

    pub fn home(&mut self, extend: bool) {
        let (l, _) = self.line_col();
        self.begin_move(extend);
        self.cursor = self.offset_of(l, 0);
    }

    pub fn end(&mut self, extend: bool) {
        let (l, _) = self.line_col();
        self.begin_move(extend);
        self.cursor = self.offset_of(l, usize::MAX);
    }
}

/// Standard reason phrase for a status code; empty when unknown.
pub fn reason_phrase(code: u16) -> &'static str {
    match code {
        100 => "Continue",
        101 => "Switching Protocols",
        102 => "Processing",
        103 => "Early Hints",
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        203 => "Non-Authoritative Information",
        204 => "No Content",
        205 => "Reset Content",
        206 => "Partial Content",
        207 => "Multi-Status",
        300 => "Multiple Choices",
        301 => "Moved Permanently",
        302 => "Found",
        303 => "See Other",
        304 => "Not Modified",
        307 => "Temporary Redirect",
        308 => "Permanent Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        402 => "Payment Required",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        406 => "Not Acceptable",
        407 => "Proxy Authentication Required",
        408 => "Request Timeout",
        409 => "Conflict",
        410 => "Gone",
        411 => "Length Required",
        412 => "Precondition Failed",
        413 => "Content Too Large",
        414 => "URI Too Long",
        415 => "Unsupported Media Type",
        416 => "Range Not Satisfiable",
        417 => "Expectation Failed",
        418 => "I'm a teapot",
        421 => "Misdirected Request",
        422 => "Unprocessable Content",
        423 => "Locked",
        424 => "Failed Dependency",
        425 => "Too Early",
        426 => "Upgrade Required",
        428 => "Precondition Required",
        429 => "Too Many Requests",
        431 => "Request Header Fields Too Large",
        451 => "Unavailable For Legal Reasons",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        505 => "HTTP Version Not Supported",
        507 => "Insufficient Storage",
        508 => "Loop Detected",
        511 => "Network Authentication Required",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edits_and_navigation() {
        let mut b = TextBuf::new("ab\ncde");
        assert_eq!(b.line_col(), (1, 3));
        b.up(false);
        assert_eq!(b.line_col(), (0, 2));
        b.down(false);
        assert_eq!(b.line_col(), (1, 2));
        b.home(false);
        b.insert("é");
        b.backspace();
        b.delete();
        assert_eq!(b.text, "ab\nde");
        b.end(false);
        b.left(false);
        b.right(false);
        assert_eq!(b.cursor, b.text.len());
    }

    #[test]
    fn selection() {
        let mut b = TextBuf::new("hello\nworld");
        b.set_line_col(0, 1, false);
        b.set_line_col(1, 2, true);
        assert_eq!(b.selected_text(), Some("ello\nwo"));
        b.insert("X");
        assert_eq!(b.text, "hXrld");
        b.select_all();
        b.backspace();
        assert_eq!(b.text, "");
        let mut c = TextBuf::new("abc");
        c.home(false);
        c.right(true);
        c.right(true);
        assert_eq!(c.selected_text(), Some("ab"));
        c.right(false);
        assert_eq!((c.cursor, c.selection()), (2, None));
        // Clicks past the end of a line clamp to it.
        c.set_line_col(5, 99, false);
        assert_eq!(c.cursor, 3);
    }

    #[test]
    fn graphemes_move_and_delete_as_one() {
        // A family emoji (ZWJ sequence) and "e" + combining acute are one step each.
        let mut b = TextBuf::new("a👨\u{200d}👩\u{200d}👧e\u{301}");
        b.backspace();
        assert_eq!(b.text, "a👨\u{200d}👩\u{200d}👧");
        b.left(false);
        assert_eq!(b.cursor, 1);
        b.delete();
        assert_eq!(b.text, "a");
    }

    #[test]
    fn words() {
        let mut b = TextBuf::new("foo bar_1  baz");
        b.word_left(false);
        assert_eq!(b.cursor, 11);
        b.word_left(false);
        assert_eq!(b.cursor, 4);
        b.word_right(false);
        assert_eq!(b.cursor, 9);
        b.delete_word_left();
        assert_eq!(b.text, "foo   baz");
        b.select_word_at(1);
        assert_eq!(b.selected_text(), Some("foo"));
        b.delete_to_start();
        assert_eq!(b.text, "   baz");
        b.delete_word_right();
        assert_eq!(b.text, "");
    }

    #[test]
    fn reason_phrases() {
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(299), "");
    }
}
