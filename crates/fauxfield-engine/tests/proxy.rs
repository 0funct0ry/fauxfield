//! Proxy and partial mocking at the socket level (SPEC §7.1).

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fauxfield_engine::{Engine, EngineConfig, TrafficFilter};
use fauxfield_model::parse_server;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

type Heads = Arc<Mutex<Vec<String>>>;

/// A raw HTTP/1.1 upstream: `f(head)` writes the whole answer to the stream.
fn upstream(f: impl Fn(&str, &mut TcpStream) + Send + Sync + 'static) -> (u16, Heads) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let heads: Heads = Arc::default();
    let h2 = heads.clone();
    let f = Arc::new(f);
    std::thread::spawn(move || {
        for s in l.incoming().flatten() {
            let (f, h2) = (f.clone(), h2.clone());
            std::thread::spawn(move || {
                let mut s = s;
                let mut buf = Vec::new();
                let mut b = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match s.read(&mut b) {
                        Ok(0) | Err(_) => return,
                        Ok(n) => buf.extend_from_slice(&b[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf).to_string();
                h2.lock().unwrap().push(head.clone());
                f(&head, &mut s);
            });
        }
    });
    (port, heads)
}

fn simple(body: &'static str) -> impl Fn(&str, &mut TcpStream) + Send + Sync + 'static {
    move |_, s| {
        let _ = s.write_all(
            format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {}\r\ncontent-type: text/plain\r\nx-up: yes\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        );
    }
}

struct Fx {
    _rt: tokio::runtime::Runtime,
    e: Engine,
    port: u16,
}

fn fx(proxy: &str, routes: &str) -> Fx {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: dir.path().to_path_buf(),
            ..Default::default()
        },
    );
    let port = free_port();
    e.apply(vec![
        parse_server(&format!(
            "id = \"srv_a\"\nname = \"S\"\nport = {port}\n{proxy}\n{routes}"
        ))
        .unwrap(),
    ]);
    e.start("srv_a").unwrap();
    Fx { _rt: rt, e, port }
}

fn mock(id: &str, path: &str, extra: &str, body: &str) -> String {
    format!(
        "[[routes]]\nid = \"{id}\"\nmethod = \"GET\"\npath = \"{path}\"\n{extra}\n[[routes.responses]]\nid = \"rsp_{id}\"\nbody = \"{body}\"\n"
    )
}

impl Fx {
    fn req(&self, method: &str, path: &str, headers: &[(&str, &str)]) -> (String, Vec<u8>) {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        let mut r =
            format!("{method} {path} HTTP/1.1\r\nhost: client.test\r\nconnection: close\r\n");
        for (k, v) in headers {
            r += &format!("{k}: {v}\r\n");
        }
        s.write_all((r + "\r\n").as_bytes()).unwrap();
        let mut buf = Vec::new();
        let _ = s.read_to_end(&mut buf);
        let i = buf
            .windows(4)
            .position(|w| w == b"\r\n\r\n")
            .unwrap_or(buf.len());
        let head = String::from_utf8_lossy(&buf[..i]).to_string();
        let body = dechunk(&head, buf.get(i + 4..).unwrap_or_default());
        (head, body)
    }
    fn traffic(&self) -> Vec<fauxfield_engine::Recording> {
        std::thread::sleep(Duration::from_millis(150));
        self.e.traffic(&TrafficFilter::default())
    }
}

fn dechunk(head: &str, body: &[u8]) -> Vec<u8> {
    if !head
        .to_ascii_lowercase()
        .contains("transfer-encoding: chunked")
    {
        return body.to_vec();
    }
    let (mut out, mut rest) = (Vec::new(), body);
    while let Some(p) = rest.windows(2).position(|w| w == b"\r\n") {
        let n = usize::from_str_radix(String::from_utf8_lossy(&rest[..p]).trim(), 16).unwrap_or(0);
        if n == 0 {
            break;
        }
        out.extend_from_slice(&rest[p + 2..p + 2 + n]);
        rest = &rest[p + 2 + n + 2..];
    }
    out
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn fallback_mocks_matched_and_forwards_the_rest() {
    let (up, heads) = upstream(simple("from-upstream"));
    let f = fx(
        &format!(
            "[proxy]\nenabled = true\nupstream = \"http://127.0.0.1:{up}/base\"\n[proxy.request_headers]\n\"x-forwarded-by\" = \"fauxfield\"\n[proxy.response_headers]\n\"x-added\" = \"1\"\n"
        ),
        &mock("rte_a", "/mine", "", "mocked"),
    );
    let (h, b) = f.req("GET", "/mine", &[]);
    assert!(h.contains("200") && text(&b) == "mocked", "{h}");
    let (h, b) = f.req("GET", "/other/x?q=1", &[]);
    assert_eq!(text(&b), "from-upstream", "{h}");
    assert!(h.to_lowercase().contains("x-up: yes") && h.to_lowercase().contains("x-added: 1"));
    let seen = heads.lock().unwrap()[0].to_lowercase();
    assert!(seen.starts_with("get /base/other/x?q=1 http/1.1"), "{seen}");
    assert!(seen.contains("x-forwarded-by: fauxfield"));
    assert!(seen.contains(&format!("host: 127.0.0.1:{up}")));
    assert!(seen.contains("x-forwarded-host: client.test"));
    let t = f.traffic();
    let routes: Vec<_> = t.iter().map(|r| r.route.as_str()).collect();
    assert!(
        routes.contains(&"rte_a") && routes.contains(&"proxy"),
        "{routes:?}"
    );
    let p = t.iter().find(|r| r.route == "proxy").unwrap();
    assert_eq!(p.status, 200);
    assert_eq!(p.response_body, "from-upstream");
    assert!(p.upstream.as_deref().unwrap().contains("/base"));
}

#[test]
fn preserve_host_and_prefix() {
    let (up, heads) = upstream(simple("x"));
    let f = fx(
        &format!(
            "prefix = \"/api\"\n[proxy]\nenabled = true\npreserve_host = true\nupstream = \"http://127.0.0.1:{up}\"\n"
        ),
        "",
    );
    let (_, b) = f.req("GET", "/api/users", &[]);
    assert_eq!(text(&b), "x");
    let seen = heads.lock().unwrap()[0].to_lowercase();
    assert!(seen.starts_with("get /users http/1.1"), "{seen}");
    assert!(seen.contains("host: client.test"));
}

#[test]
fn passthrough_forwards_everything_but_pinned() {
    let (up, _) = upstream(simple("from-upstream"));
    let f = fx(
        &format!(
            "[proxy]\nenabled = true\nmode = \"passthrough\"\nupstream = \"http://127.0.0.1:{up}\"\n"
        ),
        &(mock("rte_a", "/a", "", "mock-a") + &mock("rte_b", "/b", "pinned = true", "mock-b")),
    );
    assert_eq!(text(&f.req("GET", "/a", &[]).1), "from-upstream");
    assert_eq!(text(&f.req("GET", "/b", &[]).1), "mock-b");
    assert_eq!(text(&f.req("GET", "/zzz", &[]).1), "from-upstream");
}

#[test]
fn route_proxy_flag_forwards_in_fallback() {
    let (up, _) = upstream(simple("from-upstream"));
    let f = fx(
        &format!("[proxy]\nenabled = true\nupstream = \"http://127.0.0.1:{up}\"\n"),
        &(mock("rte_a", "/a", "proxy = true", "mock-a") + &mock("rte_b", "/b", "", "mock-b")),
    );
    assert_eq!(text(&f.req("GET", "/a", &[]).1), "from-upstream");
    assert_eq!(text(&f.req("GET", "/b", &[]).1), "mock-b");
}

#[test]
fn disabled_proxy_keeps_404() {
    let (up, _) = upstream(simple("from-upstream"));
    let f = fx(
        &format!("[proxy]\nenabled = false\nupstream = \"http://127.0.0.1:{up}\"\n"),
        "",
    );
    let (h, _) = f.req("GET", "/a", &[]);
    assert!(h.contains("404"), "{h}");
}

#[test]
fn unreachable_upstream_is_a_502_problem() {
    let dead = free_port();
    let f = fx(
        &format!("[proxy]\nenabled = true\nupstream = \"http://127.0.0.1:{dead}\"\n"),
        "",
    );
    let (h, b) = f.req("GET", "/a", &[]);
    assert!(h.contains("502"), "{h}");
    assert!(h.to_lowercase().contains("application/problem+json"));
    assert!(
        text(&b).contains(&format!("127.0.0.1:{dead}")),
        "{}",
        text(&b)
    );
    let t = f.traffic();
    let r = t.iter().find(|r| r.route == "proxy").unwrap();
    assert_eq!(r.status, 502);
    assert!(r.error.is_some());
}

#[test]
fn slow_upstream_times_out_with_504() {
    let (up, _) = upstream(|_, s| {
        std::thread::sleep(Duration::from_millis(1500));
        let _ = s.write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\n\r\n");
    });
    let f = fx(
        &format!(
            "[proxy]\nenabled = true\ntimeout_ms = 300\nupstream = \"http://127.0.0.1:{up}\"\n"
        ),
        "",
    );
    let (h, _) = f.req("GET", "/a", &[]);
    assert!(h.contains("504"), "{h}");
}

#[test]
fn cors_is_applied_to_proxied_responses() {
    let (up, _) = upstream(simple("x"));
    let f = fx(
        &format!(
            "[cors]\nenabled = true\norigins = [\"*\"]\n[proxy]\nenabled = true\nupstream = \"http://127.0.0.1:{up}\"\n"
        ),
        "",
    );
    let (h, _) = f.req("GET", "/a", &[("origin", "http://app.test")]);
    assert!(
        h.to_lowercase().contains("access-control-allow-origin"),
        "{h}"
    );
}

#[test]
fn response_streams_before_the_upstream_finishes() {
    let (up, _) = upstream(|_, s| {
        let _ = s.write_all(
            b"HTTP/1.1 200 OK\r\ntransfer-encoding: chunked\r\nconnection: close\r\n\r\n5\r\nfirst\r\n",
        );
        let _ = s.flush();
        std::thread::sleep(Duration::from_millis(1200));
        let _ = s.write_all(b"6\r\nsecond\r\n0\r\n\r\n");
    });
    let f = fx(
        &format!("[proxy]\nenabled = true\nupstream = \"http://127.0.0.1:{up}\"\n"),
        "",
    );
    let mut s = TcpStream::connect(("127.0.0.1", f.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
    s.write_all(b"GET /s HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n")
        .unwrap();
    let t0 = Instant::now();
    let mut got = Vec::new();
    let mut b = [0u8; 512];
    while !String::from_utf8_lossy(&got).contains("first") {
        let n = s.read(&mut b).unwrap();
        assert!(n > 0, "closed early");
        got.extend_from_slice(&b[..n]);
    }
    assert!(
        t0.elapsed() < Duration::from_millis(900),
        "buffered: {:?}",
        t0.elapsed()
    );
    let mut rest = Vec::new();
    let _ = s.read_to_end(&mut rest);
    assert!(String::from_utf8_lossy(&rest).contains("second"));
}

#[test]
fn large_bodies_stream_both_ways_and_recording_truncates() {
    const N: usize = 6 * 1024 * 1024;
    let (up, _) = upstream(|head, s| {
        // Echo the request body length as a header, answer with a large body.
        let cl: usize = head
            .to_lowercase()
            .lines()
            .find_map(|l| {
                l.strip_prefix("content-length: ")
                    .and_then(|v| v.trim().parse().ok())
            })
            .unwrap_or(0);
        let have = head.split("\r\n\r\n").nth(1).map_or(0, str::len);
        let mut left = cl.saturating_sub(have);
        let mut b = vec![0u8; 65536];
        while left > 0 {
            match s.read(&mut b) {
                Ok(0) | Err(_) => break,
                Ok(n) => left = left.saturating_sub(n),
            }
        }
        let _ = s.write_all(
            format!(
                "HTTP/1.1 200 OK\r\ncontent-length: {N}\r\nx-got: {cl}\r\nconnection: close\r\n\r\n"
            )
            .as_bytes(),
        );
        let chunk = vec![b'z'; 65536];
        let mut sent = 0;
        while sent < N {
            let n = chunk.len().min(N - sent);
            if s.write_all(&chunk[..n]).is_err() {
                break;
            }
            sent += n;
        }
    });
    let f = fx(
        &format!("[proxy]\nenabled = true\nupstream = \"http://127.0.0.1:{up}\"\n"),
        "",
    );
    let payload = vec![b'q'; 3 * 1024 * 1024];
    let mut s = TcpStream::connect(("127.0.0.1", f.port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    s.write_all(
        format!(
            "POST /big HTTP/1.1\r\nhost: x\r\nconnection: close\r\ncontent-length: {}\r\n\r\n",
            payload.len()
        )
        .as_bytes(),
    )
    .unwrap();
    s.write_all(&payload).unwrap();
    let mut buf = Vec::new();
    let _ = s.read_to_end(&mut buf);
    let i = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    let head = String::from_utf8_lossy(&buf[..i]).to_lowercase();
    assert!(
        head.contains(&format!("x-got: {}", payload.len())),
        "{head}"
    );
    assert_eq!(buf.len() - i - 4, N);
    let t = f.traffic();
    let r = t.iter().find(|r| r.route == "proxy").unwrap();
    assert!(r.truncated);
    assert_eq!(r.response_body.len(), 1024 * 1024);
    assert_eq!(r.request_body.len(), 1024 * 1024);
}
