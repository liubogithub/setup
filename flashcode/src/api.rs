//! DeepSeek chat client over the OpenAI-compatible Chat Completions API.

use anyhow::{bail, Context, Result};
use futures_util::StreamExt;
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
    /// Always "function"; serialized back to the API in assistant history, which
    /// requires it per the OpenAI tool-call schema.
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
    stream: bool,
    /// Ask the API to include a usage block on the final streamed chunk.
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
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

// --- Streaming (SSE) delta types --------------------------------------------

#[derive(Deserialize)]
struct StreamChunk {
    #[serde(default)]
    choices: Vec<StreamChoice>,
    #[serde(default)]
    usage: Option<Usage>,
}

#[derive(Deserialize)]
struct StreamChoice {
    delta: Delta,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<ToolCallDelta>>,
}

/// A fragment of a tool call. Fields arrive incrementally across chunks and are
/// keyed by `index`; `id`/`name` appear once, `arguments` streams in pieces.
#[derive(Deserialize)]
struct ToolCallDelta {
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
struct FunctionDelta {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

/// Accumulator that reassembles streamed tool-call fragments into `ToolCall`s.
#[derive(Default)]
struct ToolCallBuilder {
    id: String,
    name: String,
    arguments: String,
}

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

    /// Send a chat completion, streaming the response. Invokes `on_text` with
    /// each content delta as it arrives, and returns the fully assembled message
    /// and token usage once the stream completes.
    pub async fn chat_stream<F: FnMut(&str)>(
        &self,
        messages: &[Message],
        tools: Vec<ToolDef>,
        mut on_text: F,
    ) -> Result<ChatResult> {
        let url = format!("{}/chat/completions", self.config.base_url);
        let body = ChatRequest {
            model: &self.config.model,
            messages,
            tools,
            temperature: 0.0,
            stream: true,
            stream_options: StreamOptions { include_usage: true },
        };

        let resp = self
            .http
            .post(&url)
            .bearer_auth(&self.config.api_key)
            .json(&body)
            .send()
            .await
            .context("sending streaming request to DeepSeek")?;

        let status = resp.status();
        if !status.is_success() {
            let text = resp.text().await.unwrap_or_default();
            bail!("DeepSeek API error ({status}): {text}");
        }

        let mut content = String::new();
        let mut builders: Vec<ToolCallBuilder> = Vec::new();
        let mut usage: Option<Usage> = None;

        // SSE frames are newline-delimited `data: {...}` lines. Responses may
        // split a line across network chunks, so buffer until we see a newline.
        let mut buffer = String::new();
        let mut stream = resp.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let bytes = chunk.context("reading stream chunk")?;
            buffer.push_str(&String::from_utf8_lossy(&bytes));

            while let Some(nl) = buffer.find('\n') {
                let line = buffer[..nl].trim().to_string();
                buffer.drain(..=nl);

                let Some(data) = line.strip_prefix("data:") else {
                    continue;
                };
                let data = data.trim();
                if data.is_empty() {
                    continue;
                }
                if data == "[DONE]" {
                    return Ok(finish(content, builders, usage));
                }

                let chunk: StreamChunk = match serde_json::from_str(data) {
                    Ok(c) => c,
                    Err(_) => continue, // skip unparseable keepalives/comments
                };
                if let Some(u) = chunk.usage {
                    usage = Some(u);
                }
                for choice in chunk.choices {
                    if let Some(text) = choice.delta.content {
                        if !text.is_empty() {
                            on_text(&text);
                            content.push_str(&text);
                        }
                    }
                    if let Some(tcs) = choice.delta.tool_calls {
                        apply_tool_deltas(&mut builders, tcs);
                    }
                }
            }
        }

        // Stream ended without an explicit [DONE]; assemble what we have.
        Ok(finish(content, builders, usage))
    }
}

/// Merge streamed tool-call fragments into the per-index builders.
fn apply_tool_deltas(builders: &mut Vec<ToolCallBuilder>, deltas: Vec<ToolCallDelta>) {
    for d in deltas {
        if d.index >= builders.len() {
            builders.resize_with(d.index + 1, ToolCallBuilder::default);
        }
        let b = &mut builders[d.index];
        if let Some(id) = d.id {
            b.id = id;
        }
        if let Some(f) = d.function {
            if let Some(name) = f.name {
                b.name = name;
            }
            if let Some(args) = f.arguments {
                b.arguments.push_str(&args);
            }
        }
    }
}

/// Assemble the final `ChatResult` from streamed pieces.
fn finish(content: String, builders: Vec<ToolCallBuilder>, usage: Option<Usage>) -> ChatResult {
    let tool_calls: Vec<ToolCall> = builders
        .into_iter()
        .filter(|b| !b.name.is_empty())
        .map(|b| ToolCall {
            id: b.id,
            kind: "function".into(),
            function: FunctionCall { name: b.name, arguments: b.arguments },
        })
        .collect();

    let message = Message {
        role: "assistant".into(),
        content: if content.is_empty() { None } else { Some(content) },
        tool_calls: if tool_calls.is_empty() { None } else { Some(tool_calls) },
        tool_call_id: None,
    };
    ChatResult { message, usage }
}
