use std::sync::Arc;

use rmcp::{
    Json, ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;

use mcp_common::llm_state::{ConversationId, ConversationStore, UsageStats, UsageTracker};
use mcp_common::openai::{ChatCompletionRequest, Message, ModelListResponse, OpenAiClient};

use crate::rate_limit::RateLimiter;

#[derive(Clone)]
pub struct LlmProxyServer {
    openai: Arc<OpenAiClient>,
    convos: ConversationStore,
    usage: UsageTracker,
    limiter: Option<RateLimiter>,
    tool_router: ToolRouter<LlmProxyServer>,
}

impl LlmProxyServer {
    pub fn new(
        openai: Arc<OpenAiClient>,
        convos: ConversationStore,
        usage: UsageTracker,
        limiter: Option<RateLimiter>,
    ) -> Self {
        Self {
            openai,
            convos,
            usage,
            limiter,
            tool_router: Self::tool_router(),
        }
    }

    async fn gate(&self) -> Result<(), String> {
        if let Some(limiter) = &self.limiter {
            limiter.check().await?;
        }
        Ok(())
    }

    async fn run_chat(&self, model: &str, messages: Vec<Message>) -> Result<String, String> {
        self.gate().await?;

        let request = ChatCompletionRequest {
            model: model.to_string(),
            messages,
            temperature: None,
            max_tokens: None,
            stream: None,
        };
        let response = self
            .openai
            .chat_completions(request, None)
            .await
            .map_err(|e| format!("chat failed: {e}"))?;

        let text = response
            .choices
            .get(0)
            .and_then(|c| c.message.content.as_ref())
            .map(|s| s.to_string())
            .ok_or_else(|| "chat failed: missing choices[0].message.content".to_string())?;

        self.usage.record(model, response.usage.as_ref()).await;
        Ok(text)
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct AskModelParams {
    model: String,
    prompt: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ChatModelParams {
    model: String,
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct GenerateCodeParams {
    specification: String,
    language: String,
    model: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ContinueConversationParams {
    conversation_id: ConversationId,
    model: String,
    prompt: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EndConversationParams {
    conversation_id: ConversationId,
}

#[derive(Debug, serde::Serialize, JsonSchema)]
struct StartConversationResponse {
    conversation_id: ConversationId,
}

#[derive(Debug, serde::Serialize, JsonSchema)]
struct TextResponse {
    text: String,
}

#[derive(Debug, serde::Serialize, JsonSchema)]
struct OkResponse {
    ok: bool,
}

#[tool_router]
impl LlmProxyServer {
    #[tool(description = "List models available from the local OpenAI-compatible host (GET /v1/models).")]
    async fn list_models(&self) -> Result<Json<ModelListResponse>, String> {
        self.gate().await?;
        let models = self
            .openai
            .list_models()
            .await
            .map_err(|e| format!("list_models failed: {e}"))?;
        Ok(Json(models))
    }

    #[tool(description = "Run a single-turn prompt against a chosen local model ID (POST /v1/chat/completions). Returns the final assistant text.")]
    async fn ask_model(
        &self,
        Parameters(params): Parameters<AskModelParams>,
    ) -> Result<Json<TextResponse>, String> {
        let prompt = params.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("prompt must not be empty".to_string());
        }
        let model = params.model.trim().to_string();
        if model.is_empty() {
            return Err("model must not be empty".to_string());
        }
        let reply = self
            .run_chat(
                &model,
                vec![Message {
                    role: "user".to_string(),
                    content: prompt,
                }],
            )
            .await?;
        Ok(Json(TextResponse { text: reply }))
    }

    #[tool(description = "Run a multi-message chat against a chosen local model ID (POST /v1/chat/completions). Returns the final assistant text.")]
    async fn chat_model(
        &self,
        Parameters(params): Parameters<ChatModelParams>,
    ) -> Result<Json<TextResponse>, String> {
        let model = params.model.trim().to_string();
        if model.is_empty() {
            return Err("model must not be empty".to_string());
        }
        if params.messages.is_empty() {
            return Err("messages must not be empty".to_string());
        }
        let reply = self.run_chat(&model, params.messages).await?;
        Ok(Json(TextResponse { text: reply }))
    }

    #[tool(description = "Generate code for a given specification. The caller chooses the model. Returns code-only output unless the specification explicitly asks otherwise.")]
    async fn generate_code(
        &self,
        Parameters(params): Parameters<GenerateCodeParams>,
    ) -> Result<Json<TextResponse>, String> {
        let model = params.model.trim().to_string();
        if model.is_empty() {
            return Err("model must not be empty".to_string());
        }

        let language = params.language.trim().to_string();
        if language.is_empty() {
            return Err("language must not be empty".to_string());
        }

        let specification = params.specification.trim().to_string();
        if specification.is_empty() {
            return Err("specification must not be empty".to_string());
        }

        let instruction = format!(
            "Write complete, properly formatted {language} code to satisfy the specification. \
Return only the code (no explanation) unless the specification explicitly requests explanation.\n\n\
SPECIFICATION:\n{specification}"
        );

        let reply = self
            .run_chat(
                &model,
                vec![Message {
                    role: "user".to_string(),
                    content: instruction,
                }],
            )
            .await?;
        Ok(Json(TextResponse { text: reply }))
    }

    #[tool(description = "Start a Redis-backed conversation and return a conversation_id.")]
    async fn start_conversation(&self) -> Result<Json<StartConversationResponse>, String> {
        let id = self.convos.start().await;
        Ok(Json(StartConversationResponse { conversation_id: id }))
    }

    #[tool(description = "Continue a Redis-backed conversation by appending a user prompt, calling the chosen model, appending the assistant reply, and returning the reply text.")]
    async fn continue_conversation(
        &self,
        Parameters(params): Parameters<ContinueConversationParams>,
    ) -> Result<Json<TextResponse>, String> {
        let model = params.model.trim().to_string();
        if model.is_empty() {
            return Err("model must not be empty".to_string());
        }
        let prompt = params.prompt.trim().to_string();
        if prompt.is_empty() {
            return Err("prompt must not be empty".to_string());
        }

        let mut messages = self
            .convos
            .get_messages(&params.conversation_id)
            .await
            .ok_or_else(|| format!("unknown conversation_id: {}", params.conversation_id))?;
        messages.push(Message {
            role: "user".to_string(),
            content: prompt,
        });

        let reply = self.run_chat(&model, messages.clone()).await?;

        messages.push(Message {
            role: "assistant".to_string(),
            content: reply.clone(),
        });
        if !self.convos.set_messages(&params.conversation_id, &messages).await {
            return Err("failed to persist conversation state".to_string());
        }

        Ok(Json(TextResponse { text: reply }))
    }

    #[tool(description = "End a Redis-backed conversation and delete its stored message history.")]
    async fn end_conversation(
        &self,
        Parameters(params): Parameters<EndConversationParams>,
    ) -> Result<Json<OkResponse>, String> {
        self.convos.end(&params.conversation_id).await;
        Ok(Json(OkResponse { ok: true }))
    }

    #[tool(description = "Get usage stats aggregated per model (requests + tokens when reported by upstream).")]
    async fn get_usage_stats(&self) -> Result<Json<UsageStats>, String> {
        let stats = self.usage.get_usage_stats().await;
        Ok(Json(stats))
    }
}

#[tool_handler]
impl ServerHandler for LlmProxyServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2025_06_18,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation {
                name: "llm-proxy".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "Local LLM proxy MCP server. Use list_models to discover local models, then call \
ask_model/chat_model/generate_code with an explicit model ID. For multi-turn workflows, use \
start_conversation/continue_conversation/end_conversation. Usage counters are available via \
get_usage_stats."
                    .to_string(),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::LlmProxyServer;

    #[test]
    fn tools_publish_output_schemas() {
        let tools = LlmProxyServer::tool_router().list_all();
        for name in [
            "list_models",
            "ask_model",
            "chat_model",
            "generate_code",
            "start_conversation",
            "continue_conversation",
            "end_conversation",
            "get_usage_stats",
        ] {
            let tool = tools
                .iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("missing tool: {name}"));
            assert!(
                tool.output_schema.is_some(),
                "tool {name} should publish output_schema"
            );
        }
    }
}
