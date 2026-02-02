/// Error types shared across MCP server crates.
///
/// These errors represent failures in infrastructure components (Redis, vector DB, embeddings)
/// that are common to multiple MCP servers. Application-specific errors should be defined
/// in each server crate and wrap `CommonError` via `#[from]`.

#[derive(Debug, thiserror::Error)]
pub enum CommonError {
    #[error("redis error: {0}")]
    Redis(#[from] redis::RedisError),

    #[error("redis unavailable, degrading gracefully")]
    RedisUnavailable,

    #[error("vector db error: {0}")]
    VectorDb(String),

    #[error("embedding error: {0}")]
    Embedding(String),
}
