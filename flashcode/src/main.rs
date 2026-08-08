//! flashcode — a minimal coding agent CLI powered by DeepSeek.

mod agent;
mod api;
mod config;
mod tools;
mod ui;

use anyhow::Result;
use clap::Parser;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use agent::Agent;
use api::Client;
use config::Config;

#[derive(Parser, Debug)]
#[command(name = "flashcode", version, about = "A minimal coding agent CLI powered by DeepSeek")]
struct Cli {
    /// Run a single prompt non-interactively, then exit.
    #[arg(short, long)]
    prompt: Option<String>,

    /// Skip the confirmation prompt before file writes/edits.
    #[arg(long)]
    yes: bool,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let config = Config::load()?;
    let model = config.model.clone();
    let client = Client::new(config);
    let mut agent = Agent::new(client, cli.yes);

    // Non-interactive: run once and exit.
    if let Some(prompt) = cli.prompt {
        agent.handle_turn(&prompt).await?;
        return Ok(());
    }

    // Interactive REPL.
    println!("flashcode — model: {model}");
    println!("Type your request. /help for commands, /exit or Ctrl-D to quit.\n");

    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(tokio::io::stdin());
    let mut line = String::new();

    loop {
        stdout.write_all(b"> ").await?;
        stdout.flush().await?;

        line.clear();
        let n = reader.read_line(&mut line).await?;
        if n == 0 {
            // EOF (Ctrl-D).
            println!();
            break;
        }
        let input = line.trim();
        if input.is_empty() {
            continue;
        }

        // Slash commands are handled locally, without hitting the model.
        if input.starts_with('/') {
            if handle_command(input, &mut agent) {
                break; // command requested quit
            }
            continue;
        }

        if let Err(e) = agent.handle_turn(input).await {
            eprintln!("error: {e:#}");
        }
    }

    Ok(())
}

/// Handle a REPL slash command. Returns true if the user asked to quit.
fn handle_command(input: &str, agent: &mut Agent) -> bool {
    match input {
        "/exit" | "/quit" => return true,
        "/help" => println!(
            "commands:\n  \
             /help          show this help\n  \
             /clear, /reset clear the conversation history\n  \
             /tokens        show session token usage\n  \
             /exit, /quit   leave flashcode"
        ),
        "/clear" | "/reset" => {
            agent.reset();
            println!("(conversation cleared)");
        }
        "/tokens" => println!("session tokens: {}", agent.session_tokens()),
        other => println!("unknown command: {other} (try /help)"),
    }
    false
}
