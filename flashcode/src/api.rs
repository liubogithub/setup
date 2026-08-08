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
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

pub struct Client {
    http: reqwest::Client,
    config: Config,
}

impl Client {
    pub fn new(config: Config) -> Self {
        Client { http: reqwest::Client::new(), config }
    }

    /// Send one chat completion request and return the assistant message.
    pub async fn chat(&self, messages: &[Message], tools: Vec<ToolDef>) -> Result<Message> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = ChatRequest {
            model: &self.config.model,
            messages,
            tools,
            temperature: 0.0,
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .context("sending request to DeepSeek")?;

        let status = resp.status();
        let text = resp.text().await.context("reading response body")?;

        if !status.is_success() {
            bail!("DeepSeek API error ({}): {}", status, text);
        }

        let parsed: ChatResponse =
            serde_json::from_str(&text).with_context(|| format!("parsing response: {text}"))?;

        parsed
            .choices
            .into_iter()
            .next()
            .map(|c| c.message)
            .context("response contained no choices")
    }
}
