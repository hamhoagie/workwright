//! LLM client — thin wrapper over the Anthropic Messages API.
//!
//! Keeps the wright model-agnostic. Swap this module
//! and you can use any provider.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum LlmError {
    #[error("rate limited: {0}")]
    RateLimited(String),

    #[error("network error: {0}")]
    Network(String),

    #[error("api error ({status}): {message}")]
    Api { status: u16, message: String },

    #[error("malformed response: {0}")]
    Malformed(String),
}

/// Holds the API key, model name, and HTTP client needed to call the Anthropic Messages API.
#[derive(Debug, Clone)]
pub struct LlmClient {
    api_key: String,
    model: String,
    http: reqwest::Client,
}

#[derive(Serialize)]
struct MessagesRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<Message>,
}

#[derive(Serialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Deserialize)]
struct MessagesResponse {
    content: Vec<ContentBlock>,
}

#[derive(Deserialize)]
struct ContentBlock {
    text: Option<String>,
}

#[derive(Deserialize)]
struct ErrorResponse {
    error: Option<ApiError>,
}

#[derive(Deserialize)]
struct ApiError {
    message: Option<String>,
}

impl LlmClient {
    pub fn new(api_key: &str, model: &str) -> Self {
        Self {
            api_key: api_key.to_string(),
            model: model.to_string(),
            http: reqwest::Client::new(),
        }
    }

    pub fn from_env() -> Result<Self, LlmError> {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .map_err(|_| LlmError::Api {
                status: 0,
                message: "ANTHROPIC_API_KEY not set".to_string(),
            })?;
        let model = std::env::var("WW_MODEL")
            .unwrap_or_else(|_| "claude-sonnet-4-6".to_string());
        Ok(Self::new(&api_key, &model))
    }

    /// Return a new client with a different model (same API key).
    pub fn with_model(&self, model: &str) -> Self {
        Self {
            api_key: self.api_key.clone(),
            model: model.to_string(),
            http: self.http.clone(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    pub async fn call(&self, prompt: &str) -> Result<String, LlmError> {
        let body = MessagesRequest {
            model: self.model.clone(),
            max_tokens: 4096,
            messages: vec![Message {
                role: "user".to_string(),
                content: prompt.to_string(),
            }],
        };

        let resp = self
            .http
            .post("https://api.anthropic.com/v1/messages")
            .header("x-api-key", &self.api_key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .json(&body)
            .send()
            .await
            .map_err(|e| {
                if e.is_timeout() || e.is_connect() {
                    LlmError::Network(e.to_string())
                } else {
                    LlmError::Api {
                        status: 0,
                        message: e.to_string(),
                    }
                }
            })?;

        let status = resp.status().as_u16();

        if status == 429 {
            let text = resp.text().await.unwrap_or_default();
            return Err(LlmError::RateLimited(text));
        }

        if status != 200 {
            let text = resp.text().await.unwrap_or_default();
            let msg = serde_json::from_str::<ErrorResponse>(&text)
                .ok()
                .and_then(|e| e.error)
                .and_then(|e| e.message)
                .unwrap_or(text);
            return Err(LlmError::Api {
                status,
                message: msg,
            });
        }

        let body: MessagesResponse = resp
            .json()
            .await
            .map_err(|e| LlmError::Malformed(e.to_string()))?;

        body.content
            .first()
            .and_then(|c| c.text.clone())
            .ok_or_else(|| LlmError::Malformed("no text in response".to_string()))
    }
}