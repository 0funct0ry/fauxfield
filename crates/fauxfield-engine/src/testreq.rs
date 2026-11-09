use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Header that marks a request as Send test so the recording is tagged `test`.
pub const TEST_HEADER: &str = "x-fauxfield-test";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TestRequest {
    pub method: String,
    /// Path and query, e.g. `/api/users?x=1`.
    pub path: String,
    #[serde(default)]
    pub headers: Vec<(String, String)>,
    #[serde(default)]
    pub body: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestResponse {
    pub status: u16,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

/// Minimal HTTP/1.1 client for loopback Send test; one request, `Connection: close`.
pub(crate) async fn send(
    addr: SocketAddr,
    req: &TestRequest,
    tls: Option<std::sync::Arc<rustls::ClientConfig>>,
) -> Result<TestResponse, String> {
    let work = async {
        let tcp = TcpStream::connect(addr).await.map_err(|e| e.to_string())?;
        let mut s: Box<dyn Stream> = match tls {
            None => Box::new(tcp),
            Some(cfg) => {
                let name = rustls_pki_types::ServerName::try_from("localhost").expect("name");
                let t = tokio_rustls::TlsConnector::from(cfg)
                    .connect(name, tcp)
                    .await
                    .map_err(|e| format!("TLS handshake failed: {e}"))?;
                Box::new(t)
            }
        };
        let mut head = format!(
            "{} {} HTTP/1.1\r\nhost: {addr}\r\nconnection: close\r\n{TEST_HEADER}: 1\r\n",
            req.method.to_ascii_uppercase(),
            req.path
        );
        for (k, v) in &req.headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str(&format!("content-length: {}\r\n\r\n", req.body.len()));
        s.write_all(head.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        s.write_all(req.body.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        s.read_to_end(&mut buf).await.map_err(|e| e.to_string())?;
        parse(&buf)
    };
    tokio::time::timeout(Duration::from_secs(30), work)
        .await
        .map_err(|_| "timed out waiting for the response".to_string())?
}

trait Stream: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send {}
impl<T: tokio::io::AsyncRead + tokio::io::AsyncWrite + Unpin + Send> Stream for T {}

fn parse(buf: &[u8]) -> Result<TestResponse, String> {
    let split = buf
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .ok_or("malformed response")?;
    let head = String::from_utf8_lossy(&buf[..split]).into_owned();
    let body = &buf[split + 4..];
    let mut lines = head.lines();
    let status = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or("malformed status line")?;
    let headers = lines
        .filter_map(|l| l.split_once(':'))
        .map(|(k, v)| (k.trim().to_string(), v.trim().to_string()))
        .collect();
    Ok(TestResponse {
        status,
        headers,
        body: String::from_utf8_lossy(body).into_owned(),
    })
}
