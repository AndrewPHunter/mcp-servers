use std::time::{Duration, SystemTime, UNIX_EPOCH};

use futures::StreamExt;
use reqwest::StatusCode;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use tracing::warn;

#[derive(Clone, Debug)]
pub struct OpenAiClientConfig {
    pub base_url: String,
    pub default_timeout: Duration,
    pub max_retries: u32,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
    pub max_error_body_bytes: usize,
}

impl OpenAiClientConfig {
    pub fn from_env() -> Self {
        let base_url =
            std::env::var("OPENAI_BASE_URL").unwrap_or_else(|_| "http://ai:8001/v1".to_string());

        let default_timeout = std::env::var("OPENAI_TIMEOUT_SECS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_secs)
            .unwrap_or_else(|| Duration::from_secs(30));

        let max_retries = std::env::var("OPENAI_MAX_RETRIES")
            .ok()
            .and_then(|s| s.parse::<u32>().ok())
            .unwrap_or(3);

        let initial_backoff = std::env::var("OPENAI_RETRY_INITIAL_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(200));

        let max_backoff = std::env::var("OPENAI_RETRY_MAX_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .map(Duration::from_millis)
            .unwrap_or_else(|| Duration::from_millis(5_000));

        let max_error_body_bytes = std::env::var("OPENAI_MAX_ERROR_BODY_BYTES")
            .ok()
            .and_then(|s| s.parse::<usize>().ok())
            .unwrap_or(8 * 1024);

        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            default_timeout,
            max_retries,
            initial_backoff,
            max_backoff,
            max_error_body_bytes,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum OpenAiClientError {
    #[error("request failed: {0}")]
    Request(#[from] reqwest::Error),

    #[error("invalid response JSON: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("upstream returned error: status={status} message={message}")]
    Upstream { status: StatusCode, message: String },

    #[error("upstream returned non-JSON error: status={status} body={body}")]
    UpstreamBody { status: StatusCode, body: String },

    #[error("streaming response ended without a completion")]
    StreamEnded,
}

#[derive(Clone)]
pub struct OpenAiClient {
    config: OpenAiClientConfig,
    http: reqwest::Client,
}

impl OpenAiClient {
    pub fn new(config: OpenAiClientConfig) -> Result<Self, OpenAiClientError> {
        let http = reqwest::Client::builder()
            .user_agent("mcp-servers/llm-proxy")
            .build()?;
        Ok(Self { config, http })
    }

    pub fn config(&self) -> &OpenAiClientConfig {
        &self.config
    }

    pub async fn list_models(&self) -> Result<ModelListResponse, OpenAiClientError> {
        let url = format!("{}/models", self.config.base_url);
        self.request_with_retry(|| async {
            let resp = self.http.get(&url).timeout(self.config.default_timeout).send().await?;
            Self::parse_json_response(resp, self.config.max_error_body_bytes).await
        })
        .await
    }

    pub async fn chat_completions(
        &self,
        request: ChatCompletionRequest,
        timeout_override: Option<Duration>,
    ) -> Result<ChatCompletionResponse, OpenAiClientError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let timeout = timeout_override.unwrap_or(self.config.default_timeout);
        self.request_with_retry(|| {
            let req = request.clone();
            let url = url.clone();
            async move {
                let resp = self
                    .http
                    .post(&url)
                    .timeout(timeout)
                    .json(&req)
                    .send()
                    .await?;
                Self::parse_json_response(resp, self.config.max_error_body_bytes).await
            }
        })
        .await
    }

    pub async fn chat_completions_streaming_aggregate(
        &self,
        request: ChatCompletionRequest,
        timeout_override: Option<Duration>,
    ) -> Result<String, OpenAiClientError> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let timeout = timeout_override.unwrap_or(self.config.default_timeout);
        self.request_with_retry(|| {
            let mut req = request.clone();
            req.stream = Some(true);
            let url = url.clone();
            async move {
                let resp = self
                    .http
                    .post(&url)
                    .timeout(timeout)
                    .json(&req)
                    .send()
                    .await?;

                if !resp.status().is_success() {
                    return Err(Self::to_upstream_error(resp, self.config.max_error_body_bytes).await);
                }

                let mut stream = resp.bytes_stream();
                let mut buffer = String::new();
                let mut out = String::new();
                while let Some(next) = stream.next().await {
                    let chunk = next?;
                    buffer.push_str(&String::from_utf8_lossy(&chunk));
                    while let Some(idx) = buffer.find("\n\n") {
                        let event = buffer[..idx].to_string();
                        buffer = buffer[idx + 2..].to_string();
                        for line in event.lines() {
                            let line = line.trim();
                            if let Some(rest) = line.strip_prefix("data:") {
                                let data = rest.trim();
                                if data == "[DONE]" {
                                    return Ok(out);
                                }
                                if data.is_empty() {
                                    continue;
                                }
                                if let Ok(delta) =
                                    serde_json::from_str::<ChatCompletionStreamChunk>(data)
                                {
                                    if let Some(piece) = delta
                                        .choices
                                        .get(0)
                                        .and_then(|c| c.delta.content.as_deref())
                                    {
                                        out.push_str(piece);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(OpenAiClientError::StreamEnded)
            }
        })
        .await
    }

    async fn parse_json_response<T: for<'de> Deserialize<'de>>(
        resp: reqwest::Response,
        max_error_body_bytes: usize,
    ) -> Result<T, OpenAiClientError> {
        if resp.status().is_success() {
            let json = resp.json::<T>().await?;
            return Ok(json);
        }
        Err(Self::to_upstream_error(resp, max_error_body_bytes).await)
    }

    async fn to_upstream_error(
        resp: reqwest::Response,
        max_error_body_bytes: usize,
    ) -> OpenAiClientError {
        let status = resp.status();
        let body = read_limited_text(resp, max_error_body_bytes).await;
        if let Ok(parsed) = serde_json::from_str::<OpenAiErrorEnvelope>(&body) {
            let message = parsed
                .error
                .message
                .unwrap_or_else(|| "unknown upstream error".to_string());
            return OpenAiClientError::Upstream { status, message };
        }
        OpenAiClientError::UpstreamBody { status, body }
    }

    async fn request_with_retry<T, Fut, F>(&self, mut f: F) -> Result<T, OpenAiClientError>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<T, OpenAiClientError>>,
    {
        let mut attempt: u32 = 0;
        loop {
            attempt += 1;
            let result = f().await;
            match result {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if attempt > self.config.max_retries || !should_retry(&e) {
                        return Err(e);
                    }
                    let delay = backoff_delay(
                        self.config.initial_backoff,
                        self.config.max_backoff,
                        attempt - 1,
                    );
                    warn!(
                        attempt,
                        delay_ms = delay.as_millis(),
                        error = %e,
                        "openai request failed, retrying"
                    );
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
}

fn should_retry(err: &OpenAiClientError) -> bool {
    match err {
        OpenAiClientError::Request(e) => {
            e.is_timeout() || e.is_connect() || e.is_request() || e.is_body() || e.is_decode()
        }
        OpenAiClientError::Upstream { status, .. }
        | OpenAiClientError::UpstreamBody { status, .. } => {
            *status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error()
        }
        OpenAiClientError::InvalidJson(_) | OpenAiClientError::StreamEnded => false,
    }
}

fn backoff_delay(initial: Duration, max: Duration, exponent: u32) -> Duration {
    let mult = 1u128.checked_shl(exponent).unwrap_or(u128::MAX);
    let base_ms = initial.as_millis().saturating_mul(mult);
    let capped_ms = std::cmp::min(base_ms, max.as_millis()) as u64;
    let jitter_cap = std::cmp::max(1, capped_ms / 4);
    let jitter_ms = pseudo_jitter_ms(jitter_cap);
    Duration::from_millis(capped_ms.saturating_add(jitter_ms))
}

fn pseudo_jitter_ms(max_inclusive: u64) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_else(|_| Duration::from_secs(0));
    let nanos = now.subsec_nanos() as u64;
    nanos % (max_inclusive + 1)
}

async fn read_limited_text(resp: reqwest::Response, max_bytes: usize) -> String {
    match resp.bytes().await {
        Ok(mut b) => {
            if b.len() > max_bytes {
                b.truncate(max_bytes);
            }
            String::from_utf8_lossy(&b).to_string()
        }
        Err(e) => {
            warn!(error = %e, "failed to read upstream error body");
            "<failed to read error body>".to_string()
        }
    }
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorEnvelope {
    error: OpenAiErrorObject,
}

#[derive(Debug, Deserialize)]
struct OpenAiErrorObject {
    message: Option<String>,
    #[allow(dead_code)]
    r#type: Option<String>,
    #[allow(dead_code)]
    code: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelListResponse {
    pub object: Option<String>,
    pub data: Vec<ModelInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ModelInfo {
    pub id: String,
    pub object: Option<String>,
    pub created: Option<i64>,
    pub owned_by: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<Message>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub stream: Option<bool>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChatCompletionResponse {
    pub id: Option<String>,
    pub object: Option<String>,
    pub choices: Vec<ChatCompletionChoice>,
    pub usage: Option<ChatCompletionUsage>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChatCompletionChoice {
    pub index: Option<u32>,
    pub message: ChatCompletionMessage,
    pub finish_reason: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChatCompletionMessage {
    pub role: Option<String>,
    pub content: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ChatCompletionUsage {
    pub prompt_tokens: Option<u64>,
    pub completion_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionStreamChunk {
    choices: Vec<ChatCompletionStreamChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionStreamChoice {
    delta: ChatCompletionStreamDelta,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionStreamDelta {
    content: Option<String>,
}
