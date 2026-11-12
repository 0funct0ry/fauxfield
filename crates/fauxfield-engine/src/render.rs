//! Glue between the engine and `template`: request context (SPEC §8.1), Response rendering
//! (body, body_file, headers, route headers), and the headless preview used by the GUI, CLI and MCP.

use std::path::Path;
use std::time::Instant;

use bytes::Bytes;
use fauxfield_model::{Response, Route, Server};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use crate::template::{self, Input, RenderError, Sequences};

/// Template environment carried by a request: lets rule values and Response fields render.
#[derive(Clone)]
pub(crate) struct TplEnv {
    pub context: Value,
    pub seed: Option<u64>,
    pub route_id: String,
    pub counter: u64,
    pub seq: Option<Sequences>,
}

impl TplEnv {
    pub fn render(&self, root: &Path, src: &str) -> Result<String, RenderError> {
        template::render(
            src,
            &Input {
                context: self.context.clone(),
                root,
                seed: self.seed,
                route_id: &self.route_id,
                counter: self.counter,
                seq: self.seq.clone(),
            },
        )
    }
}

fn pairs_to_map(p: &[(String, String)]) -> Value {
    let mut m = Map::new();
    for (k, v) in p {
        m.entry(k.clone())
            .or_insert_with(|| Value::String(v.clone()));
    }
    Value::Object(m)
}

/// Build the `request`/`vars`/`server`/`env`/`state` context.
#[allow(clippy::too_many_arguments)]
pub(crate) fn context(
    server: &Server,
    method: &str,
    url: &str,
    lc_headers: &[(String, String)],
    params: &[(String, String)],
    ip: Option<&str>,
    body: &[u8],
    request_id: u64,
    call: u64,
    vc: &crate::vars::VarContext,
    route_id: Option<&str>,
) -> Value {
    let (path, q) = url.split_once('?').unwrap_or((url, ""));
    let query: Vec<(String, String)> = form_urlencoded::parse(q.as_bytes())
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    let text = String::from_utf8_lossy(body).into_owned();
    let ct = lc_headers
        .iter()
        .find(|(k, _)| k == "content-type")
        .map(|(_, v)| v.to_ascii_lowercase())
        .unwrap_or_default();
    let json_body = if ct.contains("json") || text.trim_start().starts_with(['{', '[']) {
        serde_json::from_str(&text).unwrap_or(Value::Null)
    } else {
        Value::Null
    };
    let form = if ct.contains("x-www-form-urlencoded") {
        let f: Vec<(String, String)> = form_urlencoded::parse(body)
            .map(|(k, v)| (k.into_owned(), v.into_owned()))
            .collect();
        pairs_to_map(&f)
    } else {
        json!({})
    };
    let cookies: Vec<(String, String)> = lc_headers
        .iter()
        .find(|(k, _)| k == "cookie")
        .map(|(_, v)| {
            v.split(';')
                .filter_map(|p| {
                    let (k, v) = p.split_once('=')?;
                    Some((k.trim().to_string(), v.trim().to_string()))
                })
                .collect()
        })
        .unwrap_or_default();
    let vars = vc.vars_json(server, route_id);
    let mut state = vc.state_json();
    state.insert("call".into(), json!(call));
    let host = if server.host == "0.0.0.0" {
        "127.0.0.1"
    } else {
        &server.host
    };
    json!({
        "request": {
            "method": method,
            "path": path,
            "url": url,
            "params": pairs_to_map(params),
            "query": pairs_to_map(&query),
            "headers": pairs_to_map(lc_headers),
            "cookies": pairs_to_map(&cookies),
            "body": text,
            "json": json_body,
            "form": form,
            "ip": ip,
            "id": request_id,
        },
        "vars": vars,
        "env": vc.active_env,
        "server": {
            "name": server.name,
            "port": server.port,
            "base_url": format!("http://{host}:{}{}", server.port, server.prefix.clone().unwrap_or_default()),
        },
        "data": {},
        "state": state,
    })
}

/// A rendered Response ready to send, or a problem to report.
pub(crate) struct Rendered {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
    /// Template/IO diagnostic; set together with a 500 body.
    pub error: Option<String>,
}

fn fail(path: &str, msg: String) -> Rendered {
    let (status, headers, body) = crate::engine::json_response(
        500,
        crate::problem::server_error(&msg, path),
        "application/problem+json",
    );
    Rendered {
        status,
        headers,
        body,
        error: Some(msg),
    }
}

pub(crate) fn build(
    root: &Path,
    route: &Route,
    resp: &Response,
    env: &TplEnv,
    path: &str,
) -> Rendered {
    let tpl = resp.template.unwrap_or(true);
    let rend = |what: &str, s: &str| -> Result<String, String> {
        if tpl {
            env.render(root, s).map_err(|e| format!("{what}: {e}"))
        } else {
            Ok(s.to_string())
        }
    };
    // Route headers first; a Response header of the same name wins (case-insensitive).
    let mut headers: Vec<(String, String)> = Vec::new();
    for (k, v) in &route.headers {
        let lk = k.to_ascii_lowercase();
        if resp.headers.keys().any(|rk| rk.eq_ignore_ascii_case(&lk)) {
            continue;
        }
        match rend(&format!("route header `{k}`"), v) {
            Ok(v) => headers.push((lk, v)),
            Err(e) => return fail(path, e),
        }
    }
    for (k, v) in &resp.headers {
        match rend(&format!("header `{k}`"), v) {
            Ok(v) => headers.push((k.to_ascii_lowercase(), v)),
            Err(e) => return fail(path, e),
        }
    }
    let body: Bytes = if let Some(file) = &resp.body_file {
        let file = match rend("body_file path", file) {
            Ok(f) => f,
            Err(e) => return fail(path, e),
        };
        let bytes = match crate::engine::read_confined(root, &file) {
            Ok(b) => b,
            Err(e) => return fail(path, format!("body_file `{file}`: {e}")),
        };
        if tpl {
            match String::from_utf8(bytes) {
                Ok(s) => match rend(&format!("body_file `{file}`"), &s) {
                    Ok(s) => Bytes::from(s),
                    Err(e) => return fail(path, e),
                },
                Err(e) => Bytes::from(e.into_bytes()),
            }
        } else {
            Bytes::from(bytes)
        }
    } else {
        match rend("body", resp.body.as_deref().unwrap_or("")) {
            Ok(s) => Bytes::from(s),
            Err(e) => return fail(path, e),
        }
    };
    if let Some(ct) = &resp.content_type {
        headers.retain(|(k, _)| k != "content-type");
        headers.push(("content-type".into(), ct.clone()));
    } else if !body.is_empty() && !headers.iter().any(|(k, _)| k == "content-type") {
        headers.push(("content-type".into(), "text/plain; charset=utf-8".into()));
    }
    let status = if (100..=599).contains(&resp.status) {
        resp.status
    } else {
        500
    };
    Rendered {
        status,
        headers,
        body,
        error: None,
    }
}

/// Sample request for previews (method, path+query, headers, body).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SampleRequest {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: String,
    /// Sample Dataset state for `data.<name>` (name -> rows); empty means no Datasets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub datasets: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preview {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
    pub render_ms: f64,
    pub error: Option<String>,
    /// True when the Server has no `fake_seed` and a template uses random functions.
    pub nondeterministic: bool,
    /// Set when `content-type` is JSON and the rendered body does not parse.
    pub json_error: Option<String>,
}

/// Render one Response of `route` against `sample` without any running server.
pub fn preview(
    root: &Path,
    server: &Server,
    route: &Route,
    response: &Response,
    sample: &SampleRequest,
    seq: Option<Sequences>,
) -> Preview {
    preview_with(
        root,
        server,
        route,
        response,
        sample,
        seq,
        &crate::vars::VarContext::default(),
    )
}

/// [`preview`] with the workspace variable context (active Environment, globals, runtime).
pub fn preview_with(
    root: &Path,
    server: &Server,
    route: &Route,
    response: &Response,
    sample: &SampleRequest,
    seq: Option<Sequences>,
    vc: &crate::vars::VarContext,
) -> Preview {
    let lc: Vec<(String, String)> = sample
        .headers
        .iter()
        .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
        .collect();
    let params: Vec<(String, String)> = crate::router::RouteTable::compile(server)
        .find(&sample.method, sample.url.split('?').next().unwrap_or("/"))
        .map(|m| m.params.into_iter().collect())
        .unwrap_or_default();
    let ctx = context(
        server,
        &sample.method,
        &sample.url,
        &lc,
        &params,
        Some("127.0.0.1"),
        sample.body.as_bytes(),
        1,
        1,
        vc,
        Some(&route.id),
    );
    let mut ctx = ctx;
    if let Some(d) = &sample.datasets {
        ctx["data"] = d.clone();
    }
    let env = TplEnv {
        context: ctx,
        seed: server.fake_seed,
        route_id: route.id.clone(),
        counter: 1,
        seq,
    };
    let started = Instant::now();
    let path = sample.url.split('?').next().unwrap_or("/");
    let r = build(root, route, response, &env, path);
    let render_ms = started.elapsed().as_secs_f64() * 1000.0;
    let body = String::from_utf8_lossy(&r.body).into_owned();
    let is_json = r
        .headers
        .iter()
        .any(|(k, v)| k == "content-type" && v.contains("json"));
    let json_error = (r.error.is_none() && is_json && !body.trim().is_empty())
        .then(|| {
            serde_json::from_str::<Value>(&body)
                .err()
                .map(|e| e.to_string())
        })
        .flatten();
    let nondet = server.fake_seed.is_none()
        && [response.body.as_deref()]
            .into_iter()
            .flatten()
            .chain(response.headers.values().map(String::as_str))
            .chain(route.headers.values().map(String::as_str))
            .any(template::is_nondeterministic);
    Preview {
        status: r.status,
        headers: r.headers,
        body,
        render_ms,
        error: r.error,
        nondeterministic: nondet,
        json_error,
    }
}
