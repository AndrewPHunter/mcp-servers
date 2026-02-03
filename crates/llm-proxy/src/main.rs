mod rate_limit;
mod server;

use std::sync::Arc;

use rmcp::{ServiceExt, transport::stdio};
use rmcp::transport::streamable_http_server::{
    StreamableHttpService, session::local::LocalSessionManager,
};
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use mcp_common::llm_state::{ConversationStore, UsageTracker};
use mcp_common::openai::{OpenAiClient, OpenAiClientConfig};
use mcp_common::redis::RedisCache;

use server::LlmProxyServer;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()),
        )
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .init();

    info!("starting llm-proxy MCP server");

    let openai_config = OpenAiClientConfig::from_env();
    info!(
        base_url = %openai_config.base_url,
        timeout_ms = openai_config.default_timeout.as_millis(),
        max_retries = openai_config.max_retries,
        "openai client configured"
    );
    let openai = Arc::new(OpenAiClient::new(openai_config)?);

    let redis_url = std::env::var("REDIS_URL").ok();
    let redis_cache = RedisCache::new(redis_url.as_deref());
    if redis_cache.is_available().await {
        info!("redis connected");
    } else {
        info!("redis unavailable, running without redis state");
    }

    let convos = ConversationStore::new(RedisCache::new(redis_url.as_deref()));
    let usage = UsageTracker::new(RedisCache::new(redis_url.as_deref()));

    let limiter = rate_limit::RateLimiter::from_env();

    let server = LlmProxyServer::new(openai, convos, usage, limiter);

    if let Ok(addr) = std::env::var("MCP_LISTEN_ADDR") {
        let server_for_factory = server.clone();
        let http_service = StreamableHttpService::new(
            move || Ok(server_for_factory.clone()),
            LocalSessionManager::default().into(),
            Default::default(),
        );
        let router = axum::Router::new().fallback_service(http_service);
        let listener = TcpListener::bind(&addr).await?;
        info!(listen_addr = %addr, "MCP server ready, serving HTTP/SSE");
        axum::serve(listener, router).await?;
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
