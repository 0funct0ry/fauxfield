//! tree-sitter highlighting with incremental re-parse and visible-range spans. GPUI-free.
//! Languages without a grammar here (HTML, TOML, plain text) produce only `{{var}}` spans.

use std::ops::Range;

use tree_sitter::{InputEdit, Node, Parser, Point, Tree};

use super::language_config::Language;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tok {
    Key,
    String,
    Number,
    Constant,
    Comment,
    Tag,
    Attribute,
    Heading,
    /// A `{{var}}` reference; the span covers the braces.
    Var,
    /// A Tera `{% ... %}` statement.
    Stmt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Span {
    pub range: Range<usize>,
    pub tok: Tok,
}

fn grammar(lang: Language) -> Option<tree_sitter::Language> {
    Some(match lang {
        Language::Json => tree_sitter_json::LANGUAGE.into(),
        Language::Yaml => tree_sitter_yaml::LANGUAGE.into(),
        Language::Xml => tree_sitter_xml::LANGUAGE_XML.into(),
        Language::Markdown => tree_sitter_md::LANGUAGE.into(),
        Language::Html | Language::Toml | Language::Plain => return None,
    })
}

pub struct Highlighter {
    lang: Language,
    parser: Option<Parser>,
    tree: Option<Tree>,
}

fn point(text: &str, offset: usize) -> Point {
    let ls = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    Point {
        row: text[..ls].bytes().filter(|&b| b == b'\n').count(),
        column: offset - ls,
    }
}

impl Highlighter {
    pub fn new(lang: Language) -> Highlighter {
        let parser = grammar(lang).and_then(|g| {
            let mut p = Parser::new();
            p.set_language(&g).ok()?;
            Some(p)
        });
        Highlighter {
            lang,
            parser,
            tree: None,
        }
    }

    pub fn language(&self) -> Language {
        self.lang
    }

    /// Full parse of `text`, discarding any old tree.
    pub fn reparse(&mut self, text: &str) {
        self.tree = self.parser.as_mut().and_then(|p| p.parse(text, None));
    }

    /// Incremental: `old` is the text before the edit, replaced range `range` by `new_len` bytes.
    pub fn edit(&mut self, old: &str, new: &str, range: Range<usize>, new_len: usize) {
        let (Some(parser), Some(tree)) = (self.parser.as_mut(), self.tree.as_mut()) else {
            return self.reparse(new);
        };
        let new_end = range.start + new_len;
        tree.edit(&InputEdit {
            start_byte: range.start,
            old_end_byte: range.end,
            new_end_byte: new_end,
            start_position: point(old, range.start),
            old_end_position: point(old, range.end),
            new_end_position: point(new, new_end),
        });
        self.tree = parser.parse(new, Some(tree));
    }

    /// Spans intersecting `visible`, sorted by start, non-overlapping except `Var` which wins.
    pub fn spans(&self, text: &str, visible: Range<usize>) -> Vec<Span> {
        let mut out = Vec::new();
        if let Some(tree) = &self.tree {
            walk(tree.root_node(), &visible, self.lang, &mut out);
        }
        out.sort_by_key(|s| s.range.start);
        let vars = var_spans(text, &visible);
        if vars.is_empty() {
            return out;
        }
        let mut merged: Vec<Span> = Vec::with_capacity(out.len() + vars.len());
        for s in out {
            if !vars
                .iter()
                .any(|v| v.range.start < s.range.end && s.range.start < v.range.end)
            {
                merged.push(s);
            }
        }
        merged.extend(vars);
        merged.sort_by_key(|s| s.range.start);
        merged
    }
}

/// Tera spans on lines within `visible`: `{{ expr }}` (Var), `{% stmt %}` (Stmt), `{# note #}` (Comment).
pub fn var_spans(text: &str, visible: &Range<usize>) -> Vec<Span> {
    let start = text[..visible.start.min(text.len())]
        .rfind('\n')
        .map_or(0, |i| i + 1);
    let end = visible.end.min(text.len());
    let mut out = Vec::new();
    let mut i = start;
    while i < end {
        let Some(o) = text[i..end].find('{') else {
            break;
        };
        let s = i + o;
        let (close, tok) = match text[s..].get(..2) {
            Some("{{") => ("}}", Tok::Var),
            Some("{%") => ("%}", Tok::Stmt),
            Some("{#") => ("#}", Tok::Comment),
            _ => {
                i = s + 1;
                continue;
            }
        };
        let Some(c) = text[s + 2..].find(close) else {
            break;
        };
        let e = s + 2 + c + 2;
        if tok != Tok::Comment && text[s..e].contains('\n') && tok == Tok::Var {
            i = s + 2;
            continue;
        }
        out.push(Span { range: s..e, tok });
        i = e;
    }
    out
}

/// The trimmed name inside a `{{ ... }}` span's text (first path segment, filters dropped).
pub fn var_name(span_text: &str) -> &str {
    let inner = span_text
        .trim_start_matches("{{")
        .trim_end_matches("}}")
        .trim_matches(['-', ' ']);
    let end = inner.find(['|', ' ', '.', '[', '(']).unwrap_or(inner.len());
    &inner[..end]
}

fn classify(node: Node, lang: Language) -> Option<Tok> {
    let k = node.kind();
    if k.contains("comment") || k == "Comment" {
        return Some(Tok::Comment);
    }
    match lang {
        Language::Json => match k {
            "string" | "string_content" | "escape_sequence" => {
                let is_key = node.parent().is_some_and(|p| {
                    p.kind() == "pair"
                        && p.child_by_field_name("key")
                            .is_some_and(|c| c.id() == node.id())
                });
                Some(if is_key { Tok::Key } else { Tok::String })
            }
            "number" => Some(Tok::Number),
            "true" | "false" | "null" => Some(Tok::Constant),
            _ => None,
        },
        Language::Yaml => match k {
            "double_quote_scalar" | "single_quote_scalar" | "string_scalar" | "block_scalar" => {
                let mut anc = node.parent();
                let mut key = false;
                for _ in 0..4 {
                    let Some(p) = anc else { break };
                    if p.kind().ends_with("mapping_pair") {
                        key = p.child_by_field_name("key").is_some_and(|c| {
                            c.start_byte() <= node.start_byte() && c.end_byte() >= node.end_byte()
                        });
                        break;
                    }
                    anc = p.parent();
                }
                Some(if key { Tok::Key } else { Tok::String })
            }
            "integer_scalar" | "float_scalar" => Some(Tok::Number),
            "boolean_scalar" | "null_scalar" => Some(Tok::Constant),
            "anchor" | "alias" | "tag" => Some(Tok::Attribute),
            _ => None,
        },
        Language::Xml => match k {
            "Name" => {
                let attr = node
                    .parent()
                    .is_some_and(|p| p.kind().contains("Attribute"));
                Some(if attr { Tok::Attribute } else { Tok::Tag })
            }
            "AttValue" | "CDStart" | "CData" | "CharData" if k != "CharData" => Some(Tok::String),
            _ => None,
        },
        Language::Markdown => match k {
            "atx_heading" | "setext_heading" => Some(Tok::Heading),
            "fenced_code_block" | "code_span" | "indented_code_block" => Some(Tok::String),
            _ => None,
        },
        _ => None,
    }
}

fn walk(node: Node, vis: &Range<usize>, lang: Language, out: &mut Vec<Span>) {
    if node.end_byte() <= vis.start || node.start_byte() >= vis.end {
        return;
    }
    // Markdown headings and code are whole-node tokens; everything else is leaf-driven.
    if let Some(tok) = classify(node, lang) {
        let leafish = node.child_count() == 0
            || matches!(tok, Tok::Heading | Tok::Comment | Tok::String | Tok::Key)
            || (lang == Language::Json && node.kind() == "string");
        if leafish {
            out.push(Span {
                range: node.start_byte()..node.end_byte(),
                tok,
            });
            return;
        }
    }
    let mut c = node.walk();
    for child in node.children(&mut c) {
        walk(child, vis, lang, out);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(lang: Language, text: &str) -> Vec<(String, Tok)> {
        let mut h = Highlighter::new(lang);
        h.reparse(text);
        h.spans(text, 0..text.len())
            .into_iter()
            .map(|s| (text[s.range].to_string(), s.tok))
            .collect()
    }

    #[test]
    fn json_keys_strings_numbers() {
        let t = toks(
            Language::Json,
            r#"{"a": "b", "n": 12, "t": true, "z": null}"#,
        );
        assert!(t.contains(&("\"a\"".into(), Tok::Key)));
        assert!(t.contains(&("\"b\"".into(), Tok::String)));
        assert!(t.contains(&("12".into(), Tok::Number)));
        assert!(t.contains(&("true".into(), Tok::Constant)));
        assert!(t.contains(&("null".into(), Tok::Constant)));
    }

    #[test]
    fn yaml_scalars_and_comments() {
        let t = toks(Language::Yaml, "a: 1\nb: true # hi\nc: \"s\"\n");
        assert!(t.iter().any(|(s, k)| s == "a" && *k == Tok::Key), "{t:?}");
        assert!(t.iter().any(|(s, k)| s == "1" && *k == Tok::Number));
        assert!(t.iter().any(|(s, k)| s == "true" && *k == Tok::Constant));
        assert!(t.iter().any(|(_, k)| *k == Tok::Comment));
        assert!(
            t.iter().any(|(s, k)| s == "\"s\"" && *k == Tok::String),
            "{t:?}"
        );
    }

    #[test]
    fn xml_tags_and_comments() {
        let t = toks(Language::Xml, "<a x=\"1\"><!-- c --><b/></a>");
        assert!(t.iter().any(|(s, k)| s == "a" && *k == Tok::Tag), "{t:?}");
        assert!(t.iter().any(|(_, k)| *k == Tok::Comment), "{t:?}");
    }

    #[test]
    fn markdown_heading() {
        let t = toks(Language::Markdown, "# Title\n\ntext\n");
        assert!(t.iter().any(|(_, k)| *k == Tok::Heading), "{t:?}");
    }

    #[test]
    fn vars_override_and_plain_has_only_vars() {
        let t = toks(Language::Plain, "hello {{ user.name }} bye");
        assert_eq!(t, vec![("{{ user.name }}".to_string(), Tok::Var)]);
        assert_eq!(var_name("{{ user.name | upper }}"), "user");
        let t = toks(Language::Json, r#"{"a": "{{x}}"}"#);
        assert!(t.iter().any(|(s, k)| s == "{{x}}" && *k == Tok::Var));
    }

    #[test]
    fn incremental_edit_matches_full_parse() {
        let old = r#"{"a": 1}"#;
        let new = r#"{"a": 12}"#;
        let mut h = Highlighter::new(Language::Json);
        h.reparse(old);
        h.edit(old, new, 7..7, 1);
        let inc = h.spans(new, 0..new.len());
        let mut f = Highlighter::new(Language::Json);
        f.reparse(new);
        assert_eq!(inc, f.spans(new, 0..new.len()));
    }

    #[test]
    fn visible_range_limits_spans() {
        let text = "{\n\"a\": 1,\n\"b\": 2\n}";
        let mut h = Highlighter::new(Language::Json);
        h.reparse(text);
        let s = h.spans(text, 0..9);
        assert!(!s.is_empty());
        assert!(!s.iter().any(|s| &text[s.range.clone()] == "\"b\""));
    }
}
