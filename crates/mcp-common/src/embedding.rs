/// Embedding wrapper around fastembed.
///
/// `TextEmbedding` from fastembed is synchronous and CPU-bound. All embed calls go through
/// `tokio::task::spawn_blocking`. The `Embedder` is `!Send` due to the inner ONNX runtime,
/// so it is wrapped in `Arc` and accessed only from blocking tasks.
///
/// The nomic-embed-text-v1.5 model uses task-prefixed inputs:
/// - Documents: "search_document: {text}"
/// - Queries: "search_query: {text}"
use std::sync::Arc;

use crate::error::CommonError;

/// Wraps fastembed's `TextEmbedding` model for generating vector embeddings.
///
/// The inner model is not `Send`, so all operations are dispatched to a blocking thread.
pub struct Embedder {
    model: Arc<fastembed::TextEmbedding>,
}

impl Embedder {
    /// Initialize the embedding model (nomic-embed-text-v1.5).
    ///
    /// This downloads the model on first run (~300MB). The download happens synchronously
    /// inside a blocking task.
    pub async fn new() -> Result<Self, CommonError> {
        let model = tokio::task::spawn_blocking(|| {
            let options = fastembed::InitOptions::new(fastembed::EmbeddingModel::NomicEmbedTextV15)
                .with_show_download_progress(true);
            fastembed::TextEmbedding::try_new(options)
        })
        .await
        .map_err(|e| CommonError::Embedding(format!("spawn_blocking join error: {e}")))?
        .map_err(|e| CommonError::Embedding(format!("model initialization failed: {e}")))?;

        Ok(Self {
            model: Arc::new(model),
        })
    }

    /// Embed documents for indexing.
    ///
    /// The nomic-embed-text model expects document inputs prefixed with "search_document: ".
    /// This method adds the prefix automatically.
    ///
    /// Documents are processed in small batches to bound peak memory during ONNX inference.
    pub async fn embed_documents(&self, texts: &[String]) -> Result<Vec<Vec<f32>>, CommonError> {
        let prefixed: Vec<String> = texts
            .iter()
            .map(|t| format!("search_document: {t}"))
            .collect();
        let model = Arc::clone(&self.model);
        tokio::task::spawn_blocking(move || model.embed(prefixed, Some(4)))
            .await
            .map_err(|e| CommonError::Embedding(format!("spawn_blocking join error: {e}")))?
            .map_err(|e| CommonError::Embedding(format!("document embedding failed: {e}")))
    }

    /// Embed a single query for search.
    ///
    /// The nomic-embed-text model expects query inputs prefixed with "search_query: ".
    /// This method adds the prefix automatically.
    pub async fn embed_query(&self, query: &str) -> Result<Vec<f32>, CommonError> {
        let prefixed = vec![format!("search_query: {query}")];
        let model = Arc::clone(&self.model);
        let mut results =
            tokio::task::spawn_blocking(move || model.embed(prefixed, None))
                .await
                .map_err(|e| CommonError::Embedding(format!("spawn_blocking join error: {e}")))?
                .map_err(|e| CommonError::Embedding(format!("query embedding failed: {e}")))?;
        results
            .pop()
            .ok_or_else(|| CommonError::Embedding("empty embedding result".to_string()))
    }

    /// Returns the dimensionality of the embedding vectors (768 for nomic-embed-text-v1.5).
    pub fn dimensions(&self) -> usize {
        768
    }
}
