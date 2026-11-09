//! Per-language editing rules: bracket pairs, indent rules, comment tokens. GPUI-free.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    Plain,
    Json,
    Yaml,
    Xml,
    Html,
    Markdown,
    Toml,
}

impl Language {
    pub fn name(self) -> &'static str {
        match self {
            Language::Plain => "text",
            Language::Json => "json",
            Language::Yaml => "yaml",
            Language::Xml => "xml",
            Language::Html => "html",
            Language::Markdown => "markdown",
            Language::Toml => "toml",
        }
    }

    pub fn from_name(s: &str) -> Option<Language> {
        Some(match s {
            "text" | "plain" => Language::Plain,
            "json" => Language::Json,
            "yaml" | "yml" => Language::Yaml,
            "xml" => Language::Xml,
            "html" => Language::Html,
            "markdown" | "md" => Language::Markdown,
            "toml" => Language::Toml,
            _ => return None,
        })
    }

    /// One helper for every surface: Content-Type first, else the first non-blank character.
    pub fn detect(content_type: &str, body: &str) -> Language {
        let ct = content_type
            .split(';')
            .next()
            .unwrap_or("")
            .trim()
            .to_ascii_lowercase();
        if ct.contains("json") {
            return Language::Json;
        }
        if ct.contains("html") {
            return Language::Html;
        }
        if ct.contains("xml") {
            return Language::Xml;
        }
        if ct.contains("yaml") || ct.contains("yml") {
            return Language::Yaml;
        }
        if ct.contains("markdown") {
            return Language::Markdown;
        }
        if ct.contains("toml") {
            return Language::Toml;
        }
        match body.trim_start().chars().next() {
            Some('{') | Some('[') => Language::Json,
            Some('<') => Language::Xml,
            _ => Language::Plain,
        }
    }
}

/// Where a position sits lexically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Context {
    Code,
    String,
    Comment,
}

#[derive(Debug, Clone, Copy)]
pub struct Pair {
    pub open: char,
    pub close: char,
    pub not_in_string: bool,
    pub not_in_comment: bool,
}

pub struct LanguageConfig {
    pub pairs: &'static [Pair],
    pub brackets: &'static [(char, char)],
    pub line_comment: Option<&'static str>,
    pub block_comment: Option<(&'static str, &'static str)>,
    pub indent_after_colon: bool,
}

const fn p(open: char, close: char, s: bool) -> Pair {
    Pair {
        open,
        close,
        not_in_string: s,
        not_in_comment: true,
    }
}

static JSON_PAIRS: [Pair; 3] = [p('[', ']', true), p('{', '}', true), p('"', '"', true)];
static JS_PAIRS: [Pair; 6] = [
    p('(', ')', true),
    p('[', ']', true),
    p('{', '}', true),
    p('"', '"', true),
    p('\'', '\'', true),
    p('`', '`', true),
];
static XML_PAIRS: [Pair; 2] = [p('"', '"', true), p('\'', '\'', true)];
static PLAIN_PAIRS: [Pair; 3] = [p('(', ')', true), p('[', ']', true), p('{', '}', true)];
static STD_BRACKETS: [(char, char); 3] = [('(', ')'), ('[', ']'), ('{', '}')];
static XML_BRACKETS: [(char, char); 4] = [('(', ')'), ('[', ']'), ('{', '}'), ('<', '>')];

impl Language {
    pub fn config(self) -> LanguageConfig {
        match self {
            Language::Json => LanguageConfig {
                pairs: &JSON_PAIRS,
                brackets: &STD_BRACKETS,
                line_comment: None,
                block_comment: None,
                indent_after_colon: false,
            },
            Language::Yaml => LanguageConfig {
                pairs: &JS_PAIRS,
                brackets: &STD_BRACKETS,
                line_comment: Some("#"),
                block_comment: None,
                indent_after_colon: true,
            },
            Language::Toml => LanguageConfig {
                pairs: &JS_PAIRS,
                brackets: &STD_BRACKETS,
                line_comment: Some("#"),
                block_comment: None,
                indent_after_colon: false,
            },
            Language::Xml | Language::Html | Language::Markdown => LanguageConfig {
                pairs: &XML_PAIRS,
                brackets: if self == Language::Markdown {
                    &STD_BRACKETS
                } else {
                    &XML_BRACKETS
                },
                line_comment: None,
                block_comment: Some(("<!--", "-->")),
                indent_after_colon: false,
            },
            Language::Plain => LanguageConfig {
                pairs: &PLAIN_PAIRS,
                brackets: &STD_BRACKETS,
                line_comment: None,
                block_comment: None,
                indent_after_colon: false,
            },
        }
    }

    /// Lexical context at byte `offset`, scanned from the start of the document.
    pub fn context_at(self, text: &str, offset: usize) -> Context {
        let cfg = self.config();
        let b = text.as_bytes();
        let mut i = 0;
        let mut ctx = Context::Code;
        let mut quote = 0u8;
        while i < offset.min(b.len()) {
            match ctx {
                Context::Code => {
                    if let Some(lc) = cfg.line_comment
                        && text[i..].starts_with(lc)
                    {
                        ctx = Context::Comment;
                        i += lc.len();
                        continue;
                    }
                    if let Some((open, _)) = cfg.block_comment
                        && text[i..].starts_with(open)
                    {
                        ctx = Context::Comment;
                        i += open.len();
                        continue;
                    }
                    let in_markup =
                        matches!(self, Language::Xml | Language::Html | Language::Markdown);
                    let is_quote = b[i] == b'"'
                        || (b[i] == b'\'' && self != Language::Json)
                        || (b[i] == b'`' && !in_markup && self != Language::Json);
                    // Quotes in markup text outside tags are prose, not strings.
                    if is_quote && (!in_markup || inside_tag(text, i)) {
                        ctx = Context::String;
                        quote = b[i];
                    }
                }
                Context::String => {
                    if b[i] == b'\\' && self != Language::Xml {
                        i += 2;
                        continue;
                    }
                    if b[i] == quote || (b[i] == b'\n' && quote != b'`') {
                        ctx = Context::Code;
                    }
                }
                Context::Comment => {
                    if let Some((_, close)) = cfg.block_comment {
                        if text[i..].starts_with(close) {
                            ctx = Context::Code;
                            i += close.len();
                            continue;
                        }
                    } else if b[i] == b'\n' {
                        ctx = Context::Code;
                    }
                }
            }
            i += 1;
        }
        ctx
    }
}

fn inside_tag(text: &str, at: usize) -> bool {
    match (text[..at].rfind('<'), text[..at].rfind('>')) {
        (Some(l), Some(g)) => l > g,
        (Some(_), None) => true,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_prefers_content_type() {
        assert_eq!(
            Language::detect("application/json; charset=utf-8", "<a/>"),
            Language::Json
        );
        assert_eq!(Language::detect("", "  [1]"), Language::Json);
        assert_eq!(Language::detect("", "<a/>"), Language::Xml);
        assert_eq!(Language::detect("", "hello"), Language::Plain);
    }

    #[test]
    fn json_string_context() {
        let t = r#"{"a": "b"}"#;
        assert_eq!(Language::Json.context_at(t, 1), Context::Code);
        assert_eq!(Language::Json.context_at(t, 3), Context::String);
        assert_eq!(Language::Json.context_at(t, 6), Context::Code);
    }

    #[test]
    fn yaml_comment_context() {
        let t = "a: 1 # note\nb: 2";
        assert_eq!(Language::Yaml.context_at(t, 9), Context::Comment);
        assert_eq!(Language::Yaml.context_at(t, 14), Context::Code);
    }

    #[test]
    fn xml_comment_context() {
        let t = "<a><!-- x --><b/></a>";
        assert_eq!(Language::Xml.context_at(t, 8), Context::Comment);
        assert_eq!(Language::Xml.context_at(t, 15), Context::Code);
    }

    #[test]
    fn pairs_by_language() {
        assert_eq!(Language::Json.config().pairs.len(), 3);
        assert!(
            Language::Xml
                .config()
                .pairs
                .iter()
                .all(|p| p.open == '"' || p.open == '\'')
        );
    }
}
