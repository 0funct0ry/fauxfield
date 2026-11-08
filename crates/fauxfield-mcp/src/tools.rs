//! The M1.1 tools: `ping` and `server_info`. Later milestones add theirs (SPEC §11.1).

use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Map, Value, json};

use crate::{
    class::{AllowedClasses, ToolClass},
    registry::{Registry, ToolDef},
    transport::TransportKind,
};

/// Version of the MCP tool surface; bump when a tool's schema changes incompatibly.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Deserialize, JsonSchema)]
pub struct NoInput {}

/// Static facts reported by `server_info`.
#[derive(Debug, Clone)]
pub struct ServerFacts {
    pub transports: Vec<TransportKind>,
    pub allowed: AllowedClasses,
    pub features: Vec<&'static str>,
}

pub fn core_registry(facts: ServerFacts) -> Registry {
    let mut r = Registry::new();
    r.register(ToolDef::new::<NoInput>(
        "ping",
        "Check that the Fauxfield MCP server is alive. Returns {\"ok\": true}.",
        ToolClass::Read,
        |_: Map<String, Value>| Box::pin(async { Ok(json!({ "ok": true })) }),
    ));
    r.register(ToolDef::new::<NoInput>(
        "server_info",
        "Version, schema version, active transports, allowed tool classes and build features.",
        ToolClass::Read,
        move |_: Map<String, Value>| {
            let v = json!({
                "name": "fauxfield",
                "version": env!("CARGO_PKG_VERSION"),
                "schema_version": SCHEMA_VERSION,
                "transports": facts.transports.iter().map(|t| t.as_str()).collect::<Vec<_>>(),
                "allowed_classes": facts.allowed.names(),
                "features": facts.features,
            });
            Box::pin(async move { Ok(v) })
        },
    ));
    r
}
