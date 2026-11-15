//! Resources, prompts, redaction and the path jail through a real MCP client (M16).

use std::sync::Arc;

use fauxfield_mcp::{
    AllowedClasses, FauxfieldServer, Registry,
    guard::Redactor,
    workspace_tools::{fixed_store, register_workspace_tools, secret_values},
};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, GetPromptRequestParams, ReadResourceRequestParams},
};
use serde_json::{Value, json};

const SECRET: &str = "s3cr3t-abc123-zzz";

struct Rig {
    client: rmcp::service::RunningService<rmcp::RoleClient, ()>,
    _dir: tempfile::TempDir,
}

async fn rig(allowed: AllowedClasses) -> Rig {
    let dir = tempfile::tempdir().unwrap();
    fauxfield_store::init_workspace(dir.path()).unwrap();
    // A secret-named variable in the manifest.
    let manifest = dir.path().join("fauxfield.toml");
    let text = std::fs::read_to_string(&manifest).unwrap();
    std::fs::write(
        &manifest,
        text.replace(
            "[variables]\n",
            &format!("[variables]\napi_token = \"{SECRET}\"\nsession_id = \"sess-xyz-789\"\n"),
        ),
    )
    .unwrap();
    let store = fauxfield_store::Store::open(dir.path()).unwrap();
    let mut r = Registry::new();
    register_workspace_tools(&mut r, fixed_store(store.clone()));
    let extra = vec!["*session*".to_string()];
    let s2 = store.clone();
    let e2 = extra.clone();
    r.set_redactor(Redactor::new(
        Arc::new(move || secret_values(&s2, &e2)),
        extra,
    ));
    r.set_jail(dir.path().to_path_buf());
    let (s, c) = tokio::io::duplex(1 << 20);
    let server = FauxfieldServer::new(Arc::new(r), allowed);
    tokio::spawn(async move {
        if let Ok(r) = server.serve(s).await {
            let _ = r.waiting().await;
        }
    });
    Rig {
        client: ().serve(c).await.unwrap(),
        _dir: dir,
    }
}

async fn call(c: &Rig, name: &str, args: Value) -> (bool, String) {
    let mut p = CallToolRequestParams::new(name.to_owned());
    if let Value::Object(m) = args {
        p = p.with_arguments(m);
    }
    match c.client.call_tool(p).await {
        Ok(r) => (
            r.is_error.unwrap_or(false),
            serde_json::to_string(&r).unwrap(),
        ),
        Err(e) => (true, e.to_string()),
    }
}

#[tokio::test]
async fn resources_and_prompts_are_served() {
    let c = rig(AllowedClasses::all()).await;
    let uris: Vec<String> = c
        .client
        .list_all_resources()
        .await
        .unwrap()
        .into_iter()
        .map(|r| r.uri)
        .collect();
    assert_eq!(
        uris,
        [
            "fauxfield://schema/server",
            "fauxfield://docs/templating",
            "fauxfield://traffic/recent"
        ]
    );
    let t = c.client.list_all_resource_templates().await.unwrap();
    assert_eq!(t[0].uri_template, "fauxfield://servers/{id}");
    let read = |uri: &str| {
        c.client
            .read_resource(ReadResourceRequestParams::new(uri.to_owned()))
    };
    let schema = serde_json::to_string(&read("fauxfield://schema/server").await.unwrap()).unwrap();
    assert!(schema.contains("$schema") || schema.contains("properties"));
    let doc = serde_json::to_string(&read("fauxfield://docs/templating").await.unwrap()).unwrap();
    assert!(doc.contains("Fake kinds"));
    let srv =
        serde_json::to_string(&read("fauxfield://servers/example-api").await.unwrap()).unwrap();
    assert!(srv.contains("Example API"), "{srv}");
    assert!(read("fauxfield://nope").await.is_err());
    assert!(read("fauxfield://servers/../x").await.is_err());

    let prompts = c.client.list_all_prompts().await.unwrap();
    let names: Vec<_> = prompts.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "mock_from_openapi",
            "mock_from_description",
            "mock_for_frontend_feature"
        ]
    );
    let p = c
        .client
        .get_prompt(
            GetPromptRequestParams::new("mock_for_frontend_feature").with_arguments(
                json!({"feature": "orders page"})
                    .as_object()
                    .unwrap()
                    .clone(),
            ),
        )
        .await
        .unwrap();
    let p = serde_json::to_string(&p).unwrap();
    assert!(p.contains("orders page") && p.contains("upsert_route") && p.contains("get_traffic"));
    assert!(
        c.client
            .get_prompt(GetPromptRequestParams::new("nope"))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn secrets_never_appear_in_any_output() {
    let c = rig(AllowedClasses::all()).await;
    // Plant the secret where tools can echo it: a Response body and a route name.
    let (err, out) = call(
        &c,
        "upsert_route",
        json!({
            "server": "example-api", "method": "GET", "path": "/leak",
            "name": format!("leak {SECRET}"),
            "responses": [{"status": 200, "body": format!("token={SECRET} sess-xyz-789")}],
        }),
    )
    .await;
    assert!(!err, "{out}");
    let mut all = out;
    // Every read tool, with empty or minimal arguments, plus errors and resources.
    let tools = c.client.list_all_tools().await.unwrap();
    for t in tools {
        let (_, o) = call(&c, &t.name, json!({"server": "example-api"})).await;
        all.push_str(&o);
        let (_, o) = call(&c, &t.name, json!({})).await;
        all.push_str(&o);
    }
    for uri in [
        "fauxfield://servers/example-api",
        "fauxfield://traffic/recent",
    ] {
        match c
            .client
            .read_resource(ReadResourceRequestParams::new(uri.to_owned()))
            .await
        {
            Ok(r) => all.push_str(&serde_json::to_string(&r).unwrap_or_default()),
            Err(e) => all.push_str(&e.to_string()),
        }
    }
    assert!(
        all.contains("/leak"),
        "the route should exist in the outputs"
    );
    assert!(!all.contains(SECRET), "secret leaked");
    assert!(!all.contains("sess-xyz-789"), "[mcp] redact extra leaked");
    assert!(all.contains("***"));
}

#[tokio::test]
async fn path_jail_refuses_escapes_and_logs_denials() {
    let c = rig(AllowedClasses::all()).await;
    for bad in ["../../etc/passwd", "/etc/passwd"] {
        let (err, out) = call(
            &c,
            "upsert_route",
            json!({
                "server": "example-api", "method": "GET", "path": "/jail",
                "responses": [{"status": 200, "body_file": bad}],
            }),
        )
        .await;
        assert!(err && out.contains("outside the workspace"), "{bad}: {out}");
    }
    let (err, out) = call(
        &c,
        "upsert_route",
        json!({
            "server": "example-api", "method": "GET", "path": "/jail-ok",
            "responses": [{"status": 200, "body_file": "files/ok.json"}],
        }),
    )
    .await;
    assert!(!err, "{out}");
}

#[tokio::test]
async fn read_only_hides_write_tools_and_refuses_them() {
    let c = rig(AllowedClasses::read_only()).await;
    let names: Vec<String> = c
        .client
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.contains(&"list_routes".into()));
    assert!(!names.contains(&"upsert_route".into()));
    let (err, _) = call(&c, "upsert_route", json!({"server": "example-api"})).await;
    assert!(err);
    // Annotations and output schemas are present on every tool.
    for t in c.client.list_all_tools().await.unwrap() {
        assert!(t.output_schema.is_some(), "{} has no outputSchema", t.name);
        let a = t
            .annotations
            .as_ref()
            .unwrap_or_else(|| panic!("{} no annotations", t.name));
        assert_eq!(a.read_only_hint, Some(true), "{}", t.name);
    }
}

/// Every scenario file in `fixtures/scenarios` parses, including the M16 `call` step.
#[test]
fn scenario_fixtures_parse() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/scenarios");
    let mut n = 0;
    for e in std::fs::read_dir(&dir).unwrap().flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "yaml") {
            let text = std::fs::read_to_string(&p).unwrap();
            fauxfield_mcp::scenario::Scenario::from_yaml(&text)
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            n += 1;
        }
    }
    assert!(n >= 10, "found {n} scenarios");
    let m16 = std::fs::read_to_string(dir.join("m16-agents.yaml")).unwrap();
    let s = fauxfield_mcp::scenario::Scenario::from_yaml(&m16).unwrap();
    assert!(
        s.steps
            .iter()
            .any(|st| matches!(st, fauxfield_mcp::scenario::Step::Call { tool, .. } if tool == "upsert_route"))
    );
}
