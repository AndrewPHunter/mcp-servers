/// LanceDB vector database wrapper.
///
/// Provides a typed interface over LanceDB for storing and searching vector embeddings.
/// The table schema is:
/// - id: Utf8 (not null)
/// - title: Utf8 (not null)
/// - category: Utf8 (not null)
/// - text: Utf8 (not null) — the text that was embedded
/// - embedding: FixedSizeList<Float32, 768> (not null)
use std::sync::Arc;

use arrow_array::{RecordBatch, RecordBatchIterator};
use arrow_schema::Schema;
use lancedb::query::{ExecutableQuery, QueryBase};
use tracing::info;

use crate::error::CommonError;

pub struct VectorDb {
    db: lancedb::Connection,
}

impl VectorDb {
    /// Connect to a LanceDB database at the given filesystem path.
    pub async fn connect(path: &str) -> Result<Self, CommonError> {
        let db = lancedb::connect(path)
            .execute()
            .await
            .map_err(|e| CommonError::VectorDb(format!("connection failed: {e}")))?;
        Ok(Self { db })
    }

    /// Create or replace a table with the given schema and data.
    ///
    /// This drops the existing table (if any) and creates a fresh one.
    /// Acceptable for ~513 records where re-indexing is cheap.
    pub async fn create_or_replace_table(
        &self,
        table_name: &str,
        schema: Arc<Schema>,
        batches: Vec<RecordBatch>,
    ) -> Result<(), CommonError> {
        // Drop existing table if present (ignore errors — table may not exist)
        let _ = self.db.drop_table(table_name).await;

        let batch_iter = RecordBatchIterator::new(batches.into_iter().map(Ok), schema);
        self.db
            .create_table(table_name, Box::new(batch_iter))
            .execute()
            .await
            .map_err(|e| CommonError::VectorDb(format!("create table failed: {e}")))?;

        info!(table = table_name, "vector table created");
        Ok(())
    }

    /// Search for the nearest vectors to the given query embedding.
    ///
    /// Returns up to `limit` results as RecordBatches, including a `_distance` column
    /// added by LanceDB.
    pub async fn search(
        &self,
        table_name: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<RecordBatch>, CommonError> {
        let table = self
            .db
            .open_table(table_name)
            .execute()
            .await
            .map_err(|e| CommonError::VectorDb(format!("open table failed: {e}")))?;

        let results = table
            .vector_search(query_embedding)
            .map_err(|e| CommonError::VectorDb(format!("vector search setup failed: {e}")))?
            .limit(limit)
            .execute()
            .await
            .map_err(|e| CommonError::VectorDb(format!("vector search failed: {e}")))?;

        futures::TryStreamExt::try_collect(results)
            .await
            .map_err(|e| CommonError::VectorDb(format!("collecting search results failed: {e}")))
    }

    /// Look up a single row by its `id` column value.
    ///
    /// Returns `None` if the id is not found. Returns the first match if multiple exist.
    pub async fn get_by_id(
        &self,
        table_name: &str,
        id: &str,
    ) -> Result<Option<RecordBatch>, CommonError> {
        let table = self
            .db
            .open_table(table_name)
            .execute()
            .await
            .map_err(|e| CommonError::VectorDb(format!("open table failed: {e}")))?;

        // Use a SQL filter to find the row by id.
        // LanceDB uses DataFusion SQL syntax for filters.
        let filter = format!("id = '{}'", id.replace('\'', "''"));
        let results = table
            .query()
            .only_if(filter)
            .limit(1)
            .execute()
            .await
            .map_err(|e| CommonError::VectorDb(format!("query by id failed: {e}")))?;

        let batches: Vec<RecordBatch> = futures::TryStreamExt::try_collect(results)
            .await
            .map_err(|e| CommonError::VectorDb(format!("collecting query results failed: {e}")))?;

        Ok(batches.into_iter().next().filter(|b| b.num_rows() > 0))
    }
}
