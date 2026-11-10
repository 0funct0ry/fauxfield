//! Template shorthand, rewritten to plain Tera before compiling:
//!
//! * `{{ fake.name }}`, `{{ fake.email }}`, `{{ fake.address.city }}` → `fake(kind='…')`
//! * `{{ uuid }}`, `{{ ulid }}`, `{{ now }}` → `uuid()`, `ulid()`, `now()`
//!
//! Only expressions inside `{{ }}` and `{% %}` change, never string literals, comments or
//! `{% raw %}` blocks. Rewrites stay on their line, so error line numbers are unchanged.

use std::borrow::Cow;

/// Functions that can be written without parentheses (they need no arguments).
const BARE_FUNCTIONS: &[&str] = &["uuid", "ulid", "now"];

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// True when the template defines its own variable `name` (`set`, `for`), which wins over the
/// shorthand.
fn shadowed(src: &str, name: &str) -> bool {
    ["set", "set_global", "for"].iter().any(|kw| {
        src.match_indices(&format!("{kw} {name}")).any(|(i, m)| {
            src[i + m.len()..]
                .chars()
                .next()
                .is_none_or(|c| !is_ident(c))
        })
    })
}

pub fn desugar(src: &str) -> Cow<'_, str> {
    if !src.contains("{{") && !src.contains("{%") {
        return Cow::Borrowed(src);
    }
    let bare: Vec<&str> = BARE_FUNCTIONS
        .iter()
        .copied()
        .filter(|n| !shadowed(src, n))
        .collect();
    let mut out = String::with_capacity(src.len() + 16);
    let mut rest = src;
    let mut raw = false;
    let mut changed = false;
    while let Some(i) = rest
        .find("{{")
        .into_iter()
        .chain(rest.find("{%"))
        .chain(rest.find("{#"))
        .min()
    {
        out.push_str(&rest[..i]);
        let close = match &rest[i..i + 2] {
            "{{" => "}}",
            "{%" => "%}",
            _ => "#}",
        };
        let Some(len) = rest[i + 2..].find(close) else {
            out.push_str(&rest[i..]);
            return finish(src, out, changed);
        };
        let end = i + 2 + len + 2;
        let tag = &rest[i..end];
        let body = &tag[2..tag.len() - 2];
        let words: Vec<&str> = body
            .trim_matches(|c: char| c == '-' || c == '~' || c.is_whitespace())
            .split_whitespace()
            .collect();
        if raw {
            raw = !(close == "%}" && words.first() == Some(&"endraw"));
            out.push_str(tag);
        } else if close == "#}" {
            out.push_str(tag);
        } else {
            raw = close == "%}" && words.first() == Some(&"raw");
            let new = rewrite(body, &bare);
            changed |= new != body;
            out.push_str(&tag[..2]);
            out.push_str(&new);
            out.push_str(&tag[tag.len() - 2..]);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    finish(src, out, changed)
}

fn finish(src: &str, out: String, changed: bool) -> Cow<'_, str> {
    if changed {
        Cow::Owned(out)
    } else {
        Cow::Borrowed(src)
    }
}

/// Rewrites the expression text of one tag.
fn rewrite(body: &str, bare: &[&str]) -> String {
    let chars: Vec<(usize, char)> = body.char_indices().collect();
    let mut out = String::with_capacity(body.len() + 8);
    let mut i = 0;
    let mut prev: Option<char> = None;
    while i < chars.len() {
        let (b, c) = chars[i];
        if c == '"' || c == '\'' || c == '`' {
            // Copy a string literal untouched.
            let mut j = i + 1;
            while j < chars.len() && chars[j].1 != c {
                j += 1;
            }
            let end = chars.get(j + 1).map_or(body.len(), |x| x.0);
            out.push_str(&body[b..end]);
            prev = Some(c);
            i = j + 1;
            continue;
        }
        if is_ident(c) && !prev.is_some_and(|p| is_ident(p) || p == '.') {
            let mut j = i;
            while j < chars.len() && (is_ident(chars[j].1) || chars[j].1 == '.') {
                j += 1;
            }
            let end = chars.get(j).map_or(body.len(), |x| x.0);
            let tok = body[b..end].trim_end_matches('.');
            let tok_end = b + tok.len();
            let next = body[tok_end..].chars().find(|c| !c.is_whitespace());
            let calls = body[tok_end..].starts_with('(');
            if let Some(kind) = tok.strip_prefix("fake.")
                && !calls
                && !kind.is_empty()
                && !kind.split('.').any(str::is_empty)
            {
                out.push_str(&format!("fake(kind='{kind}')"));
            } else if bare.contains(&tok) && !calls && next != Some('=') {
                out.push_str(tok);
                out.push_str("()");
            } else {
                out.push_str(tok);
            }
            out.push_str(&body[tok_end..end]);
            prev = body[..end].chars().last();
            i = j;
            continue;
        }
        out.push(c);
        prev = Some(c);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::desugar;

    fn d(s: &str) -> String {
        desugar(s).into_owned()
    }

    #[test]
    fn fake_paths_and_bare_functions() {
        assert_eq!(d("{{ fake.name }}"), "{{ fake(kind='name') }}");
        assert_eq!(d("{{fake.address.city}}"), "{{fake(kind='address.city')}}");
        assert_eq!(d("{{ uuid }} {{ ulid }}"), "{{ uuid() }} {{ ulid() }}");
        assert_eq!(
            d("{{ now | date(format='%Y') }}"),
            "{{ now() | date(format='%Y') }}"
        );
        assert_eq!(
            d("{{ fake.email | upper }}"),
            "{{ fake(kind='email') | upper }}"
        );
    }

    #[test]
    fn leaves_everything_else_alone() {
        for s in [
            "{{ uuid() }}",
            "{{ fake(kind='name') }}",
            "{{ \"fake.name uuid\" }}",
            "fake.name uuid outside tags",
            "{# {{ uuid }} #}",
            "{% raw %}{{ uuid }}{% endraw %}",
            "{{ request.uuid }} {{ vars.now }} {{ my_uuid }}",
            "{% set uuid = 1 %}{{ uuid }}",
        ] {
            assert_eq!(d(s), s, "{s}");
        }
        assert_eq!(
            d("{% raw %}{{ uuid }}{% endraw %}{{ uuid }}"),
            "{% raw %}{{ uuid }}{% endraw %}{{ uuid() }}"
        );
    }

    #[test]
    fn works_inside_statements_and_json() {
        assert_eq!(
            d("{% if fake.digit == \"1\" %}{\"id\":\"{{ uuid }}\"}{% endif %}"),
            "{% if fake(kind='digit') == \"1\" %}{\"id\":\"{{ uuid() }}\"}{% endif %}"
        );
    }
}
