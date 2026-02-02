use std::path::{Path, PathBuf};

use crate::error::AppError;

/// Application configuration loaded explicitly from environment variables.
#[derive(Debug, Clone)]
pub struct Config {
    /// Redis connection URL (e.g. "redis://127.0.0.1:6379"). `None` disables caching.
    pub redis_url: Option<String>,
    /// Filesystem path to the LanceDB data directory.
    pub lancedb_path: String,
    /// Filesystem path to the cloned Rust API Guidelines repository.
    pub repo_path: String,
}

impl Config {
    /// Required:
    /// - `LANCEDB_PATH`: path to LanceDB data directory
    /// - `RUST_API_GUIDELINES_REPO_PATH`: path to the cloned rust-lang/api-guidelines repo
    ///
    /// Optional:
    /// - `REDIS_URL`: Redis connection string
    pub fn from_env() -> Result<Self, AppError> {
        let lancedb_path = std::env::var("LANCEDB_PATH").map_err(|_| {
            AppError::Config("LANCEDB_PATH environment variable is required".to_string())
        })?;

        let repo_path = std::env::var("RUST_API_GUIDELINES_REPO_PATH").map_err(|_| {
            AppError::Config(
                "RUST_API_GUIDELINES_REPO_PATH environment variable is required".to_string(),
            )
        })?;

        let required = [
            "src/checklist.md",
            "src/SUMMARY.md",
            "src/naming.md",
            "src/documentation.md",
        ];

        for rel in required {
            let file = Path::new(&repo_path).join(rel);
            if !file.exists() {
                return Err(AppError::Config(format!("required file not found: {}", file.display())));
            }
        }

        Ok(Self {
            redis_url: std::env::var("REDIS_URL").ok(),
            lancedb_path,
            repo_path,
        })
    }

    pub fn repo_path(&self) -> PathBuf {
        Path::new(&self.repo_path).to_path_buf()
    }
}
