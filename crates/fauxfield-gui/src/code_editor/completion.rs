//! Synchronous completion: provider trait, ordered composition, buffer-word provider. GPUI-free.

use std::collections::HashSet;

use nucleo_matcher::pattern::{AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Config, Matcher, Utf32Str};

/// Buffers above this size skip the buffer-word provider.
pub const WORD_PROVIDER_MAX: usize = 200 * 1024;
/// Rows shown in the popup.
pub const POPUP_ROWS: usize = 10;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Variable,
    Keyword,
    Word,
    Snippet,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub label: String,
    pub kind: Kind,
    pub group: String,
    pub detail: String,
    pub doc: String,
    /// Text inserted in place of the typed prefix; `$0` marks the caret position.
    pub insert: String,
}

impl Item {
    pub fn word(label: &str) -> Item {
        Item {
            label: label.into(),
            kind: Kind::Word,
            group: "Words".into(),
            detail: String::new(),
            doc: String::new(),
            insert: label.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Trigger {
    Typing,
    Manual,
}

/// Result: the byte range of the prefix being completed and the candidate items.
pub struct Completions {
    pub replace: std::ops::Range<usize>,
    pub items: Vec<Item>,
}

pub trait CompletionProvider {
    /// Returns candidates for the caret at `offset`, or `None` when this provider has no answer.
    fn completions(&self, text: &str, offset: usize, trigger: Trigger) -> Option<Completions>;
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The identifier prefix ending at `offset`.
pub fn prefix_range(text: &str, offset: usize) -> std::ops::Range<usize> {
    let start = text[..offset]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| is_word(c))
        .last()
        .map_or(offset, |(i, _)| i);
    start..offset
}

/// Distinct words in the buffer.
pub struct BufferWords;

impl CompletionProvider for BufferWords {
    fn completions(&self, text: &str, offset: usize, _t: Trigger) -> Option<Completions> {
        if text.len() > WORD_PROVIDER_MAX {
            return None;
        }
        let replace = prefix_range(text, offset);
        let mut seen = HashSet::new();
        let mut items = Vec::new();
        for w in text
            .split(|c: char| !is_word(c))
            .filter(|w| w.chars().count() >= 3)
        {
            if seen.insert(w) {
                items.push(Item::word(w));
            }
        }
        Some(Completions { replace, items })
    }
}

/// Providers in order: earlier ranks higher, labels de-duplicated.
#[derive(Default)]
pub struct Composite {
    providers: Vec<Box<dyn CompletionProvider>>,
}

impl Composite {
    pub fn new() -> Composite {
        Composite::default()
    }
    pub fn with(mut self, p: impl CompletionProvider + 'static) -> Composite {
        self.providers.push(Box::new(p));
        self
    }
}

impl CompletionProvider for Composite {
    fn completions(&self, text: &str, offset: usize, trigger: Trigger) -> Option<Completions> {
        let mut replace = None;
        let mut items: Vec<Item> = Vec::new();
        let mut seen = HashSet::new();
        for p in &self.providers {
            let Some(c) = p.completions(text, offset, trigger) else {
                continue;
            };
            replace.get_or_insert(c.replace);
            for it in c.items {
                if seen.insert(it.label.clone()) {
                    items.push(it);
                }
            }
        }
        replace.map(|replace| Completions { replace, items })
    }
}

/// Fuzzy-ranks `items` against `query`; stable for equal scores so provider order wins. The word
/// equal to the query is dropped.
pub fn rank(items: Vec<Item>, query: &str) -> Vec<Item> {
    if query.is_empty() {
        return items.into_iter().take(POPUP_ROWS).collect();
    }
    let mut m = Matcher::new(Config::DEFAULT);
    let pat = Pattern::new(
        query,
        CaseMatching::Smart,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );
    let mut buf = Vec::new();
    let mut scored: Vec<(u32, usize, Item)> = items
        .into_iter()
        .enumerate()
        .filter(|(_, it)| it.label != query)
        .filter_map(|(i, it)| {
            pat.score(Utf32Str::new(&it.label, &mut buf), &mut m)
                .map(|s| (s, i, it))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    scored
        .into_iter()
        .take(POPUP_ROWS)
        .map(|(_, _, it)| it)
        .collect()
}

/// Expands snippet `$0` in `insert`; returns the text and the caret offset within it.
pub fn expand_snippet(insert: &str) -> (String, usize) {
    match insert.find("$0") {
        Some(i) => (insert.replacen("$0", "", 1), i),
        None => (insert.to_string(), insert.len()),
    }
}

/// Variables inside `{{ }}` only.
pub struct Variables(pub Vec<String>);

impl CompletionProvider for Variables {
    fn completions(&self, text: &str, offset: usize, _t: Trigger) -> Option<Completions> {
        let open = text[..offset].rfind("{{")?;
        if text[open..offset].contains("}}") {
            return None;
        }
        let replace = prefix_range(text, offset);
        let items = self
            .0
            .iter()
            .map(|v| Item {
                label: v.clone(),
                kind: Kind::Variable,
                group: "Variables".into(),
                detail: String::new(),
                doc: String::new(),
                insert: v.clone(),
            })
            .collect();
        Some(Completions { replace, items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buffer_words_dedupe_and_skip_short() {
        let c = BufferWords
            .completions("foo bar foo ab", 14, Trigger::Typing)
            .unwrap();
        let labels: Vec<_> = c.items.iter().map(|i| i.label.as_str()).collect();
        assert_eq!(labels, ["foo", "bar"]);
    }

    #[test]
    fn large_buffers_skip_words() {
        let big = "word ".repeat(WORD_PROVIDER_MAX / 4);
        assert!(BufferWords.completions(&big, 0, Trigger::Typing).is_none());
    }

    #[test]
    fn variables_answer_only_inside_braces() {
        let v = Variables(vec!["host".into()]);
        assert!(v.completions("a ho", 4, Trigger::Typing).is_none());
        assert!(v.completions("{{ho", 4, Trigger::Typing).is_some());
        assert!(v.completions("{{x}} ho", 8, Trigger::Typing).is_none());
    }

    #[test]
    fn composite_orders_and_dedupes() {
        let c = Composite::new()
            .with(Variables(vec!["host".into()]))
            .with(BufferWords);
        let r = c.completions("{{host}} {{ho", 13, Trigger::Typing).unwrap();
        assert_eq!(r.items[0].kind, Kind::Variable);
        assert_eq!(r.items.iter().filter(|i| i.label == "host").count(), 1);
    }

    #[test]
    fn rank_fuzzy_and_limit() {
        let items: Vec<Item> = ["alpha", "beta", "albatross"]
            .iter()
            .map(|s| Item::word(s))
            .collect();
        let r = rank(items, "alb");
        assert_eq!(r[0].label, "albatross");
        let many: Vec<Item> = (0..30).map(|i| Item::word(&format!("item{i}"))).collect();
        assert_eq!(rank(many, "").len(), POPUP_ROWS);
    }

    #[test]
    fn snippet_caret() {
        assert_eq!(expand_snippet("foo($0)"), ("foo()".to_string(), 4));
    }
}
