mod cache;
mod config;
mod error;
mod model;
mod parser;
mod search;
mod server;
mod update;

use std::sync::Arc;

use rmcp::{ServiceExt, transport::stdio};
use tracing::info;
use tracing_subscriber::EnvFilter;

use cache::GuidelineCache;
use config::Config;
use server::CppGuidelinesServer;
use update::UpdateService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing to stderr (stdout is reserved for MCP JSON-RPC)
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    info!("starting cpp-guidelines MCP server");

    // 1. Load config from environment
    let config = Config::from_env()?;
    info!(
        repo_path = %config.repo_path,
        lancedb_path = %config.lancedb_path,
        redis = config.redis_url.is_some(),
        "configuration loaded"
    );

    // 2. Connect to Redis (optional — graceful degradation if unavailable)
    let redis_cache = mcp_common::redis::RedisCache::new(config.redis_url.as_deref());
    if redis_cache.is_available().await {
        info!("redis connected");
    } else {
        info!("redis unavailable, running without cache");
    }
    let cache = Arc::new(GuidelineCache::new(redis_cache));

    // 3. Initialize embedding model
    info!("initializing embedding model (may download on first run)");
    let embedder = Arc::new(mcp_common::embedding::Embedder::new().await?);
    info!("embedding model ready");

    // 4. Connect to LanceDB
    let vectordb = Arc::new(mcp_common::vectordb::VectorDb::connect(&config.lancedb_path).await?);
    info!("lancedb connected");

    // 5. Check if re-indexing is needed and perform it
    let update_service = UpdateService::new(
        config.clone(),
        Arc::clone(&embedder),
        Arc::clone(&vectordb),
        Arc::clone(&cache),
    );

    let (guidelines, categories) = if update_service.needs_update().await? {
        info!("indexing guidelines (first run or content changed)");
        let (guidelines, categories, commit) = update_service.full_reindex().await?;
        info!(
            commit = %commit,
            guidelines = guidelines.len(),
            categories = categories.len(),
            "indexing complete"
        );
        (guidelines, categories)
    } else {
        info!("guidelines up to date, loading from source");
        // Parse from source file (LanceDB table already populated from prior run)
        let content = std::fs::read_to_string(config.guidelines_file_path())?;
        let (guidelines, categories) = parser::parse_guidelines(&content);
        info!(
            guidelines = guidelines.len(),
            categories = categories.len(),
            "loaded guidelines from source"
        );
        (guidelines, categories)
    };

    // 6. Build MCP server and serve on stdio
    let server = CppGuidelinesServer::new(
        guidelines,
        categories,
        embedder,
        vectordb,
        cache,
        config,
    );

    info!("MCP server ready, serving on stdio");
    let service = server.serve(stdio()).await.inspect_err(|e| {
        tracing::error!(error = %e, "MCP server error");
    })?;

    service.waiting().await?;
    info!("MCP server shut down");
    Ok(())
}
