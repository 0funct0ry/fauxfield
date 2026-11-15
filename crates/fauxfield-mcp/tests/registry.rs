//! Registry and class filtering through a real rmcp client over an in-memory duplex.

use std::sync::Arc;

use fauxfield_mcp::{
    AllowedClasses, FauxfieldServer, Registry, ToolClass, ToolDef, TransportKind, build_registry,
    tools::{NoInput, ServerFacts, core_registry},
};
use rmcp::{
    ServiceExt,
    model::{CallToolRequestParams, ErrorCode},
};
use serde_json::{Map, Value, json};

fn registry_with_write_tool() -> Registry {
    let mut r = core_registry(ServerFacts {
        transports: vec![TransportKind::Stdio],
        allowed: AllowedClasses::all(),
        features: vec![],
    });
    r.register(ToolDef::new::<NoInput>(
        "danger_write",
        "test-only write tool",
        ToolClass::Write,
        |_: Map<String, Value>| Box::pin(async { Ok(json!({"wrote": true})) }),
    ));
    r.register(ToolDef::new::<NoInput>(
        "always_fails",
        "test-only failing tool",
        ToolClass::Read,
        |_: Map<String, Value>| Box::pin(async { Err("boom".to_string()) }),
    ));
    r
}

async fn client(allowed: AllowedClasses) -> rmcp::service::RunningService<rmcp::RoleClient, ()> {
    let (s, c) = tokio::io::duplex(65536);
    let server = FauxfieldServer::new(Arc::new(registry_with_write_tool()), allowed);
    tokio::spawn(async move {
        if let Ok(r) = server.serve(s).await {
            let _ = r.waiting().await;
        }
    });
    ().serve(c).await.unwrap()
}

#[tokio::test]
async fn lists_and_calls_ping_and_server_info() {
    let c = client(AllowedClasses::all()).await;
    let names: Vec<String> = c
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(names.contains(&"ping".into()) && names.contains(&"server_info".into()));
    for t in c.list_all_tools().await.unwrap() {
        assert_eq!(
            t.input_schema.get("type"),
            Some(&json!("object")),
            "{}",
            t.name
        );
    }

    let ping = c
        .call_tool(CallToolRequestParams::new("ping"))
        .await
        .unwrap();
    assert_eq!(ping.structured_content, Some(json!({"ok": true})));

    let info = c
        .call_tool(CallToolRequestParams::new("server_info"))
        .await
        .unwrap()
        .structured_content
        .unwrap();
    assert_eq!(info["name"], "fauxfield");
    assert_eq!(info["schema_version"], 1);
    assert_eq!(info["allowed_classes"], json!(["read", "write", "execute"]));
    assert!(info["transports"].is_array() && info["features"].is_array());
}

#[tokio::test]
async fn disallowed_class_is_hidden_and_refused_with_protocol_error() {
    let c = client(AllowedClasses::read_only()).await;
    let names: Vec<String> = c
        .list_all_tools()
        .await
        .unwrap()
        .into_iter()
        .map(|t| t.name.to_string())
        .collect();
    assert!(!names.contains(&"danger_write".into()));

    let err = c
        .call_tool(CallToolRequestParams::new("danger_write"))
        .await
        .expect_err("refused calls are protocol errors");
    match err {
        rmcp::ServiceError::McpError(e) => {
            assert_eq!(e.code, ErrorCode::INVALID_PARAMS);
            assert!(e.message.contains("not allowed"), "{}", e.message);
        }
        other => panic!("expected a protocol error, got {other:?}"),
    }

    // Allowed again: it runs.
    let c = client(AllowedClasses::all()).await;
    let ok = c
        .call_tool(CallToolRequestParams::new("danger_write"))
        .await
        .unwrap();
    assert_eq!(ok.structured_content, Some(json!({"wrote": true})));
}

#[tokio::test]
async fn tool_failures_are_tool_errors_and_unknown_tools_protocol_errors() {
    let c = client(AllowedClasses::all()).await;
    let r = c
        .call_tool(CallToolRequestParams::new("always_fails"))
        .await
        .unwrap();
    assert_eq!(r.is_error, Some(true));
    assert!(
        c.call_tool(CallToolRequestParams::new("nope"))
            .await
            .is_err()
    );
}

#[test]
fn default_registry_has_exactly_the_core_tools() {
    let r = build_registry(&Default::default());
    let mut n: Vec<_> = r
        .list(AllowedClasses::all())
        .iter()
        .map(|t| t.name)
        .collect();
    n.sort();
    assert_eq!(n, ["ping", "server_info"]);
}

/// Every tool the generated AGENTS.md section names exists in the real registry, so the
/// instructions cannot drift from the tools (M16).
#[test]
fn agents_md_tool_names_exist_in_the_registry() {
    use fauxfield_mcp::{
        control_tools::register_control_tools,
        workspace_tools::{fixed_store, register_workspace_tools},
    };
    let dir = tempfile::tempdir().unwrap();
    fauxfield_store::init_workspace(dir.path()).unwrap();
    let store = fauxfield_store::Store::open(dir.path()).unwrap();
    let mut r = Registry::new();
    register_workspace_tools(&mut r, fixed_store(store));
    register_control_tools(&mut r, Arc::new(|_| Ok(None)));
    for name in fauxfield_store::agents::TOOL_NAMES {
        assert!(
            r.tool(name).is_some(),
            "AGENTS.md names unknown tool `{name}`"
        );
    }
}
