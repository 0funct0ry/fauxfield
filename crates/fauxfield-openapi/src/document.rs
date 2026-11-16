//! Document generation (SPEC §12.4).
use crate::schema::{column_schema, infer, infer_many};
use crate::{Document, Options, ServerSnapshot, Stats};
use fauxfield_model::{Dataset, Response, Route, Rule, Server};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const ANY_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE"];
const OTHER_TAG: &str = "Other";

struct Ctx<'a, 'o> {
    snap: &'a ServerSnapshot<'a>,
    opts: &'a Options<'o>,
    warnings: Vec<String>,
    components: Map<String, Value>,
}

pub fn generate(snap: &ServerSnapshot, opts: &Options) -> Document {
    let s = snap.server;
    let mut cx = Ctx {
        snap,
        opts,
        warnings: Vec::new(),
        components: Map::new(),
    };
    let mut stats = Stats::default();
    let mut unrep = Vec::new();

    // Winner per (method, path): highest priority, then file order.
    let mut winner: BTreeMap<(String, String), (i32, usize)> = BTreeMap::new();
    let mut entries: Vec<(usize, &Route, String, Vec<String>)> = Vec::new(); // route, doc path, methods
    for (i, r) in s.routes.iter().enumerate() {
        if skipped(s, r) {
            stats.skipped += 1;
            continue;
        }
        let Some(dp) = doc_path(r) else {
            stats.skipped += 1;
            unrep.push(r.id.clone());
            continue;
        };
        let methods = route_methods(r);
        let prio = r.priority.unwrap_or(0);
        let mut won = r.kind == "crud";
        for m in &methods {
            let k = (m.clone(), dp.clone());
            let better = winner.get(&k).is_none_or(|(p, _)| prio > *p);
            if better {
                winner.insert(k, (prio, i));
            }
        }
        for m in &methods {
            if winner.get(&(m.clone(), dp.clone())).map(|w| w.1) == Some(i) {
                won = true;
            }
        }
        if won {
            entries.push((i, r, dp, methods));
        } else {
            stats.skipped += 1;
        }
    }

    let mut paths: Map<String, Value> = Map::new();
    let mut used_tags: Vec<String> = Vec::new();
    for (i, r, dp, methods) in &entries {
        let tags = route_tags(s, r);
        for t in &tags {
            if !used_tags.contains(t) {
                used_tags.push(t.clone());
            }
        }
        let any = r.method.eq_ignore_ascii_case("ANY") && r.kind == "http";
        let mut documented = false;
        match r.kind.as_str() {
            "crud" => {
                for (m, key, op) in cx.crud_ops(r, dp, &tags) {
                    let target = paths
                        .entry(crud_path(dp, &key))
                        .or_insert_with(|| Value::Object(Map::new()));
                    target[m.to_lowercase()] = op;
                    documented = true;
                }
            }
            _ => {
                for m in methods {
                    if winner.get(&(m.clone(), dp.clone())).map(|w| w.1) != Some(*i) {
                        continue;
                    }
                    let mut op = cx.operation(r, dp, m, &tags);
                    if any {
                        op["x-fauxfield-any"] = Value::Bool(true);
                        let base = op_id(r);
                        op["operationId"] = format!("{base}_{}", m.to_lowercase()).into();
                    }
                    paths
                        .entry(dp.clone())
                        .or_insert_with(|| Value::Object(Map::new()))[m.to_lowercase()] = op;
                    documented = true;
                }
            }
        }
        if documented {
            stats.documented += 1;
        }
    }
    // Drop empty path items (a CRUD Route writes to its own two paths).
    paths.retain(|_, v| v.as_object().is_some_and(|o| !o.is_empty()));

    let mut tags_json: Vec<Value> = Vec::new();
    for f in &s.folders {
        if used_tags.contains(&f.name) {
            let mut t = json!({"name": f.name});
            if let Some(d) = &f.docs {
                t["description"] = d.clone().into();
            }
            tags_json.push(t);
        }
    }
    for t in &used_tags {
        if !tags_json.iter().any(|x| x["name"] == *t) {
            tags_json.push(json!({"name": t}));
        }
    }

    let d = s.docs_site.clone().unwrap_or_default();
    let mut info = Map::new();
    info.insert(
        "title".into(),
        d.title.clone().unwrap_or_else(|| s.name.clone()).into(),
    );
    info.insert(
        "version".into(),
        d.version.clone().unwrap_or_else(|| "1.0.0".into()).into(),
    );
    if let Some(desc) = d.description.clone().or_else(|| s.docs.clone()) {
        info.insert("description".into(), desc.into());
    }
    let mut doc = Map::new();
    doc.insert("openapi".into(), "3.1.0".into());
    doc.insert("info".into(), Value::Object(info));
    doc.insert(
        "servers".into(),
        json!([{"url": s.prefix.clone().filter(|p| !p.is_empty()).unwrap_or_else(|| "/".into())}]),
    );
    doc.insert("tags".into(), Value::Array(tags_json));
    doc.insert("paths".into(), Value::Object(paths));
    if !cx.components.is_empty() {
        doc.insert(
            "components".into(),
            json!({"schemas": Value::Object(cx.components.clone())}),
        );
    }
    Document {
        value: Value::Object(doc),
        stats,
        warnings: cx.warnings,
        unrepresentable: unrep,
    }
}

/// CRUD item operations live at `<collection>/{id}`.
fn crud_path(collection: &str, key: &str) -> String {
    match key {
        "item" => format!("{}/{{id}}", collection.trim_end_matches('/')),
        _ => collection.to_string(),
    }
}

fn skipped(s: &Server, r: &Route) -> bool {
    if r.response_mode.as_deref() == Some("disabled") {
        return true;
    }
    if let Some(f) = &r.folder
        && s.folders.iter().any(|x| &x.id == f && !x.enabled)
    {
        return true;
    }
    r.openapi.as_ref().is_some_and(|o| o.hidden)
}

fn doc_path(r: &Route) -> Option<String> {
    if r.match_mode.as_deref() == Some("regex") {
        return r
            .openapi
            .as_ref()
            .and_then(|o| o.path.clone())
            .map(|p| convert_path(&p).0);
    }
    let p = r
        .openapi
        .as_ref()
        .and_then(|o| o.path.clone())
        .unwrap_or_else(|| r.path.clone());
    let mut p = convert_path(&p).0;
    if r.kind == "static" {
        p = format!("{}/{{path}}", p.trim_end_matches('/'));
    }
    Some(p)
}

fn route_methods(r: &Route) -> Vec<String> {
    match r.kind.as_str() {
        "static" | "websocket" => vec!["GET".into()],
        "crud" => vec![],
        _ if r.method.eq_ignore_ascii_case("ANY") => {
            ANY_METHODS.iter().map(|m| m.to_string()).collect()
        }
        _ => vec![r.method.to_uppercase()],
    }
}

/// `:id`, `{id}` and `*rest` become `{id}` / `{rest}`; returns the path and its parameters.
pub(crate) fn convert_path(p: &str) -> (String, Vec<(String, bool)>) {
    let mut params = Vec::new();
    let segs: Vec<String> = p
        .split('/')
        .map(|seg| {
            if let Some(n) = seg.strip_prefix(':') {
                params.push((n.to_string(), false));
                format!("{{{n}}}")
            } else if let Some(n) = seg.strip_prefix('*') {
                let n = if n.is_empty() { "wildcard" } else { n };
                params.push((n.to_string(), true));
                format!("{{{n}}}")
            } else if seg.starts_with('{') && seg.ends_with('}') && seg.len() > 2 {
                params.push((seg[1..seg.len() - 1].to_string(), false));
                seg.to_string()
            } else {
                seg.to_string()
            }
        })
        .collect();
    (segs.join("/"), params)
}

fn route_tags(s: &Server, r: &Route) -> Vec<String> {
    if let Some(o) = &r.openapi
        && !o.tags.is_empty()
    {
        return o.tags.clone();
    }
    if r.kind == "websocket" {
        return vec!["WebSocket".into()];
    }
    if let Some(f) = &r.folder
        && let Some(f) = s.folders.iter().find(|x| &x.id == f)
    {
        return vec![f.name.clone()];
    }
    vec![OTHER_TAG.into()]
}

fn op_id(r: &Route) -> String {
    r.openapi
        .as_ref()
        .and_then(|o| o.operation_id.clone())
        .unwrap_or_else(|| r.id.clone())
}

fn pascal(name: &str) -> String {
    name.split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| {
            let mut c = w.chars();
            c.next()
                .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
                .unwrap_or_default()
        })
        .collect()
}

fn toml_json(v: &toml::Value) -> Value {
    serde_json::to_value(v).unwrap_or(Value::Null)
}

fn rule_text(r: &Rule) -> String {
    let v = r
        .value
        .as_ref()
        .map(|v| match v {
            toml::Value::String(s) => s.clone(),
            o => o.to_string(),
        })
        .unwrap_or_default();
    let neg = if r.negate { "not " } else { "" };
    format!(
        "{} {neg}{} {v}",
        r.key.clone().unwrap_or_else(|| r.target.clone()),
        r.op
    )
    .trim()
    .to_string()
}

fn media_type(ct: Option<&str>) -> String {
    ct.map(|c| c.split(';').next().unwrap_or(c).trim().to_string())
        .filter(|c| !c.is_empty())
        .unwrap_or_else(|| "application/json".into())
}

impl Ctx<'_, '_> {
    fn read_json(&mut self, rel: &str) -> Option<Value> {
        let root = self.snap.root?;
        if rel.starts_with('/') || rel.split(['/', '\\']).any(|c| c == "..") {
            return None;
        }
        match std::fs::read_to_string(root.join(rel))
            .map_err(|e| e.to_string())
            .and_then(|t| serde_json::from_str::<Value>(&t).map_err(|e| e.to_string()))
        {
            Ok(v) => Some(v),
            Err(e) => {
                self.warnings.push(format!("schema file `{rel}`: {e}"));
                None
            }
        }
    }

    fn dataset_schema(&mut self, ds: &Dataset, assigned_id: bool) -> Value {
        let key = ds.key_name().to_string();
        let rows = self.snap.seeds.get(&ds.name).cloned().unwrap_or_default();
        let mut schema = if !ds.columns.is_empty() {
            let mut props = Map::new();
            let mut required = Vec::new();
            for c in &ds.columns {
                let mut cs = column_schema(c);
                if (c.primary_key || c.name == key) && assigned_id {
                    cs["readOnly"] = Value::Bool(true);
                }
                if c.required {
                    required.push(Value::String(c.name.clone()));
                }
                props.insert(c.name.clone(), cs);
            }
            let mut o = json!({"type": "object", "properties": props});
            if !required.is_empty() {
                o["required"] = Value::Array(required);
            }
            o
        } else {
            let sample: Vec<&Value> = rows.iter().take(100).collect();
            let mut o = infer_many(sample.into_iter())
                .unwrap_or_else(|| json!({"type": "object", "properties": {}}));
            if let Some(p) = o.get_mut("properties").and_then(|p| p.as_object_mut()) {
                if !p.contains_key(&key) {
                    p.insert(key.clone(), json!({"type": "string"}));
                }
                if assigned_id && let Some(k) = p.get_mut(&key) {
                    k["readOnly"] = Value::Bool(true);
                }
            }
            o
        };
        if let Some(d) = &ds.description {
            schema["description"] = d.clone().into();
        }
        if let Some(first) = rows.first() {
            schema["example"] = first.clone();
        }
        schema
    }

    fn component(&mut self, ds: &Dataset, assigned_id: bool) -> Value {
        let name = pascal(&ds.name);
        if !self.components.contains_key(&name) {
            let sch = self.dataset_schema(ds, assigned_id);
            self.components.insert(name.clone(), sch);
        }
        json!({"$ref": format!("#/components/schemas/{name}")})
    }

    fn summary_desc(&self, r: &Route, method: &str, dp: &str) -> (String, Option<String>) {
        let o = r.openapi.as_ref();
        let summary = o
            .and_then(|o| o.summary.clone())
            .or_else(|| r.name.clone())
            .unwrap_or_else(|| format!("{method} {dp}"));
        let desc = o
            .and_then(|o| o.description.clone())
            .or_else(|| r.docs.clone());
        (summary, desc)
    }

    fn path_params(&self, dp: &str, key_schema: Option<&Value>) -> Vec<Value> {
        let (_, params) = convert_path(dp);
        params
            .into_iter()
            .map(|(n, wild)| {
                let mut p = json!({"name": n, "in": "path", "required": true,
                    "schema": key_schema.filter(|_| n == "id").cloned().unwrap_or(json!({"type": "string"}))});
                if wild {
                    p["description"] = "May contain `/`; Swagger UI encodes it, so Try it out cannot send slashes here.".into();
                }
                p
            })
            .collect()
    }

    /// Query/header parameters derived from Rules plus the Rule summary list.
    fn rule_params(&self, r: &Route) -> (Vec<Value>, Vec<String>, Option<Value>) {
        let mut params: Vec<Value> = Vec::new();
        let mut other = Vec::new();
        let mut example = Map::new();
        for resp in r.responses.iter().filter(|x| !x.disabled) {
            for rule in &resp.rules {
                match (rule.target.as_str(), rule.key.as_deref()) {
                    ("query" | "header", Some(k)) => {
                        let loc = rule.target.as_str();
                        if !params.iter().any(|p| p["name"] == k && p["in"] == loc) {
                            params.push(json!({"name": k, "in": loc, "required": false,
                                "description": rule_text(rule), "schema": {"type": "string"}}));
                        }
                    }
                    ("body", Some(k)) => {
                        other.push(format!("body {}", rule_text(rule)));
                        if rule.op == "eq"
                            && let Some(path) = k.strip_prefix("$.")
                            && !path.contains(['[', '*', '?'])
                            && let Some(v) = &rule.value
                        {
                            set_path(&mut example, path, toml_json(v));
                        }
                    }
                    (t, _) => other.push(format!("{t} {}", rule_text(rule))),
                }
            }
        }
        let ex = (!example.is_empty()).then_some(Value::Object(example));
        (params, other, ex)
    }

    fn has_body_rule(r: &Route) -> bool {
        r.responses
            .iter()
            .any(|x| x.rules.iter().any(|ru| ru.target == "body"))
    }

    fn operation(&mut self, r: &Route, dp: &str, method: &str, tags: &[String]) -> Value {
        let (summary, desc) = self.summary_desc(r, method, dp);
        let mut op = Map::new();
        op.insert("tags".into(), json!(tags));
        op.insert("summary".into(), summary.into());
        let mut description = desc.unwrap_or_default();
        if r.responses.iter().any(|x| !x.callbacks.is_empty()) {
            if !description.is_empty() {
                description.push_str("\n\n");
            }
            description.push_str("This Route fires Callbacks after responding (not documented).");
        }
        if r.kind == "websocket" {
            description = format!(
                "WebSocket endpoint. Use a WebSocket client; Try it out cannot drive it.{}",
                if description.is_empty() {
                    String::new()
                } else {
                    format!("\n\n{description}")
                }
            );
        }
        if !description.is_empty() {
            op.insert("description".into(), description.into());
        }
        op.insert("operationId".into(), op_id(r).into());
        if r.kind == "websocket" {
            op.insert("x-fauxfield-websocket".into(), true.into());
            op.insert(
                "responses".into(),
                json!({"101": {"description": "Switching Protocols"}}),
            );
            let mut p = self.path_params(dp, None);
            p.sort_by_key(|_| 0);
            if !p.is_empty() {
                op.insert("parameters".into(), Value::Array(p));
            }
            return Value::Object(op);
        }
        let (rparams, other_rules, example) = self.rule_params(r);
        let mut params = self.path_params(dp, None);
        params.extend(rparams);
        if !params.is_empty() {
            op.insert("parameters".into(), Value::Array(params));
        }
        if !other_rules.is_empty() {
            op.insert("x-fauxfield-rules".into(), json!(other_rules));
        }
        if matches!(method, "POST" | "PUT" | "PATCH") || Self::has_body_rule(r) {
            let mut sch = json!({});
            if let Some(f) = r.openapi.as_ref().and_then(|o| o.request_schema.clone())
                && let Some(v) = self.read_json(&f)
            {
                sch = v;
            }
            let mut media = json!({"schema": sch});
            if let Some(e) = example {
                media["example"] = e;
            }
            op.insert(
                "requestBody".into(),
                json!({"required": false, "content": {"application/json": media}}),
            );
        }
        if r.kind == "static" {
            op.insert(
                "responses".into(),
                json!({"200": {"description": "File contents", "content": {"*/*": {"schema": {"type": "string", "format": "binary"}}}},
                       "404": {"description": "Not found"}}),
            );
        } else {
            op.insert(
                "responses".into(),
                self.responses(r, r.responses.iter().collect()),
            );
        }
        Value::Object(op)
    }

    fn responses(&mut self, r: &Route, resps: Vec<&Response>) -> Value {
        let mut by_status: Vec<(u16, Vec<&Response>)> = Vec::new();
        for resp in resps.into_iter().filter(|x| !x.disabled) {
            match by_status.iter_mut().find(|(s, _)| *s == resp.status) {
                Some((_, v)) => v.push(resp),
                None => by_status.push((resp.status, vec![resp])),
            }
        }
        let mut out = Map::new();
        for (status, group) in by_status {
            out.insert(status.to_string(), self.response_group(r, status, &group));
        }
        if out.is_empty() {
            out.insert("200".into(), json!({"description": "OK"}));
        }
        Value::Object(out)
    }

    fn response_group(&mut self, _r: &Route, status: u16, group: &[&Response]) -> Value {
        let mut obj = Map::new();
        let first = group[0];
        obj.insert(
            "description".into(),
            first
                .description
                .clone()
                .or_else(|| first.name.clone())
                .unwrap_or_else(|| reason(status).to_string())
                .into(),
        );
        let mut headers = Map::new();
        for resp in group {
            for k in resp.headers.keys() {
                if !k.eq_ignore_ascii_case("content-type") {
                    headers.insert(k.clone(), json!({"schema": {"type": "string"}}));
                }
            }
        }
        if !headers.is_empty() {
            obj.insert("headers".into(), Value::Object(headers));
        }
        let mut content: Map<String, Value> = Map::new();
        for resp in group {
            if resp
                .fault
                .as_deref()
                .is_some_and(|f| matches!(f, "reset" | "timeout" | "close"))
            {
                continue;
            }
            let Some(body) = self.body_text(resp) else {
                continue;
            };
            let mt = media_type(resp.content_type.as_deref().or_else(|| {
                resp.headers
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
                    .map(|(_, v)| v.as_str())
            }));
            let parsed = serde_json::from_str::<Value>(&body).ok();
            let entry = content.entry(mt.clone()).or_insert_with(|| json!({}));
            let mut schema = None;
            if let Some(f) = &resp.schema
                && let Some(v) = self.read_json(f)
            {
                schema = Some(v);
            }
            if schema.is_none()
                && let Some(p) = &parsed
            {
                schema = Some(infer(p));
            }
            if entry.get("schema").is_none()
                && let Some(s) = schema
            {
                entry["schema"] = s;
            }
            let ex = parsed.clone().unwrap_or(Value::String(body));
            let key = resp.name.clone().unwrap_or_else(|| resp.id.clone());
            let mut ex_obj = json!({"value": ex});
            if !resp.rules.is_empty() {
                ex_obj["summary"] = resp
                    .rules
                    .iter()
                    .map(rule_text)
                    .collect::<Vec<_>>()
                    .join(", ")
                    .into();
            }
            entry["examples"][key] = ex_obj;
        }
        if !content.is_empty() {
            obj.insert("content".into(), Value::Object(content));
        }
        Value::Object(obj)
    }

    /// Body text for a Response: static, `body_file`, or one preview render of a Tera body.
    fn body_text(&mut self, resp: &Response) -> Option<String> {
        let raw = match (&resp.body, &resp.body_file) {
            (Some(b), _) => b.clone(),
            (None, Some(f)) => {
                let root = self.snap.root?;
                if f.starts_with('/') || f.split(['/', '\\']).any(|c| c == "..") {
                    return None;
                }
                std::fs::read_to_string(root.join(f)).ok()?
            }
            _ => return None,
        };
        if raw.is_empty() {
            return None;
        }
        let tera = resp.template != Some(false) && (raw.contains("{{") || raw.contains("{%"));
        if !tera {
            return Some(raw);
        }
        match self.opts.renderer {
            Some(render) => match render(&raw) {
                Ok(t) => Some(t),
                Err(e) => {
                    self.warnings.push(format!("response `{}`: {e}", resp.id));
                    None
                }
            },
            None => None,
        }
    }

    fn crud_ops(&mut self, r: &Route, dp: &str, tags: &[String]) -> Vec<(String, String, Value)> {
        let Some(ds) = r
            .dataset
            .as_deref()
            .and_then(|d| self.snap.server.datasets.iter().find(|x| x.name == d))
            .cloned()
        else {
            return vec![];
        };
        let ops: Vec<String> = if r.crud_ops.is_empty() {
            fauxfield_model::CRUD_OPS
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            r.crud_ops.clone()
        };
        let assigned = true;
        let key = ds.key_name().to_string();
        let item_ref = self.component(&ds, assigned);
        let id_schema = ds
            .columns
            .iter()
            .find(|c| c.name == key)
            .map(column_schema)
            .unwrap_or(json!({"type": "string"}));
        let mut body_schema = item_ref.clone();
        if let Some(f) = &r.validate
            && let Some(v) = self.read_json(f)
        {
            body_schema = v;
        }
        let create_schema = if r.validate.is_some() {
            body_schema.clone()
        } else {
            let name = pascal(&ds.name);
            let mut base = self.components.get(&name).cloned().unwrap_or(json!({}));
            if let Some(p) = base.get_mut("properties").and_then(|p| p.as_object_mut()) {
                p.shift_remove(&key);
            }
            if let Some(req) = base.get_mut("required").and_then(|x| x.as_array_mut()) {
                req.retain(|x| x != &Value::String(key.clone()));
            }
            base.as_object_mut().map(|o| o.remove("example"));
            base
        };
        let (rparams, _, _) = self.rule_params(r);
        let base_id = op_id(r);
        let (summary_base, desc) = self.summary_desc(r, "", dp);
        let _ = summary_base;
        let mut out = Vec::new();
        let coll = dp.trim_end_matches('/').to_string();
        let item_path = format!("{coll}/{{id}}");
        let mut id_params = self.path_params(&coll, None);
        id_params.push(json!({"name": "id", "in": "path", "required": true, "schema": id_schema}));
        let _ = item_path;
        for op in ops {
            let (method, which) = match op.as_str() {
                "list" => ("GET", "collection"),
                "create" => ("POST", "collection"),
                "get" => ("GET", "item"),
                "update" => ("PUT", "item"),
                "patch" => ("PATCH", "item"),
                "delete" => ("DELETE", "item"),
                _ => continue,
            };
            let mut o = Map::new();
            o.insert("tags".into(), json!(tags));
            o.insert(
                "summary".into(),
                format!("{} {}", titlecase(&op), ds.name).into(),
            );
            if let Some(d) = &desc {
                o.insert("description".into(), d.clone().into());
            }
            o.insert("operationId".into(), format!("{base_id}_{op}").into());
            let mut params = if which == "item" {
                id_params.clone()
            } else {
                self.path_params(&coll, None)
            };
            params.extend(rparams.iter().cloned());
            let mut responses = Map::new();
            match op.as_str() {
                "list" => {
                    for (n, t) in [
                        ("_page", "integer"),
                        ("_limit", "integer"),
                        ("_sort", "string"),
                        ("_order", "string"),
                        ("q", "string"),
                    ] {
                        let mut sch = json!({"type": t});
                        if n == "_order" {
                            sch["enum"] = json!(["asc", "desc"]);
                        }
                        params.push(
                            json!({"name": n, "in": "query", "required": false, "schema": sch}),
                        );
                    }
                    for c in &ds.columns {
                        // A column default fills new rows; as a filter default it would make
                        // Swagger UI send `?price=0` and hide rows, so it is left out here.
                        let mut sch = column_schema(c);
                        if let Some(o) = sch.as_object_mut() {
                            o.remove("default");
                        }
                        params.push(json!({"name": c.name, "in": "query", "required": false,
                            "description": "Equality filter", "schema": sch}));
                    }
                    responses.insert("200".into(), json!({"description": "List of items",
                        "headers": {"X-Total-Count": {"schema": {"type": "integer"}}, "Link": {"schema": {"type": "string"}}},
                        "content": {"application/json": {"schema": {"type": "array", "items": item_ref.clone()}}}}));
                }
                "get" => {
                    responses.insert("200".into(), json!({"description": "The item", "content": {"application/json": {"schema": item_ref.clone()}}}));
                    responses.insert("404".into(), json!({"description": "Not found"}));
                }
                "create" => {
                    responses.insert(
                        "201".into(),
                        json!({"description": "Created",
                        "headers": {"Location": {"schema": {"type": "string"}}},
                        "content": {"application/json": {"schema": item_ref.clone()}}}),
                    );
                }
                "update" | "patch" => {
                    responses.insert("200".into(), json!({"description": "The updated item", "content": {"application/json": {"schema": item_ref.clone()}}}));
                    responses.insert("404".into(), json!({"description": "Not found"}));
                }
                _ => {
                    responses.insert("204".into(), json!({"description": "Deleted"}));
                    responses.insert("404".into(), json!({"description": "Not found"}));
                }
            }
            if r.validate.is_some() && matches!(op.as_str(), "create" | "update" | "patch") {
                responses.insert("422".into(), json!({"description": "Validation failed"}));
            }
            if matches!(op.as_str(), "create" | "update" | "patch") {
                let sch = if op == "create" {
                    create_schema.clone()
                } else {
                    body_schema.clone()
                };
                o.insert(
                    "requestBody".into(),
                    json!({"required": true, "content": {"application/json": {"schema": sch}}}),
                );
            }
            o.insert("parameters".into(), Value::Array(params));
            // M17.1 operation Responses, when present, become extra examples.
            if let Some(o) = r.operations.get(&op) {
                self.operation_examples(&o.responses, &mut responses);
            }
            o.insert("responses".into(), Value::Object(responses));
            out.push((method.to_string(), which.to_string(), Value::Object(o)));
        }
        out
    }

    fn operation_examples(
        &mut self,
        list: &[fauxfield_model::Response],
        responses: &mut Map<String, Value>,
    ) {
        for rv in list.iter().filter(|r| !r.disabled) {
            let status = rv.status.to_string();
            // A Response `schema` file describes what this operation returns for that status
            // (it replaces the built-in shape, e.g. an envelope instead of the item array).
            if let Some(f) = &rv.schema
                && let Some(sch) = self.read_json(f)
            {
                let entry = responses
                    .entry(status.clone())
                    .or_insert_with(|| json!({"description": "Response"}));
                entry["content"]["application/json"]["schema"] = sch;
            }
            // Same rule as other Responses: Tera bodies are rendered once in preview mode, and
            // an example that cannot be rendered is left out rather than shown as a template.
            let Some(body) = self.body_text(rv) else {
                continue;
            };
            let name = rv.name.as_deref().unwrap_or(&rv.id);
            let val = serde_json::from_str::<Value>(&body).unwrap_or(Value::String(body));
            let entry = responses
                .entry(status)
                .or_insert_with(|| json!({"description": "Response"}));
            entry["content"]["application/json"]["examples"][name] = json!({"value": val});
        }
    }
}

fn titlecase(s: &str) -> String {
    let mut c = s.chars();
    c.next()
        .map(|f| f.to_uppercase().collect::<String>() + c.as_str())
        .unwrap_or_default()
}

fn set_path(m: &mut Map<String, Value>, path: &str, v: Value) {
    match path.split_once('.') {
        None => {
            m.insert(path.to_string(), v);
        }
        Some((h, rest)) => {
            let e = m.entry(h.to_string()).or_insert_with(|| json!({}));
            if let Some(o) = e.as_object_mut() {
                set_path(o, rest, v);
            }
        }
    }
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        202 => "Accepted",
        204 => "No Content",
        301 | 302 | 307 | 308 => "Redirect",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        422 => "Unprocessable Entity",
        429 => "Too Many Requests",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        _ => "Response",
    }
}
