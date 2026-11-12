//! Proxy and partial mocking (SPEC §7.1): forwards a request to the upstream, streaming the
//! request and response bodies instead of buffering them, and records the exchange.
use std::io;
use std::pin::Pin;
use std::sync::OnceLock;
use std::task::{Context, Poll};

use fauxfield_model::{Proxy, Route};
use hyper::body::{Body, Frame, SizeHint};
use hyper::header::{HeaderName, HeaderValue};
use hyper_util::rt::TokioIo;
use tokio_rustls::TlsConnector;

use super::*;
use crate::recording::BODY_LIMIT;

const HOP_BY_HOP: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

/// What a request becomes when the proxy handles it.
pub(super) struct Plan {
    pub proxy: Proxy,
}

/// Decide whether the proxy handles a request (SPEC §7.1). `pinned` wins; `passthrough`
/// forwards everything else; `fallback` forwards unmatched and `proxy = true` Routes.
pub(super) fn plan(def: &Server, route: Option<&Route>) -> Option<Plan> {
    let p = def.proxy.as_ref().filter(|p| p.enabled)?;
    let go = match route {
        None => true,
        Some(r) if r.pinned => false,
        Some(_) if p.mode_name() == "passthrough" => true,
        Some(r) => r.proxy && r.kind == "http",
    };
    go.then(|| Plan { proxy: p.clone() })
}

/// Body that is both the response body and the transfer: plain, or a streamed upstream body.
pub enum RespBody {
    Sim(SimBody),
    Up(Tap<Incoming>),
}

impl Body for RespBody {
    type Data = Bytes;
    type Error = io::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, io::Error>>> {
        match &mut *self {
            RespBody::Sim(b) => Pin::new(b).poll_frame(cx),
            RespBody::Up(b) => Pin::new(b).poll_frame(cx).map_err(io::Error::other),
        }
    }
    fn is_end_stream(&self) -> bool {
        match self {
            RespBody::Sim(b) => b.is_end_stream(),
            RespBody::Up(b) => b.is_end_stream(),
        }
    }
    fn size_hint(&self) -> SizeHint {
        match self {
            RespBody::Sim(b) => b.size_hint(),
            RespBody::Up(b) => b.size_hint(),
        }
    }
}

/// Captured prefix of a streamed body (up to [`BODY_LIMIT`]).
#[derive(Default)]
pub(super) struct Cap {
    pub data: Vec<u8>,
    pub truncated: bool,
}

type Shared = Arc<Mutex<Cap>>;

/// Passes frames through unchanged while copying the first 1 MiB aside. `done` fires once
/// when the body ends, fails or is dropped.
pub struct Tap<B> {
    inner: B,
    cap: Shared,
    done: Option<Box<dyn FnOnce() + Send>>,
}

impl<B> Tap<B> {
    fn new(inner: B, cap: Shared, done: Option<Box<dyn FnOnce() + Send>>) -> Self {
        Self { inner, cap, done }
    }
    fn finish(&mut self) {
        if let Some(f) = self.done.take() {
            f();
        }
    }
}

impl<B> Drop for Tap<B> {
    fn drop(&mut self) {
        self.finish();
    }
}

impl<B: Body<Data = Bytes> + Unpin> Body for Tap<B> {
    type Data = Bytes;
    type Error = B::Error;
    fn poll_frame(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<Frame<Bytes>, B::Error>>> {
        let this = &mut *self;
        let out = Pin::new(&mut this.inner).poll_frame(cx);
        match &out {
            Poll::Ready(Some(Ok(f))) => {
                if let Some(d) = f.data_ref() {
                    let mut c = this.cap.lock().unwrap();
                    let room = BODY_LIMIT.saturating_sub(c.data.len());
                    if d.len() > room {
                        c.truncated = true;
                    }
                    let take = room.min(d.len());
                    c.data.extend_from_slice(&d[..take]);
                }
            }
            Poll::Ready(None) | Poll::Ready(Some(Err(_))) => this.finish(),
            Poll::Pending => {}
        }
        out
    }
    fn is_end_stream(&self) -> bool {
        self.inner.is_end_stream()
    }
    fn size_hint(&self) -> SizeHint {
        self.inner.size_hint()
    }
}

fn roots() -> rustls::RootCertStore {
    let mut r = rustls::RootCertStore::empty();
    for c in rustls_native_certs::load_native_certs().certs {
        let _ = r.add(c);
    }
    r
}

fn provider() -> Arc<rustls::crypto::CryptoProvider> {
    Arc::new(rustls::crypto::ring::default_provider())
}

/// Accepts any upstream certificate (the `insecure` option).
#[derive(Debug)]
struct AcceptAny(Arc<rustls::crypto::CryptoProvider>);

impl rustls::client::danger::ServerCertVerifier for AcceptAny {
    fn verify_server_cert(
        &self,
        _: &rustls_pki_types::CertificateDer<'_>,
        _: &[rustls_pki_types::CertificateDer<'_>],
        _: &rustls_pki_types::ServerName<'_>,
        _: &[u8],
        _: rustls_pki_types::UnixTime,
    ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
        Ok(rustls::client::danger::ServerCertVerified::assertion())
    }
    fn verify_tls12_signature(
        &self,
        m: &[u8],
        c: &rustls_pki_types::CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls12_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn verify_tls13_signature(
        &self,
        m: &[u8],
        c: &rustls_pki_types::CertificateDer<'_>,
        d: &rustls::DigitallySignedStruct,
    ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
        rustls::crypto::verify_tls13_signature(m, c, d, &self.0.signature_verification_algorithms)
    }
    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.0.signature_verification_algorithms.supported_schemes()
    }
}

fn tls_config(insecure: bool) -> Arc<rustls::ClientConfig> {
    static SECURE: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    static LAX: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    let build = |insecure: bool| {
        let b = rustls::ClientConfig::builder_with_provider(provider())
            .with_safe_default_protocol_versions()
            .expect("protocol versions");
        Arc::new(if insecure {
            b.dangerous()
                .with_custom_certificate_verifier(Arc::new(AcceptAny(provider())))
                .with_no_client_auth()
        } else {
            b.with_root_certificates(roots()).with_no_client_auth()
        })
    };
    if insecure {
        LAX.get_or_init(|| build(true)).clone()
    } else {
        SECURE.get_or_init(|| build(false)).clone()
    }
}

/// Everything `forward` needs from `handle`.
pub(super) struct Call {
    pub engine: Engine,
    pub rt: Arc<ServerRt>,
    pub server_id: String,
    pub def: Server,
    pub proxy: Proxy,
    pub ip: String,
    pub parts: hyper::http::request::Parts,
    pub req_headers: Vec<(String, String)>,
    pub method: String,
    pub url: String,
    pub path: String,
    pub is_test: bool,
    pub match_us: u64,
    pub cors: Option<fauxfield_model::Cors>,
    pub rate_headers: Vec<(String, String)>,
    pub network_ms: u64,
    pub started: Instant,
}

enum Fail {
    Unreachable(String),
}

struct Target {
    tls: bool,
    host: String,
    port: u16,
    authority: String,
    base: String,
}

fn parse_target(up: &str) -> Result<Target, String> {
    let uri: hyper::Uri = up
        .trim()
        .parse()
        .map_err(|e| format!("invalid upstream: {e}"))?;
    let tls = match uri.scheme_str() {
        Some("http") => false,
        Some("https") => true,
        _ => {
            return Err(format!(
                "upstream `{up}` must be an http:// or https:// URL"
            ));
        }
    };
    let host = uri
        .host()
        .ok_or_else(|| format!("upstream `{up}` has no host"))?
        .trim_matches(['[', ']'])
        .to_string();
    let port = uri.port_u16().unwrap_or(if tls { 443 } else { 80 });
    let authority = uri.authority().map(|a| a.to_string()).unwrap_or_default();
    Ok(Target {
        tls,
        host,
        port,
        authority,
        base: uri.path().trim_end_matches('/').to_string(),
    })
}

fn bad_gateway(
    c: &Call,
    status: u16,
    upstream: &str,
    detail: &str,
    cap_req: &Shared,
    upstream_ms: u64,
) -> HyperResponse<RespBody> {
    let (st, mut headers, bytes) = json_response(
        status,
        problem::bad_gateway(status, upstream, detail, &c.path),
        "application/problem+json",
    );
    if let Some(cfg) = &c.cors {
        crate::cors::apply(cfg, &c.req_headers, &mut headers, false);
    }
    let mut rec = base_recording(c, upstream);
    rec.status = st;
    rec.response_headers = redact(headers.clone());
    rec.response_body = body_text(&bytes).0;
    rec.error = Some(detail.to_string());
    rec.timing.upstream_ms = upstream_ms;
    rec.timing.total_ms = c.started.elapsed().as_millis() as u64;
    {
        let g = cap_req.lock().unwrap();
        rec.request_body = String::from_utf8_lossy(&g.data).into_owned();
        rec.truncated = g.truncated;
    }
    c.engine.record(&c.rt, rec);
    let mut b = HyperResponse::builder()
        .status(StatusCode::from_u16(st).unwrap_or(StatusCode::BAD_GATEWAY));
    for (k, v) in &headers {
        b = b.header(k.as_str(), v.as_str());
    }
    b.body(RespBody::Sim(SimBody::new(bytes, BodyMode::Full, None)))
        .unwrap_or_else(|_| {
            HyperResponse::new(RespBody::Sim(SimBody::new(
                Bytes::new(),
                BodyMode::Full,
                None,
            )))
        })
}

fn base_recording(c: &Call, upstream: &str) -> Recording {
    let ts_ms = now_ms();
    Recording {
        id: c.engine.inner.seq.fetch_add(1, Ordering::Relaxed),
        dataset_changes: vec![],
        ts: rfc3339(ts_ms),
        ts_ms,
        server_id: c.server_id.clone(),
        server: c.def.name.clone(),
        route: "proxy".into(),
        response_id: String::new(),
        response_name: String::new(),
        trace: None,
        params: vec![],
        route_pattern: String::new(),
        method: c.method.clone(),
        url: c.url.clone(),
        request_headers: redact(c.req_headers.clone()),
        request_body: String::new(),
        status: 0,
        response_headers: vec![],
        response_body: String::new(),
        timing: Timing {
            match_us: c.match_us,
            network_ms: c.network_ms,
            delay_ms: c.network_ms,
            ..Timing::default()
        },
        truncated: false,
        error: None,
        fault: None,
        source: if c.is_test { "test" } else { "mock" }.into(),
        upstream: Some(upstream.to_string()),
    }
}

async fn connect(
    t: &Target,
    insecure: bool,
    req: Request<Tap<Incoming>>,
) -> Result<HyperResponse<Incoming>, Fail> {
    let tcp = tokio::net::TcpStream::connect((t.host.as_str(), t.port))
        .await
        .map_err(|e| Fail::Unreachable(format!("connection failed: {e}")))?;
    let _ = tcp.set_nodelay(true);
    macro_rules! drive {
        ($io:expr) => {{
            let (mut sender, conn) = hyper::client::conn::http1::handshake(TokioIo::new($io))
                .await
                .map_err(|e| Fail::Unreachable(format!("handshake failed: {e}")))?;
            tokio::spawn(async move {
                let _ = conn.await;
            });
            sender
                .send_request(req)
                .await
                .map_err(|e| Fail::Unreachable(format!("request failed: {e}")))
        }};
    }
    if t.tls {
        let name = rustls_pki_types::ServerName::try_from(t.host.clone())
            .map_err(|e| Fail::Unreachable(format!("invalid server name: {e}")))?;
        let tls = TlsConnector::from(tls_config(insecure))
            .connect(name, tcp)
            .await
            .map_err(|e| Fail::Unreachable(format!("TLS failed: {e}")))?;
        drive!(tls)
    } else {
        drive!(tcp)
    }
}

/// Forward the request and stream the answer. Never fails: errors become 502/504 problems.
pub(super) async fn forward(c: Call, body: Incoming) -> Result<HyperResponse<RespBody>, ConnAbort> {
    let upstream_text = {
        let raw = c.proxy.upstream.clone().unwrap_or_default();
        match c.engine.render_str(&c.def, &raw) {
            Ok(s) => s.trim().to_string(),
            Err(e) => {
                let cap = Shared::default();
                return Ok(bad_gateway(
                    &c,
                    502,
                    &raw,
                    &format!("upstream template: {e}"),
                    &cap,
                    0,
                ));
            }
        }
    };
    let cap_req = Shared::default();
    let target = match parse_target(&upstream_text) {
        Ok(t) => t,
        Err(e) => return Ok(bad_gateway(&c, 502, &upstream_text, &e, &cap_req, 0)),
    };
    if c.network_ms > 0 {
        tokio::time::sleep(Duration::from_millis(c.network_ms)).await;
    }
    // Strip the Server prefix, then join onto the upstream base path.
    let prefix = c
        .def
        .prefix
        .as_deref()
        .map(|p| {
            c.engine
                .render_str(&c.def, p)
                .unwrap_or_else(|_| p.to_string())
        })
        .unwrap_or_default();
    let prefix = prefix.trim().trim_end_matches('/');
    let mut rel = c.parts.uri.path();
    if !prefix.is_empty()
        && let Some(r) = rel.strip_prefix(prefix)
        && (r.is_empty() || r.starts_with('/'))
    {
        rel = r;
    }
    let rel = if rel.is_empty() { "/" } else { rel };
    let pq = match c.parts.uri.query() {
        Some(q) => format!("{}{}?{}", target.base, rel, q),
        None => format!("{}{}", target.base, rel),
    };

    let skip: Vec<String> = c
        .parts
        .headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    let orig_host = c
        .parts
        .headers
        .get("host")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let mut b = Request::builder()
        .method(c.parts.method.clone())
        .uri(pq.as_str());
    {
        let h = b.headers_mut().expect("builder");
        for (k, v) in &c.parts.headers {
            let n = k.as_str();
            if n == "host"
                || n == crate::testreq::TEST_HEADER
                || HOP_BY_HOP.contains(&n)
                || skip.iter().any(|s| s == n)
            {
                continue;
            }
            h.append(k.clone(), v.clone());
        }
        let host = if c.proxy.preserve_host && !orig_host.is_empty() {
            orig_host.clone()
        } else {
            target.authority.clone()
        };
        if let Ok(v) = HeaderValue::from_str(&host) {
            h.insert("host", v);
        }
        if !c.ip.is_empty()
            && let Ok(v) = HeaderValue::from_str(&c.ip)
        {
            h.append("x-forwarded-for", v);
        }
        if !orig_host.is_empty()
            && let Ok(v) = HeaderValue::from_str(&orig_host)
        {
            h.insert("x-forwarded-host", v);
        }
        for (k, v) in &c.proxy.request_headers {
            let val = c.engine.render_str(&c.def, v).unwrap_or_else(|_| v.clone());
            if let (Ok(n), Ok(v)) = (
                HeaderName::from_bytes(k.as_bytes()),
                HeaderValue::from_str(val.trim()),
            ) {
                h.insert(n, v);
            }
        }
    }
    let up_label = upstream_text.clone();
    let req_cap = cap_req.clone();
    let req = match b.body(Tap::new(body, req_cap, None)) {
        Ok(r) => r,
        Err(e) => {
            return Ok(bad_gateway(
                &c,
                502,
                &up_label,
                &format!("bad request: {e}"),
                &cap_req,
                0,
            ));
        }
    };
    let sent = Instant::now();
    let timeout = Duration::from_millis(c.proxy.timeout());
    let resp = match tokio::time::timeout(timeout, connect(&target, c.proxy.insecure, req)).await {
        Ok(Ok(r)) => r,
        Ok(Err(Fail::Unreachable(m))) => {
            let ms = sent.elapsed().as_millis() as u64;
            return Ok(bad_gateway(&c, 502, &up_label, &m, &cap_req, ms));
        }
        Err(_) => {
            let ms = sent.elapsed().as_millis() as u64;
            let m = format!("upstream did not answer within {} ms", c.proxy.timeout());
            return Ok(bad_gateway(&c, 504, &up_label, &m, &cap_req, ms));
        }
    };
    let upstream_ms = sent.elapsed().as_millis() as u64;
    let (rparts, rbody) = resp.into_parts();
    let status = rparts.status;
    let rskip: Vec<String> = rparts
        .headers
        .get_all("connection")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(|s| s.trim().to_ascii_lowercase())
        .collect();
    let mut headers: Vec<(String, String)> = rparts
        .headers
        .iter()
        .filter(|(k, _)| {
            !HOP_BY_HOP.contains(&k.as_str()) && !rskip.iter().any(|s| s == k.as_str())
        })
        .map(|(k, v)| {
            (
                k.as_str().to_string(),
                String::from_utf8_lossy(v.as_bytes()).into_owned(),
            )
        })
        .collect();
    for (k, v) in &c.proxy.response_headers {
        let val = c.engine.render_str(&c.def, v).unwrap_or_else(|_| v.clone());
        headers.retain(|(h, _)| !h.eq_ignore_ascii_case(k));
        headers.push((k.clone(), val.trim().to_string()));
    }
    headers.extend(c.rate_headers.clone());
    if let Some(cfg) = &c.cors {
        crate::cors::apply(cfg, &c.req_headers, &mut headers, false);
    }

    let cap_resp = Shared::default();
    let done = {
        let (c2, up, hdrs, cr, cq, st) = (
            Call {
                parts: c.parts.clone(),
                ..clone_call(&c)
            },
            up_label.clone(),
            headers.clone(),
            cap_resp.clone(),
            cap_req.clone(),
            status.as_u16(),
        );
        Box::new(move || {
            let mut rec = base_recording(&c2, &up);
            rec.status = st;
            rec.response_headers = redact(hdrs);
            let (rb, rcut) = {
                let g = cr.lock().unwrap();
                (String::from_utf8_lossy(&g.data).into_owned(), g.truncated)
            };
            let (qb, qcut) = {
                let g = cq.lock().unwrap();
                (String::from_utf8_lossy(&g.data).into_owned(), g.truncated)
            };
            rec.response_body = rb;
            rec.request_body = qb;
            rec.truncated = rcut || qcut;
            rec.timing.upstream_ms = upstream_ms;
            rec.timing.total_ms = c2.started.elapsed().as_millis() as u64;
            c2.engine.record(&c2.rt, rec);
        }) as Box<dyn FnOnce() + Send>
    };
    let mut out = HyperResponse::builder().status(status);
    for (k, v) in &headers {
        out = out.header(k.as_str(), v.as_str());
    }
    Ok(out
        .body(RespBody::Up(Tap::new(rbody, cap_resp, Some(done))))
        .unwrap_or_else(|_| {
            HyperResponse::new(RespBody::Sim(SimBody::new(
                Bytes::from_static(b"invalid upstream headers"),
                BodyMode::Full,
                None,
            )))
        }))
}

fn clone_call(c: &Call) -> Call {
    Call {
        engine: c.engine.clone(),
        rt: c.rt.clone(),
        server_id: c.server_id.clone(),
        def: Server {
            routes: vec![],
            folders: vec![],
            datasets: vec![],
            ..c.def.clone()
        },
        proxy: c.proxy.clone(),
        ip: c.ip.clone(),
        parts: c.parts.clone(),
        req_headers: c.req_headers.clone(),
        method: c.method.clone(),
        url: c.url.clone(),
        path: c.path.clone(),
        is_test: c.is_test,
        match_us: c.match_us,
        cors: c.cors.clone(),
        rate_headers: c.rate_headers.clone(),
        network_ms: c.network_ms,
        started: c.started,
    }
}
