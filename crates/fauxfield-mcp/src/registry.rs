//! Tool registry with class filtering (SPEC §11.1).

use std::{future::Future, pin::Pin, sync::Arc};

use schemars::JsonSchema;
use serde_json::{Map, Value};

use crate::{
    class::{AllowedClasses, ToolClass},
    guard::{ActivityLog, Outcome, Redactor, cap_bodies, target_of},
};

/// MCP tool annotations derived from the class and the name (SPEC §11.1).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hints {
    pub read_only: bool,
    pub destructive: bool,
    pub idempotent: bool,
}

pub fn hints_for(name: &str, class: ToolClass) -> Hints {
    let read_only = matches!(class, ToolClass::Read | ToolClass::Ui) && !name.starts_with("ui.");
    let read_only = read_only || class == ToolClass::Read;
    let destructive = !read_only
        && (name.starts_with("delete_")
            || matches!(
                name,
                "reset_state" | "clear_dataset" | "disconnect_ws_client"
            ));
    let idempotent = read_only
        || destructive
        || name.starts_with("upsert_")
        || name.starts_with("set_")
        || name.starts_with("update_")
        || matches!(name, "stop_server" | "import_dataset" | "bake_dataset");
    Hints {
        read_only,
        destructive,
        idempotent,
    }
}

pub type ToolFuture = Pin<Box<dyn Future<Output = Result<Value, String>> + Send>>;
type Handler = dyn Fn(Map<String, Value>) -> ToolFuture + Send + Sync;

#[derive(Clone)]
pub struct ToolDef {
    pub name: &'static str,
    pub description: &'static str,
    pub class: ToolClass,
    /// JSON Schema generated from a Rust input type; always an object at the root.
    pub input_schema: Arc<Map<String, Value>>,
    /// JSON Schema of the structured result; always an object at the root.
    pub output_schema: Arc<Map<String, Value>>,
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
            output_schema: Arc::new(generic_output()),
            handler: Arc::new(handler),
        }
    }
}

fn generic_output() -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("type".into(), Value::String("object".into()));
    m.insert("additionalProperties".into(), Value::Bool(true));
    m
}

impl ToolDef {
    /// Replace the generic output schema with one generated from a Rust type.
    pub fn output<O: JsonSchema>(mut self) -> Self {
        if let Ok(Value::Object(m)) = serde_json::to_value(schemars::schema_for!(O)) {
            self.output_schema = Arc::new(m);
        }
        self
    }
    pub fn hints(&self) -> Hints {
        hints_for(self.name, self.class)
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

/// Called after every successful tool call (used by `ui.record`).
pub type Observer = Arc<dyn Fn(&str, &Map<String, Value>) + Send + Sync>;

#[derive(Default, Clone)]
pub struct Registry {
    tools: Vec<ToolDef>,
    observer: Option<Observer>,
    redactor: Redactor,
    activity: ActivityLog,
    jail_root: Option<std::path::PathBuf>,
    sink: Option<std::sync::Arc<dyn crate::guard::AgentSink>>,
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
    /// Forward every activity entry (and agent sessions) to the host.
    pub fn set_sink(&mut self, sink: std::sync::Arc<dyn crate::guard::AgentSink>) {
        self.sink = Some(sink);
    }
    pub fn sink(&self) -> Option<std::sync::Arc<dyn crate::guard::AgentSink>> {
        self.sink.clone()
    }
    fn note(&self, client: &str, tool: &str, target: String, outcome: Outcome) {
        self.activity.record(client, tool, target, outcome);
        if let (Some(sink), Some(a)) = (&self.sink, self.activity.snapshot().last()) {
            sink.record(a);
        }
    }
    /// Confine file-path arguments to this workspace folder.
    pub fn set_jail(&mut self, root: std::path::PathBuf) {
        self.jail_root = Some(root);
    }
    pub fn set_redactor(&mut self, r: Redactor) {
        self.redactor = r;
    }
    pub fn activity(&self) -> ActivityLog {
        self.activity.clone()
    }
    pub fn tool(&self, name: &str) -> Option<&ToolDef> {
        self.tools.iter().find(|t| t.name == name)
    }
    pub fn all(&self) -> &[ToolDef] {
        &self.tools
    }
    pub fn set_observer(&mut self, observer: Observer) {
        self.observer = Some(observer);
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
        self.call_for("unknown", allowed, name, args).await
    }

    /// Run a tool on behalf of `client`: refusals and failures go to the Agent activity log,
    /// results and errors pass through the redactor, bodies are capped.
    pub async fn call_for(
        &self,
        client: &str,
        allowed: AllowedClasses,
        name: &str,
        args: Map<String, Value>,
    ) -> Result<Value, CallError> {
        let target = self.redactor.text(&target_of(&args));
        let Some(tool) = self.tools.iter().find(|t| t.name == name) else {
            self.note(client, name, target, Outcome::Denied);
            return Err(CallError::Unknown(name.into()));
        };
        if !allowed.contains(tool.class) {
            self.note(client, name, target, Outcome::Denied);
            return Err(CallError::Refused {
                name: name.into(),
                class: tool.class,
            });
        }
        let args = match &self.jail_root {
            Some(root) => match crate::guard::jail_args(root, args) {
                Ok(a) => a,
                Err(e) => {
                    self.note(client, name, target, Outcome::Denied);
                    return Err(CallError::Tool(e));
                }
            },
            None => args,
        };
        let seen = self.observer.as_ref().map(|_| args.clone());
        let out = match (tool.handler)(args).await {
            Ok(v) => v,
            Err(e) => {
                self.note(client, name, target, Outcome::Error);
                return Err(CallError::Tool(self.redactor.text(&e)));
            }
        };
        if let (Some(o), Some(a)) = (&self.observer, seen) {
            o(name, &a);
        }
        let outcome = match name {
            n if n.starts_with("delete_") => Outcome::Deleted,
            n if n.starts_with("start_") => Outcome::Started,
            n if n.starts_with("create_") => Outcome::Created,
            n if n.starts_with("upsert_") || n.starts_with("update_") || n.starts_with("set_") => {
                Outcome::Updated
            }
            _ => Outcome::Read,
        };
        self.note(client, name, target, outcome);
        let mut out = self.redactor.value(out);
        cap_bodies(&mut out);
        Ok(out)
    }
}
