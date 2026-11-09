//! Pure edit operations. Each returns a [`Plan`] (one replacement plus the selection after it)
//! so every operation is exactly one undo step. GPUI-free.

use super::buffer::{Buffer, Plan};
use super::language_config::{Context, Language};

#[derive(Debug, Clone, Copy)]
pub struct Indent {
    pub tab_size: usize,
    pub hard_tabs: bool,
}

impl Indent {
    pub fn unit(&self) -> String {
        if self.hard_tabs {
            "\t".into()
        } else {
            " ".repeat(self.tab_size.max(1))
        }
    }
}

fn leading_ws(line: &str) -> &str {
    &line[..line.len() - line.trim_start_matches([' ', '\t']).len()]
}

fn line_of(b: &Buffer, offset: usize) -> &str {
    &b.text()[b.line_start(offset)..b.line_end(offset)]
}

/// Next character allows auto-close: end of line, whitespace, or one of `;:.,=}])>`, backtick, quote.
fn close_allowed(next: Option<char>) -> bool {
    match next {
        None => true,
        Some(c) => c.is_whitespace() || ";:.,=}])>`\"'".contains(c),
    }
}

/// Typing one character: overtype, surround, auto-close, or plain insert.
pub fn type_char(b: &Buffer, lang: Language, ch: char, auto_close: bool) -> Plan {
    let sel = b.selection();
    let cfg = lang.config();
    let s = ch.to_string();
    if auto_close {
        // Surround a selection with a pair.
        if !sel.is_empty()
            && let Some(p) = cfg.pairs.iter().find(|p| p.open == ch)
        {
            let inner = b.selected_text();
            let text = format!("{ch}{inner}{}", p.close);
            return Plan {
                range: sel.clone(),
                text,
                sel: (
                    sel.start + ch.len_utf8(),
                    sel.start + ch.len_utf8() + inner.len(),
                ),
            };
        }
        // Overtype an existing closer.
        let next = b.text()[b.head()..].chars().next();
        if sel.is_empty() && next == Some(ch) && cfg.pairs.iter().any(|p| p.close == ch) {
            let to = b.head() + ch.len_utf8();
            return Plan {
                range: to..to,
                text: String::new(),
                sel: (to, to),
            };
        }
        if sel.is_empty()
            && let Some(p) = cfg.pairs.iter().find(|p| p.open == ch)
        {
            let ctx = lang.context_at(b.text(), b.head());
            let blocked = (p.not_in_string && ctx == Context::String)
                || (p.not_in_comment && ctx == Context::Comment);
            // A quote after a word character is an apostrophe, not an opener.
            let prev = b.text()[..b.head()].chars().next_back();
            let word_before = p.open == p.close && prev.is_some_and(|c| c.is_alphanumeric());
            if !blocked && !word_before && close_allowed(next) {
                let at = b.head();
                return Plan {
                    range: at..at,
                    text: format!("{ch}{}", p.close),
                    sel: (at + ch.len_utf8(), at + ch.len_utf8()),
                };
            }
        }
    }
    Plan {
        range: sel.clone(),
        text: s,
        sel: (sel.start + ch.len_utf8(), sel.start + ch.len_utf8()),
    }
}

/// Plain text insertion (paste, IME commit) replacing the selection.
pub fn insert_text(b: &Buffer, text: &str) -> Plan {
    let sel = b.selection();
    Plan {
        range: sel.clone(),
        text: text.to_string(),
        sel: (sel.start + text.len(), sel.start + text.len()),
    }
}

/// Backspace: delete the selection, a whole empty pair, or one grapheme.
pub fn backspace(b: &Buffer, lang: Language, auto_close: bool) -> Option<Plan> {
    let sel = b.selection();
    if !sel.is_empty() {
        return Some(Plan {
            range: sel.clone(),
            text: String::new(),
            sel: (sel.start, sel.start),
        });
    }
    let at = b.head();
    if at == 0 {
        return None;
    }
    if auto_close {
        let prev = b.text()[..at].chars().next_back();
        let next = b.text()[at..].chars().next();
        if let (Some(p), Some(n)) = (prev, next)
            && lang
                .config()
                .pairs
                .iter()
                .any(|x| x.open == p && x.close == n)
        {
            let s = at - p.len_utf8();
            return Some(Plan {
                range: s..at + n.len_utf8(),
                text: String::new(),
                sel: (s, s),
            });
        }
    }
    let p = b.prev_boundary(at);
    Some(Plan {
        range: p..at,
        text: String::new(),
        sel: (p, p),
    })
}

/// Deletes the range `from..to` (delete-word, delete-to-line-start, forward delete).
pub fn delete_range(from: usize, to: usize) -> Plan {
    let (s, e) = (from.min(to), from.max(to));
    Plan {
        range: s..e,
        text: String::new(),
        sel: (s, s),
    }
}

/// Smart Enter: keep the indent, indent more after an opener, split an empty bracket pair.
pub fn newline(b: &Buffer, lang: Language, ind: Indent) -> Plan {
    let sel = b.selection();
    let line = line_of(b, sel.start);
    let upto = &b.text()[b.line_start(sel.start)..sel.start];
    let base = leading_ws(line).to_string();
    let cfg = lang.config();
    let trimmed = upto.trim_end();
    let last = trimmed.chars().next_back();
    let opens = last.is_some_and(|c| "{[(".contains(c))
        || (cfg.indent_after_colon
            && trimmed.ends_with(':')
            && lang.context_at(b.text(), sel.start) != Context::Comment);
    let next = b.text()[sel.end..].chars().next();
    let split = last.is_some_and(|c| "{[(".contains(c))
        && next.is_some_and(|n| {
            cfg.brackets
                .iter()
                .any(|&(o, cl)| Some(o) == last && cl == n)
        });
    let inner = if opens {
        format!("{base}{}", ind.unit())
    } else {
        base.clone()
    };
    if split {
        let text = format!("\n{inner}\n{base}");
        let caret = sel.start + 1 + inner.len();
        return Plan {
            range: sel,
            text,
            sel: (caret, caret),
        };
    }
    let text = format!("\n{inner}");
    let caret = sel.start + text.len();
    Plan {
        range: sel,
        text,
        sel: (caret, caret),
    }
}

/// Offsets of the line starts touched by the selection.
fn touched_lines(b: &Buffer) -> (usize, usize) {
    let sel = b.selection();
    let start = b.line_start(sel.start);
    // A selection ending at column 0 of a line does not include that line.
    let end_off = if sel.end > sel.start && b.line_start(sel.end) == sel.end {
        sel.end - 1
    } else {
        sel.end
    };
    (start, b.line_end(end_off))
}

/// Tab: insert indentation, or block-indent a multi-line selection.
pub fn indent(b: &Buffer, ind: Indent) -> Plan {
    let sel = b.selection();
    let multi = b.text()[sel.clone()].contains('\n');
    if !multi {
        let unit = if ind.hard_tabs {
            "\t".to_string()
        } else {
            let col = b.line_col(sel.start).1;
            " ".repeat(ind.tab_size - col % ind.tab_size.max(1))
        };
        let c = sel.start + unit.len();
        return Plan {
            range: sel,
            text: unit,
            sel: (c, c),
        };
    }
    let (s, e) = touched_lines(b);
    let unit = ind.unit();
    let block = &b.text()[s..e];
    let out: Vec<String> = block
        .split('\n')
        .map(|l| {
            if l.is_empty() {
                String::new()
            } else {
                format!("{unit}{l}")
            }
        })
        .collect();
    let text = out.join("\n");
    let end = s + text.len();
    Plan {
        range: s..e,
        text,
        sel: (s, end),
    }
}

/// Shift+Tab: remove one indent unit from the caret line or every touched line.
pub fn outdent(b: &Buffer, ind: Indent) -> Option<Plan> {
    let (s, e) = touched_lines(b);
    let block = &b.text()[s..e];
    let mut removed_before_caret = 0usize;
    let head = b.head();
    let mut pos = s;
    let mut head_adj = 0usize;
    let mut anchor_adj = 0usize;
    let anchor = b.anchor();
    let mut changed = false;
    let lines: Vec<String> = block
        .split('\n')
        .map(|l| {
            let n = if l.starts_with('\t') {
                1
            } else {
                l.chars()
                    .take(ind.tab_size.max(1))
                    .take_while(|&c| c == ' ')
                    .count()
            };
            if n > 0 {
                changed = true;
            }
            let line_end = pos + l.len();
            for (target, adj) in [(head, &mut head_adj), (anchor, &mut anchor_adj)] {
                if target > pos {
                    *adj += n.min(target - pos);
                }
            }
            let _ = (line_end, &mut removed_before_caret);
            pos = line_end + 1;
            l[n..].to_string()
        })
        .collect();
    if !changed {
        return None;
    }
    let text = lines.join("\n");
    Some(Plan {
        range: s..e,
        text,
        sel: (anchor - anchor_adj, head - head_adj),
    })
}

/// Toggles comments on the touched lines (line tokens) or the selection (block tokens).
pub fn toggle_comment(b: &Buffer, lang: Language) -> Option<Plan> {
    let cfg = lang.config();
    let (s, e) = touched_lines(b);
    if let Some(tok) = cfg.line_comment {
        let block = &b.text()[s..e];
        let lines: Vec<&str> = block.split('\n').collect();
        let all = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .all(|l| l.trim_start().starts_with(tok));
        let min_indent = lines
            .iter()
            .filter(|l| !l.trim().is_empty())
            .map(|l| leading_ws(l).len())
            .min()
            .unwrap_or(0);
        let out: Vec<String> = lines
            .iter()
            .map(|l| {
                if l.trim().is_empty() {
                    l.to_string()
                } else if all {
                    let ws = leading_ws(l).len();
                    let rest = &l[ws..][tok.len()..];
                    let rest = rest.strip_prefix(' ').unwrap_or(rest);
                    format!("{}{rest}", &l[..ws])
                } else {
                    format!("{}{tok} {}", &l[..min_indent], &l[min_indent..])
                }
            })
            .collect();
        let text = out.join("\n");
        let end = s + text.len();
        return Some(Plan {
            range: s..e,
            text,
            sel: (s, end),
        });
    }
    let (open, close) = cfg.block_comment?;
    let sel = b.selection();
    let (rs, re) = if sel.is_empty() {
        (s, e)
    } else {
        (sel.start, sel.end)
    };
    let inner = &b.text()[rs..re];
    let t = inner.trim();
    let text = if t.starts_with(open) && t.ends_with(close) && t.len() >= open.len() + close.len() {
        let lead = inner.len() - inner.trim_start().len();
        let body = &t[open.len()..t.len() - close.len()];
        let body = body.strip_prefix(' ').unwrap_or(body);
        let body = body.strip_suffix(' ').unwrap_or(body);
        format!(
            "{}{body}{}",
            &inner[..lead],
            &inner[inner.len() - (inner.len() - lead - t.len())..]
        )
    } else {
        let lead = inner.len() - inner.trim_start().len();
        format!(
            "{}{open} {} {close}{}",
            &inner[..lead],
            t,
            &inner[lead + t.len()..]
        )
    };
    let end = rs + text.len();
    Some(Plan {
        range: rs..re,
        text,
        sel: (rs, end),
    })
}

/// Moves the touched lines up (`up`) or down by one line.
pub fn move_lines(b: &Buffer, up: bool) -> Option<Plan> {
    let (s, e) = touched_lines(b);
    let t = b.text();
    let block = &t[s..e];
    let (head, anchor) = (b.head(), b.anchor());
    if up {
        if s == 0 {
            return None;
        }
        let ps = b.line_start(s - 1);
        let prev = &t[ps..s - 1];
        let text = format!("{block}\n{prev}");
        let d = prev.len() + 1;
        Some(Plan {
            range: ps..e,
            text,
            sel: (anchor - d, head - d),
        })
    } else {
        if e >= t.len() {
            return None;
        }
        let ne = b.line_end(e + 1);
        let next = &t[e + 1..ne];
        let text = format!("{next}\n{block}");
        let d = next.len() + 1;
        Some(Plan {
            range: s..ne,
            text,
            sel: (anchor + d, head + d),
        })
    }
}

/// Duplicates the touched lines below themselves; the selection moves to the copy.
pub fn duplicate_lines(b: &Buffer) -> Plan {
    let (s, e) = touched_lines(b);
    let block = &b.text()[s..e];
    let d = block.len() + 1;
    Plan {
        range: e..e,
        text: format!("\n{block}"),
        sel: (b.anchor() + d, b.head() + d),
    }
}

/// Deletes the touched lines including their line break.
pub fn delete_lines(b: &Buffer) -> Plan {
    let (s, e) = touched_lines(b);
    let t = b.text();
    let (rs, re) = if e < t.len() {
        (s, e + 1)
    } else if s > 0 {
        (s - 1, e)
    } else {
        (s, e)
    };
    let caret = rs.min(t.len());
    let caret = b.line_start(caret.min(t.len())).max(rs);
    Plan {
        range: rs..re,
        text: String::new(),
        sel: (caret, caret),
    }
}

/// Offset of the bracket matching the one next to `offset` (before the caret, else at it).
pub fn matching_bracket(text: &str, lang: Language, offset: usize) -> Option<(usize, usize)> {
    let cfg = lang.config();
    let cands = [
        text[..offset].char_indices().next_back(),
        text[offset..]
            .char_indices()
            .next()
            .map(|(i, c)| (offset + i, c)),
    ];
    for (pos, c) in cands.into_iter().flatten() {
        for &(o, cl) in cfg.brackets {
            let (forward, ch, other) = if c == o {
                (true, o, cl)
            } else if c == cl {
                (false, cl, o)
            } else {
                continue;
            };
            let mut depth = 0i32;
            if forward {
                for (i, x) in text[pos..].char_indices() {
                    if x == ch {
                        depth += 1;
                    } else if x == other {
                        depth -= 1;
                        if depth == 0 {
                            return Some((pos, pos + i));
                        }
                    }
                }
            } else {
                for (i, x) in text[..pos + c.len_utf8()].char_indices().rev() {
                    if x == ch {
                        depth += 1;
                    } else if x == other {
                        depth -= 1;
                        if depth == 0 {
                            return Some((i, pos));
                        }
                    }
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::code_editor::buffer::EditKind;

    const IND: Indent = Indent {
        tab_size: 2,
        hard_tabs: false,
    };

    fn buf(text: &str, a: usize, h: usize) -> Buffer {
        let mut b = Buffer::new(text);
        b.set_selection(a, h);
        b
    }
    fn run(b: &mut Buffer, p: Plan) -> String {
        b.apply(p, EditKind::Other);
        b.text().to_string()
    }

    #[test]
    fn auto_close_json_brace_and_quote() {
        let mut b = buf("", 0, 0);
        let p = type_char(&b, Language::Json, '{', true);
        assert_eq!(run(&mut b, p), "{}");
        assert_eq!(b.head(), 1);
        let p = type_char(&b, Language::Json, '"', true);
        assert_eq!(run(&mut b, p), "{\"\"}");
    }

    #[test]
    fn no_auto_close_in_string_or_before_word() {
        let b = buf("\"a\"", 2, 2);
        assert_eq!(type_char(&b, Language::Json, '{', true).text, "{");
        let b = buf("x", 0, 0);
        assert_eq!(type_char(&b, Language::Json, '[', true).text, "[");
    }

    #[test]
    fn apostrophe_after_word_not_paired() {
        let b = buf("don", 3, 3);
        assert_eq!(type_char(&b, Language::Yaml, '\'', true).text, "'");
    }

    #[test]
    fn overtype_closer() {
        let mut b = buf("{}", 1, 1);
        let p = type_char(&b, Language::Json, '}', true);
        run(&mut b, p);
        assert_eq!(b.text(), "{}");
        assert_eq!(b.head(), 2);
    }

    #[test]
    fn surround_selection() {
        let mut b = buf("abc", 0, 3);
        let p = type_char(&b, Language::Yaml, '(', true);
        assert_eq!(run(&mut b, p), "(abc)");
        assert_eq!(b.selection(), 1..4);
    }

    #[test]
    fn pair_backspace() {
        let mut b = buf("[]", 1, 1);
        let p = backspace(&b, Language::Json, true).unwrap();
        assert_eq!(run(&mut b, p), "");
    }

    #[test]
    fn enter_indents_and_splits_pair() {
        let mut b = buf("  {}", 3, 3);
        let p = newline(&b, Language::Json, IND);
        assert_eq!(run(&mut b, p), "  {\n    \n  }");
        assert_eq!(b.head(), 8);
        let mut b = buf("a:", 2, 2);
        let p = newline(&b, Language::Yaml, IND);
        assert_eq!(run(&mut b, p), "a:\n  ");
    }

    #[test]
    fn tab_and_block_indent() {
        let mut b = buf("a\nb", 1, 1);
        let p = indent(&b, IND);
        assert_eq!(run(&mut b, p), "a \nb");
        let mut b = buf("a\nb\nc", 0, 3);
        let p = indent(&b, IND);
        assert_eq!(run(&mut b, p), "  a\n  b\nc");
        let p = outdent(&b, IND).unwrap();
        assert_eq!(run(&mut b, p), "a\nb\nc");
    }

    #[test]
    fn comment_toggle_line_and_block() {
        let mut b = buf("a: 1\nb: 2", 0, 9);
        let p = toggle_comment(&b, Language::Yaml).unwrap();
        assert_eq!(run(&mut b, p), "# a: 1\n# b: 2");
        let p = toggle_comment(&b, Language::Yaml).unwrap();
        assert_eq!(run(&mut b, p), "a: 1\nb: 2");
        let mut b = buf("<a/>", 0, 4);
        let p = toggle_comment(&b, Language::Xml).unwrap();
        assert_eq!(run(&mut b, p), "<!-- <a/> -->");
        let p = toggle_comment(&b, Language::Xml).unwrap();
        assert_eq!(run(&mut b, p), "<a/>");
        assert!(toggle_comment(&b, Language::Json).is_none());
    }

    #[test]
    fn move_duplicate_delete_lines() {
        let mut b = buf("a\nb\nc", 2, 2);
        let p = move_lines(&b, true).unwrap();
        assert_eq!(run(&mut b, p), "b\na\nc");
        assert_eq!(b.head(), 0);
        let mut b = buf("a\nb\nc", 0, 0);
        let p = move_lines(&b, false).unwrap();
        assert_eq!(run(&mut b, p), "b\na\nc");
        assert_eq!(b.head(), 2);
        let mut b = buf("a\nb", 0, 0);
        let p = duplicate_lines(&b);
        assert_eq!(run(&mut b, p), "a\na\nb");
        let mut b = buf("a\nb\nc", 2, 2);
        let p = delete_lines(&b);
        assert_eq!(run(&mut b, p), "a\nc");
        let mut b = buf("a\nb", 2, 2);
        let p = delete_lines(&b);
        assert_eq!(run(&mut b, p), "a");
    }

    #[test]
    fn bracket_matching() {
        let t = "{[()]}";
        assert_eq!(matching_bracket(t, Language::Json, 1), Some((0, 5)));
        assert_eq!(matching_bracket(t, Language::Json, 6), Some((0, 5)));
        assert_eq!(matching_bracket(t, Language::Json, 3), Some((2, 3)));
        assert_eq!(matching_bracket("abc", Language::Json, 1), None);
    }
}
