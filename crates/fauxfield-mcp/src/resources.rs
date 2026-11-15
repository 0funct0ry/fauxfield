//! MCP resources and prompts (SPEC §11.1, M16). Resources read through the same registry as
//! the tools, so redaction, caps and the activity log apply to them too.

use rmcp::model::{
    Prompt, PromptArgument, PromptMessage, Resource, ResourceContents, ResourceTemplate, Role,
};
use serde_json::{Map, Value, json};

use crate::{
    class::AllowedClasses,
    registry::{CallError, Registry},
};

pub const URI_SCHEMA: &str = "fauxfield://schema/server";
pub const URI_TEMPLATING: &str = "fauxfield://docs/templating";
pub const URI_TRAFFIC: &str = "fauxfield://traffic/recent";
pub const URI_SERVER_PREFIX: &str = "fauxfield://servers/";

pub fn resources() -> Vec<Resource> {
    vec![
        Resource::new(URI_SCHEMA, "Server file JSON Schema")
            .with_description("JSON Schema of servers/<slug>.toml")
            .with_mime_type("application/schema+json"),
        Resource::new(URI_TEMPLATING, "Templating reference")
            .with_description("Tera functions, filters and fake kinds available in Responses")
            .with_mime_type("text/markdown"),
        Resource::new(URI_TRAFFIC, "Recent traffic")
            .with_description("The last 100 Recordings, secrets redacted")
            .with_mime_type("application/json"),
    ]
}

pub fn resource_templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("fauxfield://servers/{id}", "Server")
            .with_description("One Server as structured JSON; id is an ID, name or file slug")
            .with_mime_type("application/json"),
    ]
}

/// Tera reference text, from the engine registry (the docs page uses the same tables).
pub fn templating_markdown() -> String {
    use fauxfield_engine::template::{FAKE_KINDS, FILTERS, FUNCTIONS};
    let mut o = String::from("# Template functions and fake kinds\n\n");
    for (title, rows) in [
        ("Functions", FUNCTIONS),
        ("Filters", FILTERS),
        ("Fake kinds", FAKE_KINDS),
    ] {
        o.push_str(&format!(
            "## {title}\n\n| Name | Description |\n|---|---|\n"
        ));
        for (n, d) in rows {
            o.push_str(&format!("| `{n}` | {} |\n", d.replace('|', "\\|")));
        }
        o.push('\n');
    }
    o
}

fn text(uri: &str, mime: &str, body: String) -> ResourceContents {
    let mut c = ResourceContents::text(body, uri);
    if let ResourceContents::TextResourceContents { mime_type, .. } = &mut c {
        *mime_type = Some(mime.into());
    }
    c
}

async fn tool(
    reg: &Registry,
    client: &str,
    allowed: AllowedClasses,
    name: &str,
    args: Value,
) -> Result<Value, String> {
    let Value::Object(args) = args else {
        return Err("bad arguments".into());
    };
    reg.call_for(client, allowed, name, args)
        .await
        .map_err(|e| match e {
            CallError::Tool(m) => m,
            CallError::Unknown(n) => format!("`{n}` is not available on this server"),
            CallError::Refused { name, class } => {
                format!("`{name}` needs the `{class}` class, which is not allowed")
            }
        })
}

/// Read one resource. Errors become protocol errors in the handler.
pub async fn read(
    reg: &Registry,
    client: &str,
    allowed: AllowedClasses,
    uri: &str,
) -> Result<ResourceContents, String> {
    match uri {
        URI_SCHEMA => Ok(text(
            uri,
            "application/schema+json",
            serde_json::to_string_pretty(&fauxfield_model::server_schema()).unwrap_or_default(),
        )),
        URI_TEMPLATING => Ok(text(uri, "text/markdown", templating_markdown())),
        URI_TRAFFIC => {
            let v = tool(reg, client, allowed, "get_traffic", json!({ "limit": 100 })).await?;
            Ok(text(uri, "application/json", pretty(&v)))
        }
        u if u.starts_with(URI_SERVER_PREFIX) => {
            let id = &u[URI_SERVER_PREFIX.len()..];
            if id.is_empty() || id.contains('/') {
                return Err(format!("bad Server id in `{u}`"));
            }
            let v = tool(reg, client, allowed, "get_server", json!({ "server": id })).await?;
            Ok(text(uri, "application/json", pretty(&v)))
        }
        other => Err(format!("unknown resource `{other}`")),
    }
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Server ids and slugs for `fauxfield://servers/{id}` completion.
pub async fn server_ids(reg: &Registry, allowed: AllowedClasses) -> Vec<String> {
    let Ok(v) = tool(reg, "completion", allowed, "list_servers", json!({})).await else {
        return vec![];
    };
    v.get("servers")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|s| {
            let id = s.get("id").and_then(Value::as_str).map(str::to_owned);
            let slug = s
                .get("file")
                .and_then(Value::as_str)
                .and_then(|f| std::path::Path::new(f).file_stem())
                .map(|x| x.to_string_lossy().into_owned());
            [id, slug]
        })
        .flatten()
        .collect()
}

// ---- prompts ----

/// Tools the prompts tell the agent to call; a test checks each exists in the registry.
pub const PROMPT_TOOLS: &[&str] = &[
    "workspace_info",
    "list_servers",
    "create_server",
    "upsert_route",
    "start_server",
    "send_test_request",
    "get_traffic",
    "validate",
    "match_path",
];

pub fn prompts() -> Vec<Prompt> {
    let arg = |n: &str, d: &str| {
        PromptArgument::new(n)
            .with_description(d)
            .with_required(true)
    };
    vec![
        Prompt::new(
            "mock_from_openapi",
            Some("Mock the endpoints of an OpenAPI document and verify them."),
            Some(vec![arg("spec", "Path or URL of the OpenAPI document")]),
        ),
        Prompt::new(
            "mock_from_description",
            Some("Create mock Routes from a plain-language description and verify them."),
            Some(vec![arg("description", "What the API should do")]),
        ),
        Prompt::new(
            "mock_for_frontend_feature",
            Some("Read the code of a frontend feature, mock every endpoint it calls and verify."),
            Some(vec![arg(
                "feature",
                "Describe the feature or list the files that call the API",
            )]),
        ),
    ]
}

pub fn prompt_text(name: &str, args: &Map<String, Value>) -> Option<String> {
    let a = |k: &str| {
        args.get(k)
            .and_then(Value::as_str)
            .unwrap_or("(not provided)")
            .to_owned()
    };
    let verify = "Then `start_server` for the Server, call `send_test_request` for each Route and check the status and body, and call `get_traffic` to confirm every request was recorded. Fix any Route that does not answer as intended and repeat. Use the Fauxfield vocabulary (Server, Folder, Route, Response, Rule, Dataset).";
    Some(match name {
        "mock_from_openapi" => format!(
            "Mock the API described by the OpenAPI document `{}`. Read it, then call `workspace_info` and `list_servers` to see what exists. Use `create_server` when no Server fits, and call `upsert_route` once per operation with realistic Responses (status, headers, body) taken from the document's examples and schemas. {verify}",
            a("spec")
        ),
        "mock_from_description" => format!(
            "Mock this API: {}\n\nCall `workspace_info` and `list_servers` first. Use `create_server` if needed, and call `upsert_route` once per Route with realistic Responses. Run `validate` and fix any diagnostics. {verify}",
            a("description")
        ),
        "mock_for_frontend_feature" => format!(
            "Mock the backend for this frontend feature: {}\n\nRead the code to list every endpoint it calls (method, path, request and response shape). Call `workspace_info` and `list_servers`, use `create_server` if needed, then call `upsert_route` once per endpoint; use `match_path` to check path patterns. {verify}",
            a("feature")
        ),
        _ => return None,
    })
}

pub fn prompt_messages(name: &str, args: &Map<String, Value>) -> Option<Vec<PromptMessage>> {
    prompt_text(name, args).map(|t| vec![PromptMessage::new_text(Role::User, t)])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompts_name_only_registered_tool_names() {
        for p in prompts() {
            let t = prompt_text(&p.name, &Map::new()).unwrap();
            let named: Vec<&str> = t
                .split('`')
                .skip(1)
                .step_by(2)
                .filter(|s| s.chars().all(|c| c.is_ascii_lowercase() || c == '_'))
                .collect();
            assert!(!named.is_empty(), "{}", p.name);
            for n in named {
                assert!(
                    PROMPT_TOOLS.contains(&n),
                    "{} names unlisted tool {n}",
                    p.name
                );
            }
        }
    }

    #[test]
    fn templating_reference_has_tables() {
        let m = templating_markdown();
        assert!(m.contains("## Functions") && m.contains("## Fake kinds"));
    }
}
