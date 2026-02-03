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
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use cache::GuidelineCache;
use config::Config;
use server::NodejsGuidelinesServer;
use update::UpdateService;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    info!("starting nodejs-guidelines MCP server");

    let config = Config::from_env()?;
    info!(
        repo_path = %config.repo_path,
        lancedb_path = %config.lancedb_path,
        redis = config.redis_url.is_some(),
        "configuration loaded"
    );

    let redis_cache = mcp_common::redis::RedisCache::new(config.redis_url.as_deref());
    if redis_cache.is_available().await {
        info!("redis connected");
    } else {
        info!("redis unavailable, running without cache");
    }
    let cache = Arc::new(GuidelineCache::new(redis_cache));

    info!("initializing embedding model (may download on first run)");
    let embedder = Arc::new(mcp_common::embedding::Embedder::new().await?);
    info!("embedding model ready");

    let vectordb = Arc::new(mcp_common::vectordb::VectorDb::connect(&config.lancedb_path).await?);
    info!("lancedb connected");

    let update_service = UpdateService::new(
        config.clone(),
        Arc::clone(&embedder),
        Arc::clone(&vectordb),
        Arc::clone(&cache),
    );

    let (guidelines, categories) = if update_service.needs_update().await? {
        info!("indexing nodejs best practices (first run or content changed)");
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
        let (guidelines, categories) = parser::parse_guidelines_repo(&config.repo_path())?;
        info!(
            guidelines = guidelines.len(),
            categories = categories.len(),
            "loaded guidelines from source"
        );
        (guidelines, categories)
    };

    let server = NodejsGuidelinesServer::new(
        guidelines,
        categories,
        embedder,
        vectordb,
        cache,
        config,
    );

    if let Ok(addr) = std::env::var("MCP_TCP_LISTEN_ADDR") {
        let listener = TcpListener::bind(&addr).await?;
        info!(listen_addr = %addr, "MCP server ready, serving on TCP");
        loop {
            let (stream, peer) = listener.accept().await?;
            let server = server.clone();
            tokio::spawn(async move {
                tracing::info!(peer = %peer, "MCP client connected");
                let service = server.serve(stream).await.inspect_err(|e| {
                    tracing::error!(error = %e, "MCP server error");
                })?;
                service.waiting().await?;
                tracing::info!(peer = %peer, "MCP client disconnected");
                Ok::<(), anyhow::Error>(())
            });
        }
    } else {
        info!("MCP server ready, serving on stdio");
        let service = server.serve(stdio()).await.inspect_err(|e| {
            tracing::error!(error = %e, "MCP server error");
        })?;
        service.waiting().await?;
        info!("MCP server shut down");
    }
    Ok(())
}
