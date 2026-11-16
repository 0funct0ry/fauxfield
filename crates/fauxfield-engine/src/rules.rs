//! Response selection and rule evaluation (SPEC §5.3, §5.4) with a decision trace.

use std::cell::OnceCell;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use fauxfield_model::{Response, Route, Rule};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Per-route runtime state: calls so far (also drives `sequential` and the `count` target).
#[derive(Debug, Default)]
pub(crate) struct Counters(Mutex<HashMap<String, u64>>);

impl Counters {
    /// Increment and return this request's 1-based call number.
    pub fn bump(&self, route_id: &str) -> u64 {
        let mut m = self.0.lock().unwrap();
        let c = m.entry(route_id.to_string()).or_insert(0);
        *c += 1;
        *c
    }
    pub fn reset(&self) {
        self.0.lock().unwrap().clear();
    }
}

/// Compiled regexes and JSON Schemas, cleared on hot reload.
#[derive(Default)]
pub(crate) struct Compiled {
    regex: Mutex<HashMap<String, Option<Regex>>>,
    schema: Mutex<HashMap<String, Option<Arc<jsonschema::Validator>>>>,
}

impl Compiled {
    pub fn clear(&self) {
        self.regex.lock().unwrap().clear();
        self.schema.lock().unwrap().clear();
    }
    fn regex(&self, src: &str) -> Option<Regex> {
        let mut m = self.regex.lock().unwrap();
        m.entry(src.to_string())
            .or_insert_with(|| fauxfield_model::pattern::compile_regex(src).ok())
            .clone()
    }
    pub(crate) fn schema(&self, root: &Path, path: &str) -> Option<Arc<jsonschema::Validator>> {
        let mut m = self.schema.lock().unwrap();
        m.entry(path.to_string())
            .or_insert_with(|| {
                let bytes = crate::engine::read_confined(root, path).ok()?;
                let v: Value = serde_json::from_slice(&bytes).ok()?;
                jsonschema::validator_for(&v).ok().map(Arc::new)
            })
            .clone()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuleTrace {
    pub target: String,
    pub key: String,
    pub op: String,
    pub expected: String,
    /// The value that was compared; `None` when the request had none.
    pub actual: Option<String>,
    pub negate: bool,
    pub passed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ResponseTrace {
    pub id: String,
    pub name: String,
    pub logic: String,
    pub rules: Vec<RuleTrace>,
    pub passed: bool,
    /// Why this response was skipped or how it was judged (`disabled`, `no rules`, ...).
    #[serde(default)]
    pub note: String,
}

/// Why a Response was chosen for one request.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Trace {
    pub mode: String,
    /// This route's call number for the request (1-based).
    pub call: u64,
    pub evaluated: Vec<ResponseTrace>,
    pub selected: Option<String>,
    pub reason: String,
}

/// The request as rules see it. The body is parsed on first use.
pub(crate) struct Ctx<'a> {
    pub method: &'a str,
    pub path: &'a str,
    pub query: Vec<(String, String)>,
    /// Lower-cased names.
    pub headers: &'a [(String, String)],
    pub params: &'a [(String, String)],
    pub ip: Option<String>,
    pub body: &'a [u8],
    pub call: u64,
    pub root: &'a Path,
    pub tpl: Option<crate::render::TplEnv>,
    body_parsed: OnceCell<Body>,
}

enum Body {
    None,
    Json(Value),
    Form(Vec<(String, String)>),
    Text(String),
}

impl<'a> Ctx<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        method: &'a str,
        url: &'a str,
        headers: &'a [(String, String)],
        params: &'a [(String, String)],
        ip: Option<String>,
        body: &'a [u8],
        call: u64,
        root: &'a Path,
    ) -> Self {
        let (path, q) = url.split_once('?').unwrap_or((url, ""));
        Ctx {
            method,
            path,
            query: form_urlencoded::parse(q.as_bytes())
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
            headers,
            params,
            ip,
            body,
            call,
            root,
            tpl: None,
            body_parsed: OnceCell::new(),
        }
    }

    fn header(&self, name: &str) -> Option<&str> {
        let n = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(k, _)| *k == n)
            .map(|(_, v)| v.as_str())
    }

    fn cookie(&self, name: &str) -> Option<String> {
        self.header("cookie")?.split(';').find_map(|p| {
            let (k, v) = p.split_once('=')?;
            (k.trim() == name).then(|| v.trim().to_string())
        })
    }

    fn body(&self) -> &Body {
        self.body_parsed.get_or_init(|| parse_body(self))
    }

    fn json_at(&self, key: &str) -> Option<Value> {
        let Body::Json(v) = self.body() else {
            return None;
        };
        if key.is_empty() {
            return Some(v.clone());
        }
        let p = serde_json_path::JsonPath::parse(key).ok()?;
        p.query(v).first().cloned()
    }

    /// `name.$.path` (JSONPath) or `name.field.sub` over the live `data.<name>`.
    fn dataset_value(&self, key: &str) -> Option<String> {
        let data = &self.tpl.as_ref()?.context["data"];
        let (name, rest) = key.split_once('.')?;
        let root = data.get(name)?;
        let path = if rest.starts_with('$') {
            rest.to_string()
        } else {
            format!("$.{rest}")
        };
        let v = serde_json_path::JsonPath::parse(&path)
            .ok()?
            .query(root)
            .first()
            .map(|v| (*v).clone())?;
        Some(match v {
            Value::String(s) => s,
            other => other.to_string(),
        })
    }

    /// Rows of a Dataset matching CRUD-style filters: `orders?status=open&total_gte=5`.
    fn dataset_count(&self, key: &str) -> Option<String> {
        let data = &self.tpl.as_ref()?.context["data"];
        let (name, q) = key.split_once('?').unwrap_or((key, ""));
        let rows = data.get(name)?;
        let filters: Vec<(String, String)> = form_urlencoded::parse(q.as_bytes())
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        Some(crate::data::matching(rows, &filters).len().to_string())
    }

    fn actual(&self, r: &Rule) -> Option<String> {
        let key = r.key.as_deref().unwrap_or("");
        match r.target.as_str() {
            "header" => self.header(key).map(String::from),
            "query" => self
                .query
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone()),
            "cookie" => self.cookie(key),
            "param" => self
                .params
                .iter()
                .find(|(k, _)| k == key)
                .map(|(_, v)| v.clone()),
            "method" => Some(self.method.to_ascii_uppercase()),
            "path" => Some(self.path.to_string()),
            "ip" => self.ip.clone(),
            "count" => Some(self.call.to_string()),
            "body" => match self.body() {
                Body::Json(_) => self.json_at(key).map(|v| match v {
                    Value::String(s) => s,
                    other => other.to_string(),
                }),
                Body::Form(f) => f.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()),
                Body::Text(t) => key.is_empty().then(|| t.clone()),
                Body::None => None,
            },
            "dataset" => self.dataset_value(key),
            "dataset_count" => self.dataset_count(key),
            // Variables are resolved elsewhere; anything else never matches.
            _ => None,
        }
    }
}

fn parse_body(c: &Ctx) -> Body {
    if c.body.is_empty() {
        return Body::None;
    }
    let ct = c.header("content-type").unwrap_or("").to_ascii_lowercase();
    if ct.contains("application/x-www-form-urlencoded") {
        return Body::Form(
            form_urlencoded::parse(c.body)
                .map(|(k, v)| (k.into_owned(), v.into_owned()))
                .collect(),
        );
    }
    if ct.contains("multipart/form-data") {
        let boundary = c
            .header("content-type")
            .and_then(|h| {
                h.split(';')
                    .find_map(|p| p.trim().strip_prefix("boundary="))
            })
            .map(|b| b.trim_matches('"').to_string());
        return Body::Form(
            boundary
                .map(|b| multipart_fields(c.body, &b))
                .unwrap_or_default(),
        );
    }
    if let Ok(v) = serde_json::from_slice::<Value>(c.body) {
        return Body::Json(v);
    }
    Body::Text(String::from_utf8_lossy(c.body).into_owned())
}

/// Text fields of a multipart body; parts with a filename are skipped.
fn multipart_fields(body: &[u8], boundary: &str) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(body);
    let delim = format!("--{boundary}");
    let mut out = Vec::new();
    for part in text.split(delim.as_str()).skip(1) {
        let part = part.strip_prefix("\r\n").unwrap_or(part);
        if part.starts_with("--") {
            break;
        }
        let Some((head, val)) = part.split_once("\r\n\r\n") else {
            continue;
        };
        let disp = head
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("content-disposition"))
            .unwrap_or("");
        if disp.contains("filename=") {
            continue;
        }
        let Some(name) = disp
            .split(';')
            .find_map(|p| p.trim().strip_prefix("name="))
            .map(|n| n.trim_matches('"').to_string())
        else {
            continue;
        };
        out.push((name, val.strip_suffix("\r\n").unwrap_or(val).to_string()));
    }
    out
}

fn fmt_value(v: &Option<toml::Value>) -> String {
    match v {
        None => String::new(),
        Some(toml::Value::String(s)) => s.clone(),
        Some(o) => o.to_string(),
    }
}

fn scalar(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => s.clone(),
        o => o.to_string(),
    }
}

fn num(s: &str) -> Option<f64> {
    s.trim().parse().ok()
}

fn same(a: &str, b: &str) -> bool {
    match (num(a), num(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// Evaluate one rule. Returns `(passed, actual)`; `actual` is what was compared.
fn eval_rule(r: &Rule, c: &Ctx, cc: &Compiled) -> (bool, Option<String>) {
    let mut actual = c.actual(r);
    let want = r.value.as_ref();
    let raw = match r.op.as_str() {
        "exists" => actual.is_some(),
        "absent" => actual.is_none(),
        "json_schema" => {
            let Some(toml::Value::String(path)) = want else {
                return (false, actual);
            };
            let inst = c.json_at(if r.target == "body" {
                r.key.as_deref().unwrap_or("")
            } else {
                return (false, actual);
            });
            let ok = match (cc.schema(c.root, path), inst) {
                (Some(v), Some(i)) => v.is_valid(&i),
                _ => false,
            };
            actual = Some(
                if ok {
                    "valid"
                } else {
                    "invalid or no JSON body"
                }
                .into(),
            );
            ok
        }
        op => match (&actual, want) {
            // Missing value: only `ne` passes.
            (None, _) => op == "ne",
            (Some(_), None) => false,
            (Some(a), Some(w)) => match op {
                "eq" => same(a, &scalar(w)),
                "ne" => !same(a, &scalar(w)),
                "contains" => a.contains(&scalar(w)),
                "starts_with" => a.starts_with(&scalar(w)),
                "ends_with" => a.ends_with(&scalar(w)),
                "regex" => cc.regex(&scalar(w)).is_some_and(|re| re.is_match(a)),
                "in" => match w {
                    toml::Value::Array(xs) => xs.iter().any(|x| same(a, &scalar(x))),
                    other => same(a, &scalar(other)),
                },
                "gt" | "gte" | "lt" | "lte" => match (num(a), fauxfield_model::rules::value_f64(w))
                {
                    (Some(x), Some(y)) => match op {
                        "gt" => x > y,
                        "gte" => x >= y,
                        "lt" => x < y,
                        _ => x <= y,
                    },
                    _ => false,
                },
                _ => false,
            },
        },
    };
    (raw != r.negate, actual)
}

/// Render a string rule value that contains Tera syntax (SPEC §5.5, M6).
fn resolved(r: &Rule, c: &Ctx) -> Rule {
    let mut r = r.clone();
    if let (Some(env), Some(toml::Value::String(s))) = (&c.tpl, &r.value)
        && s.contains("{{")
        && let Ok(out) = env.render(c.root, s)
    {
        r.value = Some(toml::Value::String(out));
    }
    // Dataset rule keys may use the request: `orders?id={{ request.params.id }}`.
    if matches!(r.target.as_str(), "dataset" | "dataset_count")
        && let (Some(env), Some(k)) = (&c.tpl, r.key.as_deref())
        && k.contains("{{")
        && let Ok(out) = env.render(c.root, k)
    {
        r.key = Some(out);
    }
    r
}

/// Evaluate one standalone rule (WebSocket `when`, M13): template values resolve first.
pub(crate) fn rule_passes(r: &Rule, c: &Ctx, cc: &Compiled) -> bool {
    eval_rule(&resolved(r, c), c, cc).0
}

fn eval_response(resp: &Response, c: &Ctx, cc: &Compiled) -> ResponseTrace {
    let or = resp.rules_logic.as_deref() == Some("or");
    let rules: Vec<RuleTrace> = resp
        .rules
        .iter()
        .map(|r| {
            let r = &resolved(r, c);
            let (passed, mut actual) = eval_rule(r, c, cc);
            let key = r.key.as_deref().unwrap_or("").to_ascii_lowercase();
            if actual.is_some()
                && (r.target == "cookie"
                    || (r.target == "header" && crate::recording::REDACT.contains(&key.as_str())))
            {
                actual = Some("***".into());
            }
            RuleTrace {
                target: r.target.clone(),
                key: r.key.clone().unwrap_or_default(),
                op: r.op.clone(),
                expected: fmt_value(&r.value),
                actual,
                negate: r.negate,
                passed,
            }
        })
        .collect();
    let passed = if or {
        rules.iter().any(|r| r.passed)
    } else {
        rules.iter().all(|r| r.passed)
    };
    ResponseTrace {
        id: resp.id.clone(),
        name: resp.name.clone().unwrap_or_default(),
        logic: if or { "or" } else { "and" }.into(),
        rules,
        passed,
        note: String::new(),
    }
}

fn skipped(resp: &Response, note: &str) -> ResponseTrace {
    ResponseTrace {
        id: resp.id.clone(),
        name: resp.name.clone().unwrap_or_default(),
        logic: "and".into(),
        rules: vec![],
        passed: false,
        note: note.into(),
    }
}

fn random_below(n: usize) -> usize {
    let mut b = [0u8; 8];
    let _ = getrandom::fill(&mut b);
    (u64::from_le_bytes(b) % n as u64) as usize
}

/// Choose the Response for this request and explain why (SPEC §5.3).
/// Pick the first Response of a crud operation whose Rules pass (SPEC 6.2.1). Unlike
/// [`select`] there is no `default` or first-Response fallback: `None` means "run the built-in
/// handler".
pub(crate) fn select_operation<'r>(
    responses: &'r [Response],
    c: &Ctx,
    cc: &Compiled,
) -> (Option<&'r Response>, Trace) {
    let mut trace = Trace {
        mode: "rules".into(),
        call: c.call,
        ..Default::default()
    };
    let mut hit = None;
    for resp in responses {
        if resp.disabled {
            trace.evaluated.push(skipped(resp, "disabled"));
            continue;
        }
        if hit.is_some() {
            continue;
        }
        if resp.rules.is_empty() {
            trace
                .evaluated
                .push(skipped(resp, "no rules (never matches)"));
            continue;
        }
        let t = eval_response(resp, c, cc);
        if t.passed {
            hit = Some(resp);
        }
        trace.evaluated.push(t);
    }
    trace.reason = if hit.is_some() {
        "first operation response whose rules passed".into()
    } else {
        "no operation response matched; built-in handler".into()
    };
    (hit, trace)
}

pub(crate) fn select<'r>(
    route: &'r Route,
    c: &Ctx,
    cc: &Compiled,
) -> (Option<&'r Response>, Trace) {
    let mode = route.response_mode.as_deref().unwrap_or("rules");
    let mut trace = Trace {
        mode: mode.into(),
        call: c.call,
        ..Default::default()
    };
    let enabled: Vec<&Response> = route.responses.iter().filter(|r| !r.disabled).collect();
    if enabled.is_empty() {
        trace.reason = if route.responses.is_empty() {
            "route has no responses".into()
        } else {
            "every response is disabled".into()
        };
        return (None, trace);
    }
    let pick = match mode {
        "sequential" => {
            let r = enabled[((c.call - 1) % enabled.len() as u64) as usize];
            trace.reason = format!("sequential: call {} of {} responses", c.call, enabled.len());
            r
        }
        "random" => {
            trace.reason = "random pick".into();
            enabled[random_below(enabled.len())]
        }
        _ => {
            let mut hit = None;
            for resp in &route.responses {
                if resp.disabled {
                    trace.evaluated.push(skipped(resp, "disabled"));
                    continue;
                }
                if hit.is_some() {
                    continue;
                }
                if resp.rules.is_empty() {
                    trace
                        .evaluated
                        .push(skipped(resp, "no rules (fallback only)"));
                    continue;
                }
                let t = eval_response(resp, c, cc);
                if t.passed {
                    hit = Some(resp);
                }
                trace.evaluated.push(t);
            }
            match hit {
                Some(r) => {
                    trace.reason = "first response whose rules passed".into();
                    r
                }
                None => {
                    let fb = enabled
                        .iter()
                        .find(|r| r.default)
                        .copied()
                        .unwrap_or(enabled[0]);
                    trace.reason = if fb.default {
                        "no rules passed: default response".into()
                    } else {
                        "no rules passed: first response".into()
                    };
                    fb
                }
            }
        }
    };
    trace.selected = Some(pick.id.clone());
    (Some(pick), trace)
}

#[cfg(test)]
mod tests;
