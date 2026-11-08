//! Hand-written `ServerHandler` over our own registry, so class filtering applies to both
//! `tools/list` and `tools/call` (SPEC §11.1).

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer,
    handler::server::ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};

use crate::{
    class::AllowedClasses,
    registry::{CallError, Registry},
};

#[derive(Clone)]
pub struct FauxfieldServer {
    registry: Arc<Registry>,
    allowed: AllowedClasses,
}

impl FauxfieldServer {
    pub fn new(registry: Arc<Registry>, allowed: AllowedClasses) -> Self {
        Self { registry, allowed }
    }
}

impl ServerHandler for FauxfieldServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("fauxfield", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Fauxfield is a local-first mock API tool. Call `server_info` for the \
                 available tool classes.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let tools = self
            .registry
            .list(self.allowed)
            .into_iter()
            .map(|t| Tool::new(t.name, t.description, t.input_schema.clone()))
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        match self.registry.call(self.allowed, &request.name, args).await {
            Ok(v) => Ok(CallToolResult::structured(v).into()),
            Err(CallError::Tool(msg)) => {
                Ok(CallToolResult::error(vec![ContentBlock::text(msg)]).into())
            }
            // Protocol errors: an agent must not mistake a refusal for a failed operation.
            Err(CallError::Unknown(name)) => Err(McpError::invalid_params(
                format!("unknown tool `{name}`"),
                None,
            )),
            Err(CallError::Refused { name, class }) => Err(McpError::invalid_params(
                format!(
                    "tool `{name}` is in the `{class}` class, which is not allowed on this server"
                ),
                None,
            )),
        }
    }
}
