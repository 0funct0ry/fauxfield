//! Datasets and `crud` Routes (SPEC §6): in-memory JSON collections loaded from a seed,
//! optional persistence under `.fauxfield/state/`, json-server-style list queries,
//! JSON merge patch, and optional JSON Schema validation.

use std::cmp::Ordering;
use std::path::Path;
use std::sync::Arc;

use bytes::Bytes;
use fauxfield_model::{Dataset, Server};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

type Out = (u16, Vec<(String, String)>, Bytes);

const PROBLEM: &str = "application/problem+json";
const RESERVED: [&str; 5] = ["_page", "_limit", "_sort", "_order", "q"];

/// A Dataset as the GUI, CLI and MCP see it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DatasetInfo {
    pub server_id: String,
    pub server: String,
    pub name: String,
    /// Items in an array Dataset; 1 for an object (singleton) Dataset.
    pub rows: usize,
    pub key: String,
    pub id_strategy: String,
    pub persist: bool,
    pub seed: Option<String>,
    pub seed_template: Option<String>,
    /// Seed loading problem (the Dataset then starts empty).
    pub error: Option<String>,
}

/// One live Dataset.
pub(crate) struct DatasetRt {
    pub def: Dataset,
    pub rows: Value,
    counter: u64,
    pub error: Option<String>,
}

/// All Datasets of one Server.
#[derive(Default)]
pub(crate) struct DataStore {
    pub sets: Vec<DatasetRt>,
    snap: Option<Arc<Value>>,
}

fn sig(d: &Dataset) -> String {
    format!(
        "{:?}|{:?}|{:?}|{:?}|{}",
        d.seed, d.seed_template, d.key, d.id_strategy, d.persist
    )
}

fn state_file(root: &Path, server_id: &str, name: &str) -> std::path::PathBuf {
    root.join(".fauxfield/state")
        .join(server_id)
        .join(format!("{name}.json"))
}

fn load_seed(root: &Path, server: &Server, d: &Dataset) -> (Value, Option<String>) {
    let empty = || Value::Array(vec![]);
    let text = if let Some(f) = &d.seed {
        match crate::engine::read_confined(root, f) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => return (empty(), Some(format!("seed `{f}`: {e}"))),
        }
    } else if let Some(f) = &d.seed_template {
        let src = match crate::engine::read_confined(root, f) {
            Ok(b) => String::from_utf8_lossy(&b).into_owned(),
            Err(e) => return (empty(), Some(format!("seed_template `{f}`: {e}"))),
        };
        let vc = crate::vars::VarContext::default();
        let ctx = json!({
            "request": {}, "vars": vc.vars_json(server, None), "env": "",
            "server": {"name": server.name, "port": server.port},
            "data": {}, "state": {},
        });
        let route = format!("dataset:{}", d.name);
        let input = crate::template::Input {
            context: ctx,
            root,
            seed: server.fake_seed,
            route_id: &route,
            counter: 0,
            seq: None,
        };
        match crate::template::render(&src, &input) {
            Ok(t) => t,
            Err(e) => return (empty(), Some(format!("seed_template `{f}`: {e}"))),
        }
    } else {
        return (empty(), None);
    };
    match serde_json::from_str::<Value>(&text) {
        Ok(v @ (Value::Array(_) | Value::Object(_))) => (v, None),
        Ok(_) => (empty(), Some("seed must be a JSON array or object".into())),
        Err(e) => (empty(), Some(format!("seed is not valid JSON: {e}"))),
    }
}

fn max_numeric_id(rows: &Value, key: &str) -> u64 {
    rows.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| r.get(key).and_then(Value::as_u64))
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0)
}

impl DatasetRt {
    fn fresh(root: &Path, server_id: &str, server: &Server, d: &Dataset) -> DatasetRt {
        let (seed, error) = load_seed(root, server, d);
        let mut rows = seed;
        if d.persist
            && let Ok(t) = std::fs::read_to_string(state_file(root, server_id, &d.name))
            && let Ok(v @ (Value::Array(_) | Value::Object(_))) = serde_json::from_str::<Value>(&t)
        {
            rows = v;
        }
        let counter = max_numeric_id(&rows, d.key_name());
        DatasetRt {
            def: d.clone(),
            rows,
            counter,
            error,
        }
    }

    pub fn len(&self) -> usize {
        match &self.rows {
            Value::Array(a) => a.len(),
            _ => 1,
        }
    }

    fn persist(&self, root: &Path, server_id: &str) {
        if !self.def.persist {
            return;
        }
        let p = state_file(root, server_id, &self.def.name);
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        let tmp = p.with_extension("json.tmp");
        if std::fs::write(
            &tmp,
            serde_json::to_string_pretty(&self.rows).unwrap_or_default(),
        )
        .is_ok()
        {
            let _ = std::fs::rename(tmp, p);
        }
    }

    fn next_id(&mut self) -> Value {
        match self.def.id_strategy.as_deref().unwrap_or("uuid") {
            "increment" => {
                self.counter = self
                    .counter
                    .max(max_numeric_id(&self.rows, self.def.key_name()))
                    + 1;
                json!(self.counter)
            }
            "ulid" => json!(ulid::Ulid::new().to_string()),
            _ => json!(uuid::Uuid::new_v4().to_string()),
        }
    }
}

impl DataStore {
    /// Reconcile with the Server's Dataset definitions (hot reload): unchanged Datasets keep
    /// their live rows; new or changed ones are loaded from their seed.
    pub fn sync(&mut self, root: &Path, server_id: &str, server: &Server) {
        let mut old = std::mem::take(&mut self.sets);
        for d in &server.datasets {
            let kept = old
                .iter()
                .position(|o| o.def.name == d.name && sig(&o.def) == sig(d))
                .map(|i| old.remove(i));
            self.sets
                .push(kept.unwrap_or_else(|| DatasetRt::fresh(root, server_id, server, d)));
        }
        self.snap = None;
    }

    pub fn reset(&mut self, root: &Path, server_id: &str, server: &Server, only: Option<&str>) {
        for s in &mut self.sets {
            if only.is_some_and(|n| n != s.def.name) {
                continue;
            }
            let _ = std::fs::remove_file(state_file(root, server_id, &s.def.name));
            let mut fresh = DatasetRt::fresh(root, server_id, server, &s.def.clone());
            // `fresh` re-reads persisted state; it was just deleted, so this is the seed.
            std::mem::swap(s, &mut fresh);
        }
        self.snap = None;
    }

    pub fn counts(&self) -> Vec<(String, usize)> {
        self.sets
            .iter()
            .map(|s| (s.def.name.clone(), s.len()))
            .collect()
    }

    pub fn get(&self, name: &str) -> Option<&DatasetRt> {
        self.sets.iter().find(|s| s.def.name == name)
    }

    pub fn get_mut(&mut self, name: &str) -> Option<&mut DatasetRt> {
        self.snap = None;
        self.sets.iter_mut().find(|s| s.def.name == name)
    }

    /// `data.<name>` for templates (SPEC §8.1); cached until the next mutation.
    pub fn snapshot(&mut self) -> Arc<Value> {
        if self.snap.is_none() {
            let m: Map<String, Value> = self
                .sets
                .iter()
                .map(|s| (s.def.name.clone(), s.rows.clone()))
                .collect();
            self.snap = Some(Arc::new(Value::Object(m)));
        }
        self.snap.clone().unwrap()
    }

    pub fn info(&self, server_id: &str, server: &str) -> Vec<DatasetInfo> {
        self.sets
            .iter()
            .map(|s| DatasetInfo {
                server_id: server_id.into(),
                server: server.into(),
                name: s.def.name.clone(),
                rows: s.len(),
                key: s.def.key_name().into(),
                id_strategy: s.def.id_strategy.clone().unwrap_or_else(|| "uuid".into()),
                persist: s.def.persist,
                seed: s.def.seed.clone(),
                seed_template: s.def.seed_template.clone(),
                error: s.error.clone(),
            })
            .collect()
    }

    /// Replace live rows (GUI edits, `data import`). Persists when enabled.
    pub fn replace(&mut self, root: &Path, server_id: &str, name: &str, rows: Value) -> bool {
        let Some(s) = self.get_mut(name) else {
            return false;
        };
        s.counter = max_numeric_id(&rows, s.def.key_name());
        s.rows = rows;
        s.persist(root, server_id);
        true
    }
}

// ---------------------------------------------------------------- HTTP handling

fn problem(status: u16, title: &str, detail: &str, path: &str) -> Out {
    let mut v = json!({
        "type": "about:blank", "title": title, "status": status,
        "detail": detail, "instance": path,
    });
    if status == 404 {
        v["title"] = json!("Not found");
    }
    (
        status,
        vec![("content-type".into(), PROBLEM.into())],
        Bytes::from(v.to_string()),
    )
}

fn json_out(status: u16, v: &Value, mut extra: Vec<(String, String)>) -> Out {
    let mut h = vec![("content-type".into(), "application/json".into())];
    h.append(&mut extra);
    (status, h, Bytes::from(v.to_string()))
}

fn scalar_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        Value::Bool(b) => Some(b.to_string()),
        _ => None,
    }
}

fn field<'a>(item: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = item;
    for seg in path.split('.') {
        cur = cur.get(seg)?;
    }
    Some(cur)
}

fn cmp_values(a: Option<&Value>, b: Option<&Value>) -> Ordering {
    match (a, b) {
        (None | Some(Value::Null), None | Some(Value::Null)) => Ordering::Equal,
        (None | Some(Value::Null), _) => Ordering::Less,
        (_, None | Some(Value::Null)) => Ordering::Greater,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x
            .as_f64()
            .partial_cmp(&y.as_f64())
            .unwrap_or(Ordering::Equal),
        (Some(x), Some(y)) => scalar_text(x)
            .unwrap_or_else(|| x.to_string())
            .cmp(&scalar_text(y).unwrap_or_else(|| y.to_string())),
    }
}

/// Compare a stored value with a query-string value (numeric when both parse as numbers).
fn cmp_query(stored: &Value, q: &str) -> Option<Ordering> {
    if let (Some(a), Ok(b)) = (stored.as_f64(), q.parse::<f64>()) {
        return a.partial_cmp(&b);
    }
    scalar_text(stored).map(|s| s.as_str().cmp(q))
}

#[derive(Debug)]
enum Op {
    Eq,
    Ne,
    Gte,
    Lte,
    Like,
}

fn parse_filter(key: &str) -> (&str, Op) {
    for (suffix, op) in [
        ("_gte", Op::Gte),
        ("_lte", Op::Lte),
        ("_ne", Op::Ne),
        ("_like", Op::Like),
    ] {
        if let Some(f) = key.strip_suffix(suffix)
            && !f.is_empty()
        {
            return (f, op);
        }
    }
    (key, Op::Eq)
}

fn matches(item: &Value, key: &str, op: &Op, want: &str) -> bool {
    let got = field(item, key);
    match op {
        Op::Eq => got.is_some_and(|g| {
            cmp_query(g, want) == Some(Ordering::Equal)
                || got.is_some_and(|g| scalar_text(g).as_deref() == Some(want))
        }),
        Op::Ne => got.is_none_or(|g| cmp_query(g, want) != Some(Ordering::Equal)),
        Op::Gte => got.is_some_and(|g| {
            matches!(
                cmp_query(g, want),
                Some(Ordering::Greater | Ordering::Equal)
            )
        }),
        Op::Lte => got
            .is_some_and(|g| matches!(cmp_query(g, want), Some(Ordering::Less | Ordering::Equal))),
        Op::Like => got.and_then(scalar_text).is_some_and(|s| {
            match regex::RegexBuilder::new(want)
                .case_insensitive(true)
                .build()
            {
                Ok(re) => re.is_match(&s),
                Err(_) => s.to_lowercase().contains(&want.to_lowercase()),
            }
        }),
    }
}

fn contains_text(v: &Value, needle: &str) -> bool {
    match v {
        Value::String(s) => s.to_lowercase().contains(needle),
        Value::Number(n) => n.to_string().contains(needle),
        Value::Bool(b) => b.to_string().contains(needle),
        Value::Array(a) => a.iter().any(|x| contains_text(x, needle)),
        Value::Object(o) => o.values().any(|x| contains_text(x, needle)),
        Value::Null => false,
    }
}

/// RFC 7386 JSON merge patch.
pub(crate) fn merge_patch(target: &mut Value, patch: &Value) {
    if let Value::Object(p) = patch {
        if !target.is_object() {
            *target = Value::Object(Map::new());
        }
        let t = target.as_object_mut().unwrap();
        for (k, v) in p {
            if v.is_null() {
                t.remove(k);
            } else {
                merge_patch(t.entry(k.clone()).or_insert(Value::Null), v);
            }
        }
    } else {
        *target = patch.clone();
    }
}

fn with_page(base: &str, query: &[(String, String)], page: usize) -> String {
    let mut q: Vec<String> = query
        .iter()
        .filter(|(k, _)| k != "_page")
        .map(|(k, v)| format!("{}={}", urlencoding::encode(k), urlencoding::encode(v)))
        .collect();
    q.push(format!("_page={page}"));
    format!("{base}?{}", q.join("&"))
}

fn list(ds: &DatasetRt, base: &str, query: &[(String, String)], page_size: Option<usize>) -> Out {
    let rows = match &ds.rows {
        Value::Array(a) => a,
        other => return json_out(200, other, vec![]),
    };
    let mut items: Vec<&Value> = rows.iter().collect();
    let q = query
        .iter()
        .find(|(k, _)| k == "q")
        .map(|(_, v)| v.to_lowercase());
    if let Some(n) = q.filter(|n| !n.is_empty()) {
        items.retain(|i| contains_text(i, &n));
    }
    for (k, v) in query {
        if RESERVED.contains(&k.as_str()) {
            continue;
        }
        let (f, op) = parse_filter(k);
        items.retain(|i| matches(i, f, &op, v));
    }
    if let Some((_, sort)) = query.iter().find(|(k, _)| k == "_sort") {
        let orders: Vec<String> = query
            .iter()
            .find(|(k, _)| k == "_order")
            .map(|(_, v)| v.split(',').map(|s| s.trim().to_lowercase()).collect())
            .unwrap_or_default();
        let fields: Vec<&str> = sort
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        items.sort_by(|a, b| {
            for (i, f) in fields.iter().enumerate() {
                let desc = orders
                    .get(i)
                    .or(orders.first())
                    .is_some_and(|o| o == "desc");
                let mut o = cmp_values(field(a, f), field(b, f));
                if desc {
                    o = o.reverse();
                }
                if o != Ordering::Equal {
                    return o;
                }
            }
            Ordering::Equal
        });
    }
    let total = items.len();
    let get = |k: &str| {
        query
            .iter()
            .find(|(n, _)| n == k)
            .and_then(|(_, v)| v.parse::<usize>().ok())
    };
    let (page_q, limit_q) = (get("_page"), get("_limit"));
    let mut headers = vec![("x-total-count".to_string(), total.to_string())];
    let paged = page_q.is_some() || limit_q.is_some() || page_size.is_some();
    let out: Vec<Value> = if paged {
        let limit = limit_q.or(page_size).unwrap_or(10).max(1);
        let page = page_q.unwrap_or(1).max(1);
        let last = total.div_ceil(limit).max(1);
        let mut links = vec![format!("<{}>; rel=\"first\"", with_page(base, query, 1))];
        if page > 1 {
            links.push(format!(
                "<{}>; rel=\"prev\"",
                with_page(base, query, page - 1)
            ));
        }
        if page < last {
            links.push(format!(
                "<{}>; rel=\"next\"",
                with_page(base, query, page + 1)
            ));
        }
        links.push(format!("<{}>; rel=\"last\"", with_page(base, query, last)));
        headers.push(("link".into(), links.join(", ")));
        items
            .into_iter()
            .skip((page - 1) * limit)
            .take(limit)
            .cloned()
            .collect()
    } else {
        items.into_iter().cloned().collect()
    };
    json_out(200, &Value::Array(out), headers)
}

fn position(rows: &[Value], key: &str, id: &str) -> Option<usize> {
    rows.iter()
        .position(|r| r.get(key).and_then(scalar_text).as_deref() == Some(id))
}

fn validate(schema: Option<&Arc<jsonschema::Validator>>, body: &Value, path: &str) -> Option<Out> {
    let v = schema?;
    let errors: Vec<Value> = v
        .iter_errors(body)
        .map(|e| json!({"path": e.instance_path().to_string(), "message": e.to_string()}))
        .collect();
    if errors.is_empty() {
        return None;
    }
    let mut p = json!({
        "type": "about:blank", "title": "Validation failed", "status": 422,
        "detail": format!("{} validation error(s).", errors.len()),
        "instance": path, "errors": errors,
    });
    p["title"] = json!("Validation failed");
    Some((
        422,
        vec![("content-type".into(), PROBLEM.into())],
        Bytes::from(p.to_string()),
    ))
}

/// Everything `handle` needs besides the Dataset itself.
pub(crate) struct Call<'a> {
    pub method: &'a str,
    /// The request path (used for `Location`, `Link` and problem `instance`).
    pub path: &'a str,
    /// The `:id` path parameter for item operations.
    pub id: Option<&'a str>,
    pub query: &'a [(String, String)],
    pub body: &'a [u8],
    pub page_size: Option<usize>,
    pub schema: Option<Arc<jsonschema::Validator>>,
    pub root: &'a Path,
    pub server_id: &'a str,
}

/// Serve one CRUD operation. Returns the response and whether the Dataset changed.
pub(crate) fn handle(ds: &mut DatasetRt, c: &Call<'_>) -> (Out, bool) {
    let key = ds.def.key_name().to_string();
    let trimmed = c.path.trim_end_matches('/');
    let base = match c.id {
        Some(_) => trimmed
            .rsplit_once('/')
            .map_or(trimmed, |(b, _)| b)
            .to_string(),
        None => trimmed.to_string(),
    };
    let parse = || -> Result<Value, Out> {
        match serde_json::from_slice::<Value>(c.body) {
            Ok(v @ Value::Object(_)) => Ok(v),
            Ok(_) => Err(problem(
                400,
                "Bad request",
                "Body must be a JSON object.",
                c.path,
            )),
            Err(e) => Err(problem(
                400,
                "Bad request",
                &format!("Body is not valid JSON: {e}"),
                c.path,
            )),
        }
    };
    let Value::Array(_) = &ds.rows else {
        return match (c.method, c.id) {
            ("GET", None) => (json_out(200, &ds.rows, vec![]), false),
            _ => (
                problem(
                    405,
                    "Method not allowed",
                    "This dataset is a singleton object; only GET is supported.",
                    c.path,
                ),
                false,
            ),
        };
    };
    match (c.method, c.id) {
        ("GET", None) => (list(ds, &base, c.query, c.page_size), false),
        ("GET", Some(id)) => {
            let rows = ds.rows.as_array().unwrap();
            match position(rows, &key, id) {
                Some(i) => (json_out(200, &rows[i], vec![]), false),
                None => (
                    problem(
                        404,
                        "Not found",
                        &format!("No item with {key} `{id}`."),
                        c.path,
                    ),
                    false,
                ),
            }
        }
        ("POST", None) => {
            let mut body = match parse() {
                Ok(b) => b,
                Err(o) => return (o, false),
            };
            if let Some(o) = validate(c.schema.as_ref(), &body, c.path) {
                return (o, false);
            }
            if body.get(&key).is_none_or(Value::is_null) {
                let id = ds.next_id();
                body[&key] = id;
            } else if let Some(id) = body.get(&key).and_then(scalar_text)
                && position(ds.rows.as_array().unwrap(), &key, &id).is_some()
            {
                return (
                    problem(
                        409,
                        "Conflict",
                        &format!("An item with {key} `{id}` already exists."),
                        c.path,
                    ),
                    false,
                );
            }
            let id = body.get(&key).and_then(scalar_text).unwrap_or_default();
            ds.rows.as_array_mut().unwrap().push(body.clone());
            ds.persist(c.root, c.server_id);
            (
                json_out(
                    201,
                    &body,
                    vec![(
                        "location".into(),
                        format!("{base}/{}", urlencoding::encode(&id)),
                    )],
                ),
                true,
            )
        }
        ("PUT", Some(id)) => {
            let mut body = match parse() {
                Ok(b) => b,
                Err(o) => return (o, false),
            };
            if let Some(o) = validate(c.schema.as_ref(), &body, c.path) {
                return (o, false);
            }
            let rows = ds.rows.as_array_mut().unwrap();
            let Some(i) = position(rows, &key, id) else {
                return (
                    problem(
                        404,
                        "Not found",
                        &format!("No item with {key} `{id}`."),
                        c.path,
                    ),
                    false,
                );
            };
            body[&key] = rows[i].get(&key).cloned().unwrap_or_else(|| json!(id));
            rows[i] = body.clone();
            ds.persist(c.root, c.server_id);
            (json_out(200, &body, vec![]), true)
        }
        ("PATCH", Some(id)) => {
            let patch = match parse() {
                Ok(b) => b,
                Err(o) => return (o, false),
            };
            let rows = ds.rows.as_array_mut().unwrap();
            let Some(i) = position(rows, &key, id) else {
                return (
                    problem(
                        404,
                        "Not found",
                        &format!("No item with {key} `{id}`."),
                        c.path,
                    ),
                    false,
                );
            };
            let mut merged = rows[i].clone();
            merge_patch(&mut merged, &patch);
            merged[&key] = rows[i].get(&key).cloned().unwrap_or_else(|| json!(id));
            if let Some(o) = validate(c.schema.as_ref(), &merged, c.path) {
                return (o, false);
            }
            rows[i] = merged.clone();
            ds.persist(c.root, c.server_id);
            (json_out(200, &merged, vec![]), true)
        }
        ("DELETE", Some(id)) => {
            let rows = ds.rows.as_array_mut().unwrap();
            let Some(i) = position(rows, &key, id) else {
                return (
                    problem(
                        404,
                        "Not found",
                        &format!("No item with {key} `{id}`."),
                        c.path,
                    ),
                    false,
                );
            };
            rows.remove(i);
            ds.persist(c.root, c.server_id);
            ((204, vec![], Bytes::new()), true)
        }
        _ => (
            problem(405, "Method not allowed", "Unsupported operation.", c.path),
            false,
        ),
    }
}

/// Field types a CRUD resource wizard accepts besides the `fake` kinds.
pub const PLAIN_FIELD_KINDS: &[&str] = &["int", "float", "bool", "uuid"];

/// Tera text that renders `rows` items with an incrementing `id` and one field per
/// `(name, kind)`; `kind` is a `fake` kind or one of [`PLAIN_FIELD_KINDS`].
pub fn crud_seed_template(fields: &[(String, String)], rows: usize) -> Result<String, String> {
    let mut parts = vec![r#""id": {{ i }}"#.to_string()];
    for (name, kind) in fields {
        if name.is_empty()
            || !name
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '-')
        {
            return Err(format!("`{name}` is not a valid field name."));
        }
        if name == "id" {
            continue;
        }
        let v = match kind.as_str() {
            "int" => "{{ random_int(min=1, max=1000) }}".to_string(),
            "float" => "{{ random_float(min=0, max=100, precision=2) }}".to_string(),
            "bool" => "{{ pick(from=[true, false]) }}".to_string(),
            "uuid" => "\"{{ uuid() }}\"".to_string(),
            k if crate::template::FAKE_KINDS.iter().any(|(n, _)| *n == k) => {
                format!("{{{{ fake(kind=\"{k}\") | json_encode() }}}}")
            }
            k => {
                return Err(format!(
                    "unknown field kind `{k}`; use a fake kind or {}",
                    PLAIN_FIELD_KINDS.join(", ")
                ));
            }
        };
        parts.push(format!("\"{name}\": {v}"));
    }
    Ok(format!(
        "[\n{{% for i in range(start=1, end={}) %}}  {{ {} }}{{% if not loop.last %}},{{% endif %}}\n{{% endfor %}}]\n",
        rows + 1,
        parts.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wizard_template_renders_to_json_rows() {
        let t = crud_seed_template(
            &[
                ("name".into(), "name".into()),
                ("price".into(), "float".into()),
                ("stock".into(), "int".into()),
                ("ok".into(), "bool".into()),
            ],
            5,
        )
        .unwrap();
        let input = crate::template::Input {
            context: json!({}),
            root: std::path::Path::new("."),
            seed: Some(1),
            route_id: "t",
            counter: 0,
            seq: None,
        };
        let out = crate::template::render(&t, &input).unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 5);
        assert_eq!(v[0]["id"], 1);
        assert!(v[0]["name"].is_string() && v[0]["price"].is_number() && v[0]["ok"].is_boolean());
        assert!(crud_seed_template(&[("x".into(), "nope".into())], 1).is_err());
        assert!(crud_seed_template(&[("a b".into(), "int".into())], 1).is_err());
    }

    #[test]
    fn merge_patch_follows_rfc_7386() {
        let mut v = json!({"a": "b", "c": {"d": "e", "f": "g"}});
        merge_patch(&mut v, &json!({"a": "z", "c": {"f": null}}));
        assert_eq!(v, json!({"a": "z", "c": {"d": "e"}}));
        let mut v = json!({"a": [1]});
        merge_patch(&mut v, &json!({"a": [2, 3]}));
        assert_eq!(v, json!({"a": [2, 3]}));
    }
}
