//! The streamable HTTP transport: loopback only, bearer token, Origin check (M16).

use std::sync::Arc;

use fauxfield_mcp::{AllowedClasses, FauxfieldServer, Registry, http};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

async fn start(run_dir: Option<&std::path::Path>) -> (std::net::SocketAddr, String) {
    let mut r = Registry::new();
    r.register(
        fauxfield_mcp::ToolDef::new::<fauxfield_mcp::tools::NoInput>(
            "ping",
            "ping",
            fauxfield_mcp::ToolClass::Read,
            |_| Box::pin(async { Ok(serde_json::json!({"ok": true})) }),
        ),
    );
    let h = FauxfieldServer::new(Arc::new(r), AllowedClasses::all());
    let bound = http::bind(h, None, run_dir).await.unwrap();
    let (addr, token) = (bound.addr, bound.token.clone());
    tokio::spawn(bound.serve(std::future::pending()));
    (addr, token)
}

const INIT: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-client","version":"1"}}}"#;

async fn post(addr: std::net::SocketAddr, headers: &[(&str, String)], body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(addr).await.unwrap();
    let mut req = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nContent-Type: application/json\r\nAccept: application/json, text/event-stream\r\nContent-Length: {}\r\nConnection: close\r\n",
        addr.port(),
        body.len()
    );
    for (k, v) in headers {
        req.push_str(&format!("{k}: {v}\r\n"));
    }
    req.push_str("\r\n");
    req.push_str(body);
    s.write_all(req.as_bytes()).await.unwrap();
    let mut out = Vec::new();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), s.read_to_end(&mut out)).await;
    let text = String::from_utf8_lossy(&out).into_owned();
    let code = text
        .split_whitespace()
        .nth(1)
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    (code, text)
}

#[tokio::test]
async fn binds_loopback_and_enforces_token_and_origin() {
    let dir = tempfile::tempdir().unwrap();
    let run = dir.path().join(".fauxfield/run");
    let (addr, token) = start(Some(&run)).await;
    assert!(addr.ip().is_loopback());
    let bearer = ("Authorization", format!("Bearer {token}"));

    // No token: 401. Wrong token: 401.
    assert_eq!(post(addr, &[], INIT).await.0, 401);
    assert_eq!(
        post(addr, &[("Authorization", "Bearer nope".into())], INIT)
            .await
            .0,
        401
    );
    // Right token but a foreign Origin: 403 (DNS rebinding defence).
    let (code, _) = post(
        addr,
        &[bearer.clone(), ("Origin", "https://evil.example".into())],
        INIT,
    )
    .await;
    assert_eq!(code, 403);
    // Loopback Origin and the token: the server answers initialize.
    let (code, body) = post(
        addr,
        &[bearer.clone(), ("Origin", "http://localhost:3000".into())],
        INIT,
    )
    .await;
    assert_eq!(code, 200, "{body}");
    assert!(body.contains("fauxfield"), "{body}");
    // No Origin (a CLI client) is fine too.
    assert_eq!(post(addr, &[bearer], INIT).await.0, 200);

    // The info file carries the url and token and is private.
    let info: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(run.join("mcp-http.json")).unwrap()).unwrap();
    assert_eq!(info["token"], token.as_str());
    assert!(info["url"].as_str().unwrap().ends_with("/mcp"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(run.join("mcp-http.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
}

#[test]
fn non_loopback_host_is_refused() {
    assert!(http::check_host("0.0.0.0").is_err());
    assert!(http::check_host("192.168.0.4").is_err());
    assert!(http::check_host("127.0.0.1").is_ok());
}
