//! Network and error simulation at the socket level (SPEC §5.5, §5.6).

use std::io::{ErrorKind, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use fauxfield_engine::{Engine, EngineConfig, NetworkProfile, TrafficFilter};
use fauxfield_model::parse_server;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// `top` goes before the routes (server-level tables); `resp` extends the Response.
fn server(port: u16, top: &str, resp: &str) -> fauxfield_model::Server {
    parse_server(&format!(
        r#"
id = "srv_a"
name = "Sim"
port = {port}
{top}
[[routes]]
id = "rte_1"
method = "GET"
path = "/x"
[[routes.responses]]
id = "rsp_1"
status = 200
body = "0123456789abcdefghij"
{resp}
"#
    ))
    .unwrap()
}

struct Fx {
    rt: tokio::runtime::Runtime,
    e: Engine,
    port: u16,
    _dir: tempfile::TempDir,
}

fn fx(top: &str, resp: &str) -> Fx {
    fx_with(top, resp, EngineConfig::default())
}

fn fx_with(top: &str, resp: &str, mut cfg: EngineConfig) -> Fx {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    cfg.root = dir.path().to_path_buf();
    let e = Engine::new(rt.handle().clone(), cfg);
    let port = free_port();
    e.apply(vec![server(port, top, resp)]);
    e.start("srv_a").unwrap();
    Fx {
        rt,
        e,
        port,
        _dir: dir,
    }
}

impl Fx {
    fn connect(&self) -> TcpStream {
        let s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
        s
    }
    /// Send a request and read until EOF or error. Returns (bytes, error kind).
    fn raw(&self, path: &str) -> (Vec<u8>, Option<ErrorKind>) {
        let mut s = self.connect();
        write!(
            s,
            "GET {path} HTTP/1.1\r\nhost: x\r\nconnection: close\r\n\r\n"
        )
        .unwrap();
        let mut out = vec![];
        let mut buf = [0u8; 256];
        loop {
            match s.read(&mut buf) {
                Ok(0) => return (out, None),
                Ok(n) => out.extend_from_slice(&buf[..n]),
                Err(e) => return (out, Some(e.kind())),
            }
        }
    }
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[test]
fn reset_sends_a_tcp_rst() {
    let f = fx("", "fault = \"reset\"");
    let (bytes, err) = f.raw("/x");
    assert!(bytes.is_empty());
    assert_eq!(err, Some(ErrorKind::ConnectionReset));
    let rec = &f.e.traffic(&TrafficFilter::default())[0];
    assert_eq!(rec.fault.as_deref(), Some("reset"));
    assert_eq!(rec.status, 0);
    f.e.stop_all();
}

#[test]
fn close_sends_headers_then_drops_the_connection() {
    let f = fx("", "fault = \"close\"");
    let (bytes, _) = f.raw("/x");
    let t = text(&bytes);
    assert!(t.starts_with("HTTP/1.1 200"), "got {t:?}");
    assert!(!t.contains("0123456789"));
    f.e.stop_all();
}

#[test]
fn empty_has_no_body() {
    let f = fx("", "fault = \"empty\"");
    let t = text(&f.raw("/x").0);
    assert!(t.starts_with("HTTP/1.1 200"));
    assert!(t.ends_with("\r\n\r\n"), "got {t:?}");
    f.e.stop_all();
}

#[test]
fn malformed_truncates_the_declared_body() {
    let f = fx("", "fault = \"malformed\"");
    let t = text(&f.raw("/x").0);
    assert!(
        t.to_ascii_lowercase().contains("content-length: 20"),
        "got {t:?}"
    );
    let body = t.split("\r\n\r\n").nth(1).unwrap_or("");
    assert!(body.len() < 20, "body {body:?}");
    f.e.stop_all();
}

#[test]
fn timeout_holds_until_the_client_gives_up() {
    let f = fx("", "fault = \"timeout\"");
    let mut s = f.connect();
    s.set_read_timeout(Some(Duration::from_millis(400)))
        .unwrap();
    write!(s, "GET /x HTTP/1.1\r\nhost: x\r\n\r\n").unwrap();
    let mut buf = [0u8; 64];
    let err = s.read(&mut buf).unwrap_err();
    assert!(matches!(
        err.kind(),
        ErrorKind::WouldBlock | ErrorKind::TimedOut
    ));
    let rec = &f.e.traffic(&TrafficFilter::default())[0];
    assert_eq!(rec.fault.as_deref(), Some("timeout"));
    f.e.stop_all();
}

#[test]
fn fault_rate_zero_never_and_one_always() {
    let f = fx("", "fault = \"empty\"\nfault_rate = 0.0");
    for _ in 0..10 {
        assert!(text(&f.raw("/x").0).contains("0123456789"));
    }
    f.e.stop_all();
    let f = fx("", "fault = \"empty\"\nfault_rate = 1.0");
    assert!(!text(&f.raw("/x").0).contains("0123456789"));
    f.e.stop_all();
}

#[test]
fn fault_rate_is_roughly_the_probability() {
    let f = fx("", "fault = \"empty\"\nfault_rate = 0.5");
    let hits = (0..120)
        .filter(|_| !text(&f.raw("/x").0).contains("0123456789"))
        .count();
    assert!((30..=90).contains(&hits), "hits {hits}");
    f.e.stop_all();
}

#[test]
fn latency_range_and_bandwidth_throttle() {
    let f = fx("", "delay_ms = \"150..200\"");
    let t = Instant::now();
    f.raw("/x");
    let ms = t.elapsed().as_millis();
    assert!((150..400).contains(&ms), "{ms}");
    let rec = &f.e.traffic(&TrafficFilter::default())[0];
    assert!((150..=200).contains(&rec.timing.delay_ms));
    f.e.stop_all();

    // 20 bytes at 1 kbps (125 B/s) takes ~160 ms.
    let f = fx("[network]\nlatency_ms = 100\nbandwidth_kbps = 1", "");
    let t = Instant::now();
    f.raw("/x");
    assert!(t.elapsed().as_millis() >= 220, "{:?}", t.elapsed());
    let rec = &f.e.traffic(&TrafficFilter::default())[0];
    assert_eq!(rec.timing.network_ms, 100);
    assert!(rec.timing.transfer_ms >= 100);
    f.e.stop_all();
}

#[test]
fn slow_body_streams_at_the_server_bandwidth() {
    let f = fx("[network]\nbandwidth_kbps = 1", "fault = \"slow_body\"");
    let t = Instant::now();
    let (b, _) = f.raw("/x");
    assert!(text(&b).contains("0123456789abcdefghij"));
    assert!(t.elapsed().as_millis() >= 100);
    assert_eq!(
        f.e.traffic(&TrafficFilter::default())[0].fault.as_deref(),
        Some("slow_body")
    );
    f.e.stop_all();
}

#[test]
fn rate_limit_returns_429_with_headers() {
    let f = fx(
        "[rate_limit]\nenabled = true\nrequests = 2\nwindow = \"1m\"",
        "",
    );
    let a = text(&f.raw("/x").0);
    assert!(a.starts_with("HTTP/1.1 200") && a.to_lowercase().contains("x-ratelimit-remaining: 1"));
    f.raw("/x");
    let c = text(&f.raw("/x").0).to_lowercase();
    assert!(c.starts_with("http/1.1 429"), "{c}");
    assert!(c.contains("retry-after:") && c.contains("x-ratelimit-limit: 2"));
    assert!(c.contains("too many requests"));
    let recs = f.e.traffic(&TrafficFilter::default());
    assert!(
        recs.iter()
            .any(|r| r.status == 429 && r.fault.as_deref() == Some("rate_limited"))
    );
    // Reset state clears the counters.
    f.e.reset_state(None).unwrap();
    assert!(text(&f.raw("/x").0).starts_with("HTTP/1.1 200"));
    f.e.stop_all();
}

#[test]
fn rate_limit_keyed_by_header() {
    let f = fx(
        "[rate_limit]\nenabled = true\nrequests = 1\nwindow = \"1m\"\nkey = \"header:x-key\"",
        "",
    );
    let call = |k: &str| {
        let mut s = f.connect();
        write!(
            s,
            "GET /x HTTP/1.1\r\nhost: x\r\nx-key: {k}\r\nconnection: close\r\n\r\n"
        )
        .unwrap();
        let mut o = String::new();
        s.read_to_string(&mut o).unwrap();
        o
    };
    assert!(call("a").starts_with("HTTP/1.1 200"));
    assert!(call("b").starts_with("HTTP/1.1 200"));
    assert!(call("a").starts_with("HTTP/1.1 429"));
    f.e.stop_all();
}

#[test]
fn chaos_injects_faults_at_the_rate_and_can_be_switched_off() {
    let f = fx("", "");
    f.e.set_chaos(Some("srv_a"), Some(1.0)).unwrap();
    assert_eq!(f.e.chaos("srv_a").unwrap(), Some(1.0));
    assert_eq!(f.e.list()[0].chaos, Some(1.0));
    for _ in 0..6 {
        f.raw("/x");
    }
    let recs = f.e.traffic(&TrafficFilter::default());
    assert!(
        recs.iter()
            .all(|r| r.fault.as_deref().is_some_and(|x| x.starts_with("chaos:")))
    );
    f.e.set_chaos(None, None).unwrap();
    assert!(text(&f.raw("/x").0).contains("0123456789"));
    f.e.stop_all();
}

#[test]
fn network_profile_overrides_and_offline_refuses_then_recovers() {
    let f = fx("", "");
    f.e.set_network_profile(NetworkProfile::Fast3g);
    let t = Instant::now();
    f.raw("/x");
    assert!(t.elapsed().as_millis() >= 150);
    assert_eq!(
        f.e.traffic(&TrafficFilter::default())[0].timing.network_ms,
        150
    );

    f.e.set_network_profile(NetworkProfile::Offline);
    std::thread::sleep(Duration::from_millis(150));
    let err = TcpStream::connect(("127.0.0.1", f.port)).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::ConnectionRefused);

    f.e.set_network_profile(NetworkProfile::None);
    std::thread::sleep(Duration::from_millis(300));
    assert!(text(&f.raw("/x").0).starts_with("HTTP/1.1 200"));
    f.rt.block_on(async {});
    f.e.stop_all();
}

#[test]
fn cli_start_options_apply() {
    let f = fx_with(
        "",
        "",
        EngineConfig {
            chaos: Some(1.0),
            network: Some(NetworkProfile::Slow3g),
            ..Default::default()
        },
    );
    assert_eq!(f.e.network_profile(), NetworkProfile::Slow3g);
    assert_eq!(f.e.chaos("srv_a").unwrap(), Some(1.0));
    f.e.stop_all();
}
