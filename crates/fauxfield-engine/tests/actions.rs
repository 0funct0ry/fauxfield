//! M10.06: Dataset reads, actions, rules and singleton PUT/PATCH on non-CRUD Routes.

use std::net::TcpListener;

use fauxfield_engine::{Engine, EngineConfig, TestRequest};
use fauxfield_model::parse_server;
use serde_json::{Value, json};

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

const SERVER: &str = r#"
id = "srv_a"
name = "Shop"
port = PORT
[[datasets]]
name = "orders"
id_strategy = "increment"
[[datasets]]
name = "stock"
seed = "data/stock.json"
key = "sku"
[[datasets]]
name = "settings"
seed = "data/settings.json"

[[routes]]
id = "rte_settings"
kind = "crud"
path = "/settings"
dataset = "settings"

[[routes]]
id = "rte_checkout"
method = "POST"
path = "/checkout"
[[routes.responses]]
id = "rsp_ok"
status = 201
action_error_status = 422
body = '{{ action.0 | json_encode() | safe }}'
[[routes.responses.actions]]
dataset = "orders"
op = "insert"
[routes.responses.actions.fields]
sku = "{{ request.json.sku }}"
qty = "{{ request.json.qty }}"
[[routes.responses.actions]]
dataset = "stock"
op = "increment"
key = "{{ request.json.sku }}"
field = "qty"
by = "-{{ request.json.qty }}"
fail_if = "{{ result.qty < 0 }}"

[[routes]]
id = "rte_latest"
method = "GET"
path = "/orders/latest"
[[routes.responses]]
id = "rsp_latest"
body = '{{ data.orders | last | json_encode() | safe }}'

[[routes]]
id = "rte_gate"
method = "GET"
path = "/gate"
[[routes.responses]]
id = "rsp_sold"
body = "soldout"
[[routes.responses.rules]]
target = "dataset"
key = "stock.$[0].qty"
op = "lt"
value = 1
[[routes.responses]]
id = "rsp_busy"
body = "busy"
[[routes.responses.rules]]
target = "dataset_count"
key = "orders?qty_gte=2"
op = "gte"
value = 2
[[routes.responses]]
id = "rsp_default"
default = true
body = "open"

[[routes]]
id = "rte_find"
method = "GET"
path = "/orders/:id"
[[routes.responses]]
id = "rsp_missing"
status = 404
body = '{"error":"none"}'
[[routes.responses.rules]]
target = "dataset_count"
key = "orders?id={{ request.params.id }}"
op = "eq"
value = 0
[[routes.responses]]
id = "rsp_found"
default = true
body = '{{ dataset_rows(data=data.orders, filter="id=" ~ request.params.id) | first | json_encode() | safe }}'

[[routes]]
id = "rte_big"
method = "GET"
path = "/big"
[[routes.responses]]
id = "rsp_big"
body = '{{ dataset_rows(data=data.orders, filter="qty_gte=2", sort="qty", order="desc", limit=1) | json_encode() | safe }}|{{ dataset_count(data=data.orders, filter="sku=w") }}'

[[routes]]
id = "rte_wipe"
method = "DELETE"
path = "/orders"
[[routes.responses]]
id = "rsp_wipe"
status = 200
body = '{{ action.0.count }}'
[[routes.responses.actions]]
dataset = "orders"
op = "delete_where"
[routes.responses.actions.filter]
qty_gte = "{{ request.query.min }}"

[[routes]]
id = "rte_set"
method = "POST"
path = "/flag"
[[routes.responses]]
id = "rsp_set"
body = '{{ action.0 | json_encode() | safe }}'
[[routes.responses.actions]]
dataset = "settings"
op = "set"
field = "theme"
value = "dark"
"#;

fn setup(rt: &tokio::runtime::Runtime, dir: &tempfile::TempDir) -> Engine {
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::write(
        dir.path().join("data/stock.json"),
        r#"[{"sku":"w","qty":3}]"#,
    )
    .unwrap();
    std::fs::write(
        dir.path().join("data/settings.json"),
        r#"{"theme":"light","lang":"en"}"#,
    )
    .unwrap();
    let e = Engine::new(
        rt.handle().clone(),
        EngineConfig {
            root: dir.path().to_path_buf(),
            ..Default::default()
        },
    );
    let s = parse_server(&SERVER.replace("PORT", &free_port().to_string())).unwrap();
    e.apply(vec![s]);
    e.start("srv_a").unwrap();
    e
}

fn call(
    rt: &tokio::runtime::Runtime,
    e: &Engine,
    method: &str,
    path: &str,
    body: Value,
) -> (u16, String) {
    let mut r = TestRequest {
        method: method.into(),
        path: path.into(),
        ..Default::default()
    };
    if !body.is_null() {
        r.body = body.to_string();
        r.headers
            .push(("content-type".into(), "application/json".into()));
    }
    let res = rt.block_on(e.send_test("srv_a", &r)).unwrap();
    (res.status, res.body)
}

fn qty(rt: &tokio::runtime::Runtime, e: &Engine) -> i64 {
    let _ = rt;
    e.dataset_rows("srv_a", "stock").unwrap()[0]["qty"]
        .as_i64()
        .unwrap()
}

#[test]
fn checkout_inserts_decrements_and_rolls_back() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = setup(&rt, &dir);
    let (s, b) = call(&rt, &e, "POST", "/checkout", json!({"sku":"w","qty":2}));
    assert_eq!(s, 201, "{b}");
    let order: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(order["sku"], "w");
    assert_eq!(order["id"], 1);
    assert_eq!(qty(&rt, &e), 1);
    // Would go negative: nothing applies, error status.
    let (s, b) = call(&rt, &e, "POST", "/checkout", json!({"sku":"w","qty":5}));
    assert_eq!(s, 422, "{b}");
    assert_eq!(qty(&rt, &e), 1);
    assert_eq!(
        e.dataset_rows("srv_a", "orders")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // GET /orders/latest through the Dataset read.
    let (s, b) = call(&rt, &e, "GET", "/orders/latest", Value::Null);
    assert_eq!(s, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&b).unwrap()["qty"],
        "2".parse::<Value>().unwrap()
    );
    // Recorded changes.
    let rec = e.traffic(&Default::default());
    let with = rec.iter().find(|r| !r.dataset_changes.is_empty()).unwrap();
    assert_eq!(with.dataset_changes.len(), 2);
}

#[test]
fn rules_read_datasets_and_delete_where_and_set() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = setup(&rt, &dir);
    assert_eq!(call(&rt, &e, "GET", "/gate", Value::Null).1, "open");
    call(&rt, &e, "POST", "/checkout", json!({"sku":"w","qty":2}));
    call(&rt, &e, "POST", "/checkout", json!({"sku":"w","qty":1}));
    // 2 orders; one has qty>=2, stock now 0 -> soldout first.
    assert_eq!(call(&rt, &e, "GET", "/gate", Value::Null).1, "soldout");
    let (_, b) = call(&rt, &e, "DELETE", "/orders?min=2", Value::Null);
    assert_eq!(b, "1");
    assert_eq!(
        e.dataset_rows("srv_a", "orders")
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    // singleton set + PUT + PATCH
    let (_, b) = call(&rt, &e, "POST", "/flag", Value::Null);
    assert_eq!(serde_json::from_str::<Value>(&b).unwrap()["theme"], "dark");
    let (s, b) = call(&rt, &e, "PATCH", "/settings", json!({"lang":"fr"}));
    assert_eq!(s, 200, "{b}");
    let v: Value = serde_json::from_str(&b).unwrap();
    assert_eq!(
        (v["theme"].as_str(), v["lang"].as_str()),
        (Some("dark"), Some("fr"))
    );
    let (s, b) = call(&rt, &e, "PUT", "/settings", json!({"only":1}));
    assert_eq!(s, 200);
    assert_eq!(
        serde_json::from_str::<Value>(&b).unwrap(),
        json!({"only":1})
    );
}

#[test]
fn dataset_reads_filter_sort_page_count_and_no_match_status() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let e = setup(&rt, &dir);
    call(&rt, &e, "POST", "/checkout", json!({"sku":"w","qty":1}));
    call(&rt, &e, "POST", "/checkout", json!({"sku":"w","qty":2}));
    let (s, b) = call(&rt, &e, "GET", "/orders/2", Value::Null);
    assert_eq!((s, b.contains("\"qty\":2")), (200, true), "{b}");
    assert_eq!(call(&rt, &e, "GET", "/orders/99", Value::Null).0, 404);
    let (_, b) = call(&rt, &e, "GET", "/big", Value::Null);
    let (rows, n) = b.split_once('|').unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(rows)
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(n, "2");
}
