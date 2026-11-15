//! Streamable HTTP transport (SPEC §11.1, §14, M16): `127.0.0.1` only, endpoint `/mcp`, an
//! `Origin` check against DNS rebinding and a per-run bearer token written to
//! `.fauxfield/run/mcp-http.json` (mode 0600).

use std::{
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    Router,
    extract::{Request, State},
    http::{StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde_json::json;

use crate::server::FauxfieldServer;

pub const ENDPOINT: &str = "/mcp";

/// Is `host` (no port) a loopback name? Used for `--host` and for the `Origin` header.
pub fn is_loopback_host(host: &str) -> bool {
    let h = host.trim_start_matches('[').trim_end_matches(']');
    h.eq_ignore_ascii_case("localhost") || h.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
}

/// `--host` must be loopback; anything else is refused (usage error).
pub fn check_host(host: &str) -> Result<(), String> {
    if is_loopback_host(host) {
        Ok(())
    } else {
        Err(format!(
            "the http transport binds 127.0.0.1 only; `--host {host}` is refused"
        ))
    }
}

/// Host part of an `Origin` header value (`http://host:port`).
fn origin_host(origin: &str) -> Option<&str> {
    let rest = origin.split_once("://")?.1;
    let authority = rest.split('/').next()?;
    if let Some(stripped) = authority.strip_prefix('[') {
        return stripped.split(']').next();
    }
    Some(authority.split(':').next().unwrap_or(authority))
}

fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}

#[derive(Clone)]
struct Guard {
    token: Arc<String>,
}

async fn guard(State(g): State<Guard>, req: Request, next: Next) -> Response {
    if let Some(origin) = req.headers().get(header::ORIGIN) {
        let ok = origin
            .to_str()
            .ok()
            .and_then(origin_host)
            .is_some_and(is_loopback_host);
        if !ok {
            return (StatusCode::FORBIDDEN, "origin not allowed").into_response();
        }
    }
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));
    match presented {
        Some(t) if ct_eq(t.as_bytes(), g.token.as_bytes()) => next.run(req).await,
        _ => (
            StatusCode::UNAUTHORIZED,
            [(header::WWW_AUTHENTICATE, "Bearer")],
            "missing or invalid bearer token",
        )
            .into_response(),
    }
}

pub fn new_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| format!("no random source: {e}"))?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

/// `<workspace>/.fauxfield/run/mcp-http.json`, mode 0600.
pub fn write_info(run_dir: &Path, url: &str, token: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(run_dir).map_err(|e| format!("{}: {e}", run_dir.display()))?;
    let path = run_dir.join("mcp-http.json");
    let body = serde_json::to_string_pretty(&json!({
        "url": url, "token": token, "pid": std::process::id(),
    }))
    .unwrap_or_default();
    #[cfg(unix)]
    {
        use std::{io::Write, os::unix::fs::OpenOptionsExt};
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        f.write_all(body.as_bytes())
            .map_err(|e| format!("{}: {e}", path.display()))?;
    }
    #[cfg(not(unix))]
    std::fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// A bound, not yet serving, HTTP transport.
pub struct HttpBound {
    pub addr: SocketAddr,
    pub token: String,
    pub info_file: Option<PathBuf>,
    listener: tokio::net::TcpListener,
    router: Router,
    cancel: StreamableHttpServerConfig,
}

impl HttpBound {
    pub fn url(&self) -> String {
        format!("http://{}{ENDPOINT}", self.addr)
    }

    /// Serve until `shutdown` resolves, then remove the info file.
    pub async fn serve(self, shutdown: impl std::future::Future<Output = ()> + Send + 'static) {
        let cancel = self.cancel.cancellation_token.clone();
        let info = self.info_file.clone();
        let _ = axum::serve(self.listener, self.router)
            .with_graceful_shutdown(async move {
                shutdown.await;
                cancel.cancel();
            })
            .await;
        if let Some(p) = info {
            let _ = std::fs::remove_file(p);
        }
    }
}

/// Bind `127.0.0.1:port` (random free port when `None`), write the info file and build the router.
pub async fn bind(
    handler: FauxfieldServer,
    port: Option<u16>,
    run_dir: Option<&Path>,
) -> Result<HttpBound, String> {
    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port.unwrap_or(0));
    let listener = tokio::net::TcpListener::bind(addr).await.map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            format!("port {} is in use", addr.port())
        } else {
            format!("cannot bind {addr}: {e}")
        }
    })?;
    let addr = listener.local_addr().map_err(|e| e.to_string())?;
    let token = new_token()?;
    let config = StreamableHttpServerConfig::default();
    let cancel = config.clone();
    let service = StreamableHttpService::new(
        move || Ok(handler.clone()),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router =
        Router::new()
            .nest_service(ENDPOINT, service)
            .layer(middleware::from_fn_with_state(
                Guard {
                    token: Arc::new(token.clone()),
                },
                guard,
            ));
    let url = format!("http://{addr}{ENDPOINT}");
    let info_file = match run_dir {
        Some(d) => Some(write_info(d, &url, &token)?),
        None => None,
    };
    Ok(HttpBound {
        addr,
        token,
        info_file,
        listener,
        router,
        cancel,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_detection() {
        for h in [
            "127.0.0.1",
            "localhost",
            "LOCALHOST",
            "[::1]",
            "::1",
            "127.9.9.9",
        ] {
            assert!(is_loopback_host(h), "{h}");
        }
        for h in [
            "0.0.0.0",
            "192.168.1.5",
            "example.com",
            "evil.localhost.com",
        ] {
            assert!(!is_loopback_host(h), "{h}");
        }
        assert!(check_host("0.0.0.0").is_err());
        assert!(check_host("127.0.0.1").is_ok());
    }

    #[test]
    fn origin_parsing() {
        assert_eq!(origin_host("http://localhost:3000"), Some("localhost"));
        assert_eq!(origin_host("https://evil.example"), Some("evil.example"));
        assert_eq!(origin_host("http://[::1]:8080"), Some("::1"));
        assert_eq!(origin_host("null"), None);
    }

    #[test]
    fn tokens_are_random_hex() {
        let (a, b) = (new_token().unwrap(), new_token().unwrap());
        assert_eq!(a.len(), 64);
        assert_ne!(a, b);
        assert!(ct_eq(a.as_bytes(), a.as_bytes()) && !ct_eq(a.as_bytes(), b.as_bytes()));
    }
}
