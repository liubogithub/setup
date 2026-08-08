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
    println!("Type your request. Use /exit or Ctrl-D to quit.\n");

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
        if input == "/exit" || input == "/quit" {
            break;
        }

        if let Err(e) = agent.handle_turn(input).await {
            eprintln!("error: {e:#}");
        }
    }

    Ok(())
}
