# flashcode

A minimal coding agent CLI powered by **DeepSeek** (`deepseek-v4-flash` by default),
inspired by Claude Code. It runs an agent loop over DeepSeek's OpenAI-compatible
Chat Completions API and gives the model tools to read, write, and edit files in your
current working directory.

## Features

- Interactive REPL and one-shot (`--prompt`) modes
- Tool calling: `read_file`, `write_file`, `edit_file`
- Permission gate: confirms before any file write/edit (bypass with `--yes`)
- Configurable model / base URL / key via env vars or a config file

## Install

```sh
cargo build --release
# binary at target/release/flashcode
```

## Configuration

Resolution order (later wins): built-in defaults → config file → environment variables.

Environment variables:

| Variable            | Default                      | Meaning                        |
| ------------------- | ---------------------------- | ------------------------------ |
| `DEEPSEEK_API_KEY`  | *(required)*                 | Your DeepSeek API key          |
| `DEEPSEEK_BASE_URL` | `https://api.deepseek.com`   | OpenAI-compatible API base URL |
| `DEEPSEEK_MODEL`    | `deepseek-v4-flash`          | Model ID                       |

Or `~/.config/flashcode/config.toml`:

```toml
api_key = "sk-..."
base_url = "https://api.deepseek.com"
model = "deepseek-v4-flash"
```

## Usage

```sh
export DEEPSEEK_API_KEY=sk-...

# interactive
flashcode

# one-shot
flashcode -p "Add a docstring to src/lib.rs"

# skip write confirmations
flashcode --yes -p "Create a hello.txt with 'hi'"
```

In the REPL, type `/exit` or press Ctrl-D to quit.

## Architecture

| Module        | Responsibility                                                        |
| ------------- | --------------------------------------------------------------------- |
| `config.rs`   | Load settings from env + `config.toml`                                |
| `api.rs`      | DeepSeek chat client; OpenAI-compatible request/response + tool types |
| `tools.rs`    | Tool JSON schemas and their handlers                                  |
| `agent.rs`    | The agent loop: chat → run tools → feed results back, permission gate |
| `main.rs`     | CLI parsing and the interactive REPL                                  |

## Notes

- `deepseek-v4-flash` is used as the default model ID; set `DEEPSEEK_MODEL` to whatever
  ID your endpoint actually serves (e.g. `deepseek-chat`).
- v1 intentionally ships file tools only — no shell execution.
