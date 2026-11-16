//! M17.1: Responses and Rules on the operations of a crud Route.

use std::net::TcpListener;

use fauxfield_engine::{Engine, EngineConfig, TestRequest, TrafficFilter};
use fauxfield_model::parse_server;
use serde_json::Value;

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

const PEOPLE: &str =
    r#"[{"id":1,"name":"Ann","email":"ann@x.io"},{"id":2,"name":"Bob","email":"bob@x.io"}]"#;

fn server(port: u16, route_extra: &str, more: &str) -> fauxfield_model::Server {
    parse_server(&format!(
        r#"
id = "srv_o"
name = "Ops"
port = {port}
[[datasets]]
name = "people"
seed = "data/people.json"
[[datasets]]
name = "audit"
id_strategy = "increment"
[[routes]]
id = "rte_crud"
kind = "crud"
path = "/people"
dataset = "people"
{route_extra}
{more}
"#
    ))
    .unwrap()
}

fn engine(
    rt: &tokio::runtime::Runtime,
    dir: &tempfile::TempDir,
    route_extra: &str,
    more: &str,
) -> Engine {
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(dir.path().join("data/people.json"), PEOPLE).unwrap();
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: dir.path().to_path_buf(),
            ..Default::default()
        },
    );
    e.apply(vec![server(free_port(), route_extra, more)]);
    e.start("srv_o").unwrap();
    e
}

fn call(
    rt: &tokio::runtime::Runtime,
    e: &Engine,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, Value) {
    let mut r = TestRequest {
        method: method.into(),
        path: path.into(),
        ..Default::default()
    };
    r.body = body.into();
    if !body.is_empty() {
        r.headers
            .push(("content-type".into(), "application/json".into()));
    }
    let res = rt.block_on(e.send_test("srv_o", &r)).unwrap();
    (
        res.status,
        serde_json::from_str(&res.body).unwrap_or(Value::Null),
    )
}

const DUP: &str = r#"
[[routes.operations.create.responses]]
id = "rsp_dup"
name = "Duplicate email"
status = 409
content_type = "application/problem+json"
body = '{"title":"Duplicate email"}'
[[routes.operations.create.responses.rules]]
target = "body"
key = "$.email"
op = "eq"
value = "taken@x.io"
"#;

#[test]
fn matching_requests_use_the_response_and_others_fall_through() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, &dir, DUP, "");
    let (s, v) = call(
        &rt,
        &e,
        "POST",
        "/people",
        r#"{"name":"Z","email":"taken@x.io"}"#,
    );
    assert_eq!((s, v["title"].as_str()), (409, Some("Duplicate email")));
    let (s, v) = call(
        &rt,
        &e,
        "POST",
        "/people",
        r#"{"name":"Z","email":"new@x.io"}"#,
    );
    assert_eq!(s, 201);
    assert_eq!(v["name"], "Z");
    // Other operations are untouched.
    assert_eq!(call(&rt, &e, "GET", "/people", "").0, 200);
    assert_eq!(call(&rt, &e, "GET", "/people/1", "").0, 200);
    // Recording: Response name, or crud:<op> when the built-in handler answered.
    let t = e.traffic(&TrafficFilter::default());
    let names: Vec<_> = t
        .iter()
        .map(|r| (r.response_id.as_str(), r.response_name.as_str()))
        .collect();
    assert!(names.contains(&("rsp_dup", "Duplicate email")), "{names:?}");
    assert!(names.contains(&("", "crud:create")), "{names:?}");
    assert!(names.contains(&("", "crud:list")), "{names:?}");
    e.stop_all();
}

#[test]
fn first_match_wins_and_rule_less_responses_never_match() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let extra = r#"
[[routes.operations.get.responses]]
id = "rsp_norules"
status = 418
[[routes.operations.get.responses]]
id = "rsp_a"
status = 202
body = "first"
[[routes.operations.get.responses.rules]]
target = "header"
key = "x-mode"
op = "eq"
value = "beta"
[[routes.operations.get.responses]]
id = "rsp_b"
status = 203
body = "second"
[[routes.operations.get.responses.rules]]
target = "header"
key = "x-mode"
op = "exists"
"#;
    let e = engine(&rt, &dir, extra, "");
    let go = |mode: Option<&str>| {
        let mut r = TestRequest {
            method: "GET".into(),
            path: "/people/1".into(),
            ..Default::default()
        };
        if let Some(m) = mode {
            r.headers.push(("x-mode".into(), m.into()));
        }
        rt.block_on(e.send_test("srv_o", &r)).unwrap().status
    };
    assert_eq!(go(Some("beta")), 202);
    assert_eq!(go(Some("other")), 203);
    assert_eq!(go(None), 200);
    e.stop_all();
}

#[test]
fn every_operation_can_be_answered() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let mut extra = String::new();
    for (op, status) in [
        ("list", 211),
        ("get", 212),
        ("create", 213),
        ("update", 214),
        ("patch", 215),
        ("delete", 216),
    ] {
        extra.push_str(&format!(
            "[[routes.operations.{op}.responses]]\nid = \"rsp_{op}\"\nstatus = {status}\n[[routes.operations.{op}.responses.rules]]\ntarget = \"method\"\nop = \"ne\"\nvalue = \"NOPE\"\n"
        ));
    }
    let e = engine(&rt, &dir, &extra, "");
    for (m, p, s) in [
        ("GET", "/people", 211),
        ("GET", "/people/1", 212),
        ("POST", "/people", 213),
        ("PUT", "/people/1", 214),
        ("PATCH", "/people/1", 215),
        ("DELETE", "/people/1", 216),
    ] {
        assert_eq!(call(&rt, &e, m, p, "{}").0, s, "{m} {p}");
    }
    // The answered DELETE did not run the built-in handler.
    assert_eq!(call(&rt, &e, "GET", "/people/1", "").0, 212);
    e.stop_all();
}

#[test]
fn dataset_actions_run_in_an_operation_response() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let extra = r#"
[[routes.operations.delete.responses]]
id = "rsp_soft"
status = 200
body = '{"audited":{{ action.0.id }}}'
[[routes.operations.delete.responses.rules]]
target = "header"
key = "x-soft"
op = "exists"
[[routes.operations.delete.responses.actions]]
dataset = "audit"
op = "insert"
[routes.operations.delete.responses.actions.fields]
who = "{{ request.params.id }}"
"#;
    let e = engine(&rt, &dir, extra, "");
    let mut r = TestRequest {
        method: "DELETE".into(),
        path: "/people/2".into(),
        ..Default::default()
    };
    r.headers.push(("x-soft".into(), "1".into()));
    let res = rt.block_on(e.send_test("srv_o", &r)).unwrap();
    assert_eq!(res.status, 200);
    assert!(res.body.contains("audited"), "{}", res.body);
    // The Response answered, so the built-in delete did not run.
    assert_eq!(call(&rt, &e, "GET", "/people/2", "").0, 200);
    e.stop_all();
}

#[test]
fn count_rules_are_per_operation_and_reset() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let extra = r#"
[[routes.operations.list.responses]]
id = "rsp_flaky"
status = 503
[[routes.operations.list.responses.rules]]
target = "count"
op = "eq"
value = 2
"#;
    let e = engine(&rt, &dir, extra, "");
    assert_eq!(call(&rt, &e, "GET", "/people", "").0, 200);
    // Another operation's calls do not advance the list count.
    assert_eq!(call(&rt, &e, "GET", "/people/1", "").0, 200);
    assert_eq!(call(&rt, &e, "GET", "/people", "").0, 503);
    assert_eq!(call(&rt, &e, "GET", "/people", "").0, 200);
    e.reset_state(None).unwrap();
    assert_eq!(call(&rt, &e, "GET", "/people", "").0, 200);
    assert_eq!(call(&rt, &e, "GET", "/people", "").0, 503);
    e.stop_all();
}

#[test]
fn a_higher_priority_http_route_still_wins() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let more = r#"
[[routes]]
id = "rte_over"
method = "POST"
path = "/people"
priority = 5
[[routes.responses]]
id = "rsp_o"
status = 403
body = "no"
"#;
    let e = engine(&rt, &dir, DUP, more);
    assert_eq!(
        call(&rt, &e, "POST", "/people", r#"{"email":"taken@x.io"}"#).0,
        403
    );
    e.stop_all();
}

#[test]
fn hot_reload_swaps_operations_without_restarting_the_server() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, &dir, "", "");
    let addr = e.start("srv_o").unwrap();
    assert_eq!(
        call(&rt, &e, "POST", "/people", r#"{"email":"taken@x.io"}"#).0,
        201
    );
    e.apply(vec![server(addr.port(), DUP, "")]);
    assert_eq!(
        call(&rt, &e, "POST", "/people", r#"{"email":"taken@x.io"}"#).0,
        409
    );
    e.apply(vec![server(addr.port(), "", "")]);
    assert_eq!(
        call(&rt, &e, "POST", "/people", r#"{"email":"taken@x.io"}"#).0,
        201
    );
    assert_eq!(e.start("srv_o").unwrap(), addr);
    e.stop_all();
}

#[test]
fn validate_runs_before_operation_rules() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("schemas")).unwrap();
    std::fs::write(
        dir.path().join("schemas/p.json"),
        r#"{"type":"object","required":["name"]}"#,
    )
    .unwrap();
    let e = engine(
        &rt,
        &dir,
        &format!("validate = \"schemas/p.json\"\n{DUP}"),
        "",
    );
    // Matches the Rule but violates the schema: the built-in 422 wins.
    assert_eq!(
        call(&rt, &e, "POST", "/people", r#"{"email":"taken@x.io"}"#).0,
        422
    );
    assert_eq!(
        call(
            &rt,
            &e,
            "POST",
            "/people",
            r#"{"name":"Z","email":"taken@x.io"}"#
        )
        .0,
        409
    );
    e.stop_all();
}
