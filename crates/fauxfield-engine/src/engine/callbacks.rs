//! Callbacks (webhook simulation, SPEC §7.3): outgoing requests fired after a Response is sent.
//! Each attempt is recorded as an entry (`source = "callback"`) linked to the triggering request.
use fauxfield_model::Callback;
use http_body_util::Full;
use serde_json::{Value, json};

use super::proxy::{connect, parse_target};
use super::*;
use crate::recording::BODY_LIMIT;

const DEFAULT_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_BACKOFF_MS: u64 = 500;
/// Backoff never waits longer than this between attempts.
const MAX_BACKOFF_MS: u64 = 30_000;

/// Everything needed to fire the Callbacks of one Response.
pub(super) struct Job {
    pub engine: Engine,
    pub rt: Arc<ServerRt>,
    pub server_id: String,
    pub server: Server,
    /// Recording id of the triggering request.
    pub parent: u64,
    pub route_id: String,
    pub response_id: String,
    pub response_name: String,
    pub tpl: crate::render::TplEnv,
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

/// Fire every enabled Callback of `callbacks` in the background.
pub(super) fn spawn(job: Job, callbacks: &[Callback]) {
    let job = Arc::new(job);
    for cb in callbacks.iter().filter(|c| !c.disabled) {
        let (job, cb) = (job.clone(), cb.clone());
        tokio::spawn(async move {
            if let Some(ms) = cb.delay_ms.filter(|m| *m > 0) {
                tokio::time::sleep(Duration::from_millis(ms)).await;
            }
            run(&job, &cb).await;
        });
    }
}

fn header_map(h: &[(String, String)]) -> Value {
    let mut m = serde_json::Map::new();
    for (k, v) in h {
        m.entry(k.to_ascii_lowercase())
            .or_insert_with(|| Value::String(v.clone()));
    }
    Value::Object(m)
}

/// The `response` context of a Callback template (SPEC §8.1).
fn response_value(status: u16, headers: &[(String, String)], body: &[u8]) -> Value {
    let text = String::from_utf8_lossy(body).into_owned();
    let json = serde_json::from_str::<Value>(&text).unwrap_or(Value::Null);
    json!({ "status": status, "headers": header_map(headers), "body": text, "json": json })
}

fn render(job: &Job, ctx: &Value, src: &str) -> Result<String, String> {
    let input = crate::template::Input {
        context: ctx.clone(),
        root: &job.engine.inner.cfg.root,
        seed: job.tpl.seed,
        route_id: &job.route_id,
        counter: job.tpl.counter,
        seq: job.tpl.seq.clone(),
    };
    crate::template::render(src, &input).map_err(|e| e.to_string())
}

struct Rendered {
    method: String,
    url: String,
    headers: Vec<(String, String)>,
    body: String,
}

/// Render the url, headers and body. The body renders first so header templates can sign
/// `callback.body`.
fn prepare(job: &Job, cb: &Callback) -> Result<Rendered, String> {
    let mut ctx = job.tpl.context.clone();
    ctx["response"] = response_value(job.status, &job.headers, &job.body);
    let method = cb
        .method
        .clone()
        .unwrap_or_else(|| "POST".into())
        .to_ascii_uppercase();
    let body =
        render(job, &ctx, cb.body.as_deref().unwrap_or("")).map_err(|e| format!("body: {e}"))?;
    ctx["callback"] = json!({ "id": cb.id, "method": method, "body": body });
    let url = render(job, &ctx, &cb.url)
        .map_err(|e| format!("url: {e}"))?
        .trim()
        .to_string();
    let mut headers = Vec::new();
    for (k, v) in &cb.headers {
        let v = render(job, &ctx, v).map_err(|e| format!("header `{k}`: {e}"))?;
        headers.push((k.clone(), v.trim().to_string()));
    }
    Ok(Rendered {
        method,
        url,
        headers,
        body,
    })
}

struct Outcome {
    status: u16,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
    error: Option<String>,
}

async fn send(r: &Rendered, timeout_ms: u64) -> Outcome {
    let fail = |e: String| Outcome {
        status: 0,
        headers: vec![],
        body: vec![],
        error: Some(e),
    };
    let target = match parse_target(&r.url) {
        Ok(t) => t,
        Err(e) => return fail(e.replace("upstream", "callback url")),
    };
    let uri: hyper::Uri = match r.url.parse() {
        Ok(u) => u,
        Err(e) => return fail(format!("invalid callback url: {e}")),
    };
    let pq = uri
        .path_and_query()
        .map(|p| p.as_str().to_string())
        .unwrap_or_else(|| "/".into());
    let mut b = Request::builder()
        .method(r.method.as_str())
        .uri(pq)
        .header("host", target.authority.as_str());
    let has_ct = r
        .headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-type"));
    for (k, v) in &r.headers {
        b = b.header(k.as_str(), v.as_str());
    }
    if !has_ct && !r.body.is_empty() {
        let t = r.body.trim_start();
        b = b.header(
            "content-type",
            if t.starts_with(['{', '[']) {
                "application/json"
            } else {
                "text/plain"
            },
        );
    }
    let req = match b.body(Full::new(Bytes::from(r.body.clone()))) {
        Ok(q) => q,
        Err(e) => return fail(format!("invalid callback request: {e}")),
    };
    let work = async {
        let resp = connect(&target, false, req)
            .await
            .map_err(|e| e.message().to_string())?;
        let (parts, body) = resp.into_parts();
        let bytes = body
            .collect()
            .await
            .map_err(|e| format!("reading response failed: {e}"))?
            .to_bytes();
        Ok::<_, String>((parts, bytes))
    };
    match tokio::time::timeout(Duration::from_millis(timeout_ms), work).await {
        Err(_) => fail(format!("timed out after {timeout_ms} ms")),
        Ok(Err(e)) => fail(e),
        Ok(Ok((parts, bytes))) => {
            let headers = parts
                .headers
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
                .collect();
            let error = (parts.status.as_u16() >= 400)
                .then(|| format!("receiver answered {}", parts.status.as_u16()));
            Outcome {
                status: parts.status.as_u16(),
                headers,
                body: bytes.to_vec(),
                error,
            }
        }
    }
}

fn record(
    job: &Job,
    cb: &Callback,
    attempt: u32,
    r: &Rendered,
    out: &Outcome,
    ms: u64,
) -> Recording {
    let engine = &job.engine;
    let ts_ms = now_ms();
    let (req_body, req_cut) = body_text(r.body.as_bytes());
    let (resp_body, resp_cut) = body_text(&out.body[..out.body.len().min(BODY_LIMIT)]);
    Recording {
        id: engine.inner.seq.fetch_add(1, Ordering::Relaxed),
        ts: rfc3339(ts_ms),
        ts_ms,
        server_id: job.server_id.clone(),
        server: job.server.name.clone(),
        route: job.route_id.clone(),
        response_id: job.response_id.clone(),
        response_name: job.response_name.clone(),
        trace: None,
        params: vec![],
        route_pattern: String::new(),
        method: r.method.clone(),
        url: r.url.clone(),
        request_headers: redact(r.headers.clone()),
        request_body: req_body,
        status: out.status,
        response_headers: redact(out.headers.clone()),
        response_body: resp_body,
        timing: Timing {
            total_ms: ms,
            upstream_ms: ms,
            ..Default::default()
        },
        truncated: req_cut || resp_cut,
        error: out.error.clone(),
        fault: None,
        dataset_changes: vec![],
        parent_id: Some(job.parent),
        callback_id: Some(format!("{}#{attempt}", cb.id)),
        source: "callback".into(),
        upstream: Some(r.url.clone()),
    }
}

/// Render and send one Callback with its retries; every attempt is recorded. Returns the last
/// recording.
async fn run(job: &Job, cb: &Callback) -> Option<Recording> {
    let rendered = match prepare(job, cb) {
        Ok(r) => r,
        Err(e) => {
            let r = Rendered {
                method: cb.method.clone().unwrap_or_else(|| "POST".into()),
                url: cb.url.clone(),
                headers: vec![],
                body: String::new(),
            };
            let out = Outcome {
                status: 0,
                headers: vec![],
                body: vec![],
                error: Some(format!("template error in Callback `{}`: {e}", cb.id)),
            };
            let rec = record(job, cb, 1, &r, &out, 0);
            job.engine.record(&job.rt, rec.clone());
            return Some(rec);
        }
    };
    if let Ok(t) = parse_target(&rendered.url)
        && is_private(&t.host)
    {
        eprintln!(
            "fauxfield: Callback `{}` targets private address {}",
            cb.id, t.host
        );
    }
    let attempts = 1 + cb
        .retries
        .unwrap_or(0)
        .min(fauxfield_model::CALLBACK_MAX_RETRIES);
    let timeout = cb.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).max(1);
    let mut backoff = cb.backoff_ms.unwrap_or(DEFAULT_BACKOFF_MS);
    let mut last = None;
    for attempt in 1..=attempts {
        let t0 = Instant::now();
        let out = send(&rendered, timeout).await;
        let rec = record(
            job,
            cb,
            attempt,
            &rendered,
            &out,
            t0.elapsed().as_millis() as u64,
        );
        let retry = out.status == 0 || out.status >= 500;
        job.engine.record(&job.rt, rec.clone());
        last = Some(rec);
        if !retry || attempt == attempts {
            break;
        }
        tokio::time::sleep(Duration::from_millis(backoff.min(MAX_BACKOFF_MS))).await;
        backoff = backoff.saturating_mul(2);
    }
    last
}

fn is_private(host: &str) -> bool {
    host == "localhost"
        || host.parse::<std::net::IpAddr>().is_ok_and(|ip| match ip {
            std::net::IpAddr::V4(v) => v.is_loopback() || v.is_private() || v.is_link_local(),
            std::net::IpAddr::V6(v) => v.is_loopback(),
        })
}

impl Engine {
    /// Fire a Callback again from the recorded request that triggered it (SPEC §7.3). `recording`
    /// is the id of the triggering request or of any of its callback entries. With `callback`
    /// unset every Callback of the Response is fired. The delay is skipped; the call waits for the
    /// attempts and returns the last recording of each Callback.
    pub async fn callback_refire(
        &self,
        server: &str,
        recording: u64,
        callback: Option<&str>,
    ) -> Result<Vec<Recording>, EngineError> {
        let (id, rt) = self.find(server)?;
        let (parent, wanted) = {
            let ring = rt.ring.lock().unwrap();
            let rec = ring
                .iter()
                .find(|r| r.id == recording)
                .ok_or_else(|| EngineError::Request(format!("no recording {recording}")))?;
            let pid = rec.parent_id.unwrap_or(rec.id);
            let parent = ring.iter().find(|r| r.id == pid).cloned().ok_or_else(|| {
                EngineError::Request(format!("triggering request {pid} left the buffer"))
            })?;
            let from_entry = rec
                .callback_id
                .as_deref()
                .map(|c| c.split('#').next().unwrap_or(c).to_string());
            (parent, callback.map(str::to_string).or(from_entry))
        };
        let server_def = rt.def.lock().unwrap().clone();
        let route = server_def
            .routes
            .iter()
            .find(|r| r.id == parent.route)
            .ok_or_else(|| {
                EngineError::Request(format!("Route `{}` no longer exists", parent.route))
            })?;
        let resp = route
            .responses
            .iter()
            .find(|r| r.id == parent.response_id)
            .ok_or_else(|| {
                EngineError::Request(format!(
                    "Response `{}` no longer exists",
                    parent.response_id
                ))
            })?;
        let list: Vec<Callback> = resp
            .callbacks
            .iter()
            .filter(|c| wanted.as_deref().is_none_or(|w| c.id == w))
            .cloned()
            .collect();
        if list.is_empty() {
            return Err(EngineError::Request(match &wanted {
                Some(w) => format!("Response `{}` has no Callback `{w}`", resp.id),
                None => format!("Response `{}` has no Callbacks", resp.id),
            }));
        }
        let lc: Vec<(String, String)> = parent
            .request_headers
            .iter()
            .map(|(k, v)| (k.to_ascii_lowercase(), v.clone()))
            .collect();
        let mut tpl = crate::render::TplEnv {
            context: crate::render::context(
                &server_def,
                &parent.method,
                &parent.url,
                &lc,
                &parent.params,
                None,
                parent.request_body.as_bytes(),
                parent.id,
                0,
                &self.inner.vars.read().unwrap(),
                Some(&route.id),
            ),
            seed: server_def.fake_seed,
            route_id: route.id.clone(),
            counter: 0,
            seq: Some(rt.seq.clone()),
        };
        let snap = rt.data.lock().unwrap().snapshot();
        if snap.as_object().is_some_and(|m| !m.is_empty()) {
            tpl.context["data"] = (*snap).clone();
        }
        let job = Job {
            engine: self.clone(),
            rt: rt.clone(),
            server_id: id,
            server: server_def.clone(),
            parent: parent.id,
            route_id: route.id.clone(),
            response_id: resp.id.clone(),
            response_name: resp.name.clone().unwrap_or_default(),
            tpl,
            status: parent.status,
            headers: parent.response_headers.clone(),
            body: Bytes::from(parent.response_body.clone().into_bytes()),
        };
        let mut out = Vec::new();
        for cb in &list {
            if let Some(r) = run(&job, cb).await {
                out.push(r);
            }
        }
        Ok(out)
    }
}

/// What a Callback would send, rendered against a sample (the GUI preview).
#[derive(Debug, Clone, PartialEq)]
pub struct CallbackPreview {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
    /// First template error, if any (url, headers and body after it are empty).
    pub error: Option<String>,
}

/// Render `cb` for the GUI. `sample` is a recorded request of the Route (its body and headers
/// feed `request.*`); without one `request` is an empty POST. Variables are the Server's.
pub fn callback_preview(
    root: &std::path::Path,
    server: &Server,
    route_id: &str,
    status: u16,
    response_body: &str,
    cb: &Callback,
    sample: Option<&Recording>,
) -> CallbackPreview {
    let (method, url, headers, body_text) = match sample {
        Some(r) => (
            r.method.clone(),
            r.url.clone(),
            r.request_headers.clone(),
            r.request_body.clone(),
        ),
        None => ("POST".into(), "/".into(), vec![], String::new()),
    };
    let json_body = serde_json::from_str::<Value>(&body_text).unwrap_or(Value::Null);
    let vars: serde_json::Map<String, Value> = server
        .variables
        .iter()
        .map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or(Value::Null)))
        .collect();
    let path = url.split('?').next().unwrap_or("/").to_string();
    let resp_body = response_body.to_string();
    let mut ctx = json!({
        "request": {
            "method": method, "path": path, "url": url,
            "params": {}, "query": {}, "headers": header_map(&headers), "cookies": {},
            "body": body_text, "json": json_body, "form": {}, "ip": null, "id": 0,
        },
        "vars": vars,
        "env": "",
        "server": { "name": server.name, "port": server.port },
        "data": {},
        "state": {},
    });
    ctx["response"] = response_value(status, &[], resp_body.as_bytes());
    let me = cb
        .method
        .clone()
        .unwrap_or_else(|| "POST".into())
        .to_ascii_uppercase();
    let r = |src: &str, ctx: &Value| -> Result<String, String> {
        let input = crate::template::Input {
            context: ctx.clone(),
            root,
            seed: server.fake_seed,
            route_id,
            counter: 1,
            seq: None,
        };
        crate::template::render(src, &input).map_err(|e| e.to_string())
    };
    let fail = |e: String| CallbackPreview {
        method: me.clone(),
        url: String::new(),
        headers: vec![],
        body: String::new(),
        error: Some(e),
    };
    let body = match r(cb.body.as_deref().unwrap_or(""), &ctx) {
        Ok(b) => b,
        Err(e) => return fail(format!("body: {e}")),
    };
    ctx["callback"] = json!({ "id": cb.id, "method": me, "body": body });
    let url = match r(&cb.url, &ctx) {
        Ok(u) => u.trim().to_string(),
        Err(e) => return fail(format!("url: {e}")),
    };
    let mut hs = Vec::new();
    for (k, v) in &cb.headers {
        match r(v, &ctx) {
            Ok(v) => hs.push((k.clone(), v.trim().to_string())),
            Err(e) => return fail(format!("header `{k}`: {e}")),
        }
    }
    CallbackPreview {
        method: me,
        url,
        headers: hs,
        body,
        error: None,
    }
}
