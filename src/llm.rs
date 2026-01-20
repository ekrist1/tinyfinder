use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Clone)]
pub struct LlmClient {
    http: Client,
    base_url: String,
    api_key: String,
    model: String,
}

impl LlmClient {
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("MISTRAL_API_KEY").ok()?;
        if api_key.trim().is_empty() {
            return None;
        }

        let base_url = std::env::var("MISTRAL_BASE_URL")
            .unwrap_or_else(|_| "https://api.mistral.ai/v1".to_string());
        let model = std::env::var("MISTRAL_MODEL")
            .unwrap_or_else(|_| "mistral-large-latest".to_string());

        let http = Client::builder()
            .timeout(Duration::from_secs(60))
            .build()
            .ok()?;

        Some(Self {
            http,
            base_url,
            api_key,
            model,
        })
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    fn completions_url(&self) -> String {
        format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        )
    }

    pub async fn complete(
        &self,
        request: ChatCompletionRequest,
    ) -> Result<ChatCompletionResponse> {
        let response = self
            .http
            .post(self.completions_url())
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?
            .error_for_status()?;

        Ok(response.json::<ChatCompletionResponse>().await?)
    }

    pub async fn stream(&self, request: ChatCompletionRequest) -> Result<reqwest::Response> {
        let response = self
            .http
            .post(self.completions_url())
            .bearer_auth(&self.api_key)
            .json(&request)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "LLM request failed with status {}: {}",
                status,
                body
            ));
        }

        Ok(response)
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
    pub stream: bool,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionResponse {
    pub choices: Vec<ChatCompletionChoice>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionChoice {
    pub message: ChatMessage,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionStreamChunk {
    pub choices: Vec<ChatCompletionStreamChoice>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionStreamChoice {
    pub delta: ChatCompletionDelta,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatCompletionDelta {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}
