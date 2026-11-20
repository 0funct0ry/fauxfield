//! `curl` command that triggers a Response's rules (Rules tab "Copy cURL"). GPUI-free.
//!
//! For every rule it derives a request value that satisfies it (honouring `negate`), then lays the
//! values out by target: query, header, cookie, path param, JSON body, method and path. Targets a
//! request cannot control (`ip`, `variable`, `dataset`, `count`) are listed in a trailing comment.

use fauxfield_model::Rule;

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

fn url_encode(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn text(v: &Option<toml::Value>) -> String {
    crate::rules_ui::value_text(v)
}

fn num(v: &Option<toml::Value>) -> f64 {
    v.as_ref()
        .and_then(fauxfield_model::rules::value_f64)
        .unwrap_or(0.0)
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A value that satisfies `op value` (or, with `negate`, fails it). `None` means the request
/// must not carry the field at all.
pub fn satisfying(rule: &Rule) -> Option<String> {
    let v = text(&rule.value);
    let first = v.split(',').next().unwrap_or("").trim().to_string();
    let n = num(&rule.value);
    Some(match (rule.op.as_str(), rule.negate) {
        ("eq" | "contains" | "starts_with" | "ends_with", false) => v,
        ("ne", false) => format!("not-{v}"),
        ("eq", true) => format!("not-{v}"),
        ("ne", true) => v,
        ("contains" | "starts_with" | "ends_with" | "regex" | "in", true) => "zzz-no-match".into(),
        ("regex", false) => v.trim_start_matches('^').trim_end_matches('$').to_string(),
        ("in", false) => first,
        ("exists", false) | ("absent", true) => "1".into(),
        ("exists", true) | ("absent", false) => return None,
        ("gt", false) | ("lte", true) => fmt_num(n + 1.0),
        ("gte", false) | ("lt", true) => fmt_num(n),
        ("lt", false) | ("gte", true) => fmt_num(n - 1.0),
        ("lte", false) | ("gt", true) => fmt_num(n),
        ("json_schema", _) => "{}".into(),
        _ => v,
    })
}

/// Sets `$.a.b` (dotted JSONPath of plain names) in `root`.
fn set_path(root: &mut serde_json::Value, path: &str, val: serde_json::Value) {
    let names: Vec<&str> = path
        .trim_start_matches('$')
        .split('.')
        .filter(|s| !s.is_empty())
        .collect();
    if names.is_empty() {
        *root = val;
        return;
    }
    let mut cur = root;
    for (i, name) in names.iter().enumerate() {
        if !cur.is_object() {
            *cur = serde_json::json!({});
        }
        let map = cur.as_object_mut().expect("object");
        if i + 1 == names.len() {
            map.insert((*name).into(), val);
            return;
        }
        cur = map.entry(*name).or_insert_with(|| serde_json::json!({}));
    }
}

fn json_value(s: &str) -> serde_json::Value {
    serde_json::from_str(s).unwrap_or_else(|_| serde_json::Value::String(s.to_string()))
}

/// `base` is e.g. `http://127.0.0.1:4010`; `path` the Route path under the Server prefix.
pub fn build(base: &str, prefix: &str, method: &str, path: &str, rules: &[&Rule]) -> String {
    let mut method = if ["any", "crud", "files"]
        .iter()
        .any(|m| method.eq_ignore_ascii_case(m))
    {
        "GET".to_string()
    } else {
        method.to_ascii_uppercase()
    };
    let mut path = format!(
        "{}/{}",
        prefix.trim_end_matches('/'),
        path.trim_start_matches('/')
    );
    let (mut query, mut headers, mut cookies): (Vec<String>, Vec<String>, Vec<String>) =
        Default::default();
    let mut body: Option<serde_json::Value> = None;
    let mut notes: Vec<String> = Vec::new();
    for r in rules {
        let key = r.key.clone().unwrap_or_default();
        let val = satisfying(r);
        match (r.target.as_str(), val) {
            ("query", Some(v)) => query.push(format!("{}={}", url_encode(&key), url_encode(&v))),
            ("header", Some(v)) => headers.push(format!("{key}: {v}")),
            ("cookie", Some(v)) => cookies.push(format!("{key}={v}")),
            ("param", Some(v)) => {
                path = path
                    .replace(&format!(":{key}"), &v)
                    .replace(&format!("{{{key}}}"), &v);
            }
            ("method", Some(v)) => method = v.to_ascii_uppercase(),
            ("path", Some(v)) => {
                path = if v.starts_with('/') {
                    v
                } else {
                    format!("/{v}")
                }
            }
            ("body", Some(v)) => {
                let root = body.get_or_insert(serde_json::Value::Null);
                if key.starts_with('$') {
                    set_path(root, &key, json_value(&v));
                } else {
                    *root = json_value(&v);
                }
            }
            ("ip" | "variable" | "dataset" | "count", _) => {
                notes.push(format!("{} {}", r.target, key).trim().to_string())
            }
            _ => {} // absent/exists-false: the field is simply left out
        }
    }
    // Remaining `:param` placeholders get a stand-in.
    let path = path
        .split('/')
        .map(|seg| {
            if seg.starts_with([':', '*']) || (seg.starts_with('{') && seg.ends_with('}')) {
                "1"
            } else {
                seg
            }
        })
        .collect::<Vec<_>>()
        .join("/");
    let mut url = format!("{base}{path}");
    if !query.is_empty() {
        url.push('?');
        url.push_str(&query.join("&"));
    }
    let mut out = format!("curl -X {method} {}", shell_quote(&url));
    for h in &headers {
        out.push_str(&format!(" -H {}", shell_quote(h)));
    }
    if !cookies.is_empty() {
        out.push_str(&format!(
            " -H {}",
            shell_quote(&format!("cookie: {}", cookies.join("; ")))
        ));
    }
    if let Some(b) = body {
        let raw = match &b {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if !matches!(b, serde_json::Value::String(_)) {
            out.push_str(" -H 'content-type: application/json'");
        }
        out.push_str(&format!(" --data-raw {}", shell_quote(&raw)));
    }
    if !notes.is_empty() {
        out.push_str(&format!(
            " # not reproducible from a request: {}",
            notes.join(", ")
        ));
    }
    out
}

/// `curl` for one operation of a crud Route: `:id` becomes `1`; a body is sent as JSON.
pub fn crud_curl(
    base: &str,
    prefix: &str,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> String {
    let prefix = prefix.trim_end_matches('/');
    let path = path.replace(":id", "1");
    let mut out = format!(
        "curl -X {method} {}",
        shell_quote(&format!("{}{prefix}{path}", base.trim_end_matches('/')))
    );
    if let Some(b) = body {
        out.push_str(" -H 'content-type: application/json'");
        out.push_str(&format!(" --data-raw {}", shell_quote(&b.to_string())));
    }
    out
}

/// A sample body for a crud write: `live` (a real row) without its `key`, else one field per
/// declared column with a value of its type, else `{}`.
pub fn crud_sample_body(
    live: Option<serde_json::Value>,
    key: &str,
    columns: &[(String, String)],
) -> serde_json::Value {
    use serde_json::Value;
    if let Some(Value::Object(mut o)) = live {
        o.remove(key);
        return Value::Object(o);
    }
    let mut o = serde_json::Map::new();
    for (name, ty) in columns.iter().filter(|(n, _)| n != key) {
        o.insert(name.clone(), crate::dataset_edit::default_value(ty));
    }
    Value::Object(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_operations_become_curl_commands() {
        let b = crud_sample_body(Some(serde_json::json!({"id": 3, "name": "Ann"})), "id", &[]);
        assert_eq!(b, serde_json::json!({"name": "Ann"}));
        assert_eq!(
            crud_curl("http://h:1", "/api", "PUT", "/users/:id", Some(&b)),
            "curl -X PUT 'http://h:1/api/users/1' -H 'content-type: application/json' --data-raw '{\"name\":\"Ann\"}'"
        );
        assert_eq!(
            crud_curl("http://h:1/", "", "GET", "/users", None),
            "curl -X GET 'http://h:1/users'"
        );
        let d = crud_sample_body(
            None,
            "id",
            &[("id".into(), "int".into()), ("n".into(), "int".into())],
        );
        assert_eq!(d, serde_json::json!({"n": 0}));
    }

    fn rule(target: &str, key: &str, op: &str, value: Option<toml::Value>, negate: bool) -> Rule {
        Rule {
            target: target.into(),
            key: (!key.is_empty()).then(|| key.to_string()),
            op: op.into(),
            value,
            negate,
            extra: Default::default(),
        }
    }
    fn s(v: &str) -> Option<toml::Value> {
        Some(toml::Value::String(v.into()))
    }

    #[test]
    fn query_header_and_negation() {
        let q = rule("query", "foo", "eq", s("1"), false);
        let h = rule("header", "x-env", "eq", s("prod"), false);
        assert_eq!(
            build("http://h:1", "", "GET", "/health", &[&q, &h]),
            "curl -X GET 'http://h:1/health?foo=1' -H 'x-env: prod'"
        );
        let n = rule("query", "foo", "eq", s("1"), true);
        assert_eq!(
            build("http://h:1", "/api", "GET", "health", &[&n]),
            "curl -X GET 'http://h:1/api/health?foo=not-1'"
        );
    }

    #[test]
    fn numbers_exists_absent_and_in() {
        let gt = rule("query", "n", "gt", Some(toml::Value::Integer(5)), false);
        assert_eq!(satisfying(&gt).as_deref(), Some("6"));
        let not_gt = Rule {
            negate: true,
            ..gt.clone()
        };
        assert_eq!(satisfying(&not_gt).as_deref(), Some("5"));
        let lt = rule("query", "n", "lt", Some(toml::Value::Integer(5)), false);
        assert_eq!(satisfying(&lt).as_deref(), Some("4"));
        assert_eq!(
            satisfying(&rule("header", "a", "exists", None, false)).as_deref(),
            Some("1")
        );
        assert_eq!(
            satisfying(&rule("header", "a", "absent", None, false)),
            None
        );
        let inn = rule("query", "k", "in", s("a, b"), false);
        assert_eq!(satisfying(&inn).as_deref(), Some("a"));
    }

    #[test]
    fn param_cookie_body_and_unreproducible() {
        let p = rule("param", "id", "eq", s("7"), false);
        let c = rule("cookie", "sid", "eq", s("abc"), false);
        let b = rule("body", "$.user.role", "eq", s("admin"), false);
        let ip = rule("ip", "", "eq", s("10.0.0.1"), false);
        let out = build("http://h:1", "", "POST", "/users/:id", &[&p, &c, &b, &ip]);
        assert_eq!(
            out,
            "curl -X POST 'http://h:1/users/7' -H 'cookie: sid=abc' -H 'content-type: application/json' --data-raw '{\"user\":{\"role\":\"admin\"}}' # not reproducible from a request: ip"
        );
    }

    #[test]
    fn quotes_single_quotes_and_fills_params() {
        let q = rule("query", "q", "eq", s("it's"), false);
        let out = build("http://h:1", "", "ANY", "/a/{x}", &[&q]);
        assert_eq!(out, "curl -X GET 'http://h:1/a/1?q=it%27s'");
    }

    #[test]
    fn route_curl_keeps_the_prefix_and_fills_placeholders() {
        let c = build("http://127.0.0.1:4010", "/api/v1", "GET", "/users/:id", &[]);
        assert_eq!(c, "curl -X GET 'http://127.0.0.1:4010/api/v1/users/1'");
        let c = build("http://h", "/api/v1/", "FILES", "/assets/*rest", &[]);
        assert_eq!(c, "curl -X GET 'http://h/api/v1/assets/1'");
        let c = build("http://h", "", "ANY", "/ping", &[]);
        assert_eq!(c, "curl -X GET 'http://h/ping'");
    }
}
