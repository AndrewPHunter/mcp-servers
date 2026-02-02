use mcp_common::error::CommonError;

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error(transparent)]
    Common(#[from] CommonError),

    #[error("parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("git error: {0}")]
    Git(String),

    #[error("config error: {0}")]
    Config(String),

    #[error("guideline not found: {0}")]
    NotFound(String),

    #[error("unknown category: {0}")]
    UnknownCategory(String),
}
