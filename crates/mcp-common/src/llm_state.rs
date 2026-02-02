use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::openai::{ChatCompletionUsage, Message};
use crate::redis::RedisCache;

static CONVO_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct UsageStats {
    pub models: Vec<ModelUsageStats>,
    pub redis_available: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelUsageStats {
    pub model: String,
    pub requests: u64,
    pub total_tokens: Option<u64>,
    pub token_counted_requests: u64,
    pub token_unknown_requests: u64,
}

#[derive(Clone)]
pub struct UsageTracker {
    redis: RedisCache,
}

impl UsageTracker {
    pub fn new(redis: RedisCache) -> Self {
        Self { redis }
    }

    pub async fn record(&self, model: &str, usage: Option<&ChatCompletionUsage>) {
        let _ = self
            .redis
            .hincr_by("llm_proxy:usage", &format!("requests:{model}"), 1)
            .await;

        match usage.and_then(|u| u.total_tokens) {
            Some(total) => {
                let _ = self
                    .redis
                    .hincr_by("llm_proxy:usage", &format!("tokens_total:{model}"), total as i64)
                    .await;
                let _ = self
                    .redis
                    .hincr_by("llm_proxy:usage", &format!("tokens_known_requests:{model}"), 1)
                    .await;
            }
            None => {
                let _ = self
                    .redis
                    .hincr_by("llm_proxy:usage", &format!("tokens_unknown_requests:{model}"), 1)
                    .await;
            }
        }
    }

    pub async fn get_usage_stats(&self) -> UsageStats {
        let redis_available = self.redis.is_available().await;
        let Some(entries) = self.redis.hgetall("llm_proxy:usage").await else {
            return UsageStats {
                models: vec![],
                redis_available,
            };
        };

        let mut by_model: std::collections::HashMap<String, ModelUsageStats> =
            std::collections::HashMap::new();

        for (field, value) in entries {
            let Some((kind, model)) = field.split_once(':') else {
                continue;
            };
            let stat = by_model.entry(model.to_string()).or_insert(ModelUsageStats {
                model: model.to_string(),
                requests: 0,
                total_tokens: None,
                token_counted_requests: 0,
                token_unknown_requests: 0,
            });

            let parsed = value.parse::<u64>().unwrap_or(0);
            match kind {
                "requests" => stat.requests = parsed,
                "tokens_total" => stat.total_tokens = Some(parsed),
                "tokens_known_requests" => stat.token_counted_requests = parsed,
                "tokens_unknown_requests" => stat.token_unknown_requests = parsed,
                _ => {}
            }
        }

        let mut models: Vec<ModelUsageStats> = by_model.into_values().collect();
        models.sort_by(|a, b| a.model.cmp(&b.model));
        UsageStats {
            models,
            redis_available,
        }
    }
}

pub type ConversationId = String;

#[derive(Clone)]
pub struct ConversationStore {
    redis: RedisCache,
    ttl_secs: u64,
}

impl ConversationStore {
    pub fn new(redis: RedisCache) -> Self {
        let ttl_secs = std::env::var("CONVO_TTL_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(86_400);
        Self { redis, ttl_secs }
    }

    pub fn ttl(&self) -> Duration {
        Duration::from_secs(self.ttl_secs)
    }

    pub async fn start(&self) -> ConversationId {
        let id = new_conversation_id();
        let _ = self
            .redis
            .set_with_ttl(&convo_key(&id), "[]", self.ttl_secs)
            .await;
        id
    }

    pub async fn end(&self, conversation_id: &str) {
        let _ = self.redis.delete(&convo_key(conversation_id)).await;
    }

    pub async fn get_messages(&self, conversation_id: &str) -> Option<Vec<Message>> {
        let raw = self.redis.get(&convo_key(conversation_id)).await?;
        serde_json::from_str::<Vec<Message>>(&raw).ok()
    }

    pub async fn set_messages(&self, conversation_id: &str, messages: &[Message]) -> bool {
        let Ok(raw) = serde_json::to_string(messages) else {
            return false;
        };
        self.redis
            .set_with_ttl(&convo_key(conversation_id), &raw, self.ttl_secs)
            .await
    }
}

fn convo_key(conversation_id: &str) -> String {
    format!("llm_proxy:convo:{conversation_id}")
}

fn new_conversation_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let counter = CONVO_COUNTER.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();

    let mut h = Sha256::new();
    h.update(now.as_nanos().to_le_bytes());
    h.update(pid.to_le_bytes());
    h.update(counter.to_le_bytes());
    let digest = h.finalize();
    hex_lower(&digest[..16])
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}
