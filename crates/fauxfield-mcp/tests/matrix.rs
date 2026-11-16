//! One scripted call list over every class combination (read; read,write; read,write,execute):
//! each tool is listed and runs when its class is allowed, and is unlisted and refused with a
//! protocol error otherwise. A coverage test fails when a registry tool has no call in the
//! script (M16). Other transports have their own end-to-end tests (`tests/http.rs`,
//! `crates/fauxfield/tests/mcp.rs` for stdio and the unix socket).

use std::{collections::BTreeSet, sync::Arc};

use fauxfield_mcp::{
    AllowedClasses, FauxfieldServer, Registry, ToolClass,
    control_tools::register_control_tools,
    workspace_tools::{fixed_store, register_workspace_tools},
};
use rmcp::{ServiceExt, model::CallToolRequestParams};
use serde_json::{Value, json};

/// Every tool with arguments that exercise it. Tools may fail (no host, bad id); they must never
/// fail with a protocol error while their class is allowed.
fn script() -> Vec<(&'static str, Value)> {
    let s = json!({ "server": "example-api" });
    vec![
        ("ping", json!({})),
        ("server_info", json!({})),
        ("workspace_info", json!({})),
        ("validate", json!({})),
        ("list_servers", json!({})),
        ("get_server", s.clone()),
        ("get_openapi", s.clone()),
        ("list_routes", s.clone()),
        ("get_route", json!({ "route": "rte_none" })),
        (
            "match_path",
            json!({ "server": "example-api", "method": "GET", "path": "/health" }),
        ),
        ("render_template", json!({ "template": "{{ 1 + 1 }}" })),
        (
            "preview_generator",
            json!({ "generator": { "kind": "int", "min": 1, "max": 3 } }),
        ),
        ("list_static_files", s.clone()),
        (
            "dataset_references",
            json!({ "server": "example-api", "name": "none" }),
        ),
        ("certs_status", json!({})),
        ("certs_trust_command", json!({})),
        ("get_traffic", json!({ "limit": 5 })),
        ("list_ws_connections", s.clone()),
        ("create_server", json!({ "name": "Matrix", "port": 4999 })),
        (
            "update_server",
            json!({ "server": "example-api", "name": "Example API" }),
        ),
        ("delete_server", json!({ "server": "matrix" })),
        ("update_workspace", json!({ "name": "matrix" })),
        (
            "upsert_route",
            json!({ "server": "example-api", "method": "GET", "path": "/m", "responses": [{ "status": 200, "body": "m" }], "operations": { "create": [{ "status": 409 }] } }),
        ),
        ("delete_route", json!({ "route": "rte_none" })),
        (
            "set_variable",
            json!({ "key": "k", "value": "v", "scope": "global" }),
        ),
        (
            "upsert_dataset",
            json!({ "server": "example-api", "name": "things", "seed": [{ "id": 1 }] }),
        ),
        (
            "delete_dataset",
            json!({ "server": "example-api", "name": "things" }),
        ),
        (
            "import_dataset",
            json!({ "server": "example-api", "name": "x", "file": "data/none.csv" }),
        ),
        (
            "bake_dataset",
            json!({ "server": "example-api", "name": "none" }),
        ),
        ("start_server", s.clone()),
        ("stop_server", s.clone()),
        ("restart_server", s.clone()),
        (
            "send_test_request",
            json!({ "server": "example-api", "method": "GET", "path": "/health" }),
        ),
        ("reset_state", s.clone()),
        (
            "clear_dataset",
            json!({ "server": "example-api", "name": "none" }),
        ),
        ("set_network_profile", json!({})),
        ("set_chaos", json!({ "rate": 0.0 })),
        (
            "send_ws_message",
            json!({ "server": "example-api", "message": "x" }),
        ),
        (
            "disconnect_ws_client",
            json!({ "server": "example-api", "connection": "c" }),
        ),
        (
            "refire_callback",
            json!({ "server": "example-api", "recording": 1 }),
        ),
    ]
}

fn registry(dir: &std::path::Path) -> Registry {
    let store = fauxfield_store::Store::open(dir).unwrap();
    let mut r = fauxfield_mcp::build_registry(&Default::default());
    register_workspace_tools(&mut r, fixed_store(store));
    register_control_tools(&mut r, Arc::new(|_| Ok(None)));
    r
}

#[test]
fn every_registry_tool_has_a_call_in_the_script() {
    let dir = tempfile::tempdir().unwrap();
    fauxfield_store::init_workspace(dir.path()).unwrap();
    let r = registry(dir.path());
    let called: BTreeSet<&str> = script().iter().map(|(n, _)| *n).collect();
    let missing: Vec<&str> = r
        .all()
        .iter()
        .map(|t| t.name)
        .filter(|n| !called.contains(n))
        .collect();
    assert!(
        missing.is_empty(),
        "tools with no call in the matrix: {missing:?}"
    );
    let known: BTreeSet<&str> = r.all().iter().map(|t| t.name).collect();
    let stale: Vec<&&str> = called.iter().filter(|n| !known.contains(*n)).collect();
    assert!(
        stale.is_empty(),
        "script calls tools that do not exist: {stale:?}"
    );
}

async fn run_combo(allowed: AllowedClasses, label: &str) {
    let dir = tempfile::tempdir().unwrap();
    fauxfield_store::init_workspace(dir.path()).unwrap();
    let reg = registry(dir.path());
    let classes: std::collections::BTreeMap<&str, ToolClass> =
        reg.all().iter().map(|t| (t.name, t.class)).collect();
    let (s, c) = tokio::io::duplex(1 << 20);
    let server = FauxfieldServer::new(Arc::new(reg), allowed);
    tokio::spawn(async move {
        if let Ok(r) = server.serve(s).await {
            let _ = r.waiting().await;
        }
    });
    let client = ().serve(c).await.unwrap();

    // Listing: exactly the allowed classes.
    let listed: BTreeSet<String> = client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    for (name, class) in &classes {
        assert_eq!(
            listed.contains(*name),
            allowed.contains(*class),
            "[{label}] listing of `{name}` ({class})"
        );
    }

    for (name, args) in script() {
        let class = classes[name];
        let mut p = CallToolRequestParams::new(name.to_owned());
        if let Value::Object(m) = args {
            p = p.with_arguments(m);
        }
        let res = client.call_tool(p).await;
        if allowed.contains(class) {
            // A tool error (no host, unknown id, ...) is fine; a protocol error is not.
            assert!(
                res.is_ok(),
                "[{label}] `{name}` was refused: {:?}",
                res.err()
            );
        } else {
            assert!(
                res.is_err(),
                "[{label}] `{name}` ran although `{class}` is not allowed"
            );
        }
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn read_only() {
    run_combo(AllowedClasses::read_only(), "read").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn read_write() {
    let a = AllowedClasses::parse_list("read,write").unwrap();
    run_combo(a, "read,write").await;
}

#[tokio::test(flavor = "multi_thread")]
async fn read_write_execute() {
    run_combo(AllowedClasses::all(), "read,write,execute").await;
}

/// The registry equals the checked-in `tools.expected.json` (name and class per tool, with the
/// milestone that introduced it from SPEC §11.1). Adding or reclassifying a tool means editing
/// that file (`UPDATE_EXPECTED=1 cargo test -p fauxfield-mcp registry_matches_expected` rewrites
/// names and classes and keeps the milestones).
#[test]
fn registry_matches_expected() {
    let dir = tempfile::tempdir().unwrap();
    fauxfield_store::init_workspace(dir.path()).unwrap();
    let r = registry(dir.path());
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tools.expected.json");
    let mut actual: Vec<(String, String)> = r
        .all()
        .iter()
        .map(|t| (t.name.to_string(), t.class.to_string()))
        .collect();
    actual.sort();
    let existing: Vec<Value> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    if std::env::var_os("UPDATE_EXPECTED").is_some() {
        let ms = |n: &str| {
            existing
                .iter()
                .find(|e| e["name"] == n)
                .and_then(|e| e["milestone"].as_str())
                .unwrap_or("?")
                .to_owned()
        };
        let v: Vec<Value> = actual
            .iter()
            .map(|(n, c)| json!({ "name": n, "class": c, "milestone": ms(n) }))
            .collect();
        std::fs::write(&path, serde_json::to_string_pretty(&v).unwrap() + "\n").unwrap();
    }
    let expected: Vec<(String, String)> = {
        let mut v: Vec<(String, String)> = serde_json::from_str::<Vec<Value>>(
            &std::fs::read_to_string(&path).expect("tests/tools.expected.json"),
        )
        .unwrap()
        .iter()
        .map(|e| {
            (
                e["name"].as_str().unwrap().into(),
                e["class"].as_str().unwrap().into(),
            )
        })
        .collect();
        v.sort();
        v
    };
    assert_eq!(
        actual, expected,
        "registry differs from tests/tools.expected.json"
    );
    // Tools of deferred milestones are absent, never listed-then-failing.
    for absent in [
        "create_routes_from_traffic",
        "import_openapi",
        "export_openapi",
    ] {
        assert!(
            r.tool(absent).is_none(),
            "{absent} must wait for its milestone"
        );
    }
}

/// M17.1: `upsert_route` writes operation Responses on a crud Route and `get_route` returns them.
#[tokio::test(flavor = "multi_thread")]
async fn operations_round_trip_through_upsert_and_get_route() {
    let dir = tempfile::tempdir().unwrap();
    fauxfield_store::init_workspace(dir.path()).unwrap();
    std::fs::write(
        dir.path().join("servers/crud.toml"),
        "schema = 1\nid = \"srv_crud\"\nname = \"Crud\"\nport = 4777\n\n[[datasets]]\nname = \"users\"\nseed = \"[]\"\n\n[[routes]]\nid = \"rte_users\"\nkind = \"crud\"\npath = \"/users\"\ndataset = \"users\"\n",
    )
    .unwrap();
    let reg = registry(dir.path());
    let (s, c) = tokio::io::duplex(1 << 20);
    let server = FauxfieldServer::new(Arc::new(reg), AllowedClasses::all());
    tokio::spawn(async move {
        if let Ok(r) = server.serve(s).await {
            let _ = r.waiting().await;
        }
    });
    let client = ().serve(c).await.unwrap();
    let call = |name: &'static str, args: Value| {
        let client = &client;
        async move {
            let mut p = CallToolRequestParams::new(name.to_owned());
            if let Value::Object(m) = args {
                p = p.with_arguments(m);
            }
            let r = client.call_tool(p).await.unwrap();
            let text = serde_json::to_string(&r.content).unwrap();
            (r.is_error.unwrap_or(false), text)
        }
    };
    let (err, text) = call(
        "upsert_route",
        json!({ "server": "srv_crud", "route": "rte_users", "operations": { "create": [{
            "name": "Duplicate", "status": 409,
            "rules": [{ "target": "body", "key": "$.email", "op": "eq", "value": "a@b.c" }]
        }] } }),
    )
    .await;
    assert!(!err, "{text}");
    let (err, text) = call("get_route", json!({ "route": "rte_users" })).await;
    assert!(!err, "{text}");
    assert!(
        text.contains("operations") && text.contains("Duplicate"),
        "{text}"
    );
    let (err, text) = call(
        "upsert_route",
        json!({ "server": "srv_crud", "route": "rte_users", "operations": { "bogus": [{ "status": 200 }] } }),
    )
    .await;
    assert!(err, "unknown operation should be refused: {text}");
}
