use std::sync::Arc;

use arrow_array::{Array, Float32Array, RecordBatch, StringArray};
use tracing::{info, warn};

use crate::cache::GuidelineCache;
use crate::model::GuidelineResult;
use mcp_common::embedding::Embedder;
use mcp_common::vectordb::VectorDb;

const VECTOR_TABLE_NAME: &str = "nodejs_guidelines";
const MAX_SUMMARY_LEN: usize = 300;

pub struct SearchEngine {
    embedder: Arc<Embedder>,
    vectordb: Arc<VectorDb>,
    cache: Arc<GuidelineCache>,
}

impl SearchEngine {
    pub fn new(embedder: Arc<Embedder>, vectordb: Arc<VectorDb>, cache: Arc<GuidelineCache>) -> Self {
        Self {
            embedder,
            vectordb,
            cache,
        }
    }

    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<GuidelineResult>, crate::error::AppError> {
        if let Some(cached) = self.cache.get_search_results(query, limit).await {
            info!(query, "search cache hit");
            return Ok(cached);
        }

        let query_embedding = self.embedder.embed_query(query).await?;
        let batches = self
            .vectordb
            .search(VECTOR_TABLE_NAME, &query_embedding, limit)
            .await?;

        let results = extract_search_results(&batches);
        self.cache.set_search_results(query, limit, &results).await;
        Ok(results)
    }

    pub fn table_name() -> &'static str {
        VECTOR_TABLE_NAME
    }
}

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
            let text = text_col.value(row);
            let summary = if text.chars().count() > MAX_SUMMARY_LEN {
                format!("{}...", text.chars().take(MAX_SUMMARY_LEN).collect::<String>())
            } else {
                text.to_string()
            };

            let distance = distance_col.map(|c| c.value(row)).unwrap_or(0.0);
            let score = (1.0_f32 - distance).max(0.0);

            results.push(GuidelineResult {
                id: id_col.value(row).to_string(),
                title: title_col.value(row).to_string(),
                category: category_col.value(row).to_string(),
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

