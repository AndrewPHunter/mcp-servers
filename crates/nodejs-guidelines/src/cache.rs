/// Redis caching layer for the Node.js Best Practices server.
///
/// All operations return `Option<T>` for graceful degradation.
///
/// Key schema:
/// - `njg:v1:guideline:{id}` — JSON Guideline
/// - `njg:v1:search:{sha256(query|limit)}` — JSON Vec<GuidelineResult> (TTL 3600s)
/// - `njg:v1:categories` — JSON Vec<Category>
/// - `njg:v1:category:{key}` — JSON Vec<String> of guideline IDs
/// - `njg:v1:repo_commit` — Git commit hash string
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::model::{Category, Guideline, GuidelineResult};
use mcp_common::redis::RedisCache;

const KEY_PREFIX: &str = "njg:v1:";
const SEARCH_TTL_SECS: u64 = 3600;

pub struct GuidelineCache {
    redis: RedisCache,
}

impl GuidelineCache {
    pub fn new(redis: RedisCache) -> Self {
        Self { redis }
    }

    pub async fn get_guideline(&self, id: &str) -> Option<Guideline> {
        let key = format!("{KEY_PREFIX}guideline:{id}");
        let json = self.redis.get(&key).await?;
        serde_json::from_str(&json)
            .inspect_err(|e| warn!(error = %e, key, "cache deserialization failed"))
            .ok()
    }

    pub async fn set_guideline(&self, guideline: &Guideline) {
        let key = format!("{KEY_PREFIX}guideline:{}", guideline.id);
        if let Ok(json) = serde_json::to_string(guideline) {
            self.redis.set(&key, &json).await;
        }
    }

    pub async fn get_search_results(
        &self,
        query: &str,
        limit: usize,
    ) -> Option<Vec<GuidelineResult>> {
        let key = search_key(query, limit);
        let json = self.redis.get(&key).await?;
        serde_json::from_str(&json)
            .inspect_err(|e| warn!(error = %e, key, "cache deserialization failed"))
            .ok()
    }

    pub async fn set_search_results(&self, query: &str, limit: usize, results: &[GuidelineResult]) {
        let key = search_key(query, limit);
        if let Ok(json) = serde_json::to_string(results) {
            self.redis.set_with_ttl(&key, &json, SEARCH_TTL_SECS).await;
        }
    }

    pub async fn get_categories(&self) -> Option<Vec<Category>> {
        let key = format!("{KEY_PREFIX}categories");
        let json = self.redis.get(&key).await?;
        serde_json::from_str(&json)
            .inspect_err(|e| warn!(error = %e, key, "cache deserialization failed"))
            .ok()
    }

    pub async fn set_categories(&self, categories: &[Category]) {
        let key = format!("{KEY_PREFIX}categories");
        if let Ok(json) = serde_json::to_string(categories) {
            self.redis.set(&key, &json).await;
        }
    }

    pub async fn get_category_guideline_ids(&self, key_name: &str) -> Option<Vec<String>> {
        let key = format!("{KEY_PREFIX}category:{key_name}");
        let json = self.redis.get(&key).await?;
        serde_json::from_str(&json)
            .inspect_err(|e| warn!(error = %e, key, "cache deserialization failed"))
            .ok()
    }

    pub async fn set_category_guideline_ids(&self, key_name: &str, ids: &[String]) {
        let key = format!("{KEY_PREFIX}category:{key_name}");
        if let Ok(json) = serde_json::to_string(ids) {
            self.redis.set(&key, &json).await;
        }
    }

    pub async fn get_repo_commit(&self) -> Option<String> {
        let key = format!("{KEY_PREFIX}repo_commit");
        self.redis.get(&key).await
    }

    pub async fn set_repo_commit(&self, commit: &str) {
        let key = format!("{KEY_PREFIX}repo_commit");
        self.redis.set(&key, commit).await;
    }

    pub async fn invalidate_all(&self) {
        self.redis.delete_by_prefix(KEY_PREFIX).await;
    }
}

fn search_key(query: &str, limit: usize) -> String {
    let mut hasher = Sha256::new();
    hasher.update(query.as_bytes());
    hasher.update(b"|");
    hasher.update(limit.to_string().as_bytes());
    let hash = hasher.finalize();
    format!("{KEY_PREFIX}search:{:x}", hash)
}

