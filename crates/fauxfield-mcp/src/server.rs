//! Hand-written `ServerHandler` over our own registry, so class filtering applies to both
//! `tools/list` and `tools/call` (SPEC §11.1).

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer,
    handler::server::ServerHandler,
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock,
        GetPromptRequestParams, GetPromptResponse, GetPromptResult, Implementation,
        ListPromptsResult, ListResourceTemplatesResult, ListResourcesResult, ListToolsResult,
        PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
        ReadResourceResult, ServerCapabilities, ServerConfig, Tool, ToolAnnotations,
    },
    service::RequestContext,
};

use crate::{
    class::AllowedClasses,
    registry::{CallError, Registry},
    resources,
};

/// Tools that return pictures put them under a top-level `_images` array of
/// `{ "data": <base64>, "mime": "image/png" }`. They leave the structured JSON and become MCP
/// image content blocks, so the (large) base64 is not repeated as text.
fn split_images(mut v: serde_json::Value) -> (serde_json::Value, Vec<(String, String)>) {
    let Some(arr) = v.as_object_mut().and_then(|o| o.remove("_images")) else {
        return (v, Vec::new());
    };
    let images = arr
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|i| {
            Some((
                i.get("data")?.as_str()?.to_owned(),
                i.get("mime")?.as_str()?.to_owned(),
            ))
        })
        .collect();
    (v, images)
}

#[derive(Clone)]
pub struct FauxfieldServer {
    registry: Arc<Registry>,
    allowed: AllowedClasses,
}

/// The MCP `clientInfo.name` of this session, or `unknown` before `initialize`.
fn client_name(context: &RequestContext<RoleServer>) -> String {
    context
        .peer
        .peer_info()
        .map(|i| i.client_info.name.clone())
        .unwrap_or_else(|| "unknown".into())
}

impl FauxfieldServer {
    pub fn new(registry: Arc<Registry>, allowed: AllowedClasses) -> Self {
        Self { registry, allowed }
    }
}

impl ServerHandler for FauxfieldServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_resources()
                .enable_prompts()
                .build(),
        )
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
            .map(|t| {
                let h = t.hints();
                Tool::new(t.name, t.description, t.input_schema.clone())
                    .with_raw_output_schema(t.output_schema.clone())
                    .with_annotations(
                        ToolAnnotations::new()
                            .read_only(h.read_only)
                            .destructive(h.destructive)
                            .idempotent(h.idempotent),
                    )
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn on_initialized(&self, context: rmcp::service::NotificationContext<RoleServer>) {
        // Tell the host an agent is attached, and keep telling it every second so a crashed
        // client disappears from the GUI within a few seconds.
        let Some(sink) = self.registry.sink() else {
            return;
        };
        let name = context
            .peer
            .peer_info()
            .map(|i| i.client_info.name.clone())
            .unwrap_or_else(|| "unknown".into());
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = format!(
            "{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let peer = context.peer.clone();
        tokio::spawn(async move {
            loop {
                if peer.is_transport_closed() {
                    sink.detach(&id);
                    break;
                }
                sink.attach(&id, &name);
                tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            }
        });
    }

    async fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        Ok(ListResourcesResult::with_all_items(resources::resources()))
    }

    async fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListResourceTemplatesResult, McpError> {
        Ok(ListResourceTemplatesResult::with_all_items(
            resources::resource_templates(),
        ))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let client = client_name(&context);
        match resources::read(&self.registry, &client, self.allowed, &request.uri).await {
            Ok(c) => Ok(ReadResourceResult::new(vec![c]).into()),
            Err(m) => Err(McpError::invalid_params(m, None)),
        }
    }

    async fn list_prompts(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListPromptsResult, McpError> {
        Ok(ListPromptsResult::with_all_items(resources::prompts()))
    }

    async fn get_prompt(
        &self,
        request: GetPromptRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<GetPromptResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        match resources::prompt_messages(&request.name, &args) {
            Some(m) => Ok(GetPromptResult::new(m).into()),
            None => Err(McpError::invalid_params(
                format!("unknown prompt `{}`", request.name),
                None,
            )),
        }
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = request.arguments.unwrap_or_default();
        let client = client_name(&context);
        match self
            .registry
            .call_for(&client, self.allowed, &request.name, args)
            .await
        {
            Ok(v) => {
                let (v, images) = split_images(v);
                let mut result = CallToolResult::structured(v);
                result.content.extend(
                    images
                        .into_iter()
                        .map(|(data, mime)| ContentBlock::image(data, mime)),
                );
                Ok(result.into())
            }
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

#[cfg(test)]
mod tests {
    use super::split_images;
    use serde_json::json;

    #[test]
    fn images_leave_the_structured_json() {
        let (v, imgs) = split_images(json!({
            "ok": true,
            "_images": [{ "data": "QUJD", "mime": "image/png" }],
        }));
        assert_eq!(v, json!({ "ok": true }));
        assert_eq!(imgs, [("QUJD".to_owned(), "image/png".to_owned())]);
        let (v, imgs) = split_images(json!({ "ok": 1 }));
        assert_eq!(v, json!({ "ok": 1 }));
        assert!(imgs.is_empty());
    }
}
