//! The agent loop: send the conversation to the model, execute any tool calls it
//! requests, feed results back, and repeat until the model replies with plain text.

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::api::{Client, Message, Usage};
use crate::tools;
use crate::ui;

const SYSTEM_PROMPT: &str = "\
You are flashcode, a concise coding assistant running in a terminal. You help the \
user with software tasks in their current working directory. You have tools to read, \
write, edit, list, and search files, and to run shell commands. Prefer edit_file for \
small changes and read a file before editing it. Use bash to build and run tests to \
verify your work. When you have finished the task, reply with a short summary and no \
tool calls. Keep prose brief.";

/// Guards a single loop against runaway tool-calling.
const MAX_STEPS: usize = 50;

/// Outcome of a single permission prompt.
enum Decision {
    Allow,
    AllowAll,
    Deny,
}

pub struct Agent {
    client: Client,
    messages: Vec<Message>,
    /// When true, mutating tools run without asking (set by --yes or "allow all").
    auto_approve: bool,
    /// Running token total across the session.
    session_tokens: u64,
}

impl Agent {
    pub fn new(client: Client, auto_approve: bool) -> Self {
        Agent {
            client,
            messages: vec![Message::system(SYSTEM_PROMPT)],
            auto_approve,
            session_tokens: 0,
        }
    }

    /// Clear the conversation history, keeping the system prompt.
    /// Session token totals are preserved.
    pub fn reset(&mut self) {
        self.messages.truncate(0);
        self.messages.push(Message::system(SYSTEM_PROMPT));
    }

    pub fn model(&self) -> &str {
        self.client.model()
    }

    pub fn set_model(&mut self, model: &str) {
        self.client.set_model(model);
    }

    pub fn session_tokens(&self) -> u64 {
        self.session_tokens
    }

    /// Number of non-system messages currently in the history.
    pub fn history_len(&self) -> usize {
        self.messages.iter().filter(|m| m.role != "system").count()
    }

    /// Run one user turn to completion (through any number of tool calls).
    pub async fn handle_turn(&mut self, user_input: &str) -> Result<()> {
        self.messages.push(Message::user(user_input));

        for _ in 0..MAX_STEPS {
            let result = self.model_turn().await?;

            if let Some(usage) = &result.usage {
                self.record_usage(usage);
            }
            let reply = result.message;

            let tool_calls = reply.tool_calls.clone();
            self.messages.push(reply);

            let Some(calls) = tool_calls.filter(|c| !c.is_empty()) else {
                // No tool calls => turn is complete.
                return Ok(());
            };

            for call in calls {
                let result = self.execute_call(&call).await;
                self.messages.push(Message::tool(&call.id, result));
            }
        }

        println!("{}", ui::dim(&format!(
            "(stopped: reached the maximum of {MAX_STEPS} steps for this turn)"
        )));
        Ok(())
    }

    /// Run one model call for the current message history. Streams the response
    /// (printing text live); if the stream connection fails, retries once with
    /// the non-streaming `chat` path.
    async fn model_turn(&self) -> Result<crate::api::ChatResult> {
        let mut streamed_any = false;
        let mut stdout = std::io::stdout();
        println!();

        let streamed = self
            .client
            .chat_stream(&self.messages, tools::definitions(), |delta| {
                use std::io::Write;
                streamed_any = true;
                print!("{delta}");
                let _ = stdout.flush();
            })
            .await;

        match streamed {
            Ok(result) => {
                if streamed_any {
                    println!("\n");
                }
                Ok(result)
            }
            Err(e) => {
                // Streaming failed mid-flight: fall back to the robust path.
                println!("{}", ui::dim(&format!("  [stream failed: {e}; retrying non-streamed]")));
                let result = self.client.chat(&self.messages, tools::definitions()).await?;
                if let Some(text) = &result.message.content {
                    let t = text.trim();
                    if !t.is_empty() {
                        println!("{t}\n");
                    }
                }
                Ok(result)
            }
        }
    }

    fn record_usage(&mut self, usage: &Usage) {
        self.session_tokens += usage.total_tokens;
        println!(
            "{}",
            ui::dim(&format!(
                "  [tokens: {} this call ({}+{}), {} session]",
                usage.total_tokens,
                usage.prompt_tokens,
                usage.completion_tokens,
                self.session_tokens
            ))
        );
    }

    /// Execute a single tool call, applying the permission gate for mutating tools.
    async fn execute_call(&mut self, call: &crate::api::ToolCall) -> String {
        let name = call.function.name.as_str();

        let args: Value = match serde_json::from_str(&call.function.arguments) {
            Ok(v) => v,
            Err(e) => {
                return format!(
                    "Error: could not parse arguments for {name}: {e}. Raw: {}",
                    call.function.arguments
                );
            }
        };

        println!("  {} {}", ui::cyan("\u{2192}"), describe_call(name, &args));

        if tools::needs_permission(name) && !self.auto_approve {
            if let Some(preview) = tools::preview(name, &args) {
                for line in preview.lines() {
                    println!("    {}", ui::diff_line(line));
                }
            }
            match prompt_permission().await {
                Ok(Decision::Allow) => {}
                Ok(Decision::AllowAll) => self.auto_approve = true,
                Ok(Decision::Deny) => return "Error: user denied this action.".to_string(),
                Err(e) => return format!("Error: could not read confirmation: {e}"),
            }
        }

        tools::run(name, &args)
    }
}

/// A one-line, human-readable description of a tool call for the terminal.
fn describe_call(name: &str, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or("");
    match name {
        "read_file" | "write_file" | "edit_file" | "list_files" => {
            format!("{name} {path}")
        }
        "search" => {
            let q = args.get("query").and_then(Value::as_str).unwrap_or("");
            format!("search \"{q}\"")
        }
        "bash" => {
            let c = args.get("command").and_then(Value::as_str).unwrap_or("");
            format!("bash: {c}")
        }
        other => format!("{other} {args}"),
    }
}

/// Ask the user to approve a mutating action. `a` allows all for the session.
async fn prompt_permission() -> Result<Decision> {
    let mut stdout = tokio::io::stdout();
    stdout
        .write_all(ui::yellow("    Allow? [y]es / [n]o / [a]llow all: ").as_bytes())
        .await?;
    stdout.flush().await?;

    let mut line = String::new();
    let mut reader = BufReader::new(tokio::io::stdin());
    reader.read_line(&mut line).await?;
    Ok(match line.trim().to_lowercase().as_str() {
        "y" | "yes" => Decision::Allow,
        "a" | "all" => Decision::AllowAll,
        _ => Decision::Deny,
    })
}
