//! DeepSeek chat client over the OpenAI-compatible Chat Completions API.

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::config::Config;

/// A message in the conversation. Mirrors the OpenAI chat schema closely enough
/// for DeepSeek. `content` is optional because assistant tool-call turns may
/// carry no text, and `tool_call_id` is set only on `role = "tool"` results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
}

impl Message {
    pub fn system(content: impl Into<String>) -> Self {
        Message { role: "system".into(), content: Some(content.into()), tool_calls: None, tool_call_id: None }
    }
    pub fn user(content: impl Into<String>) -> Self {
        Message { role: "user".into(), content: Some(content.into()), tool_calls: None, tool_call_id: None }
    }
    /// A `tool` result message answering a specific tool call.
    pub fn tool(tool_call_id: impl Into<String>, content: impl Into<String>) -> Self {
        Message {
            role: "tool".into(),
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    #[serde(rename = "type", default = "default_tool_type")]
    pub kind: String,
    pub function: FunctionCall,
}

fn default_tool_type() -> String {
    "function".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCall {
    pub name: String,
    /// JSON-encoded string of the arguments object, per the OpenAI schema.
    pub arguments: String,
}

/// A tool definition advertised to the model.
#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: FunctionDef,
}

#[derive(Debug, Clone, Serialize)]
pub struct FunctionDef {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Serialize)]
struct ChatRequest<'a> {
    model: &'a str,
    messages: &'a [Message],
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ToolDef>,
    temperature: f32,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

/// Token accounting returned by the API (fields optional across providers).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct Usage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
    #[serde(default)]
    pub total_tokens: u64,
}

/// One assistant turn plus its token usage.
pub struct ChatResult {
    pub message: Message,
    pub usage: Option<Usage>,
}

/// Number of attempts on transient failures (network errors, 429, 5xx).
const MAX_ATTEMPTS: usize = 4;

pub struct Client {
    http: reqwest::Client,
    config: Config,
}

impl Client {
    pub fn new(config: Config) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");
        Client { http, config }
    }

    /// Send one chat completion request, retrying transient failures with
    /// exponential backoff. Returns the assistant message and token usage.
    pub async fn chat(&self, messages: &[Message], tools: Vec<ToolDef>) -> Result<ChatResult> {
        let url = format!("{}/chat/completions", self.config.base_url);

        let mut last_err = None;
        for attempt in 0..MAX_ATTEMPTS {
            if attempt > 0 {
                // Backoff: 0.5s, 1s, 2s.
                let delay = 500u64 << (attempt - 1);
                tokio::time::sleep(std::time::Duration::from_millis(delay)).await;
            }

            let body = ChatRequest {
                model: &self.config.model,
                messages,
                tools: tools.clone(),
                temperature: 0.0,
            };

            let send = self
                .http
                .post(&url)
                .bearer_auth(&self.config.api_key)
                .json(&body)
                .send()
                .await;

            let resp = match send {
                Ok(r) => r,
                Err(e) => {
                    // Network/timeout error: worth retrying.
                    last_err = Some(anyhow::anyhow!("request failed: {e}"));
                    continue;
                }
            };

            let status = resp.status();
            let text = resp.text().await.context("reading response body")?;

            if status.is_success() {
                let parsed: ChatResponse = serde_json::from_str(&text)
                    .with_context(|| format!("parsing response: {text}"))?;
                let message = parsed
                    .choices
                    .into_iter()
                    .next()
                    .map(|c| c.message)
                    .context("response contained no choices")?;
                return Ok(ChatResult { message, usage: parsed.usage });
            }

            // Retry on rate limits and server errors; fail fast otherwise.
            let retryable = status.as_u16() == 429 || status.is_server_error();
            let err = anyhow::anyhow!("DeepSeek API error ({status}): {text}");
            if retryable {
                last_err = Some(err);
                continue;
            }
            bail!(err);
        }

        Err(last_err.unwrap_or_else(|| anyhow::anyhow!("request failed after {MAX_ATTEMPTS} attempts")))
    }
}
