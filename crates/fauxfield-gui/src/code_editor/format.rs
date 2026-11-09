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
}

fn json_tokens(text: &str) -> Result<Vec<(usize, J<'_>)>, FormatError> {
    let b = text.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let c = b[i];
        match c {
            b' ' | b'\t' | b'\n' | b'\r' => i += 1,
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

/// Re-indents JSON. `{{var}}` atoms outside strings survive; structure errors are reported.
pub fn format_json(text: &str, indent: &str) -> Result<String, FormatError> {
    let toks = json_tokens(text)?;
    let mut out = String::with_capacity(text.len() * 2);
    let mut depth = 0usize;
    let mut stack: Vec<char> = Vec::new();
    let nl = |out: &mut String, d: usize| {
        out.push('\n');
        for _ in 0..d {
            out.push_str(indent);
        }
    };
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
            J::Close(c) => {
                if stack.pop() != Some(*c) {
                    return Err(err(text, *off, "mismatched bracket"));
                }
                depth -= 1;
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
    Ok(out)
}

/// Re-indents XML/HTML markup: one tag per line, elements holding only text stay on one line.
pub fn format_markup(text: &str, indent: &str) -> Result<String, FormatError> {
    // Tokenise into tags and text runs.
    let mut toks: Vec<(usize, &str)> = Vec::new();
    let mut i = 0;
    while i < text.len() {
        if text[i..].starts_with("<!--") {
            let e = text[i..]
                .find("-->")
                .ok_or_else(|| err(text, i, "unterminated comment"))?
                + 3;
            toks.push((i, &text[i..i + e]));
            i += e;
        } else if text[i..].starts_with('<') {
            let e = text[i..]
                .find('>')
                .ok_or_else(|| err(text, i, "unterminated tag"))?
                + 1;
            toks.push((i, &text[i..i + e]));
            i += e;
        } else {
            let e = text[i..].find('<').unwrap_or(text.len() - i);
            let t = text[i..i + e].trim();
            if !t.is_empty() {
                toks.push((i, t));
            }
            i += e;
        }
    }
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
        if t.starts_with("</") {
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
