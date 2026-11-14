//! Callbacks (webhook simulation, SPEC §7.3) against a local receiver.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use fauxfield_engine::{Engine, EngineConfig, TrafficFilter};
use fauxfield_model::parse_server;
use hmac::{Hmac, Mac};
use sha2::Sha256;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// (head, body) of every request the receiver saw.
type Seen = Arc<Mutex<Vec<(String, String, Instant)>>>;

/// Receiver answering with `status(n)` for the n-th request (0-based).
fn receiver(status: impl Fn(usize) -> u16 + Send + Sync + 'static) -> (u16, Seen) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let seen: Seen = Arc::default();
    let s2 = seen.clone();
    let n = Arc::new(AtomicUsize::new(0));
    let status = Arc::new(status);
    std::thread::spawn(move || {
        for mut s in l.incoming().flatten() {
            let mut buf = Vec::new();
            let mut b = [0u8; 1024];
            let (head, body) = loop {
                match s.read(&mut b) {
                    Ok(0) | Err(_) => break (String::new(), String::new()),
                    Ok(k) => buf.extend_from_slice(&b[..k]),
                }
                if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&buf[..i]).to_string();
                    let len = head
                        .to_ascii_lowercase()
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: ").map(str::to_string))
                        .and_then(|v| v.trim().parse::<usize>().ok())
                        .unwrap_or(0);
                    if buf.len() >= i + 4 + len {
                        break (
                            head,
                            String::from_utf8_lossy(&buf[i + 4..i + 4 + len]).to_string(),
                        );
                    }
                }
            };
            if head.is_empty() {
                continue;
            }
            s2.lock().unwrap().push((head, body, Instant::now()));
            let st = status(n.fetch_add(1, Ordering::SeqCst));
            let _ = s.write_all(
                format!("HTTP/1.1 {st} X\r\ncontent-length: 2\r\nconnection: close\r\n\r\nok")
                    .as_bytes(),
            );
        }
    });
    (port, seen)
}

struct Fx {
    _rt: tokio::runtime::Runtime,
    e: Engine,
    port: u16,
}

fn fx(callbacks: &str) -> Fx {
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
            "id = \"srv_a\"\nname = \"S\"\nport = {port}\n[variables]\nwebhook_secret = \"s3cret\"\n\
             [[routes]]\nid = \"rte_c\"\nmethod = \"POST\"\npath = \"/charges\"\n\
             [[routes.responses]]\nid = \"rsp_c\"\nstatus = 202\nbody = '{{\"id\":\"ch_1\"}}'\n{callbacks}"
        ))
        .unwrap(),
    ]);
    e.start("srv_a").unwrap();
    Fx { _rt: rt, e, port }
}

impl Fx {
    fn post(&self, path: &str, body: &str) -> String {
        let mut s = TcpStream::connect(("127.0.0.1", self.port)).unwrap();
        s.set_read_timeout(Some(Duration::from_secs(10))).unwrap();
        s.write_all(
            format!(
                "POST {path} HTTP/1.1\r\nhost: t\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
        let mut out = String::new();
        let _ = s.read_to_string(&mut out);
        out
    }

    fn callbacks(&self, n: usize) -> Vec<fauxfield_engine::Recording> {
        let t0 = Instant::now();
        loop {
            let v: Vec<_> = self
                .e
                .traffic(&TrafficFilter::default())
                .into_iter()
                .filter(|r| r.source == "callback")
                .collect();
            if v.len() >= n || t0.elapsed() > Duration::from_secs(10) {
                return v;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

fn sign(key: &str, data: &str) -> String {
    let mut m = Hmac::<Sha256>::new_from_slice(key.as_bytes()).unwrap();
    m.update(data.as_bytes());
    m.finalize()
        .into_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn signed_webhook_arrives_after_delay_and_is_linked() {
    let (port, seen) = receiver(|_| 200);
    let f = fx(
        "[[routes.responses.callbacks]]\nid = \"cb_settled\"\nurl = \"{{ request.json.callback_url }}\"\ndelay_ms = 400\n\
         body = '{\"event\":\"payment.settled\",\"id\":\"{{ request.json.id }}\"}'\n\
         [routes.responses.callbacks.headers]\n\"x-signature\" = \"{{ hmac_sha256(key=vars.webhook_secret, data=callback.body) }}\"\n",
    );
    let t0 = Instant::now();
    let reply = f.post(
        "/charges",
        &format!("{{\"id\":\"ch_9\",\"callback_url\":\"http://127.0.0.1:{port}/hook\"}}"),
    );
    assert!(reply.starts_with("HTTP/1.1 202"), "{reply}");
    let cbs = f.callbacks(1);
    assert_eq!(cbs.len(), 1);
    assert!(cbs[0].error.is_none(), "{:?}", cbs[0].error);
    let (head, body, at) = seen.lock().unwrap()[0].clone();
    assert!(at.duration_since(t0) >= Duration::from_millis(400));
    assert_eq!(body, r#"{"event":"payment.settled","id":"ch_9"}"#);
    assert!(head.starts_with("POST /hook "), "{head}");
    assert!(
        head.to_ascii_lowercase()
            .contains(&format!("x-signature: {}", sign("s3cret", &body))),
        "{head}"
    );
    let parent =
        f.e.traffic(&TrafficFilter::default())
            .into_iter()
            .find(|r| r.source != "callback")
            .unwrap();
    assert_eq!(cbs[0].parent_id, Some(parent.id));
    assert_eq!(cbs[0].status, 200);
    assert_eq!(cbs[0].callback_id.as_deref(), Some("cb_settled#1"));
    assert!(cbs[0].error.is_none());
}

#[test]
fn failures_retry_with_backoff_and_each_attempt_is_recorded() {
    let (port, seen) = receiver(|n| if n == 0 { 500 } else { 200 });
    let f = fx(&format!(
        "[[routes.responses.callbacks]]\nid = \"cb\"\nurl = \"http://127.0.0.1:{port}/h\"\nretries = 2\nbackoff_ms = 50\nbody = \"x\"\n"
    ));
    f.post("/charges", "{}");
    let cbs = f.callbacks(2);
    assert_eq!(cbs.len(), 2);
    assert_eq!(seen.lock().unwrap().len(), 2);
    let mut by_attempt = cbs.clone();
    by_attempt.sort_by_key(|r| r.id);
    assert_eq!(by_attempt[0].status, 500);
    assert!(by_attempt[0].error.is_some());
    assert_eq!(by_attempt[1].status, 200);
    assert_eq!(by_attempt[1].callback_id.as_deref(), Some("cb#2"));
}

#[test]
fn unreachable_receiver_records_the_error_and_refire_works() {
    let dead = free_port();
    let f = fx(&format!(
        "[[routes.responses.callbacks]]\nid = \"cb\"\nurl = \"http://127.0.0.1:{dead}/h\"\ntimeout_ms = 500\n"
    ));
    f.post("/charges", "{}");
    let cbs = f.callbacks(1);
    assert_eq!(cbs[0].status, 0);
    assert!(
        cbs[0]
            .error
            .as_deref()
            .unwrap()
            .contains("connection failed")
    );
    let again = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(f.e.callback_refire("srv_a", cbs[0].id, None))
        .unwrap();
    assert_eq!(again.len(), 1);
    assert_eq!(again[0].parent_id, cbs[0].parent_id);
    assert_eq!(f.callbacks(2).len(), 2);
}
