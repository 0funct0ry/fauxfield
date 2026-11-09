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
        if let Some((i, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.text.replace_range(i..self.cursor, "");
            self.cursor = i;
        }
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.text
                .replace_range(self.cursor..self.cursor + c.len_utf8(), "");
        }
    }

    pub fn left(&mut self, extend: bool) {
        if !extend && let Some((a, _)) = self.selection() {
            self.anchor = None;
            self.cursor = a;
            return;
        }
        self.begin_move(extend);
        if let Some((i, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = i;
        }
    }

    pub fn right(&mut self, extend: bool) {
        if !extend && let Some((_, b)) = self.selection() {
            self.anchor = None;
            self.cursor = b;
            return;
        }
        self.begin_move(extend);
        if let Some(c) = self.text[self.cursor..].chars().next() {
            self.cursor += c.len_utf8();
        }
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
    fn reason_phrases() {
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(299), "");
    }
}
