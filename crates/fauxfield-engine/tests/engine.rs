use std::net::TcpListener;

use fauxfield_engine::{
    Engine, EngineConfig, EngineError, EngineEvent, ServerStatus, TestRequest, TrafficFilter,
};
use fauxfield_model::parse_server;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn server(port: u16, body: &str, extra: &str) -> fauxfield_model::Server {
    parse_server(&format!(
        r#"
id = "srv_a"
name = "Users API"
port = {port}
prefix = "/api"
[[routes]]
id = "rte_1"
method = "GET"
path = "/users"
[[routes.responses]]
id = "rsp_1"
status = 201
content_type = "application/json"
body = '{body}'
{extra}
[routes.responses.headers]
x-mock = "yes"
"#
    ))
    .unwrap()
}

fn req(method: &str, path: &str) -> TestRequest {
    TestRequest {
        method: method.into(),
        path: path.into(),
        ..Default::default()
    }
}

fn engine(rt: &tokio::runtime::Runtime, root: &std::path::Path) -> Engine {
    Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: root.to_path_buf(),
            ..Default::default()
        },
    )
}

#[test]
fn serves_static_response_and_records_it() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let port = free_port();
    e.apply(vec![server(port, r#"{"ok":true}"#, "")]);
    let mut events = e.subscribe();
    e.start("srv_a").unwrap();
    let r = rt
        .block_on(e.send_test("srv_a", &req("GET", "/api/users/")))
        .unwrap();
    assert_eq!(r.status, 201);
    assert_eq!(r.body, r#"{"ok":true}"#);
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("x-mock") && v == "yes")
    );
    assert!(
        r.headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "application/json")
    );
    let t = e.traffic(&TrafficFilter::default());
    assert_eq!(t.len(), 1);
    assert_eq!(t[0].route, "rte_1");
    assert_eq!(t[0].source, "test");
    // Status then Traffic events were broadcast.
    let mut saw_traffic = false;
    while let Ok(ev) = events.try_recv() {
        saw_traffic |= matches!(ev, EngineEvent::Traffic(_));
    }
    assert!(saw_traffic);
    e.stop_all();
    assert_eq!(e.running_count(), 0);
}

#[test]
fn unmatched_is_404_problem_with_hints() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    e.apply(vec![server(free_port(), "{}", "")]);
    e.start("srv_a").unwrap();
    let r = rt
        .block_on(e.send_test("srv_a", &req("GET", "/api/user")))
        .unwrap();
    assert_eq!(r.status, 404);
    assert!(r.headers.iter().any(|(k, v)| k.eq_ignore_ascii_case("content-type") && v == "application/problem+json"));
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert_eq!(v["closest"][0]["path"], "/api/users");
    assert_eq!(e.traffic(&TrafficFilter::default())[0].route, "unmatched");
    e.stop_all();
}

#[test]
fn delay_is_applied_and_timed() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    e.apply(vec![server(free_port(), "{}", "delay_ms = 120")]);
    e.start("srv_a").unwrap();
    let t = std::time::Instant::now();
    rt.block_on(e.send_test("srv_a", &req("GET", "/api/users")))
        .unwrap();
    assert!(t.elapsed().as_millis() >= 120);
    let rec = &e.traffic(&TrafficFilter::default())[0];
    assert_eq!(rec.timing.delay_ms, 120);
    assert!(rec.timing.total_ms >= 120);
    e.stop_all();
}

#[test]
fn hot_reload_swaps_routes_without_restarting_the_listener() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let port = free_port();
    e.apply(vec![server(port, "one", "")]);
    let addr = e.start("srv_a").unwrap();
    assert_eq!(
        rt.block_on(e.send_test("srv_a", &req("GET", "/api/users")))
            .unwrap()
            .body,
        "one"
    );
    e.apply(vec![server(port, "two", "")]);
    assert_eq!(
        rt.block_on(e.send_test("srv_a", &req("GET", "/api/users")))
            .unwrap()
            .body,
        "two"
    );
    // Same listener: start is idempotent and returns the same address.
    assert_eq!(e.start("srv_a").unwrap(), addr);
    e.stop_all();
}

#[test]
fn port_change_restarts_only_that_listener() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let (p1, p2) = (free_port(), free_port());
    e.apply(vec![server(p1, "x", "")]);
    e.start("srv_a").unwrap();
    e.apply(vec![server(p2, "x", "")]);
    let info = &e.list()[0];
    assert_eq!(info.port, p2);
    match &info.status {
        ServerStatus::Running { addr } => assert!(addr.ends_with(&p2.to_string())),
        s => panic!("expected running, got {s:?}"),
    }
    assert!(std::net::TcpStream::connect(("127.0.0.1", p1)).is_err());
    e.stop_all();
}

#[test]
fn port_in_use_is_a_typed_error() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let blocker = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = blocker.local_addr().unwrap().port();
    e.apply(vec![server(port, "x", "")]);
    match e.start("srv_a") {
        Err(EngineError::PortInUse { port: p, .. }) => assert_eq!(p, port),
        other => panic!("expected PortInUse, got {other:?}"),
    }
    assert!(matches!(
        e.list()[0].status,
        ServerStatus::Failed {
            port_in_use: true,
            ..
        }
    ));
}

#[test]
fn body_file_is_confined_to_the_workspace() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.json"), "from-file").unwrap();
    let e = engine(&rt, dir.path());
    let mut s = server(free_port(), "x", "");
    s.routes[0].responses[0].body = None;
    s.routes[0].responses[0].body_file = Some("a.json".into());
    e.apply(vec![s.clone()]);
    e.start("srv_a").unwrap();
    assert_eq!(
        rt.block_on(e.send_test("srv_a", &req("GET", "/api/users")))
            .unwrap()
            .body,
        "from-file"
    );
    s.routes[0].responses[0].body_file = Some("../escape".into());
    e.apply(vec![s]);
    assert_eq!(
        rt.block_on(e.send_test("srv_a", &req("GET", "/api/users")))
            .unwrap()
            .status,
        500
    );
    e.stop_all();
}

#[test]
fn authorization_header_is_redacted_in_recordings() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    e.apply(vec![server(free_port(), "x", "")]);
    e.start("srv_a").unwrap();
    let mut r = req("GET", "/api/users");
    r.headers
        .push(("Authorization".into(), "Bearer secret".into()));
    rt.block_on(e.send_test("srv_a", &r)).unwrap();
    let rec = &e.traffic(&TrafficFilter::default())[0];
    assert!(
        rec.request_headers
            .iter()
            .any(|(k, v)| k.eq_ignore_ascii_case("authorization") && v == "***")
    );
    e.stop_all();
}
