//! Workspace product tools (M2, SPEC §11.1): read and edit servers and routes through
//! `fauxfield-store`, the same `toml_edit` path the GUI uses. Writes are flushed to disk at once.

use fauxfield_model::{Route, Server, Severity};
use fauxfield_store::{Store, Target};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    class::ToolClass,
    registry::{Registry, ToolDef},
};

#[derive(Deserialize, JsonSchema)]
struct NoInput {}

#[derive(Deserialize, JsonSchema)]
struct ServerRef {
    /// Server ID (`srv_…`), name, or file slug.
    server: String,
}

#[derive(Deserialize, JsonSchema)]
struct OptServer {
    /// Only this Server (ID, name, or file slug).
    server: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct CreateServer {
    name: String,
    port: u16,
}

#[derive(Deserialize, JsonSchema)]
struct UpdateServer {
    /// Server ID (`srv_…`), name, or file slug.
    server: String,
    name: Option<String>,
    port: Option<u16>,
    host: Option<String>,
    enabled: Option<bool>,
    /// Must start with `/` and not end with `/`; an empty string removes the prefix.
    prefix: Option<String>,
    /// Add the closest-routes hint to 404 bodies (default true).
    unmatched_hint: Option<bool>,
    /// Seed for fake data and random template functions; 0 or omitted keeps it, `clear_seed` removes it.
    fake_seed: Option<u64>,
    /// Remove the seed (output becomes random).
    clear_seed: Option<bool>,
}

#[derive(Deserialize, JsonSchema)]
struct UpdateWorkspace {
    /// New workspace name (1 to 80 characters). The folder on disk is not renamed.
    name: String,
}

#[derive(Deserialize, JsonSchema)]
struct SetVariable {
    /// `global`, `environment`, `server` or `route`.
    scope: String,
    /// Environment name, Server ID or Route ID (not needed for `global`).
    #[serde(default)]
    owner: Option<String>,
    /// Variable name.
    key: String,
    /// New value; omit or null to remove the variable.
    #[serde(default)]
    value: Option<String>,
}

#[derive(Deserialize, JsonSchema)]
struct RouteRef {
    /// Route ID (`rte_…`).
    route: String,
}

#[derive(Deserialize, JsonSchema)]
struct ResponseSpec {
    /// Existing Response ID to update; omit to add one.
    id: Option<String>,
    name: Option<String>,
    status: Option<u16>,
    content_type: Option<String>,
    body: Option<String>,
    delay_ms: Option<u64>,
    default: Option<bool>,
    headers: Option<std::collections::BTreeMap<String, String>>,
    /// One-line note shown under the name; not sent.
    description: Option<String>,
    /// Skip this Response during selection.
    disabled: Option<bool>,
    /// `false` sends body, body_file and header values verbatim (default true: Tera).
    template: Option<bool>,
    /// `and` (default) or `or`.
    rules_logic: Option<String>,
    /// Replaces all rules of the Response when given. Fields: target, key, op, value, negate.
    rules: Option<Vec<fauxfield_model::Rule>>,
}

#[derive(Deserialize, JsonSchema)]
struct UpsertRoute {
    /// Server ID, name, or file slug.
    server: String,
    /// Existing Route ID to update; omit to create.
    route: Option<String>,
    /// Folder ID (`fld_…`).
    folder: Option<String>,
    method: Option<String>,
    path: Option<String>,
    name: Option<String>,
    /// Match mode: exact, param or regex; omit to keep (create: inferred).
    #[serde(rename = "match")]
    #[schemars(rename = "match")]
    match_mode: Option<String>,
    /// Higher wins; 0 is the default and is not written.
    priority: Option<i32>,
    /// `rules` (default), `sequential`, `random` or `disabled`.
    response_mode: Option<String>,
    /// Route headers (values are templated); set or replace by name.
    headers: Option<std::collections::BTreeMap<String, String>>,
    responses: Option<Vec<ResponseSpec>>,
}

#[derive(Deserialize, JsonSchema)]
struct RenderTemplate {
    /// Route ID (`rte_…`) whose Response is rendered.
    route: Option<String>,
    /// Response ID; default: the Route's default or first Response.
    response: Option<String>,
    /// A raw template string to render instead of a Response (needs `route` or `server` for context).
    template: Option<String>,
    /// Server (ID, name or slug) providing variables and `fake_seed` when no `route` is given.
    server: Option<String>,
    /// Sample request method (default: the Route's method).
    method: Option<String>,
    /// Sample request path and query (default: the Route path).
    url: Option<String>,
    /// Sample request headers.
    headers: Option<std::collections::BTreeMap<String, String>>,
    /// Sample request body.
    body: Option<String>,
    /// Override the Server's `fake_seed` for this render.
    seed: Option<u64>,
}

#[derive(Deserialize, JsonSchema)]
struct MatchPath {
    /// Server ID, name, or file slug.
    server: String,
    method: String,
    /// Request path (with the Server prefix), query ignored.
    path: String,
}

fn parse<T: for<'de> Deserialize<'de>>(m: Map<String, Value>) -> Result<T, String> {
    serde_json::from_value(Value::Object(m)).map_err(|e| format!("invalid arguments: {e}"))
}

fn is_secret(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    ["token", "secret", "password", "key"]
        .iter()
        .any(|s| k.contains(s))
}

fn vars_json(vars: &std::collections::BTreeMap<String, toml::Value>) -> Value {
    let mut m = Map::new();
    for (k, v) in vars {
        let v = if is_secret(k) {
            json!("<redacted>")
        } else {
            serde_json::to_value(v).unwrap_or(Value::Null)
        };
        m.insert(k.clone(), v);
    }
    Value::Object(m)
}

fn slug(file: &str) -> String {
    std::path::Path::new(file)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn find_server(store: &Store, q: &str) -> Result<(String, Server), String> {
    let snap = store.snapshot();
    snap.servers
        .into_iter()
        .find(|(f, s)| s.id == q || s.name == q || slug(f) == q)
        .ok_or_else(|| format!("no server `{q}`; use list_servers"))
}

fn find_route(store: &Store, id: &str) -> Result<(Server, Route), String> {
    let snap = store.snapshot();
    for (_, s) in snap.servers {
        if let Some(r) = s.routes.iter().find(|r| r.id == id).cloned() {
            return Ok((s, r));
        }
    }
    Err(format!("no route `{id}`; use list_routes"))
}

fn summary(file: &str, s: &Server) -> Value {
    json!({ "id": s.id, "name": s.name, "file": file, "host": s.host, "port": s.port, "enabled": s.enabled, "prefix": s.prefix, "unmatched_hint": s.unmatched_hint.unwrap_or(true), "routes": s.routes.len(), "folders": s.folders.len() })
}

fn shadowed_by(s: &Server, id: &str) -> Option<String> {
    fauxfield_model::pattern::shadowed_routes(s)
        .into_iter()
        .find(|x| x.route == id)
        .map(|x| x.by)
}

fn route_json(s: &Server, r: &Route) -> Value {
    let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
    v["server"] = json!(s.id);
    v["shadowed_by"] = json!(shadowed_by(s, &r.id));
    v
}

fn e<T: std::fmt::Display>(x: T) -> String {
    x.to_string()
}

fn apply_response(store: &Store, id: &str, r: &ResponseSpec) -> Result<(), String> {
    let t = Target::Response(id.into());
    if let Some(v) = &r.name {
        store.set_str(&t, "name", Some(v)).map_err(e)?;
    }
    if let Some(v) = r.status {
        store.set_int(&t, "status", Some(v.into())).map_err(e)?;
    }
    if let Some(v) = &r.content_type {
        store.set_str(&t, "content_type", Some(v)).map_err(e)?;
    }
    if let Some(v) = &r.body {
        store.set_str(&t, "body", Some(v)).map_err(e)?;
    }
    if let Some(v) = r.delay_ms {
        store.set_int(&t, "delay_ms", Some(v as i64)).map_err(e)?;
    }
    for (k, v) in r.headers.iter().flatten() {
        store.set_header(id, k, Some(v)).map_err(e)?;
    }
    if r.default == Some(true) {
        store.set_default_response(id).map_err(e)?;
    }
    if let Some(v) = &r.description {
        store
            .set_str(
                &t,
                "description",
                Some(v).filter(|v| !v.is_empty()).map(String::as_str),
            )
            .map_err(e)?;
    }
    if let Some(v) = r.disabled {
        store.set_response_disabled(id, v).map_err(e)?;
    }
    if let Some(v) = r.template {
        store.set_response_template(id, v).map_err(e)?;
    }
    if let Some(v) = &r.rules_logic {
        store.set_rules_logic(id, v).map_err(e)?;
    }
    if let Some(rules) = &r.rules {
        for rule in rules {
            fauxfield_model::rules::check_rule(rule).map_err(|(c, m)| format!("{c}: {m}"))?;
        }
        let have = store
            .snapshot()
            .servers
            .iter()
            .flat_map(|(_, s)| s.routes.iter())
            .flat_map(|r| r.responses.iter())
            .find(|x| x.id == id)
            .map_or(0, |x| x.rules.len());
        for _ in 0..have {
            store.delete_rule(id, 0).map_err(e)?;
        }
        for rule in rules {
            store.set_rule(id, None, rule).map_err(e)?;
        }
    }
    Ok(())
}

/// Where the tools find the workspace: fixed for `fauxfield mcp`, swappable inside the app.
pub type StoreProvider = std::sync::Arc<dyn Fn() -> Option<Store> + Send + Sync>;

fn no_workspace() -> crate::registry::ToolFuture {
    Box::pin(async {
        Err("no workspace is open; pass a PATH or call ui.open_workspace".to_string())
    })
}

pub fn fixed_store(store: Store) -> StoreProvider {
    std::sync::Arc::new(move || Some(store.clone()))
}

pub fn register_workspace_tools(r: &mut Registry, provider: StoreProvider) {
    let p = provider.clone();
    r.register(ToolDef::new::<NoInput>(
        "workspace_info",
        "Workspace manifest, environments, servers and a validation summary.",
        ToolClass::Read,
        move |_| {
        let Some(s) = p() else { return no_workspace() };
            let snap = s.snapshot();
            let errors = snap.diagnostics.iter().filter(|d| d.severity == Severity::Error).count();
            let v = json!({
                "root": snap.root.to_string_lossy(),
                "name": snap.manifest.name,
                "default_environment": snap.manifest.default_environment,
                "environments": snap.environments.iter().map(|(_, e)| e.name.clone()).collect::<Vec<_>>(),
                "servers": snap.servers.iter().map(|(f, s)| summary(f, s)).collect::<Vec<_>>(),
                "validation": { "errors": errors, "warnings": snap.diagnostics.len() - errors },
            });
            Box::pin(async move { Ok(v) })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<UpdateWorkspace>(
        "update_workspace",
        "Rename the workspace (the `name` in fauxfield.toml; comments and key order are kept).",
        ToolClass::Write,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<UpdateWorkspace>(m).and_then(|a| {
                s.set_workspace_name(&a.name).map_err(e)?;
                s.flush().map_err(e)?;
                Ok(json!({ "name": s.snapshot().manifest.name }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<SetVariable>(
        "set_variable",
        "Set or remove a variable at a scope (global, environment, server, route). Values for secret-looking names (*token*, *secret*, *password*, *key*) are never echoed back.",
        ToolClass::Write,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<SetVariable>(m).and_then(|a| {
                let scope = fauxfield_model::vars::Scope::parse(&a.scope)
                    .filter(|sc| *sc != fauxfield_model::vars::Scope::Runtime)
                    .ok_or_else(|| "scope must be global, environment, server or route".to_string())?;
                s.set_variable(scope, a.owner.as_deref(), &a.key, a.value.as_deref()).map_err(e)?;
                s.flush().map_err(e)?;
                let secret = fauxfield_model::vars::is_secret_key(&a.key);
                Ok(json!({
                    "key": a.key,
                    "scope": scope.as_str(),
                    "removed": a.value.is_none(),
                    "value": a.value.filter(|_| !secret),
                }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<NoInput>("list_servers", "List Servers.", ToolClass::Read, move |_| {
        let Some(s) = p() else { return no_workspace() };
        let v = json!({ "servers": s.snapshot().servers.iter().map(|(f, x)| summary(f, x)).collect::<Vec<_>>() });
        Box::pin(async move { Ok(v) })
    }));
    let p = provider.clone();
    r.register(ToolDef::new::<ServerRef>(
        "get_server",
        "Full Server including folders and routes. Secret-looking variables are redacted.",
        ToolClass::Read,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<ServerRef>(m).and_then(|a| {
                let (f, srv) = find_server(&s, &a.server)?;
                let mut v = serde_json::to_value(&srv).map_err(e)?;
                v["variables"] = vars_json(&srv.variables);
                v["file"] = json!(f);
                Ok(v)
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<CreateServer>(
        "create_server",
        "Create a Server (writes servers/<slug>.toml).",
        ToolClass::Write,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<CreateServer>(m).and_then(|a| {
                let id = s.create_server(&a.name, a.port).map_err(e)?;
                s.flush().map_err(e)?;
                Ok(json!({ "id": id }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<UpdateServer>(
        "update_server",
        "Update Server fields (name, port, host, enabled, prefix, unmatched_hint).",
        ToolClass::Write,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<UpdateServer>(m).and_then(|a| {
                let (_, srv) = find_server(&s, &a.server)?;
                let t = Target::Server(srv.id.clone());
                if let Some(v) = &a.name {
                    s.rename_server(&srv.id, v).map_err(e)?;
                }
                if let Some(v) = a.port {
                    s.set_int(&t, "port", Some(v.into())).map_err(e)?;
                }
                if let Some(v) = &a.host {
                    let v = fauxfield_model::validate_host(v)?;
                    s.set_str(&t, "host", Some(&v)).map_err(e)?;
                }
                if let Some(v) = a.enabled {
                    s.set_bool(&t, "enabled", v).map_err(e)?;
                }
                if let Some(v) = &a.prefix {
                    let v = fauxfield_model::validate_prefix(v)?;
                    s.set_str(&t, "prefix", v.as_deref()).map_err(e)?;
                }
                if let Some(v) = a.unmatched_hint {
                    s.set_bool(&t, "unmatched_hint", v).map_err(e)?;
                }
                if a.clear_seed == Some(true) {
                    s.set_fake_seed(&srv.id, None).map_err(e)?;
                } else if let Some(v) = a.fake_seed {
                    s.set_fake_seed(&srv.id, Some(v)).map_err(e)?;
                }
                s.flush().map_err(e)?;
                Ok(json!({ "id": srv.id }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<ServerRef>(
        "delete_server",
        "Delete a Server and its file.",
        ToolClass::Write,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<ServerRef>(m).and_then(|a| {
                let (_, srv) = find_server(&s, &a.server)?;
                s.delete_server(&srv.id).map_err(e)?;
                Ok(json!({ "deleted": srv.id }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<OptServer>("list_routes", "List Routes, optionally of one Server.", ToolClass::Read, move |m| {
        let Some(s) = p() else { return no_workspace() };
        let out = parse::<OptServer>(m).map(|a| {
            let snap = s.snapshot();
            let mut rows = vec![];
            for (f, srv) in &snap.servers {
                if a.server.as_ref().is_some_and(|q| *q != srv.id && *q != srv.name && *q != slug(f)) {
                    continue;
                }
                rows.extend(srv.routes.iter().map(|r| json!({ "id": r.id, "server": srv.id, "folder": r.folder, "kind": r.kind, "method": r.method, "path": r.path, "name": r.name, "responses": r.responses.len(), "match": r.match_mode, "priority": r.priority.unwrap_or(0), "shadowed_by": shadowed_by(srv, &r.id) })));
            }
            json!({ "routes": rows })
        });
        Box::pin(async move { out })
    }));
    let p = provider.clone();
    r.register(ToolDef::new::<RouteRef>(
        "get_route",
        "A Route with its Responses.",
        ToolClass::Read,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<RouteRef>(m)
                .and_then(|a| find_route(&s, &a.route).map(|(srv, r)| route_json(&srv, &r)));
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<UpsertRoute>(
        "upsert_route",
        "Create or update a Route and its Responses in one call.",
        ToolClass::Write,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<UpsertRoute>(m).and_then(|a| {
                let (_, srv) = find_server(&s, &a.server)?;
                let rid = match &a.route {
                    Some(id) => {
                        find_route(&s, id)?;
                        id.clone()
                    }
                    None => {
                        let path = a
                            .path
                            .as_deref()
                            .ok_or("path is required to create a route")?;
                        s.create_route_with(
                            &srv.id,
                            a.folder.as_deref(),
                            a.method.as_deref().unwrap_or("GET"),
                            path,
                            &fauxfield_store::RouteOpts {
                                match_mode: a.match_mode.clone(),
                                name: None,
                                priority: a.priority,
                            },
                        )
                        .map_err(e)?
                    }
                };
                let created = a.route.is_none();
                let t = Target::Route(rid.clone());
                if !created {
                    if let Some(v) = &a.method {
                        s.set_str(&t, "method", Some(&v.to_ascii_uppercase()))
                            .map_err(e)?;
                    }
                    if let Some(v) = &a.path {
                        s.set_str(&t, "path", Some(v)).map_err(e)?;
                    }
                    if let Some(v) = &a.folder {
                        s.move_route(&rid, Some(v), None).map_err(e)?;
                    }
                    if let Some(v) = &a.match_mode {
                        s.set_route_match(&rid, Some(v)).map_err(e)?;
                    }
                    if let Some(v) = a.priority {
                        s.set_route_priority(&rid, v).map_err(e)?;
                    }
                }
                for (k, v) in a.headers.iter().flatten() {
                    s.set_route_header(&rid, k, Some(v)).map_err(e)?;
                }
                if let Some(v) = &a.name {
                    s.set_str(&t, "name", Some(v)).map_err(e)?;
                }
                if let Some(v) = &a.response_mode {
                    s.set_response_mode(&rid, v).map_err(e)?;
                }
                // A new route starts with one placeholder response; the first given response reuses it.
                let mut spare = if created {
                    find_route(&s, &rid)?
                        .1
                        .responses
                        .first()
                        .map(|r| r.id.clone())
                } else {
                    None
                };
                for spec in a.responses.iter().flatten() {
                    let id = match (&spec.id, spare.take()) {
                        (Some(id), _) => id.clone(),
                        (None, Some(sp)) => sp,
                        (None, None) => s.create_response(&rid).map_err(e)?,
                    };
                    apply_response(&s, &id, spec)?;
                }
                s.flush().map_err(e)?;
                let (srv, r) = find_route(&s, &rid)?;
                Ok(route_json(&srv, &r))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<RenderTemplate>(
        "render_template",
        "Render a Tera template string, or a Route's Response, against a sample request (no server needs to run). Returns status, headers, body, render time and any error.",
        ToolClass::Read,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<RenderTemplate>(m).and_then(|a| {
                let (srv, route) = match (&a.route, &a.server) {
                    (Some(r), _) => find_route(&s, r)?,
                    (None, Some(q)) => {
                        let (_, srv) = find_server(&s, q)?;
                        let route = fauxfield_model::parse_server("id=\"x\"\nname=\"x\"\nport=1\n[[routes]]\nid=\"r\"\npath=\"/\"")
                            .map_err(e)?
                            .routes
                            .remove(0);
                        (srv, route)
                    }
                    _ => return Err("give `route` or `server`".into()),
                };
                let mut srv = srv;
                if a.seed.is_some() {
                    srv.fake_seed = a.seed;
                }
                let resp = match (&a.template, &a.response) {
                    (Some(t), _) => {
                        let mut r = fauxfield_model::parse_server(
                            "id=\"x\"\nname=\"x\"\nport=1\n[[routes]]\nid=\"r\"\npath=\"/\"\n[[routes.responses]]\nid=\"s\"",
                        )
                        .map_err(e)?
                        .routes
                        .remove(0)
                        .responses
                        .remove(0);
                        r.body = Some(t.clone());
                        r
                    }
                    (None, Some(id)) => route
                        .responses
                        .iter()
                        .find(|r| r.id == *id)
                        .cloned()
                        .ok_or_else(|| format!("response `{id}` not found"))?,
                    (None, None) => route
                        .responses
                        .iter()
                        .find(|r| r.default)
                        .or(route.responses.first())
                        .cloned()
                        .ok_or("route has no responses")?,
                };
                let url = a.url.clone().unwrap_or_else(|| route.path.clone());
                let sample = fauxfield_engine::SampleRequest {
                    method: a.method.clone().unwrap_or_else(|| route.method.clone()),
                    url,
                    headers: a.headers.clone().unwrap_or_default().into_iter().collect(),
                    body: a.body.clone().unwrap_or_default(),
                };
                let p = fauxfield_engine::preview(&s.root(), &srv, &route, &resp, &sample, None);
                Ok(json!({
                    "status": p.status,
                    "headers": p.headers,
                    "body": p.body,
                    "render_ms": p.render_ms,
                    "error": p.error,
                    "json_error": p.json_error,
                    "deterministic": !p.nondeterministic,
                }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<MatchPath>(
        "match_path",
        "Which Route a request would match: the winner, all candidates (best first) and closest hints.",
        ToolClass::Read,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<MatchPath>(m).and_then(|a| {
                let (_, srv) = find_server(&s, &a.server)?;
                let t = fauxfield_engine::RouteTable::compile(&srv);
                let row = |m: &fauxfield_engine::Match<'_>| {
                    let folder = m.route.folder.clone();
                    json!({ "route_id": m.route.id, "pattern": m.pattern, "params": m.params.iter().cloned().collect::<std::collections::BTreeMap<_, _>>(), "folder": folder, "priority": m.priority })
                };
                let c = t.candidates(&a.method, &a.path);
                let closest: Vec<_> = t
                    .closest(&a.method, &a.path)
                    .into_iter()
                    .map(|(m, p, _)| format!("{m} {p}"))
                    .collect();
                Ok(json!({ "matched": c.first().map(row), "candidates": c.iter().map(row).collect::<Vec<_>>(), "closest": closest }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<RouteRef>(
        "delete_route",
        "Delete a Route.",
        ToolClass::Write,
        move |m| {
            let Some(s) = p() else { return no_workspace() };
            let out = parse::<RouteRef>(m).and_then(|a| {
                find_route(&s, &a.route)?;
                s.delete(&Target::Route(a.route.clone())).map_err(e)?;
                s.flush().map_err(e)?;
                Ok(json!({ "deleted": a.route }))
            });
            Box::pin(async move { out })
        },
    ));
    let p = provider.clone();
    r.register(ToolDef::new::<NoInput>("validate", "Validate the workspace; returns structured diagnostics.", ToolClass::Read, move |_| {
        let Some(s) = p() else { return no_workspace() };
        let snap = s.snapshot();
        let errors = snap.diagnostics.iter().filter(|d| d.severity == Severity::Error).count();
        let v = json!({ "ok": errors == 0, "errors": errors, "warnings": snap.diagnostics.len() - errors, "diagnostics": snap.diagnostics });
        Box::pin(async move { Ok(v) })
    }));
}
