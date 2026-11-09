//! Text buffer with selection, light syntax highlighting, and the Response header catalog used
//! by the Response pane (body editor, inline fields, header-name autocomplete). Pure, no GPUI.

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

    fn char_before(&self) -> Option<char> {
        self.text[..self.cursor].chars().next_back()
    }

    fn char_after(&self) -> Option<char> {
        self.text[self.cursor..].chars().next()
    }

    /// Indentation of the line the cursor is on.
    fn line_indent(&self) -> String {
        let (l, _) = self.line_col();
        self.text
            .split('\n')
            .nth(l)
            .unwrap_or("")
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect()
    }

    /// True when only whitespace sits between the line start and the cursor.
    fn only_indent_before(&self) -> bool {
        let before = &self.text[..self.cursor];
        let start = before.rfind('\n').map_or(0, |i| i + 1);
        before[start..].chars().all(|c| c == ' ' || c == '\t')
    }

    /// Enter: keeps the indent; for JSON also indents after `{`/`[` and, between a bracket
    /// pair, opens an empty indented line with the closer on its own line below.
    pub fn newline_for(&mut self, lang: Lang) {
        if lang != Lang::Json {
            return self.newline();
        }
        self.delete_selection();
        let indent = self.line_indent();
        let inner = format!("{indent}{INDENT}");
        match (self.char_before(), self.char_after()) {
            (Some('{'), Some('}')) | (Some('['), Some(']')) => {
                self.insert(&format!("\n{inner}\n{indent}"));
                self.cursor -= indent.len() + 1;
            }
            (Some('{' | '['), _) => self.insert(&format!("\n{inner}")),
            _ => self.insert(&format!("\n{indent}")),
        }
    }

    /// Typing one character with language-aware help (JSON): auto-close `{ [ "`, step over a
    /// closer that is already there, and pull a lone `}`/`]` back one indent level.
    pub fn type_for(&mut self, lang: Lang, ch: &str) {
        let mut it = ch.chars();
        let (Some(c), None) = (it.next(), it.next()) else {
            return self.insert(ch);
        };
        if lang != Lang::Json {
            return self.insert(ch);
        }
        let next = self.char_after();
        let prev = self.char_before();
        let has_sel = self.selection().is_some();
        match c {
            '}' | ']' if !has_sel && next == Some(c) => self.right(false),
            '"' if !has_sel && next == Some('"') && prev != Some('\\') => self.right(false),
            '{' | '[' | '"' if !has_sel => {
                let closer = match c {
                    '{' => '}',
                    '[' => ']',
                    _ => '"',
                };
                let ok_next = next
                    .is_none_or(|n| n.is_whitespace() || matches!(n, '}' | ']' | ',' | ':' | ')'));
                let ok_prev =
                    c != '"' || prev.is_none_or(|p| !p.is_alphanumeric() && p != '\\' && p != '"');
                if ok_next && ok_prev {
                    self.insert(&format!("{c}{closer}"));
                    self.left(false);
                } else {
                    self.insert(ch);
                }
            }
            '}' | ']' if !has_sel && self.only_indent_before() => {
                // Dedent one level, then type the bracket.
                let before = &self.text[..self.cursor];
                let start = before.rfind('\n').map_or(0, |i| i + 1);
                let cut = if before[start..].ends_with(INDENT) {
                    INDENT.len()
                } else {
                    before[start..]
                        .chars()
                        .next_back()
                        .map_or(0, char::len_utf8)
                };
                self.text.replace_range(self.cursor - cut..self.cursor, "");
                self.cursor -= cut;
                self.insert(ch);
            }
            _ => self.insert(ch),
        }
    }

    /// Backspace that also removes the closer of an empty auto-closed pair.
    pub fn backspace_for(&mut self, lang: Lang) {
        if lang == Lang::Json
            && self.selection().is_none()
            && matches!(
                (self.char_before(), self.char_after()),
                (Some('{'), Some('}')) | (Some('['), Some(']')) | (Some('"'), Some('"'))
            )
        {
            self.delete();
        }
        self.backspace();
    }

    /// Insert a newline keeping the current line's indentation.
    pub fn newline(&mut self) {
        self.delete_selection();
        let (l, _) = self.line_col();
        let line = self.text.split('\n').nth(l).unwrap_or("");
        let indent: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        self.insert(&format!("\n{indent}"));
    }
}

/// One indent level in the body editor.
pub const INDENT: &str = "  ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lang {
    Json,
    Yaml,
    /// XML and HTML share one tokenizer.
    Xml,
    Plain,
}

impl Lang {
    pub fn label(self) -> &'static str {
        match self {
            Lang::Json => "JSON",
            Lang::Yaml => "YAML",
            Lang::Xml => "XML/HTML",
            Lang::Plain => "Plain text",
        }
    }
}

/// Highlighting language from the content type, else from the body shape.
pub fn lang_for(content_type: &str, body: &str) -> Lang {
    let ct = content_type.to_ascii_lowercase();
    if ct.contains("json") {
        return Lang::Json;
    }
    if ct.contains("yaml") || ct.contains("yml") {
        return Lang::Yaml;
    }
    if ct.contains("xml") || ct.contains("html") {
        return Lang::Xml;
    }
    if ct.is_empty() {
        let t = body.trim_start();
        if (t.starts_with('{') || t.starts_with('[')) && validate_json(body).is_ok() {
            return Lang::Json;
        }
        if t.starts_with('<') {
            return Lang::Xml;
        }
    }
    Lang::Plain
}

/// Language that drives typing help. Like [`lang_for`], but a Response with no content type
/// whose body is empty or starts with `{`/`[` is treated as JSON while it is being typed.
pub fn editing_lang(content_type: &str, body: &str) -> Lang {
    let l = lang_for(content_type, body);
    let t = body.trim_start();
    if l == Lang::Plain && content_type.is_empty() && (t.is_empty() || t.starts_with(['{', '['])) {
        Lang::Json
    } else {
        l
    }
}

/// `Ok` when the body parses as JSON; the error carries `line:col message`.
pub fn validate_json(body: &str) -> Result<(), String> {
    serde_json::from_str::<serde_json::Value>(body)
        .map(|_| ())
        .map_err(|e| format!("{}:{} {}", e.line(), e.column(), e))
}

/// Pretty-print JSON with two-space indentation.
pub fn format_json(body: &str) -> Result<String, String> {
    let v: serde_json::Value = serde_json::from_str(body).map_err(|e| e.to_string())?;
    serde_json::to_string_pretty(&v).map_err(|e| e.to_string())
}

/// True when saving this into a Response with no content type should set `application/json`.
pub fn looks_like_json_document(body: &str) -> bool {
    let t = body.trim_start();
    (t.starts_with('{') || t.starts_with('[')) && validate_json(body).is_ok()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tok {
    Key,
    Str,
    Num,
    Keyword,
    Template,
    Comment,
    Tag,
    Attr,
    Plain,
}

/// Split one line into coloured spans for `lang`.
pub fn tokenize(lang: Lang, line: &str) -> Vec<(Tok, String)> {
    match lang {
        Lang::Json => tokenize_json(line),
        Lang::Yaml => tokenize_yaml(line),
        Lang::Xml => tokenize_xml(line),
        Lang::Plain => tokenize_templates(line),
    }
}

struct Spans(Vec<(Tok, String)>);

impl Spans {
    fn push(&mut self, t: Tok, s: String) {
        if s.is_empty() {
            return;
        }
        match self.0.last_mut() {
            Some((lt, ls)) if *lt == t && t == Tok::Plain => ls.push_str(&s),
            _ => self.0.push((t, s)),
        }
    }
}

/// `{{ … }}` / `{% … %}` span starting at `i`, if any; returns its end (exclusive).
fn template_at(c: &[char], i: usize) -> Option<usize> {
    let close = match (c.get(i), c.get(i + 1)) {
        (Some('{'), Some('{')) => '}',
        (Some('{'), Some('%')) => '%',
        _ => return None,
    };
    let mut j = i + 2;
    while j < c.len() && !(c[j] == close && c.get(j + 1) == Some(&'}')) {
        j += 1;
    }
    Some((j + 2).min(c.len()))
}

fn quoted(c: &[char], i: usize) -> usize {
    let q = c[i];
    let mut j = i + 1;
    while j < c.len() && c[j] != q {
        if c[j] == '\\' {
            j += 1;
        }
        j += 1;
    }
    (j + 1).min(c.len())
}

fn number_at(c: &[char], i: usize) -> Option<usize> {
    let ch = c[i];
    if !(ch.is_ascii_digit() || (ch == '-' && c.get(i + 1).is_some_and(char::is_ascii_digit))) {
        return None;
    }
    let mut j = i + 1;
    while j < c.len() && (c[j].is_ascii_digit() || matches!(c[j], '.' | 'e' | 'E' | '+' | '-')) {
        j += 1;
    }
    Some(j)
}

fn tokenize_templates(line: &str) -> Vec<(Tok, String)> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Spans(Vec::new());
    let mut i = 0;
    while i < c.len() {
        if let Some(end) = template_at(&c, i) {
            out.push(Tok::Template, c[i..end].iter().collect());
            i = end;
        } else {
            out.push(Tok::Plain, c[i].to_string());
            i += 1;
        }
    }
    out.0
}

/// JSON line spans. `{{ … }}` template spans (M6) stand out.
pub fn tokenize_json(line: &str) -> Vec<(Tok, String)> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Spans(Vec::new());
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        if let Some(end) = template_at(&c, i) {
            out.push(Tok::Template, c[i..end].iter().collect());
            i = end;
        } else if ch == '"' {
            let end = quoted(&c, i);
            let mut k = end;
            while k < c.len() && c[k] == ' ' {
                k += 1;
            }
            let t = if c.get(k) == Some(&':') {
                Tok::Key
            } else {
                Tok::Str
            };
            out.push(t, c[i..end].iter().collect());
            i = end;
        } else if let Some(end) = number_at(&c, i) {
            out.push(Tok::Num, c[i..end].iter().collect());
            i = end;
        } else if ch.is_ascii_alphabetic() {
            let mut j = i;
            while j < c.len() && c[j].is_ascii_alphabetic() {
                j += 1;
            }
            let w: String = c[i..j].iter().collect();
            let t = if matches!(w.as_str(), "true" | "false" | "null") {
                Tok::Keyword
            } else {
                Tok::Plain
            };
            out.push(t, w);
            i = j;
        } else {
            out.push(Tok::Plain, ch.to_string());
            i += 1;
        }
    }
    out.0
}

/// YAML line spans: `key:`, quoted strings, scalars, `# comments`, templates.
pub fn tokenize_yaml(line: &str) -> Vec<(Tok, String)> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Spans(Vec::new());
    let mut i = 0;
    // Leading indentation and list markers.
    while i < c.len() && (c[i] == ' ' || c[i] == '-' || c[i] == '\t') {
        out.push(Tok::Plain, c[i].to_string());
        i += 1;
    }
    // `key:` at the start of the content.
    let mut j = i;
    while j < c.len() && c[j] != ':' && c[j] != '#' && c[j] != '"' && c[j] != '\'' {
        j += 1;
    }
    if j > i && c.get(j) == Some(&':') && c.get(j + 1).is_none_or(|x| *x == ' ') {
        out.push(Tok::Key, c[i..j].iter().collect());
        i = j;
    }
    while i < c.len() {
        let ch = c[i];
        if ch == '#' && (i == 0 || c[i - 1] == ' ') {
            out.push(Tok::Comment, c[i..].iter().collect());
            break;
        } else if let Some(end) = template_at(&c, i) {
            out.push(Tok::Template, c[i..end].iter().collect());
            i = end;
        } else if ch == '"' || ch == '\'' {
            let end = quoted(&c, i);
            out.push(Tok::Str, c[i..end].iter().collect());
            i = end;
        } else if let Some(end) = number_at(&c, i).filter(|_| i == 0 || !c[i - 1].is_alphanumeric())
        {
            out.push(Tok::Num, c[i..end].iter().collect());
            i = end;
        } else if ch.is_ascii_alphabetic() {
            let mut j = i;
            while j < c.len() && (c[j].is_ascii_alphanumeric() || c[j] == '_') {
                j += 1;
            }
            let w: String = c[i..j].iter().collect();
            let t = if matches!(
                w.as_str(),
                "true" | "false" | "null" | "yes" | "no" | "on" | "off"
            ) {
                Tok::Keyword
            } else {
                Tok::Str
            };
            out.push(t, w);
            i = j;
        } else {
            out.push(Tok::Plain, ch.to_string());
            i += 1;
        }
    }
    out.0
}

/// XML/HTML line spans: tags, attributes, attribute values, `<!-- comments -->`, templates.
pub fn tokenize_xml(line: &str) -> Vec<(Tok, String)> {
    let c: Vec<char> = line.chars().collect();
    let mut out = Spans(Vec::new());
    let mut i = 0;
    let mut in_tag = false;
    while i < c.len() {
        let ch = c[i];
        if !in_tag && c[i..].starts_with(&['<', '!', '-', '-']) {
            let mut j = i + 4;
            while j < c.len() && !c[j..].starts_with(&['-', '-', '>']) {
                j += 1;
            }
            let end = (j + 3).min(c.len());
            out.push(Tok::Comment, c[i..end].iter().collect());
            i = end;
        } else if let Some(end) = template_at(&c, i) {
            out.push(Tok::Template, c[i..end].iter().collect());
            i = end;
        } else if !in_tag && ch == '<' {
            let mut j = i + 1;
            while j < c.len() && matches!(c[j], '/' | '?' | '!') {
                j += 1;
            }
            while j < c.len() && (c[j].is_alphanumeric() || matches!(c[j], '-' | '_' | ':' | '.')) {
                j += 1;
            }
            out.push(Tok::Tag, c[i..j].iter().collect());
            in_tag = true;
            i = j;
        } else if in_tag && (ch == '>' || (ch == '/' && c.get(i + 1) == Some(&'>'))) {
            let end = if ch == '/' { i + 2 } else { i + 1 };
            out.push(Tok::Tag, c[i..end].iter().collect());
            in_tag = false;
            i = end;
        } else if in_tag && (ch == '"' || ch == '\'') {
            let end = quoted(&c, i);
            out.push(Tok::Str, c[i..end].iter().collect());
            i = end;
        } else if in_tag && (ch.is_alphabetic() || ch == '_') {
            let mut j = i;
            while j < c.len() && (c[j].is_alphanumeric() || matches!(c[j], '-' | '_' | ':')) {
                j += 1;
            }
            out.push(Tok::Attr, c[i..j].iter().collect());
            i = j;
        } else {
            out.push(Tok::Plain, ch.to_string());
            i += 1;
        }
    }
    out.0
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
        b.newline();
        assert!(b.text.ends_with('\n'));
        let mut c = TextBuf::new("  x");
        c.newline();
        assert_eq!(c.text, "  x\n  ");
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
    fn smart_typing_for_json() {
        let j = Lang::Json;
        // `{` + Enter opens an indented line with the closer below, cursor inside.
        let mut b = TextBuf::default();
        b.type_for(j, "{");
        assert_eq!(b.text, "{}");
        b.newline_for(j);
        assert_eq!(b.text, "{\n  \n}");
        assert_eq!(b.line_col(), (1, 2));
        // Quotes auto-close and step over.
        b.type_for(j, "\"");
        b.insert("a");
        b.type_for(j, "\"");
        assert_eq!(b.text, "{\n  \"a\"\n}");
        b.insert(": 1,");
        b.newline_for(j);
        // Same indent continues; typing `}` on an indent-only line dedents.
        assert!(b.text.ends_with(",\n  \n}"));
        b.insert("]"); // plain char: no pair needed
        let mut c = TextBuf::new("{\n  \"a\": 1\n  ");
        c.type_for(j, "}");
        assert_eq!(c.text, "{\n  \"a\": 1\n}");
        // Backspace removes an empty pair; non-JSON gets no help.
        let mut d = TextBuf::default();
        d.type_for(j, "[");
        d.backspace_for(j);
        assert_eq!(d.text, "");
        let mut e = TextBuf::default();
        e.type_for(Lang::Plain, "{");
        assert_eq!(e.text, "{");
        // A quote after a letter, or before a letter, does not pair.
        let mut f = TextBuf::new("ab");
        f.type_for(j, "\"");
        assert_eq!(f.text, "ab\"");
        // Inside an existing word the opener does not pair.
        let mut g = TextBuf::new("xy");
        g.cursor = 1;
        g.type_for(j, "{");
        assert_eq!(g.text, "x{y");
    }

    #[test]
    fn json_helpers() {
        assert!(validate_json("{\"a\":1}").is_ok());
        assert!(validate_json("{\"a\":").unwrap_err().starts_with("1:"));
        assert_eq!(
            format_json("{\"a\":[1]}").unwrap(),
            "{\n  \"a\": [\n    1\n  ]\n}"
        );
        assert_eq!(lang_for("", "[1]"), Lang::Json);
        assert_eq!(lang_for("text/plain", "[1]"), Lang::Plain);
        assert_eq!(lang_for("application/x-yaml", ""), Lang::Yaml);
        assert_eq!(lang_for("text/html", ""), Lang::Xml);
        assert_eq!(lang_for("", "<a/>"), Lang::Xml);
        assert!(looks_like_json_document(" {\"a\":1}"));
        assert!(!looks_like_json_document("hello"));
        let t = tokenize_json("  \"id\": \"{{ x }}\", 4, true");
        assert!(t.iter().any(|(k, s)| *k == Tok::Key && s == "\"id\""));
        assert!(t.iter().any(|(k, _)| *k == Tok::Num));
        assert!(t.iter().any(|(k, _)| *k == Tok::Keyword));
    }

    #[test]
    fn yaml_and_xml_tokens() {
        let y = tokenize_yaml("  - name: \"Ada\" # first");
        assert!(y.iter().any(|(k, s)| *k == Tok::Key && s == "name"));
        assert!(y.iter().any(|(k, _)| *k == Tok::Str));
        assert!(y.iter().any(|(k, s)| *k == Tok::Comment && s == "# first"));
        let x = tokenize_xml("<user id=\"1\">{{ name }}</user><!-- c -->");
        assert!(x.iter().any(|(k, s)| *k == Tok::Tag && s == "<user"));
        assert!(x.iter().any(|(k, s)| *k == Tok::Attr && s == "id"));
        assert!(x.iter().any(|(k, s)| *k == Tok::Str && s == "\"1\""));
        assert!(x.iter().any(|(k, _)| *k == Tok::Template));
        assert!(x.iter().any(|(k, _)| *k == Tok::Comment));
        // Every char survives tokenisation.
        for line in [
            "  - name: \"Ada\" # first",
            "<a b='c'/>",
            "{\"k\": [1, -2.5e3]}",
        ] {
            for lang in [Lang::Json, Lang::Yaml, Lang::Xml, Lang::Plain] {
                let joined: String = tokenize(lang, line).into_iter().map(|(_, s)| s).collect();
                assert_eq!(joined, line);
            }
        }
    }

    #[test]
    fn reason_phrases() {
        assert_eq!(reason_phrase(404), "Not Found");
        assert_eq!(reason_phrase(299), "");
    }
}
