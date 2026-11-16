use fauxfield_model::parse_server;
use fauxfield_openapi::{Options, ServerSnapshot, generate};
use serde_json::{Value, json};
use std::collections::BTreeMap;

const FIXTURE: &str = r#"
id = "srv_a"
name = "Shop"
port = 3000
prefix = "/api/v1"
docs = "Shop API"
[docs_site]
enabled = true
[[datasets]]
name = "users"
id_strategy = "uuid"
[[datasets.columns]]
name = "id"
type = "string"
primary_key = true
[[datasets.columns]]
name = "age"
type = "int"
required = true
[[datasets]]
name = "notes"
[[folders]]
id = "fld_1"
name = "Accounts"
docs = "Account routes"
[[folders]]
id = "fld_off"
name = "Off"
enabled = false
[[routes]]
id = "rte_hello"
name = "Hello"
folder = "fld_1"
method = "GET"
path = "/hello/:name"
[[routes.responses]]
id = "rsp_1"
status = 200
content_type = "application/json"
body = '{"msg":"hi","n":1}'
[[routes.responses.rules]]
target = "query"
key = "lang"
op = "eq"
value = "fr"
[[routes.responses]]
id = "rsp_2"
status = 404
body = "nope"
[[routes]]
id = "rte_any"
method = "ANY"
path = "/any"
[[routes.responses]]
id = "rsp_a"
[[routes]]
id = "rte_users"
kind = "crud"
path = "/users"
dataset = "users"
folder = "fld_1"
crud_ops = ["list", "get", "create", "delete"]
[[routes]]
id = "rte_notes"
kind = "crud"
path = "/notes"
dataset = "notes"
[[routes]]
id = "rte_hidden"
path = "/secret"
[routes.openapi]
hidden = true
[[routes]]
id = "rte_regex"
match = "regex"
path = "^/r/\\d+$"
[[routes]]
id = "rte_regex2"
match = "regex"
path = "^/q/\\d+$"
[routes.openapi]
path = "/q/{n}"
[[routes]]
id = "rte_off"
path = "/off"
response_mode = "disabled"
[[routes]]
id = "rte_in_off"
folder = "fld_off"
path = "/inoff"
[[routes]]
id = "rte_static"
kind = "static"
path = "/files"
dir = "public"
[[routes]]
id = "rte_ws"
kind = "websocket"
path = "/ws"
[[routes]]
id = "rte_shadow"
path = "/hello/:name"
[[routes.responses]]
id = "rsp_s"
"#;

fn doc() -> Value {
    let s = parse_server(FIXTURE).unwrap();
    let mut seeds = BTreeMap::new();
    seeds.insert(
        "notes".to_string(),
        vec![
            json!({"id": 1, "text": "a", "tag": "x"}),
            json!({"id": 2, "text": 5}),
        ],
    );
    let snap = ServerSnapshot {
        server: &s,
        root: None,
        seeds,
    };
    generate(&snap, &Options::default()).value
}

#[test]
fn golden() {
    insta::assert_json_snapshot!(doc());
}

#[test]
fn deterministic() {
    assert_eq!(doc().to_string(), doc().to_string());
}

#[test]
fn shape() {
    let d = doc();
    assert_eq!(d["openapi"], "3.1.0");
    assert_eq!(d["servers"][0]["url"], "/api/v1");
    let p = &d["paths"];
    assert!(p["/hello/{name}"]["get"].is_object());
    assert_eq!(p["/hello/{name}"]["get"]["operationId"], "rte_hello"); // shadow route skipped
    for m in ["get", "post", "put", "patch", "delete"] {
        assert_eq!(p["/any"][m]["x-fauxfield-any"], true, "{m}");
    }
    assert!(p["/users"]["get"].is_object() && p["/users"]["post"].is_object());
    assert!(p["/users/{id}"]["delete"].is_object() && p["/users/{id}"]["put"].is_null());
    assert!(p["/secret"].is_null() && p["/off"].is_null() && p["/inoff"].is_null());
    assert!(p["/q/{n}"]["get"].is_object());
    assert!(p["/files/{path}"]["get"].is_object());
    assert_eq!(p["/ws"]["get"]["x-fauxfield-websocket"], true);
    assert!(d["components"]["schemas"]["Users"]["properties"]["age"]["type"] == "integer");
    // mixed types from seeds
    assert!(d["components"]["schemas"]["Notes"]["properties"]["text"]["oneOf"].is_array());
    assert!(
        p["/hello/{name}"]["get"]["parameters"]
            .as_array()
            .unwrap()
            .iter()
            .any(|x| x["name"] == "lang" && x["in"] == "query")
    );
}

#[test]
fn assets_decompress() {
    use fauxfield_openapi::ui;
    let b = ui::asset("swagger-ui-bundle.js").unwrap().plain_bytes();
    assert!(b.len() > 1_000_000 && b.starts_with(b"/*! For license"));
    assert!(ui::asset("swagger-ui.css").unwrap().plain_bytes().len() > 100_000);
    assert!(
        ui::asset("favicon.png")
            .unwrap()
            .plain_bytes()
            .starts_with(b"\x89PNG")
    );
    assert!(
        ui::asset("init.js")
            .unwrap()
            .plain_bytes()
            .starts_with(b"window.onload")
    );
    assert!(ui::asset("nope").is_none());
    assert!(ui::page("a<b").contains("a&lt;b"));
}

/// Generation for 1,000 Routes (SPEC §15 budget: 50 ms in release; this guard is loose because
/// tests run unoptimised). A criterion bench is still to do.
#[test]
fn thousand_routes_generate_quickly() {
    let mut t = String::from("id = \"srv_b\"\nname = \"Big\"\nport = 3001\n");
    for i in 0..1000 {
        t += &format!(
            "[[routes]]\nid = \"rte_{i}\"\npath = \"/r{i}/:id\"\n[[routes.responses]]\nid = \"rsp_{i}\"\nbody = '{{\"n\":{i}}}'\n"
        );
    }
    let s = parse_server(&t).unwrap();
    let snap = ServerSnapshot {
        server: &s,
        root: None,
        seeds: BTreeMap::new(),
    };
    let start = std::time::Instant::now();
    let d = generate(&snap, &Options::default());
    assert_eq!(d.stats.documented, 1000);
    assert!(
        start.elapsed() < std::time::Duration::from_secs(2),
        "{:?}",
        start.elapsed()
    );
}

#[test]
fn column_defaults_do_not_become_filter_defaults() {
    let s = parse_server(
        r#"
id = "srv_p"
name = "P"
port = 1
[[datasets]]
name = "products"
[[datasets.columns]]
name = "id"
type = "int"
[[datasets.columns]]
name = "price"
type = "float"
default = 0
[[datasets.columns]]
name = "in_stock"
type = "bool"
default = true
[[routes]]
id = "rte_p"
kind = "crud"
path = "/products"
dataset = "products"
"#,
    )
    .unwrap();
    let snap = ServerSnapshot {
        server: &s,
        root: None,
        seeds: BTreeMap::new(),
    };
    let v = generate(&snap, &Options::default()).value;
    let params = v["paths"]["/products"]["get"]["parameters"]
        .as_array()
        .unwrap();
    for name in ["price", "in_stock"] {
        let p = params.iter().find(|p| p["name"] == name).expect(name);
        assert!(p["schema"].get("default").is_none(), "{p}");
    }
    // The item schema still documents the defaults.
    let item = &v["components"]["schemas"]["Products"]["properties"];
    assert!(item.get("price").is_some(), "{item}");
}

#[test]
fn operation_examples_render_templates_or_are_left_out() {
    let s = parse_server(
        r#"
id = "srv_p"
name = "P"
port = 1
[[datasets]]
name = "products"
[[routes]]
id = "rte_p"
kind = "crud"
path = "/products"
dataset = "products"
[[routes.operations.list.responses]]
id = "rsp_t"
name = "Template"
body = '{"total": {{ 1 + 1 }}}'
[[routes.operations.list.responses]]
id = "rsp_s"
name = "Static"
body = '{"ok": true}'
"#,
    )
    .unwrap();
    let snap = ServerSnapshot {
        server: &s,
        root: None,
        seeds: BTreeMap::new(),
    };
    // Without a renderer the Tera body is left out; the static one stays.
    let v = generate(&snap, &Options::default()).value;
    let ex = &v["paths"]["/products"]["get"]["responses"]["200"]["content"]["application/json"]["examples"];
    assert!(ex.get("Template").is_none(), "{ex}");
    assert_eq!(ex["Static"]["value"]["ok"], true);
    // With one, it is rendered.
    fn render(t: &str) -> Result<String, String> {
        Ok(t.replace("{{ 1 + 1 }}", "2"))
    }
    let opts = Options {
        renderer: Some(&render),
    };
    let v = generate(&snap, &opts).value;
    let ex = &v["paths"]["/products"]["get"]["responses"]["200"]["content"]["application/json"]["examples"];
    assert_eq!(ex["Template"]["value"]["total"], 2, "{ex}");
}
