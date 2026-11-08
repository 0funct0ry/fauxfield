//! C5: hot reload driven by the app's own Store (cross-platform).
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::time::{Duration, Instant};

use fauxfield_control::{Host, HostOptions};
use fauxfield_model::Severity;
use fauxfield_store::{Target, init_workspace};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

/// One request on an existing keep-alive connection.
fn get(s: &mut TcpStream, path: &str) -> (u16, String) {
    write!(s, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
    let mut buf = Vec::new();
    let mut b = [0u8; 1];
    while !buf.ends_with(b"\r\n\r\n") {
        s.read_exact(&mut b).unwrap();
        buf.push(b[0]);
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let status = head.split_whitespace().nth(1).unwrap().parse().unwrap();
    let len = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    let mut body = vec![0; len];
    s.read_exact(&mut body).unwrap();
    (status, String::from_utf8_lossy(&body).to_string())
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

fn setup() -> (tempfile::TempDir, Host, u16, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    init_workspace(dir.path()).unwrap();
    let port = free_port();
    let file = dir.path().join("servers/example-api.toml");
    let t = std::fs::read_to_string(&file)
        .unwrap()
        .replace("3001", &port.to_string());
    std::fs::write(&file, t).unwrap();
    let host = Host::start(
        dir.path(),
        HostOptions {
            watch: true,
            ..Default::default()
        },
    )
    .unwrap();
    host.engine().start("Example API").unwrap();
    (dir, host, port, file)
}

fn until(mut f: impl FnMut() -> bool) -> Option<Duration> {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(5) {
        if f() {
            return Some(t.elapsed());
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

#[test]
fn external_body_edit_changes_the_next_response_on_a_live_connection() {
    let (_d, _h, port, file) = setup();
    let mut c = connect(port);
    assert_eq!(get(&mut c, "/health"), (200, "{\"status\": \"ok\"}".into()));
    let t = std::fs::read_to_string(&file)
        .unwrap()
        .replace("\"status\": \"ok\"", "\"status\": \"changed\"");
    std::fs::write(&file, t).unwrap();
    let lat = until(|| get(&mut c, "/health").1.contains("changed")).expect("reloaded");
    eprintln!(
        "save-to-served latency: {} ms (budget 50 ms, SPEC §15)",
        lat.as_millis()
    );
    assert!(lat < Duration::from_millis(500), "{lat:?}");
}

#[test]
fn external_port_edit_moves_the_listener() {
    let (_d, _h, port, file) = setup();
    let new = free_port();
    let t = std::fs::read_to_string(&file)
        .unwrap()
        .replace(&port.to_string(), &new.to_string());
    std::fs::write(&file, t).unwrap();
    assert!(until(|| TcpStream::connect(("127.0.0.1", new)).is_ok()).is_some());
    assert!(until(|| TcpStream::connect(("127.0.0.1", port)).is_err()).is_some());
}

#[test]
fn external_route_delete_becomes_a_404_with_a_hint() {
    let (_d, _h, port, file) = setup();
    let mut c = connect(port);
    let t = std::fs::read_to_string(&file).unwrap();
    let t = format!(
        "{}\n[[routes]]\nid = \"rte_other\"\nmethod = \"GET\"\npath = \"/healthy\"\n  [[routes.responses]]\n  id = \"rsp_o\"\n  default = true\n  status = 200\n  body = \"x\"\n",
        t
    );
    std::fs::write(&file, &t).unwrap();
    assert!(until(|| get(&mut c, "/healthy").0 == 200).is_some());
    // Remove the /health route (first [[routes]] block up to the next one).
    let start = t.find("[[routes]]").unwrap();
    let end = t.rfind("[[routes]]").unwrap();
    std::fs::write(&file, format!("{}{}", &t[..start], &t[end..])).unwrap();
    assert!(
        until(|| get(&mut c, "/health").0 == 404 || get(&mut c, "/health").1.contains("closest"))
            .is_some()
    );
    let (s, body) = get(&mut c, "/health");
    assert_eq!(s, 404);
    assert!(body.contains("\"closest\""), "{body}");
}

#[test]
fn an_invalid_external_edit_keeps_the_previous_routes_and_shows_problems() {
    let (_d, h, port, file) = setup();
    let mut c = connect(port);
    // Syntax error.
    std::fs::write(&file, "id = \"srv_x\"\nname = [broken").unwrap();
    assert!(
        until(|| h
            .store()
            .snapshot()
            .diagnostics
            .iter()
            .any(|d| d.severity == Severity::Error))
        .is_some()
    );
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        get(&mut c, "/health").0,
        200,
        "previous routes keep serving"
    );
}

#[test]
fn a_gui_field_edit_shows_in_the_next_response() {
    let (_d, h, port, _f) = setup();
    let mut c = connect(port);
    let snap = h.store().snapshot();
    let rsp = snap.servers[0].1.routes[0].responses[0].id.clone();
    h.store()
        .set_int(&Target::Response(rsp), "status", Some(202))
        .unwrap();
    assert!(until(|| get(&mut c, "/health").0 == 202).is_some());
}
