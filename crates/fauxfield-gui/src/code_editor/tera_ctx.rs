//! Tera awareness for the editor: built-in names, autocomplete (context keys, functions, filters,
//! fake kinds, variables) and syntax diagnostics. GPUI-free.

use std::ops::Range;

use fauxfield_engine::template::{self, FAKE_KINDS, FILTERS, FUNCTIONS};

use super::completion::{CompletionProvider, Completions, Item, Kind, Trigger, prefix_range};

/// Top-level context names and keywords never flagged as unresolved variables.
const ROOTS: &[&str] = &[
    "request", "vars", "env", "server", "data", "state", "response", "loop", "true", "false", "if",
    "else", "elif", "endif", "for", "endfor", "in", "set", "not", "and", "or", "block", "endblock",
    "raw", "endraw", "include", "macro", "endmacro", "range", "get_env", "now",
];

const REQUEST_KEYS: &[&str] = &[
    "request.method",
    "request.path",
    "request.url",
    "request.params",
    "request.query",
    "request.headers",
    "request.cookies",
    "request.body",
    "request.json",
    "request.form",
    "request.ip",
    "request.id",
];
const OTHER_KEYS: &[&str] = &[
    "server.name",
    "server.port",
    "server.base_url",
    "env",
    "state.call",
    "vars",
    "data",
];

pub fn is_builtin(name: &str) -> bool {
    ROOTS.contains(&name) || FUNCTIONS.iter().any(|(f, _)| *f == name)
}

/// 1-based `line`/`col` from a Tera error to a byte range (one character wide, or to end of line).
pub fn error_range(text: &str, line: usize, col: usize) -> Range<usize> {
    let mut start = 0;
    for (i, l) in text.split_inclusive('\n').enumerate() {
        if i + 1 == line {
            let off = l
                .char_indices()
                .nth(col.saturating_sub(1))
                .map_or(l.trim_end_matches('\n').len(), |(b, _)| b);
            let a = start + off;
            let b = (start + l.trim_end_matches('\n').len()).max(a);
            return a..b.max((a + 1).min(text.len())).min(text.len().max(a));
        }
        start += l.len();
    }
    let n = text.len();
    n.saturating_sub(1)..n
}

/// Syntax diagnostics: `(range, message)` for the first Tera error, if any.
pub fn diagnostics(text: &str) -> Vec<(Range<usize>, String)> {
    match template::check_syntax(text) {
        Ok(()) => vec![],
        Err(e) => {
            let range = match (e.line, e.col) {
                (Some(l), Some(c)) => error_range(text, l, c),
                _ => 0..text.len().min(1),
            };
            let msg = e
                .message
                .lines()
                .rev()
                .find(|l| l.trim_start().starts_with("= "))
                .map(|l| l.trim_start().trim_start_matches("= ").to_string())
                .unwrap_or(e.message);
            vec![(range, msg)]
        }
    }
}

/// Completions inside `{{ }}` and `{% %}`.
pub struct TeraCompletions {
    pub variables: Vec<String>,
}

fn item(label: &str, kind: Kind, group: &str, detail: &str, insert: String) -> Item {
    Item {
        label: label.into(),
        kind,
        group: group.into(),
        detail: detail.into(),
        doc: String::new(),
        insert,
    }
}

impl CompletionProvider for TeraCompletions {
    fn completions(&self, text: &str, offset: usize, _t: Trigger) -> Option<Completions> {
        let open = text[..offset]
            .rfind("{{")
            .into_iter()
            .chain(text[..offset].rfind("{%"))
            .max()?;
        let close_kind = if text[open..].starts_with("{{") {
            "}}"
        } else {
            "%}"
        };
        if text[open..offset].contains(close_kind) {
            return None;
        }
        let before = &text[open + 2..offset];
        // Inside `fake(kind="` complete kinds.
        if let Some(i) = before.rfind("kind=") {
            let q = &before[i + 5..];
            if q.starts_with(['"', '\'']) && !q[1..].contains(['"', '\'']) {
                let start = offset - (q.len() - 1);
                let items = FAKE_KINDS
                    .iter()
                    .map(|(k, d)| item(k, Kind::Keyword, "Fake kinds", d, (*k).to_string()))
                    .collect();
                return Some(Completions {
                    replace: start..offset,
                    items,
                });
            }
        }
        let replace = {
            let r = prefix_range(text, offset);
            // Include dotted paths such as `request.pa`.
            let mut s = r.start;
            while s > open + 2 && text[..s].ends_with('.') {
                let seg = text[..s - 1]
                    .rfind(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
                    .map_or(0, |i| i + 1);
                s = seg.max(open + 2);
                if s == seg {
                    break;
                }
            }
            s..r.end
        };
        let after_pipe = text[open + 2..replace.start].trim_end().ends_with('|');
        let mut items: Vec<Item> = Vec::new();
        if after_pipe {
            for (f, d) in FILTERS {
                items.push(item(f, Kind::Keyword, "Filters", d, (*f).to_string()));
            }
            for f in [
                "upper", "lower", "trim", "length", "default", "date", "round", "first", "last",
                "join",
            ] {
                items.push(item(f, Kind::Keyword, "Filters", "built-in", f.to_string()));
            }
        } else {
            for k in REQUEST_KEYS.iter().chain(OTHER_KEYS) {
                items.push(item(k, Kind::Variable, "Context", "", (*k).to_string()));
            }
            for v in &self.variables {
                items.push(item(
                    &format!("vars.{v}"),
                    Kind::Variable,
                    "Variables",
                    "",
                    format!("vars.{v}"),
                ));
            }
            // Shorthand: `{{ fake.email }}`; `uuid`, `ulid` and `now` also work without `()`.
            for (k, d) in FAKE_KINDS {
                let last = k.rsplit('.').next().unwrap_or(k);
                let short = if template::resolve_kind(last) == Some(*k) {
                    last
                } else {
                    k
                };
                let label = format!("fake.{short}");
                items.push(item(&label, Kind::Variable, "Fake data", d, label.clone()));
            }
            for f in ["uuid", "ulid", "now"] {
                items.push(item(
                    f,
                    Kind::Variable,
                    "Shorthand",
                    "no parentheses needed",
                    f.into(),
                ));
            }
            for (f, d) in FUNCTIONS {
                items.push(item(f, Kind::Snippet, "Functions", d, format!("{f}($0)")));
            }
            if close_kind == "%}" {
                for k in ["if", "elif", "else", "endif", "for", "endfor", "set"] {
                    items.push(item(k, Kind::Keyword, "Statements", "", k.to_string()));
                }
            }
        }
        Some(Completions { replace, items })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(text: &str) -> Vec<String> {
        let off = text.len();
        TeraCompletions {
            variables: vec!["host".into()],
        }
        .completions(text, off, Trigger::Typing)
        .map(|c| c.items.into_iter().map(|i| i.label).collect())
        .unwrap_or_default()
    }

    #[test]
    fn only_inside_tags() {
        assert!(labels("plain re").is_empty());
        assert!(labels("{{ x }} re").is_empty());
        assert!(labels("{{ re").contains(&"request.method".to_string()));
        assert!(labels("{{ ho").contains(&"vars.host".to_string()));
        assert!(labels("{{ fa").contains(&"fake.email".to_string()));
        assert!(
            labels("{{ fa").contains(&"fake.address.postcode".to_string())
                || labels("{{ fa").contains(&"fake.postcode".to_string())
        );
    }

    #[test]
    fn fake_kinds_and_filters() {
        assert!(labels("{{ fake(kind=\"na").contains(&"name.first".to_string()));
        assert!(labels("{{ x | sl").contains(&"slugify".to_string()));
        assert!(labels("{% if ").contains(&"endif".to_string()));
    }

    #[test]
    fn diagnostics_locate_errors() {
        let d = diagnostics("ok\n{{ foo( }}");
        assert_eq!(d.len(), 1);
        assert!(d[0].0.start >= 3, "{:?}", d[0]);
        assert!(diagnostics("{{ request.method }}").is_empty());
    }

    #[test]
    fn builtins() {
        assert!(is_builtin("request") && is_builtin("fake") && !is_builtin("nope"));
    }
}
