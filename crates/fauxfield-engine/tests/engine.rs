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

#[test]
fn explicit_start_of_a_disabled_server_fails_and_run_all_skips_it() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let mut s = server(free_port(), "{}", "");
    s.enabled = false;
    e.apply(vec![s]);
    match e.start("srv_a") {
        Err(EngineError::Disabled(n)) => assert_eq!(n, "Users API"),
        other => panic!("expected Disabled, got {other:?}"),
    }
    assert!(e.start_all().is_empty(), "run all skips disabled servers");
    assert_eq!(e.list()[0].status.label(), "stopped");
}

#[test]
fn unmatched_hint_false_omits_closest_but_keeps_the_problem() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let mut s = server(free_port(), "{}", "");
    s.unmatched_hint = Some(false);
    e.apply(vec![s]);
    e.start("srv_a").unwrap();
    let r = rt
        .block_on(e.send_test("srv_a", &req("GET", "/api/user")))
        .unwrap();
    assert_eq!(r.status, 404);
    let v: serde_json::Value = serde_json::from_str(&r.body).unwrap();
    assert!(v.get("closest").is_none(), "{v}");
    assert_eq!(v["status"], 404);
    e.stop_all();
}

fn conditional(port: u16, mode: &str) -> fauxfield_model::Server {
    parse_server(&format!(
        r#"
id = "srv_c"
name = "Cond"
port = {port}
[[routes]]
id = "rte_c"
path = "/c"
response_mode = "{mode}"
[[routes.responses]]
id = "rsp_admin"
name = "Admin"
status = 200
  [[routes.responses.rules]]
  target = "query"
  key = "role"
  op = "eq"
  value = "admin"
[[routes.responses]]
id = "rsp_401"
name = "Unauthorized"
status = 401
  [[routes.responses.rules]]
  target = "header"
  key = "Authorization"
  op = "absent"
[[routes.responses]]
id = "rsp_default"
name = "Default"
default = true
status = 202
"#
    ))
    .unwrap()
}

#[test]
fn rules_choose_the_response_and_the_trace_explains_it() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    e.apply(vec![conditional(free_port(), "rules")]);
    e.start("srv_c").unwrap();
    let send = |path: &str, auth: bool| {
        let mut r = req("GET", path);
        if auth {
            r.headers.push(("authorization".into(), "Bearer x".into()));
        }
        rt.block_on(e.send_test("srv_c", &r)).unwrap().status
    };
    assert_eq!(send("/c?role=admin", false), 200);
    assert_eq!(send("/c", false), 401);
    assert_eq!(send("/c", true), 202);
    let recs = e.traffic(&TrafficFilter::default());
    let last = recs.last().unwrap();
    let t = last.trace.as_ref().unwrap();
    assert_eq!(last.response_id, "rsp_default");
    assert_eq!(t.call, 3);
    assert_eq!(t.evaluated.len(), 3);
    assert!(!t.evaluated[1].passed);
    assert_eq!(t.evaluated[1].rules[0].actual.as_deref(), Some("***"));
}

#[test]
fn sequential_cycles_and_reset_state_restarts_it() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    e.apply(vec![conditional(free_port(), "sequential")]);
    e.start("srv_c").unwrap();
    let get = || {
        rt.block_on(e.send_test("srv_c", &req("GET", "/c")))
            .unwrap()
            .status
    };
    assert_eq!([get(), get(), get(), get()], [200, 401, 202, 200]);
    e.reset_state(None).unwrap();
    assert_eq!(get(), 200);
}

#[test]
fn disabled_mode_makes_the_route_unmatched() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    e.apply(vec![conditional(free_port(), "disabled")]);
    e.start("srv_c").unwrap();
    assert_eq!(
        rt.block_on(e.send_test("srv_c", &req("GET", "/c")))
            .unwrap()
            .status,
        404
    );
}

fn tpl_server(port: u16, seed: Option<u64>) -> fauxfield_model::Server {
    let seed = seed.map(|s| format!("fake_seed = {s}")).unwrap_or_default();
    parse_server(&format!(
        r#"
id = "srv_t"
name = "T"
port = {port}
{seed}
[[routes]]
id = "rte_l"
method = "GET"
path = "/users/:id"
[routes.headers]
x-route = "r-{{{{ request.params.id }}}}"
x-both = "route"
[[routes.responses]]
id = "rsp_1"
content_type = "application/json"
body = '[{{% for _ in repeat(n=20) %}}{{"name":"{{{{ fake(kind="name") }}}}","id":"{{{{ uuid() }}}}"}}{{% if not loop.last %}},{{% endif %}}{{% endfor %}}]'
[routes.responses.headers]
x-both = "response"
[[routes]]
id = "rte_v"
method = "GET"
path = "/verbatim"
[[routes.responses]]
id = "rsp_v"
template = false
body = "{{{{ not rendered }}}}"
[[routes]]
id = "rte_bad"
method = "GET"
path = "/bad"
[[routes.responses]]
id = "rsp_b"
body = "{{{{ oops("
"#
    ))
    .unwrap()
}

#[test]
fn templates_render_seeded_data_route_headers_and_errors() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let port = free_port();
    e.apply(vec![tpl_server(port, Some(7))]);
    e.start("srv_t").unwrap();
    let get = |p: &str| rt.block_on(e.send_test("srv_t", &req("GET", p))).unwrap();
    let a = get("/users/9");
    assert_eq!(a.status, 200);
    let h = |r: &fauxfield_engine::TestResponse, n: &str| {
        r.headers
            .iter()
            .find(|(k, _)| k == n)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(h(&a, "x-route").as_deref(), Some("r-9"));
    assert_eq!(h(&a, "x-both").as_deref(), Some("response"));
    let v: serde_json::Value = serde_json::from_str(&a.body).unwrap();
    assert_eq!(v.as_array().unwrap().len(), 20);
    // Same seed + same request counter after a state reset => identical output.
    e.reset_state(None).unwrap();
    assert_eq!(get("/users/9").body, a.body);
    assert_eq!(get("/verbatim").body, "{{ not rendered }}");
    let bad = get("/bad");
    assert_eq!(bad.status, 500);
    let t = e.traffic(&TrafficFilter::default());
    assert!(t.iter().any(|r| r.error.is_some()));
    // Unseeded: different.
    e.apply(vec![tpl_server(port, None)]);
    let b = get("/users/9");
    assert_ne!(b.body, a.body);
}

#[test]
fn switching_environment_changes_response_and_prefix_without_restart() {
    use fauxfield_engine::{EnvData, VarContext};
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = engine(&rt, dir.path());
    let port = free_port();
    let mut s = server(port, r#"{"host":"{{ vars.host }}","env":"{{ env }}"}"#, "");
    s.prefix = Some("/{{ vars.ver }}".into());
    let ev = |h: &str, v: &str| EnvData {
        variables: [
            ("host".to_string(), toml::Value::String(h.into())),
            ("ver".to_string(), toml::Value::String(v.into())),
        ]
        .into(),
        dotenv: Default::default(),
    };
    let vc = VarContext {
        active_env: "local".into(),
        globals: [("host".to_string(), toml::Value::String("global".into()))].into(),
        envs: [
            ("local".to_string(), ev("l.example", "v1")),
            ("staging".to_string(), ev("s.example", "v2")),
        ]
        .into(),
        runtime: Default::default(),
    };
    e.set_var_context(vc);
    e.apply(vec![s]);
    e.start("srv_a").unwrap();
    let r = rt
        .block_on(e.send_test("srv_a", &req("GET", "/v1/users")))
        .unwrap();
    assert!(
        r.body.contains(r#""host":"l.example","env":"local""#),
        "{r:?}"
    );
    e.set_active_env("staging");
    let r = rt
        .block_on(e.send_test("srv_a", &req("GET", "/v2/users")))
        .unwrap();
    assert!(r.body.contains(r#""host":"s.example","env":"staging""#));
    e.set_runtime_var("host", Some("rt"));
    let r = rt
        .block_on(e.send_test("srv_a", &req("GET", "/v2/users")))
        .unwrap();
    assert!(r.body.contains(r#""host":"rt""#));
    let scope = e.vars_in_scope("srv_a", Some("rte_1")).unwrap();
    assert_eq!(
        scope
            .iter()
            .find(|v| v.key == "host")
            .unwrap()
            .scope
            .as_str(),
        "runtime"
    );
    e.reset_state(None).unwrap();
    e.stop("srv_a").unwrap();
}

// ---------------------------------------------------------------- M8 datasets and CRUD

const PEOPLE: &str = r#"[
 {"id":1,"name":"Ann","age":30,"role":"admin","city":"Oslo"},
 {"id":2,"name":"Bob","age":25,"role":"user","city":"Rome"},
 {"id":3,"name":"Anders","age":41,"role":"user","city":"Oslo"},
 {"id":4,"name":"Cy","age":19,"role":"user","city":"Paris"},
 {"id":5,"name":"Dee","age":30,"role":"admin","city":"Rome"}
]"#;

fn crud_server(
    port: u16,
    ds_extra: &str,
    route_extra: &str,
    more: &str,
) -> fauxfield_model::Server {
    parse_server(&format!(
        r#"
id = "srv_d"
name = "Data"
port = {port}
[[datasets]]
name = "people"
seed = "data/people.json"
{ds_extra}
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

fn crud_engine(
    rt: &tokio::runtime::Runtime,
    dir: &tempfile::TempDir,
    ds_extra: &str,
    route_extra: &str,
    more: &str,
) -> Engine {
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(dir.path().join("data/people.json"), PEOPLE).unwrap();
    let e = engine(rt, dir.path());
    e.apply(vec![crud_server(free_port(), ds_extra, route_extra, more)]);
    e.start("srv_d").unwrap();
    e
}

fn call(
    rt: &tokio::runtime::Runtime,
    e: &Engine,
    method: &str,
    path: &str,
    body: &str,
) -> (u16, Vec<(String, String)>, serde_json::Value) {
    let mut r = req(method, path);
    r.body = body.into();
    if !body.is_empty() {
        r.headers
            .push(("content-type".into(), "application/json".into()));
    }
    let res = rt.block_on(e.send_test("srv_d", &r)).unwrap();
    let v = serde_json::from_str(&res.body).unwrap_or(serde_json::Value::Null);
    (res.status, res.headers, v)
}

fn ids(v: &serde_json::Value) -> Vec<i64> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|i| i["id"].as_i64().unwrap())
        .collect()
}

fn header<'a>(h: &'a [(String, String)], k: &str) -> Option<&'a str> {
    h.iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(k))
        .map(|(_, v)| v.as_str())
}

#[test]
fn crud_list_filters_sort_search_and_paging() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = crud_engine(&rt, &dir, "", "", "");
    let list = |q: &str| call(&rt, &e, "GET", &format!("/people{q}"), "");
    let (s, h, v) = list("");
    assert_eq!(s, 200);
    assert_eq!(ids(&v), [1, 2, 3, 4, 5]);
    assert_eq!(header(&h, "x-total-count"), Some("5"));
    assert!(header(&h, "link").is_none());
    assert_eq!(ids(&list("?role=admin").2), [1, 5]);
    assert_eq!(ids(&list("?age_gte=30").2), [1, 3, 5]);
    assert_eq!(ids(&list("?age_lte=25").2), [2, 4]);
    assert_eq!(ids(&list("?role_ne=user").2), [1, 5]);
    assert_eq!(ids(&list("?name_like=an").2), [1, 3]);
    assert_eq!(ids(&list("?city=Oslo&role=user").2), [3]);
    assert_eq!(ids(&list("?q=rome").2), [2, 5]);
    assert_eq!(ids(&list("?_sort=age&_order=desc").2), [3, 1, 5, 2, 4]);
    assert_eq!(
        ids(&list("?_sort=age,name&_order=asc,desc").2),
        [4, 2, 5, 1, 3]
    );
    let (_, h, v) = list("?_page=2&_limit=2");
    assert_eq!(ids(&v), [3, 4]);
    assert_eq!(header(&h, "x-total-count"), Some("5"));
    let link = header(&h, "link").unwrap();
    for rel in ["first", "prev", "next", "last"] {
        assert!(link.contains(&format!("rel=\"{rel}\"")), "{link}");
    }
    assert!(link.contains("_page=3"));
    // filters narrow the total
    assert_eq!(
        header(&list("?role=admin&_limit=1").1, "x-total-count"),
        Some("2")
    );
}

#[test]
fn crud_page_size_variable_is_the_default_limit() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = crud_engine(&rt, &dir, "", "", "");
    let mut s = crud_server(free_port(), "", "", "");
    s.variables
        .insert("page_size".into(), toml::Value::Integer(2));
    e.apply(vec![s]);
    e.start("srv_d").unwrap();
    let (_, _, v) = call(&rt, &e, "GET", "/people", "");
    assert_eq!(ids(&v), [1, 2]);
}

#[test]
fn crud_item_create_replace_patch_delete() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = crud_engine(&rt, &dir, "id_strategy = \"increment\"", "", "");
    let (s, _, v) = call(&rt, &e, "GET", "/people/2", "");
    assert_eq!((s, v["name"].as_str()), (200, Some("Bob")));
    assert_eq!(call(&rt, &e, "GET", "/people/99", "").0, 404);
    let (s, h, v) = call(&rt, &e, "POST", "/people", r#"{"name":"Eve"}"#);
    assert_eq!((s, v["id"].as_i64()), (201, Some(6)));
    assert_eq!(header(&h, "location"), Some("/people/6"));
    assert_eq!(
        call(&rt, &e, "POST", "/people", r#"{"id":6,"name":"X"}"#).0,
        409
    );
    assert_eq!(call(&rt, &e, "POST", "/people", "[1]").0, 400);
    let (s, _, v) = call(&rt, &e, "PUT", "/people/6", r#"{"name":"Eva"}"#);
    assert_eq!(
        (s, v["id"].as_i64(), v["name"].as_str()),
        (200, Some(6), Some("Eva"))
    );
    // merge patch: replace, add, remove via null, nested merge
    let (s, _, v) = call(
        &rt,
        &e,
        "PATCH",
        "/people/1",
        r#"{"age":31,"city":null,"meta":{"a":1}}"#,
    );
    assert_eq!(s, 200);
    assert_eq!(v["age"], 31);
    assert!(v.get("city").is_none());
    assert_eq!(v["name"], "Ann");
    let (_, _, v) = call(&rt, &e, "PATCH", "/people/1", r#"{"meta":{"b":2}}"#);
    assert_eq!(v["meta"], serde_json::json!({"a":1,"b":2}));
    assert_eq!(call(&rt, &e, "PATCH", "/people/77", "{}").0, 404);
    assert_eq!(call(&rt, &e, "DELETE", "/people/6", "").0, 204);
    assert_eq!(call(&rt, &e, "DELETE", "/people/6", "").0, 404);
    assert_eq!(
        call(&rt, &e, "GET", "/people", "")
            .2
            .as_array()
            .unwrap()
            .len(),
        5
    );
}

#[test]
fn crud_uuid_and_ulid_ids_and_a_custom_key() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = crud_engine(&rt, &dir, "id_strategy = \"ulid\"\nkey = \"pid\"", "", "");
    let (_, _, v) = call(&rt, &e, "POST", "/people", r#"{"name":"Z"}"#);
    assert_eq!(v["pid"].as_str().unwrap().len(), 26);
    let id = v["pid"].as_str().unwrap().to_string();
    assert_eq!(call(&rt, &e, "GET", &format!("/people/{id}"), "").0, 200);
    let dir2 = tempfile::tempdir().unwrap();
    let e2 = crud_engine(&rt, &dir2, "", "", "");
    let (_, _, v) = call(&rt, &e2, "POST", "/people", r#"{"name":"Z"}"#);
    assert_eq!(v["id"].as_str().unwrap().len(), 36);
}

#[test]
fn crud_validation_returns_422_problem_json() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("schemas")).unwrap();
    std::fs::write(
        dir.path().join("schemas/p.json"),
        r#"{"type":"object","required":["name"],"properties":{"age":{"type":"integer"}}}"#,
    )
    .unwrap();
    let e = crud_engine(&rt, &dir, "", "validate = \"schemas/p.json\"", "");
    let (s, h, v) = call(&rt, &e, "POST", "/people", r#"{"age":"x"}"#);
    assert_eq!(s, 422);
    assert_eq!(header(&h, "content-type"), Some("application/problem+json"));
    assert!(!v["errors"].as_array().unwrap().is_empty());
    assert_eq!(call(&rt, &e, "PUT", "/people/1", r#"{"age":1}"#).0, 422);
    assert_eq!(
        call(&rt, &e, "PATCH", "/people/1", r#"{"age":"no"}"#).0,
        422
    );
    assert_eq!(call(&rt, &e, "POST", "/people", r#"{"name":"ok"}"#).0, 201);
}

#[test]
fn a_higher_priority_http_route_overrides_one_operation() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let more = r#"
[[routes]]
id = "rte_over"
method = "DELETE"
path = "/people/:id"
priority = 5
[[routes.responses]]
id = "rsp_o"
status = 403
body = "no"
"#;
    let e = crud_engine(&rt, &dir, "", "", more);
    assert_eq!(call(&rt, &e, "DELETE", "/people/1", "").0, 403);
    assert_eq!(call(&rt, &e, "GET", "/people/1", "").0, 200);
}

#[test]
fn reset_restores_the_seed_and_persistence_survives_restart() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = crud_engine(&rt, &dir, "persist = true", "", "");
    call(&rt, &e, "DELETE", "/people/1", "");
    call(&rt, &e, "POST", "/people", r#"{"id":9,"name":"N"}"#);
    let state = dir.path().join(".fauxfield/state/srv_d/people.json");
    assert!(state.exists());
    assert_eq!(e.datasets(None).unwrap()[0].rows, 5);
    // a fresh engine on the same workspace reloads the persisted rows
    let e2 = engine(&rt, dir.path());
    e2.apply(vec![crud_server(free_port(), "persist = true", "", "")]);
    assert_eq!(
        ids(&e2.dataset_rows("srv_d", "people").unwrap()),
        [2, 3, 4, 5, 9]
    );
    // hot reload with an unchanged definition keeps live rows
    e2.apply(vec![crud_server(free_port(), "persist = true", "", "")]);
    assert_eq!(e2.datasets(None).unwrap()[0].rows, 5);
    e.reset_state(Some("srv_d")).unwrap();
    assert!(!state.exists());
    assert_eq!(
        ids(&e.dataset_rows("srv_d", "people").unwrap()),
        [1, 2, 3, 4, 5]
    );
    assert!(matches!(
        e.reset_state(Some("nope")),
        Err(EngineError::UnknownServer(_))
    ));
    assert!(matches!(
        e.dataset_reset("srv_d", "zzz"),
        Err(EngineError::UnknownDataset(_))
    ));
}

#[test]
fn seed_template_renders_and_data_is_visible_to_templates() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("files")).unwrap();
    std::fs::write(
        dir.path().join("files/p.json.tera"),
        r#"[{% for i in range(start=1, end=4) %}{"id":{{ i }},"name":"n{{ i }}"}{% if not loop.last %},{% endif %}{% endfor %}]"#,
    )
    .unwrap();
    let e = engine(&rt, dir.path());
    let s = parse_server(&format!(
        r#"
id = "srv_d"
name = "Data"
port = {}
[[datasets]]
name = "people"
seed_template = "files/p.json.tera"
[[routes]]
id = "rte_c"
kind = "crud"
path = "/people"
dataset = "people"
[[routes]]
id = "rte_n"
path = "/count"
[[routes.responses]]
id = "rsp_n"
body = "{{{{ data.people | length }}}}"
"#,
        free_port()
    ))
    .unwrap();
    e.apply(vec![s]);
    e.start("srv_d").unwrap();
    let r = rt
        .block_on(e.send_test("srv_d", &req("GET", "/count")))
        .unwrap();
    assert_eq!(r.body, "3");
    call(&rt, &e, "DELETE", "/people/1", "");
    let r = rt
        .block_on(e.send_test("srv_d", &req("GET", "/count")))
        .unwrap();
    assert_eq!(r.body, "2");
}

#[test]
fn dataset_events_fire_on_mutation() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = crud_engine(&rt, &dir, "", "", "");
    let mut rx = e.subscribe();
    call(&rt, &e, "DELETE", "/people/2", "");
    let mut seen = None;
    while let Ok(ev) = rx.try_recv() {
        if let EngineEvent::Dataset { name, rows, .. } = ev {
            seen = Some((name, rows));
        }
    }
    assert_eq!(seen, Some(("people".into(), 4)));
}

#[test]
fn dataset_row_apis_edit_insert_delete() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = crud_engine(&rt, &dir, "", "", "");
    let before = e
        .dataset_rows("srv_d", "people")
        .unwrap()
        .as_array()
        .unwrap()
        .len();
    e.dataset_update_cell("srv_d", "people", 0, "name", serde_json::json!("Zed"))
        .unwrap();
    let at = e
        .dataset_insert("srv_d", "people", None, vec![serde_json::json!({"id": 99})])
        .unwrap();
    assert_eq!(at, before);
    let rows = e.dataset_rows("srv_d", "people").unwrap();
    assert_eq!(rows[0]["name"], "Zed");
    assert_eq!(rows.as_array().unwrap().len(), before + 1);
    assert_eq!(
        e.dataset_delete("srv_d", "people", &[0, 0, 500]).unwrap(),
        1
    );
    assert_eq!(
        e.dataset_rows("srv_d", "people")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        before
    );
    assert!(
        e.dataset_update_cell("srv_d", "people", 9999, "x", serde_json::json!(1))
            .is_err()
    );
}

#[test]
fn dataset_column_constraints_are_enforced_and_rows_duplicate() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let cols = r#"
[[datasets.columns]]
name = "id"
type = "int"
primary_key = true
[[datasets.columns]]
name = "name"
type = "string"
required = true
unique = true
[[datasets.columns]]
name = "age"
type = "int"
default = 18
"#;
    let e = crud_engine(&rt, &dir, cols, "", "");
    // Type, required and unique on cell edits.
    assert!(
        e.dataset_update_cell("srv_d", "people", 0, "age", serde_json::json!("x"))
            .is_err()
    );
    assert!(
        e.dataset_update_cell("srv_d", "people", 0, "name", serde_json::json!(""))
            .is_err()
    );
    assert!(
        e.dataset_update_cell("srv_d", "people", 0, "name", serde_json::json!("Bob"))
            .is_err()
    );
    // Insert applies the default and rejects a taken unique value.
    let at = e
        .dataset_insert("srv_d", "people", None, vec![serde_json::json!({"id": 50})])
        .unwrap();
    assert_eq!(e.dataset_rows("srv_d", "people").unwrap()[at]["age"], 18);
    assert!(
        e.dataset_insert(
            "srv_d",
            "people",
            None,
            vec![serde_json::json!({"id": 51, "name": "Ann"})]
        )
        .is_err()
    );
    // HTTP writes answer 422.
    let (st, _, _) = call(&rt, &e, "POST", "/people", r#"{"name":"Ann"}"#);
    assert_eq!(st, 422);
    // Duplicate gives fresh key and unique values.
    let new = e.dataset_duplicate("srv_d", "people", &[0]).unwrap();
    let rows = e.dataset_rows("srv_d", "people").unwrap();
    assert_ne!(rows[new[0]]["id"], rows[0]["id"]);
    assert_eq!(rows[new[0]]["name"], "Ann-copy");
    e.dataset_clear("srv_d", "people").unwrap();
    assert_eq!(
        e.dataset_rows("srv_d", "people")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        0
    );
}
