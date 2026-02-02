#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("config error: {0}")]
    Config(String),

    #[error("parse error at line {line}: {message}")]
    Parse { line: usize, message: String },

    #[error("git error: {0}")]
    Git(String),

    #[error(transparent)]
    Common(#[from] mcp_common::error::CommonError),
}

