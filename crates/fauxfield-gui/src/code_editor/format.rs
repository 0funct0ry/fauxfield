//! Pure formatting and validation. JSON keeps key order and number text and passes `{{var}}`
//! atoms through; failures report a 1-based line and column. GPUI-free.

use super::language_config::Language;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormatError {
    pub line: usize,
    pub col: usize,
    pub message: String,
}

impl std::fmt::Display for FormatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} (line {}, column {})",
            self.message, self.line, self.col
        )
    }
}

fn pos(text: &str, offset: usize) -> (usize, usize) {
    let off = offset.min(text.len());
    let ls = text[..off].rfind('\n').map_or(0, |i| i + 1);
    (
        text[..ls].matches('\n').count() + 1,
        text[ls..off].chars().count() + 1,
    )
}

fn err(text: &str, offset: usize, m: &str) -> FormatError {
    let (line, col) = pos(text, offset);
    FormatError {
        line,
        col,
        message: m.into(),
    }
}

/// Strict JSON validation with line and column.
pub fn validate_json(text: &str) -> Result<(), FormatError> {
    serde_json::from_str::<serde_json::Value>(text)
        .map(|_| ())
        .map_err(|e| FormatError {
            line: e.line().max(1),
            col: e.column().max(1),
            message: e
                .to_string()
                .split(" at line")
                .next()
                .unwrap_or("invalid JSON")
                .to_string(),
        })
}

#[derive(Debug, PartialEq)]
enum J<'a> {
    Open(char),
    Close(char),
    Comma,
    Colon,
    Atom(&'a str),
    /// A Tera `{% ... %}` statement or `{# ... #}` comment, kept whole.
    Tag(&'a str),
}

fn json_tokens(text: &str) -> Result<Vec<(usize, J<'_>)>, FormatError> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
            b'{' if text[i..].starts_with("{%") || text[i..].starts_with("{#") => {
                let close = if text[i..].starts_with("{%") {
                    "%}"
                } else {
                    "#}"
                };
                match text[i..].find(close) {
                    Some(e) => {
                        out.push((i, J::Tag(&text[i..i + e + 2])));
                        i += e + 2;
                    }
                    None => return Err(err(text, i, "unterminated tag")),
                }
            }
            b'{' if !text[i..].starts_with("{{") => {
                out.push((i, J::Open('{')));
                i += 1;
            }
            b'[' => {
                out.push((i, J::Open(c as char)));
                i += 1;
            }
            b'}' | b']' => {
                out.push((i, J::Close(c as char)));
                i += 1;
            }
            b',' => {
                out.push((i, J::Comma));
                i += 1;
            }
            b':' => {
                out.push((i, J::Colon));
                i += 1;
            }
            b'"' => {
                let s = i;
                i += 1;
                while i < b.len() && b[i] != b'"' {
                    if b[i] == b'\\' {
                        i += 1;
                    }
                    i += 1;
                }
                if i >= b.len() {
                    return Err(err(text, s, "unterminated string"));
                }
                i += 1;
                out.push((s, J::Atom(&text[s..i])));
            }
            _ => {
                let s = i;
                // `{{ var }}` atoms contain spaces and braces; take them whole.
                if text[i..].starts_with("{{") {
                    match text[i..].find("}}") {
                        Some(e) => i += e + 2,
                        None => return Err(err(text, s, "unterminated {{")),
                    }
                } else {
                    while i < b.len()
                        && !matches!(
                            b[i],
                            b' ' | b'\t'
                                | b'\n'
                                | b'\r'
                                | b','
                                | b':'
                                | b'}'
                                | b']'
                                | b'{'
                                | b'['
                                | b'"'
                        )
                    {
                        i += 1;
                    }
                }
                out.push((s, J::Atom(&text[s..i])));
            }
        }
    }
    Ok(out)
}

/// Removes all whitespace outside strings. Number text and `{{var}}` atoms are kept.
pub fn minify_json(text: &str) -> Result<String, FormatError> {
    let mut out = String::with_capacity(text.len());
    for (_, t) in json_tokens(text)? {
        match t {
            J::Open(c) | J::Close(c) => out.push(c),
            J::Comma => out.push(','),
            J::Colon => out.push(':'),
            J::Atom(a) | J::Tag(a) => out.push_str(a),
        }
    }
    Ok(out)
}

/// Re-indents JSON. `{{var}}` atoms outside strings survive; structure errors are reported.
pub fn format_json(text: &str, indent: &str) -> Result<String, FormatError> {
    let toks = json_tokens(text)?;
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let mut stack: Vec<char> = Vec::new();
    // Start a fresh line at depth `d` (no blank line when one is already started).
    let nl = |out: &mut String, d: usize| {
        let keep = out.trim_end_matches([' ', '\t']).len();
        out.truncate(keep);
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        for _ in 0..d {
            out.push_str(indent);
        }
    };
    let mut blocks: Vec<&str> = Vec::new();
    let mut i = 0;
    while i < toks.len() {
        let (off, t) = &toks[i];
        match t {
            J::Open(c) => {
                let close = if *c == '{' { '}' } else { ']' };
                stack.push(close);
                out.push(*c);
                if matches!(toks.get(i + 1), Some((_, J::Close(x))) if *x == close) {
                    out.push(close);
                    stack.pop();
                    i += 1;
                } else {
                    depth += 1;
                    nl(&mut out, depth);
                }
            }
            J::Tag(tag) => {
                let kind = tag_kind(tag);
                match kind {
                    TagKind::Open(name) => {
                        nl(&mut out, depth);
                        out.push_str(tag);
                        // `{% if x %},{% endif %}`: the comma between Items stays on one line.
                        if let (Some((_, J::Comma)), Some((_, J::Tag(t2)))) =
                            (toks.get(i + 1), toks.get(i + 2))
                            && matches!(tag_kind(t2), TagKind::Close(n) if n == name)
                        {
                            out.push(',');
                            out.push_str(t2);
                            i += 2;
                        } else {
                            blocks.push(name);
                            depth += 1;
                            nl(&mut out, depth);
                        }
                    }
                    TagKind::Middle => {
                        depth = depth.saturating_sub(1);
                        nl(&mut out, depth);
                        out.push_str(tag);
                        depth += 1;
                        nl(&mut out, depth);
                    }
                    TagKind::Close(name) => {
                        if blocks.pop() != Some(name) {
                            return Err(err(text, *off, "mismatched tag"));
                        }
                        depth = depth.saturating_sub(1);
                        nl(&mut out, depth);
                        out.push_str(tag);
                        nl(&mut out, depth);
                    }
                    TagKind::Single => {
                        nl(&mut out, depth);
                        out.push_str(tag);
                        nl(&mut out, depth);
                    }
                }
            }
            J::Close(c) => {
                if stack.pop() != Some(*c) {
                    return Err(err(text, *off, "mismatched bracket"));
                }
                depth = depth.saturating_sub(1);
                nl(&mut out, depth);
                out.push(*c);
            }
            J::Comma => {
                out.push(',');
                nl(&mut out, depth);
            }
            J::Colon => out.push_str(": "),
            J::Atom(a) => out.push_str(a),
        }
        i += 1;
    }
    if !stack.is_empty() {
        return Err(err(text, text.len(), "unclosed bracket"));
    }
    if !blocks.is_empty() {
        return Err(err(text, text.len(), "unclosed tag"));
    }
    Ok(out.trim_end().to_string())
}

enum TagKind {
    Open(&'static str),
    Middle,
    Close(&'static str),
    Single,
}

/// Block structure of a Tera tag (`{% for %}`, `{% else %}`, `{% endfor %}`, comments, `set`).
fn tag_kind(tag: &str) -> TagKind {
    if tag.starts_with("{#") {
        return TagKind::Single;
    }
    let inner = tag
        .trim_start_matches("{%")
        .trim_start_matches('-')
        .trim_start();
    let word: String = inner
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    const BLOCKS: [&str; 7] = ["for", "if", "macro", "block", "filter", "call", "raw"];
    if let Some(b) = BLOCKS.iter().find(|b| **b == word) {
        return TagKind::Open(b);
    }
    if let Some(b) = BLOCKS.iter().find(|b| word == format!("end{b}")) {
        return TagKind::Close(b);
    }
    match word.as_str() {
        "else" | "elif" => TagKind::Middle,
        _ => TagKind::Single,
    }
}

/// Tokens of XML/HTML markup: tags, comments, text runs and Tera `{% %}` / `{# #}` tags.
fn markup_tokens(text: &str) -> Result<Vec<(usize, &str)>, FormatError> {
    let mut toks: Vec<(usize, &str)> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if rest.starts_with("<!--") {
            let e = rest
                .find("-->")
                .ok_or_else(|| err(text, i, "unterminated comment"))?
                + 3;
            toks.push((i, &text[i..i + e]));
            i += e;
        } else if rest.starts_with("{%") || rest.starts_with("{#") {
            let close = if rest.starts_with("{%") { "%}" } else { "#}" };
            let e = rest
                .find(close)
                .ok_or_else(|| err(text, i, "unterminated tag"))?
                + 2;
            toks.push((i, &text[i..i + e]));
            i += e;
        } else if rest.starts_with('<') {
            let e = rest
                .find('>')
                .ok_or_else(|| err(text, i, "unterminated tag"))?
                + 1;
            toks.push((i, &text[i..i + e]));
            i += e;
        } else {
            let e = [rest.find('<'), rest.find("{%"), rest.find("{#")]
                .into_iter()
                .flatten()
                .min()
                .unwrap_or(rest.len());
            let t = rest[..e].trim();
            if !t.is_empty() {
                toks.push((i, t));
            }
            i += e;
        }
    }
    Ok(toks)
}

fn is_tera(t: &str) -> bool {
    t.starts_with("{%") || t.starts_with("{#")
}

/// Removes the whitespace between tags; text runs keep their inner whitespace. Tera tags are
/// kept as written. Meant for XML and data-like HTML (whitespace in `<pre>` is not kept).
pub fn minify_markup(text: &str) -> Result<String, FormatError> {
    Ok(markup_tokens(text)?.into_iter().map(|(_, t)| t).collect())
}

/// Re-indents XML/HTML markup: one tag per line, elements holding only text stay on one line.
/// Tera `for`/`if`/... blocks indent their body like an element.
pub fn format_markup(text: &str, indent: &str) -> Result<String, FormatError> {
    let toks = markup_tokens(text)?;
    let name = |t: &str| {
        t.trim_start_matches("</")
            .trim_start_matches('<')
            .split(|c: char| c.is_whitespace() || c == '>' || c == '/')
            .next()
            .unwrap_or("")
            .to_string()
    };
    let mut out = String::new();
    let mut depth = 0usize;
    let mut stack: Vec<String> = Vec::new();
    let mut blocks: Vec<&str> = Vec::new();
    let mut k = 0;
    while k < toks.len() {
        let (off, t) = toks[k];
        let pad = |out: &mut String, d: usize| {
            if !out.is_empty() {
                out.push('\n');
            }
            for _ in 0..d {
                out.push_str(indent);
            }
        };
        if is_tera(t) {
            match tag_kind(t) {
                TagKind::Open(n) => {
                    pad(&mut out, depth);
                    out.push_str(t);
                    blocks.push(n);
                    depth += 1;
                }
                TagKind::Middle => {
                    depth = depth.saturating_sub(1);
                    pad(&mut out, depth);
                    out.push_str(t);
                    depth += 1;
                }
                TagKind::Close(n) => {
                    if blocks.pop() != Some(n) {
                        return Err(err(text, off, "mismatched tag"));
                    }
                    depth = depth.saturating_sub(1);
                    pad(&mut out, depth);
                    out.push_str(t);
                }
                TagKind::Single => {
                    pad(&mut out, depth);
                    out.push_str(t);
                }
            }
        } else if t.starts_with("</") {
            let n = name(t);
            if stack.pop().as_deref() != Some(n.as_str()) {
                return Err(err(text, off, "mismatched closing tag"));
            }
            depth = depth.saturating_sub(1);
            pad(&mut out, depth);
            out.push_str(t);
        } else if t.starts_with('<')
            && !t.starts_with("<!")
            && !t.starts_with("<?")
            && !t.ends_with("/>")
        {
            // Opening tag: keep `<a>text</a>` on one line.
            if let (Some((_, txt)), Some((_, close))) = (toks.get(k + 1), toks.get(k + 2))
                && !txt.starts_with('<')
                && !is_tera(txt)
                && close.starts_with("</")
                && name(close) == name(t)
            {
                pad(&mut out, depth);
                out.push_str(t);
                out.push_str(txt);
                out.push_str(close);
                k += 3;
                continue;
            }
            pad(&mut out, depth);
            out.push_str(t);
            stack.push(name(t));
            depth += 1;
        } else {
            pad(&mut out, depth);
            out.push_str(t);
        }
        k += 1;
    }
    if let Some(n) = stack.last() {
        return Err(err(text, text.len(), &format!("unclosed <{n}>")));
    }
    if !blocks.is_empty() {
        return Err(err(text, text.len(), "unclosed tag"));
    }
    Ok(out)
}

/// Formats for `lang`; `None` when the language has no formatter.
pub fn format(lang: Language, text: &str, indent: &str) -> Option<Result<String, FormatError>> {
    match lang {
        Language::Json => Some(format_json(text, indent)),
        Language::Xml | Language::Html => Some(format_markup(text, indent)),
        _ => None,
    }
}

/// Footer verdict for a language: `None` when not validated.
pub fn validate(lang: Language, text: &str) -> Option<Result<(), FormatError>> {
    match lang {
        Language::Json if !text.trim().is_empty() => Some(validate_json(text)),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markup_tera_blocks_are_indented() {
        let src =
            "<items>{% for i in range(end=2) %}<item><id>{{ i }}</id></item>{% endfor %}</items>";
        assert_eq!(
            format_markup(src, "  ").unwrap(),
            "<items>\n  {% for i in range(end=2) %}\n    <item>\n      <id>{{ i }}</id>\n    </item>\n  {% endfor %}\n</items>"
        );
        assert!(format_markup("{% for i in x %}<a/>", "  ").is_err());
        assert_eq!(
            minify_markup("<a>\n  <b>x y</b>\n</a>").unwrap(),
            "<a><b>x y</b></a>"
        );
    }

    #[test]
    fn minify_drops_whitespace_and_keeps_numbers() {
        let src = "{\n  \"a\": 32.00,\n  \"b\": [1, 2],\n  \"s\": \"x y\"\n}";
        assert_eq!(
            minify_json(src).unwrap(),
            r#"{"a":32.00,"b":[1,2],"s":"x y"}"#
        );
    }

    #[test]
    fn json_preserves_order_numbers_and_vars() {
        let src = r#"{"z":1.50,"a":[1,2],"v":{{ id }},"e":{},"s":"a,{"}"#;
        let out = format_json(src, "  ").unwrap();
        assert_eq!(
            out,
            "{\n  \"z\": 1.50,\n  \"a\": [\n    1,\n    2\n  ],\n  \"v\": {{ id }},\n  \"e\": {},\n  \"s\": \"a,{\"\n}"
        );
        assert!(out.find("\"z\"").unwrap() < out.find("\"a\"").unwrap());
    }

    #[test]
    fn tera_blocks_are_indented() {
        let src = r#"[{% for i in range(end=3) %}{"id":{{ i }},"n":"x"}{% if not loop.last %},{% endif %}{% endfor %}]"#;
        let out = format_json(src, "  ").unwrap();
        assert_eq!(
            out,
            "[\n  {% for i in range(end=3) %}\n    {\n      \"id\": {{ i }},\n      \"n\": \"x\"\n    }\n    {% if not loop.last %},{% endif %}\n  {% endfor %}\n]"
        );
        // Formatting twice changes nothing.
        assert_eq!(format_json(&out, "  ").unwrap(), out);
        assert!(format_json("[{% for i in x %}1]", "  ").is_err());
        assert!(format_json("{% endif %}", "  ").is_err());
    }

    #[test]
    fn json_errors_have_line_and_col() {
        let e = format_json("{\n]", "  ").unwrap_err();
        assert_eq!((e.line, e.col), (2, 1));
        let e = validate_json("{\n  \"a\": }").unwrap_err();
        assert_eq!(e.line, 2);
    }

    #[test]
    fn markup_indents() {
        let out = format_markup("<a><b>x</b><c/></a>", "  ").unwrap();
        assert_eq!(out, "<a>\n  <b>x</b>\n  <c/>\n</a>");
        assert!(format_markup("<a><b></a>", "  ").is_err());
    }
}
