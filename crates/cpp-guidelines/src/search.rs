/// Search engine for C++ Core Guidelines.
///
/// Embeds a query using the fastembed model, performs vector search in LanceDB,
/// and formats results. Caches search results in Redis when available.
use std::sync::Arc;

use arrow_array::{Array, Float32Array, RecordBatch, StringArray};
use tracing::{info, warn};

use crate::cache::GuidelineCache;
use crate::model::GuidelineResult;
use mcp_common::embedding::Embedder;
use mcp_common::vectordb::VectorDb;

const VECTOR_TABLE_NAME: &str = "guidelines";
const MAX_SUMMARY_LEN: usize = 300;

pub struct SearchEngine {
    embedder: Arc<Embedder>,
    vectordb: Arc<VectorDb>,
    cache: Arc<GuidelineCache>,
}

impl SearchEngine {
    pub fn new(
        embedder: Arc<Embedder>,
        vectordb: Arc<VectorDb>,
        cache: Arc<GuidelineCache>,
    ) -> Self {
        Self {
            embedder,
            vectordb,
            cache,
        }
    }

    /// Search guidelines by semantic similarity to the query.
    ///
    /// Returns up to `limit` results, ranked by similarity (lowest distance first).
    /// Results are cached in Redis for subsequent identical queries.
    pub async fn search(
        &self,
        query: &str,
        limit: usize,
    ) -> Result<Vec<GuidelineResult>, crate::error::AppError> {
        // Check cache first
        if let Some(cached) = self.cache.get_search_results(query, limit).await {
            info!(query, "search cache hit");
            return Ok(cached);
        }

        // Embed the query
        let query_embedding = self.embedder.embed_query(query).await?;

        // Vector search
        let batches = self
            .vectordb
            .search(VECTOR_TABLE_NAME, &query_embedding, limit)
            .await?;

        // Extract results from record batches
        let results = extract_search_results(&batches);

        // Cache the results (fire-and-forget, don't block on cache write)
        self.cache.set_search_results(query, limit, &results).await;

        Ok(results)
    }

    /// Returns the LanceDB table name used for guidelines.
    pub fn table_name() -> &'static str {
        VECTOR_TABLE_NAME
    }
}

/// Extract `GuidelineResult` values from LanceDB search result batches.
///
/// Expected columns: id (Utf8), title (Utf8), category (Utf8), text (Utf8), _distance (Float32)
fn extract_search_results(batches: &[RecordBatch]) -> Vec<GuidelineResult> {
    let mut results = Vec::new();

    for batch in batches {
        let num_rows = batch.num_rows();
        let schema = batch.schema();

        let id_col: Option<&StringArray> = get_string_column(batch, &schema, "id");
        let title_col: Option<&StringArray> = get_string_column(batch, &schema, "title");
        let category_col: Option<&StringArray> = get_string_column(batch, &schema, "category");
        let text_col: Option<&StringArray> = get_string_column(batch, &schema, "text");
        let distance_col: Option<&Float32Array> = get_float_column(batch, &schema, "_distance");

        let (Some(id_col), Some(title_col), Some(category_col), Some(text_col)) =
            (id_col, title_col, category_col, text_col)
        else {
            warn!("search result batch missing expected columns");
            continue;
        };

        for row in 0..num_rows {
            let id = id_col.value(row).to_string();
            let title = title_col.value(row).to_string();
            let category = category_col.value(row).to_string();
            let text = text_col.value(row);
            let distance: f32 = distance_col.map(|c| c.value(row)).unwrap_or(0.0);

            // Convert distance to a similarity score (1.0 - normalized distance).
            // LanceDB returns L2 distance by default; lower is more similar.
            // We invert so higher score = more similar, clamped to [0, 1].
            let score: f32 = (1.0_f32 - distance).max(0.0);

            let summary = if text.chars().count() > MAX_SUMMARY_LEN {
                format!("{}...", text.chars().take(MAX_SUMMARY_LEN).collect::<String>())
            } else {
                text.to_string()
            };

            results.push(GuidelineResult {
                id,
                title,
                category,
                score,
                summary,
            });
        }
    }

    results
}

fn get_string_column<'a>(
    batch: &'a RecordBatch,
    schema: &arrow_schema::Schema,
    name: &str,
) -> Option<&'a StringArray> {
    let idx = schema.index_of(name).ok()?;
    batch.column(idx).as_any().downcast_ref::<StringArray>()
}

fn get_float_column<'a>(
    batch: &'a RecordBatch,
    schema: &arrow_schema::Schema,
    name: &str,
) -> Option<&'a Float32Array> {
    let idx = schema.index_of(name).ok()?;
    batch.column(idx).as_any().downcast_ref::<Float32Array>()
}
