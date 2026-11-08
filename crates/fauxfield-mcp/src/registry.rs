//! Tool registry with class filtering (SPEC §11.1).

use std::{future::Future, pin::Pin, sync::Arc};

use schemars::JsonSchema;
use serde_json::{Map, Value};

use crate::class::{AllowedClasses, ToolClass};

pub type ToolFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;
type Handler = dyn Fn(Map<String, Value>) -> ToolFuture + Send + Sync;

#[derive(Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub class: ToolClass,
    /// JSON Schema generated from a Rust input type; always an object at the root.
    pub input_schema: Arc<Map<String, Value>>,
    handler: Arc<Handler>,
}

impl ToolDef {
    pub fn new<I: JsonSchema>(
        name: &'static str,
        description: &'static str,
        class: ToolClass,
        handler: impl Fn(Map<String, Value>) -> ToolFuture + Send + Sync + 'static,
    ) -> Self {
        let schema = match serde_json::to_value(schemars::schema_for!(I)) {
            Ok(Value::Object(m)) => m,
            _ => Map::new(),
        };
        Self {
            name,
            description,
            class,
            input_schema: Arc::new(schema),
            handler: Arc::new(handler),
        }
    }
}

/// Why a call did not run. Both map to protocol errors, never to tool errors.
#[derive(Debug, PartialEq, Eq)]
pub enum CallError {
    Unknown(String),
    /// The tool exists but its class is not allowed on this server.
    Refused {
        name: String,
        class: ToolClass,
    },
    /// The tool ran and failed (a tool error, not a protocol error).
    Tool(String),
}

#[derive(Default, Clone)]
pub struct Registry {
    tools: Vec<ToolDef>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn register(&mut self, tool: ToolDef) -> &mut Self {
        debug_assert!(self.tools.iter().all(|t| t.name != tool.name));
        self.tools.push(tool);
        self
    }
    /// Only tools whose class is allowed; the rest are invisible.
    pub fn list(&self, allowed: AllowedClasses) -> Vec<&ToolDef> {
        self.tools
            .iter()
            .filter(|t| allowed.contains(t.class))
            .collect()
    }
    pub async fn call(
        &self,
        allowed: AllowedClasses,
        name: &str,
        args: Map<String, Value>,
    ) -> Result<Value, CallError> {
        let Some(tool) = self.tools.iter().find(|t| t.name == name) else {
            return Err(CallError::Unknown(name.into()));
        };
        if !allowed.contains(tool.class) {
            return Err(CallError::Refused {
                name: name.into(),
                class: tool.class,
            });
        }
        (tool.handler)(args).await.map_err(CallError::Tool)
    }
}
