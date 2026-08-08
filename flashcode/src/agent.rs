//! The agent loop: send the conversation to the model, execute any tool calls it
//! requests, feed results back, and repeat until the model replies with plain text.

use anyhow::Result;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::api::{Client, Message};
use crate::tools;

const SYSTEM_PROMPT: &str = "\
You are flashcode, a concise coding assistant running in a terminal. You help the \
user with software tasks in their current working directory. You have tools to read, \
write, and edit files. Prefer edit_file for small changes and read a file before \
editing it. When you have finished the task, reply with a short summary and no tool \
calls. Keep prose brief.";

/// Guards a single loop against runaway tool-calling.
const MAX_STEPS: usize = 50;

pub struct Agent {
    client: Client,
    messages: Vec<Message>,
    /// When true, write/edit tools run without asking for confirmation.
    auto_approve: bool,
}

impl Agent {
    pub fn new(client: Client, auto_approve: bool) -> Self {
        Agent {
            client,
            messages: vec![Message::system(SYSTEM_PROMPT)],
            auto_approve,
        }
    }

    /// Run one user turn to completion (through any number of tool calls).
    pub async fn handle_turn(&mut self, user_input: &str) -> Result<()> {
        self.messages.push(Message::user(user_input));

        for _ in 0..MAX_STEPS {
            let reply = self
                .client
                .chat(&self.messages, tools::definitions())
                .await?;

            // Print any assistant prose.
            if let Some(text) = &reply.content {
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    println!("\n{trimmed}\n");
                }
            }

            let tool_calls = reply.tool_calls.clone();
            self.messages.push(reply);

            let Some(calls) = tool_calls else {
                // No tool calls => turn is complete.
                return Ok(());
            };
            if calls.is_empty() {
                return Ok(());
            }

            for call in calls {
                let result = self.execute_call(&call).await;
                self.messages.push(Message::tool(&call.id, result));
            }
        }

        println!("(stopped: reached the maximum of {MAX_STEPS} steps for this turn)");
        Ok(())
    }

    /// Execute a single tool call, applying the permission gate for write tools.
    async fn execute_call(&self, call: &crate::api::ToolCall) -> String {
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

        println!("  \u{2192} {}", describe_call(name, &args));

        if tools::is_write_tool(name) && !self.auto_approve {
            match prompt_permission().await {
                Ok(true) => {}
                Ok(false) => return "Error: user denied this action.".to_string(),
                Err(e) => return format!("Error: could not read confirmation: {e}"),
            }
        }

        tools::run(name, &args)
    }
}

/// A one-line, human-readable description of a tool call for the terminal.
fn describe_call(name: &str, args: &Value) -> String {
    let path = args.get("path").and_then(Value::as_str).unwrap_or("?");
    match name {
        "read_file" => format!("read_file {path}"),
        "write_file" => format!("write_file {path}"),
        "edit_file" => format!("edit_file {path}"),
        other => format!("{other} {args}"),
    }
}

/// Ask the user to approve a write/edit. Returns Ok(true) on yes.
async fn prompt_permission() -> Result<bool> {
    let mut stdout = tokio::io::stdout();
    stdout.write_all(b"    Allow this write? [y/N] ").await?;
    stdout.flush().await?;

    let mut line = String::new();
    let mut reader = BufReader::new(tokio::io::stdin());
    reader.read_line(&mut line).await?;
    let answer = line.trim().to_lowercase();
    Ok(answer == "y" || answer == "yes")
}
