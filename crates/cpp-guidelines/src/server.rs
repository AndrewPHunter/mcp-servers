/// MCP server implementation for C++ Core Guidelines.
///
/// Exposes four tools:
/// - `search_guidelines`: Semantic search over guidelines
/// - `get_guideline`: Look up a specific guideline by rule ID
/// - `list_category`: List all guidelines in a category
/// - `update_guidelines`: Trigger a re-index from the git repository
use std::collections::HashMap;
use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, ServerHandler,
    handler::server::router::tool::ToolRouter,
    handler::server::wrapper::Parameters,
    model::*,
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;
use tracing::info;

use crate::cache::GuidelineCache;
use crate::config::Config;
use crate::model::{Category, Guideline};
use crate::search::SearchEngine;
use crate::update::UpdateService;
use mcp_common::embedding::Embedder;
use mcp_common::vectordb::VectorDb;

// --- Tool parameter types ---

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SearchParams {
    /// The search query describing what you're looking for
    pub query: String,
    /// Maximum number of results to return (default: 10, max: 50)
    pub limit: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetGuidelineParams {
    /// The rule ID to look up (e.g. "P.1", "ES.20", "SL.con.1")
    pub rule_id: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ListCategoryParams {
    /// The category prefix (e.g. "P", "ES", "SL", "R")
    pub category: String,
}

// --- Tool response types ---

#[derive(Debug, Serialize)]
struct CategoryListResponse {
    category: Category,
    guidelines: Vec<GuidelineSummary>,
}

#[derive(Debug, Serialize)]
struct GuidelineSummary {
    id: String,
    title: String,
}

#[derive(Debug, Serialize)]
struct UpdateResponse {
    updated: bool,
    commit: String,
    guideline_count: usize,
}

// --- MCP Server ---

/// Shared application state, protected by RwLock for safe concurrent reads
/// and exclusive writes during re-indexing.
pub struct AppState {
    pub guidelines: HashMap<String, Guideline>,
    pub categories: HashMap<String, Category>,
}

#[derive(Clone)]
pub struct CppGuidelinesServer {
    state: Arc<RwLock<AppState>>,
    search_engine: Arc<SearchEngine>,
    update_service: Arc<UpdateService>,
    cache: Arc<GuidelineCache>,
    tool_router: ToolRouter<CppGuidelinesServer>,
}

impl CppGuidelinesServer {
    pub fn new(
        guidelines: Vec<Guideline>,
        categories: HashMap<String, Category>,
        embedder: Arc<Embedder>,
        vectordb: Arc<VectorDb>,
        cache: Arc<GuidelineCache>,
        config: Config,
    ) -> Self {
        let guideline_map: HashMap<String, Guideline> = guidelines
            .into_iter()
            .map(|g| (g.id.clone(), g))
            .collect();

        let search_engine = Arc::new(SearchEngine::new(
            Arc::clone(&embedder),
            Arc::clone(&vectordb),
            Arc::clone(&cache),
        ));

        let update_service = Arc::new(UpdateService::new(
            config,
            Arc::clone(&embedder),
            Arc::clone(&vectordb),
            Arc::clone(&cache),
        ));

        let state = Arc::new(RwLock::new(AppState {
            guidelines: guideline_map,
            categories,
        }));

        Self {
            state,
            search_engine,
            update_service,
            cache,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router]
impl CppGuidelinesServer {
    #[tool(description = "Search C++ Core Guidelines by semantic similarity. Returns ranked results matching the query.")]
    async fn search_guidelines(
        &self,
        Parameters(params): Parameters<SearchParams>,
    ) -> Result<CallToolResult, McpError> {
        let query = params.query.trim().to_string();
        if query.is_empty() {
            return Err(McpError::invalid_params("query must not be empty", None));
        }

        let limit = params.limit.unwrap_or(10).min(50) as usize;

        let results = self
            .search_engine
            .search(&query, limit)
            .await
            .map_err(|e| McpError::internal_error(format!("search failed: {e}"), None))?;

        let json = serde_json::to_string_pretty(&results)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Get the full content of a specific C++ Core Guideline by its rule ID (e.g. 'P.1', 'ES.20', 'SL.con.1').")]
    async fn get_guideline(
        &self,
        Parameters(params): Parameters<GetGuidelineParams>,
    ) -> Result<CallToolResult, McpError> {
        let rule_id = params.rule_id.trim().to_string();
        if rule_id.is_empty() {
            return Err(McpError::invalid_params("rule_id must not be empty", None));
        }

        // Check cache first
        if let Some(cached) = self.cache.get_guideline(&rule_id).await {
            let json = serde_json::to_string_pretty(&cached)
                .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;
            return Ok(CallToolResult::success(vec![Content::text(json)]));
        }

        // Look up in memory
        let state = self.state.read().await;
        let guideline = state
            .guidelines
            .get(&rule_id)
            .ok_or_else(|| McpError::invalid_params(format!("guideline not found: {rule_id}"), None))?;

        let json = serde_json::to_string_pretty(guideline)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "List all C++ Core Guidelines in a specific category. Use category prefixes like 'P' (Philosophy), 'R' (Resource management), 'ES' (Expressions), 'SL' (Standard Library), etc.")]
    async fn list_category(
        &self,
        Parameters(params): Parameters<ListCategoryParams>,
    ) -> Result<CallToolResult, McpError> {
        let category_prefix = params.category.trim().to_string();
        if category_prefix.is_empty() {
            return Err(McpError::invalid_params("category must not be empty", None));
        }

        let state = self.state.read().await;
        let category = state
            .categories
            .get(&category_prefix)
            .ok_or_else(|| {
                let available: Vec<&str> = state.categories.keys().map(|s| s.as_str()).collect();
                McpError::invalid_params(
                    format!(
                        "unknown category: '{category_prefix}'. Available categories: {}",
                        available.join(", ")
                    ),
                    None,
                )
            })?
            .clone();

        let mut guideline_summaries: Vec<GuidelineSummary> = state
            .guidelines
            .values()
            .filter(|g| g.category == category_prefix)
            .map(|g| GuidelineSummary {
                id: g.id.clone(),
                title: g.title.clone(),
            })
            .collect();
        guideline_summaries.sort_by(|a, b| a.id.cmp(&b.id));

        let response = CategoryListResponse {
            category,
            guidelines: guideline_summaries,
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Trigger a re-index of the C++ Core Guidelines from the git repository. Checks for updates and re-parses/re-embeds if the content has changed.")]
    async fn update_guidelines(&self) -> Result<CallToolResult, McpError> {
        info!("update_guidelines tool invoked");

        let (result, new_data) = self
            .update_service
            .update()
            .await
            .map_err(|e| McpError::internal_error(format!("update failed: {e}"), None))?;

        // If re-indexed, update the in-memory state
        if let Some((guidelines, categories)) = new_data {
            let guideline_count = guidelines.len();
            let guideline_map: HashMap<String, Guideline> = guidelines
                .into_iter()
                .map(|g| (g.id.clone(), g))
                .collect();

            let mut state = self.state.write().await;
            state.guidelines = guideline_map;
            state.categories = categories;
            info!(guideline_count, "in-memory state updated");
        }

        let response = UpdateResponse {
            updated: result.updated,
            commit: result.commit,
            guideline_count: if result.updated {
                result.guideline_count
            } else {
                let state = self.state.read().await;
                state.guidelines.len()
            },
        };

        let json = serde_json::to_string_pretty(&response)
            .map_err(|e| McpError::internal_error(format!("serialization failed: {e}"), None))?;

        Ok(CallToolResult::success(vec![Content::text(json)]))
    }
}

#[tool_handler]
impl ServerHandler for CppGuidelinesServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder()
                .enable_tools()
                .build(),
            server_info: Implementation {
                name: "cpp-guidelines".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                title: None,
                icons: None,
                website_url: None,
            },
            instructions: Some(
                "C++ Core Guidelines MCP server. Provides semantic search and lookup \
                 over the C++ Core Guidelines (~513 rules). Use search_guidelines for \
                 natural language queries, get_guideline for specific rule lookup by ID, \
                 list_category for browsing by category, and update_guidelines to \
                 refresh from the repository."
                    .to_string(),
            ),
        }
    }
}
