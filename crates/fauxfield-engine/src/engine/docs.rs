//! Published API documentation (SPEC §12.4, M17.06): the `docs path?` step of the request
//! pipeline. Answered before CORS, rate limiting and route matching; never recorded.
use super::{Engine, RespBody, ServerRt};
use crate::sim::{BodyMode, SimBody};
use bytes::Bytes;
use fauxfield_model::Server;
use fauxfield_openapi::{Document, Options, ServerSnapshot, ui};
use hyper::Response as HyperResponse;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

const RENDER_LIMIT: Duration = Duration::from_millis(250);

pub(super) struct Cached {
    json: Bytes,
    etag: String,
    documented: usize,
    skipped: usize,
    unrepresentable: Vec<String>,
    warnings: Vec<String>,
}

/// What `openapi_get` reports (control API, CLI, MCP and GUI share it).
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct OpenapiInfo {
    pub server_id: String,
    pub server: String,
    /// `docs_site.enabled`.
    pub enabled: bool,
    pub running: bool,
    /// Resolved docs path, e.g. `/__docs`.
    pub path: String,
    /// Full URL of the page while the Server runs with docs enabled.
    pub url: Option<String>,
    pub documented: usize,
    pub skipped: usize,
    /// Route ids the document cannot express (W097).
    pub unrepresentable: Vec<String>,
    pub warnings: Vec<String>,
    /// Present when requested.
    pub document: Option<serde_json::Value>,
}

/// Generate the OpenAPI document for a Server from its files (Dataset seeds only, no live rows).
/// Tera bodies render once in preview mode: `fake_seed` 0, secret Variables as `***`, 250 ms limit.
pub fn openapi_document(root: &Path, server_id: &str, server: &Server) -> Document {
    let mut store = crate::data::DataStore::default();
    store.sync(root, server_id, server);
    let mut seeds: BTreeMap<String, Vec<serde_json::Value>> = BTreeMap::new();
    for s in &store.sets {
        if let serde_json::Value::Array(a) = &s.rows {
            seeds.insert(s.def.name.clone(), a.clone());
        }
    }
    let mut preview_server = server.clone();
    preview_server.fake_seed = Some(0);
    for (k, v) in preview_server.variables.iter_mut() {
        if fauxfield_model::vars::is_secret_key(k) {
            *v = toml::Value::String("***".into());
        }
    }
    let route: fauxfield_model::Route =
        serde_json::from_value(json!({"id": "rte_docs", "path": "/"})).expect("route");
    let vc = crate::vars::VarContext::default();
    let render = |raw: &str| -> Result<String, String> {
        let resp: fauxfield_model::Response =
            serde_json::from_value(json!({"id": "rsp_docs", "body": raw})).expect("response");
        let sample = crate::render::SampleRequest {
            method: "GET".into(),
            url: "/".into(),
            ..Default::default()
        };
        let p =
            crate::render::preview_with(root, &preview_server, &route, &resp, &sample, None, &vc);
        if let Some(e) = p.error {
            return Err(e);
        }
        if p.render_ms > RENDER_LIMIT.as_secs_f64() * 1000.0 {
            return Err("template render exceeded 250 ms".into());
        }
        Ok(p.body)
    };
    let snap = ServerSnapshot {
        server,
        root: Some(root),
        seeds,
    };
    fauxfield_openapi::generate(
        &snap,
        &Options {
            renderer: Some(&render),
        },
    )
}

fn problem(status: u16, title: &str, path: &str) -> Bytes {
    Bytes::from(
        json!({"type": "about:blank", "title": title, "status": status, "instance": path})
            .to_string(),
    )
}

fn build(
    status: u16,
    headers: &[(&str, String)],
    body: Bytes,
    head: bool,
) -> HyperResponse<RespBody> {
    let mut b = HyperResponse::builder()
        .status(status)
        .header("x-content-type-options", "nosniff")
        .header("content-length", body.len());
    for (k, v) in headers {
        b = b.header(*k, v.as_str());
    }
    let body = if head { Bytes::new() } else { body };
    b.body(RespBody::Sim(SimBody::new(body, BodyMode::Full, None)))
        .unwrap_or_else(|_| {
            HyperResponse::new(RespBody::Sim(SimBody::new(
                Bytes::new(),
                BodyMode::Full,
                None,
            )))
        })
}

fn header<'a>(h: &'a [(String, String)], name: &str) -> Option<&'a str> {
    h.iter()
        .find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

impl Engine {
    /// OpenAPI state of one Server (works while it is stopped); `include_document` adds the JSON.
    pub fn openapi_get(
        &self,
        server: &str,
        include_document: bool,
    ) -> Result<OpenapiInfo, super::EngineError> {
        let (id, rt) = self.find(server)?;
        let def = rt.def.lock().unwrap().clone();
        let c = self.docs_cached(&rt, &id, &def);
        let ds = def.docs_site.clone().unwrap_or_default();
        let path = ds.path_or_default().to_string();
        let running = matches!(
            *rt.status.lock().unwrap(),
            super::ServerStatus::Running { .. }
        );
        let url = (running && ds.enabled).then(|| {
            let (host, port) = self.bind_of(&def);
            let scheme = if rt.run.lock().unwrap().as_ref().is_some_and(|r| r.https) {
                "https"
            } else {
                "http"
            };
            format!("{scheme}://{host}:{port}{path}/")
        });
        Ok(OpenapiInfo {
            server_id: id,
            server: def.name.clone(),
            enabled: ds.enabled,
            running,
            path,
            url,
            documented: c.documented,
            skipped: c.skipped,
            unrepresentable: c.unrepresentable.clone(),
            warnings: c.warnings.clone(),
            document: include_document
                .then(|| serde_json::from_slice(&c.json).ok())
                .flatten(),
        })
    }

    /// Cached document for a running Server; rebuilt when the Server definition changes.
    pub(super) fn docs_cached(&self, rt: &ServerRt, id: &str, def: &Server) -> Arc<Cached> {
        let key = serde_json::to_string(def).unwrap_or_default();
        if let Some((k, c)) = rt.docs_cache.lock().unwrap().as_ref()
            && *k == key
        {
            return c.clone();
        }
        let mut s = def.clone();
        if let Some(p) = s.prefix.as_deref()
            && let Ok(r) = self.render_str(&s, p)
        {
            s.prefix = Some(r.trim().to_string());
        }
        let doc = openapi_document(&self.inner.cfg.root, id, &s);
        let json = Bytes::from(doc.to_json());
        let c = Arc::new(Cached {
            etag: ui::etag(&json),
            json,
            documented: doc.stats.documented,
            skipped: doc.stats.skipped,
            unrepresentable: doc.unrepresentable,
            warnings: doc.warnings,
        });
        *rt.docs_cache.lock().unwrap() = Some((key, c.clone()));
        c
    }
}

/// The `docs path?` step. `None` means the request is not for the documentation.
pub(super) fn respond(
    engine: &Engine,
    rt: &ServerRt,
    id: &str,
    method: &str,
    path: &str,
    headers: &[(String, String)],
) -> Option<HyperResponse<RespBody>> {
    let def = rt.def.lock().unwrap().clone();
    let ds = def.docs_site.as_ref().filter(|d| d.enabled)?;
    let base = ds.path_or_default().to_string();
    let rest = path.strip_prefix(base.as_str())?;
    if !(rest.is_empty() || rest.starts_with('/')) {
        return None;
    }
    let head = method == "HEAD";
    if method != "GET" && !head {
        return Some(build(
            405,
            &[
                ("allow", "GET, HEAD".into()),
                ("content-type", "application/problem+json".into()),
            ],
            problem(405, "Method not allowed", path),
            false,
        ));
    }
    let not_found = || {
        build(
            404,
            &[("content-type", "application/problem+json".into())],
            problem(404, "Not found", path),
            head,
        )
    };
    Some(match rest {
        "" => build(308, &[("location", format!("{base}/"))], Bytes::new(), head),
        "/" => {
            let title = ds.title.clone().unwrap_or_else(|| def.name.clone());
            build(
                200,
                &[
                    ("content-type", "text/html; charset=utf-8".into()),
                    ("content-security-policy", ui::CSP.into()),
                    ("cache-control", "no-cache".into()),
                ],
                Bytes::from(ui::page(&title)),
                head,
            )
        }
        "/openapi.json" => {
            let c = engine.docs_cached(rt, id, &def);
            if header(headers, "if-none-match")
                .is_some_and(|v| v.split(',').any(|t| t.trim() == c.etag))
            {
                return Some(build(
                    304,
                    &[
                        ("etag", c.etag.clone()),
                        ("cache-control", "no-cache".into()),
                    ],
                    Bytes::new(),
                    head,
                ));
            }
            build(
                200,
                &[
                    ("content-type", "application/json".into()),
                    ("etag", c.etag.clone()),
                    ("cache-control", "no-cache".into()),
                ],
                c.json.clone(),
                head,
            )
        }
        r if r.starts_with("/assets/") => {
            let Some(a) = ui::asset(&r["/assets/".len()..]) else {
                return Some(not_found());
            };
            let accepts_gz = header(headers, "accept-encoding").is_some_and(|v| v.contains("gzip"));
            let mut h: Vec<(&str, String)> = vec![
                ("content-type", a.content_type.into()),
                (
                    "cache-control",
                    "public, max-age=31536000, immutable".into(),
                ),
                ("vary", "accept-encoding".into()),
            ];
            let body = match a.gz {
                Some(gz) if accepts_gz => {
                    h.push(("content-encoding", "gzip".into()));
                    Bytes::from_static(gz)
                }
                _ => Bytes::from(a.plain_bytes()),
            };
            build(200, &h, body, head)
        }
        _ => not_found(),
    })
}
