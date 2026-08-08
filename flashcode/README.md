# flashcode

A minimal coding agent CLI powered by **DeepSeek** (`deepseek-v4-flash` by default),
inspired by Claude Code. It runs an agent loop over DeepSeek's OpenAI-compatible
Chat Completions API and gives the model tools to read, write, and edit files in your
current working directory.

## Features

- Interactive REPL and one-shot (`--prompt`) modes
- **Streaming** responses: assistant text prints live as it arrives
- **REPL slash commands**: `/help`, `/clear` (`/reset`), `/tokens`, `/exit`
- Tools: `read_file`, `write_file`, `edit_file`, `list_files`, `search`, `bash`
- Permission gate with a diff preview before each mutating action; answer
  **y**es / **n**o / **a**llow-all-this-session (bypass entirely with `--yes`)
- Sandboxed: all file/shell access is confined to the working directory tree
- Per-call and per-session token usage reporting
- Colored output (auto-disabled when piped or when `NO_COLOR` is set)
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

In the REPL, type `/exit` or press Ctrl-D to quit. Other commands:

| Command | Effect |
| ------- | ------ |
| `/help` | List available commands |
| `/clear`, `/reset` | Clear the conversation history (keeps session token count) |
| `/tokens` | Show session token usage |
| `/exit`, `/quit` | Leave flashcode |

## Testing

```sh
cargo test
```

Unit tests cover the path sandbox (escape rejection, normalization), output
truncation, the permission classifier, and the diff/command previews.

## Architecture

| Module        | Responsibility                                                        |
| ------------- | --------------------------------------------------------------------- |
| `config.rs`   | Load settings from env + `config.toml`                                |
| `api.rs`      | DeepSeek streaming chat client; OpenAI-compatible types + token usage |
| `tools.rs`    | Tool JSON schemas, handlers, sandbox, and diff previews               |
| `agent.rs`    | The agent loop: chat → run tools → feed results back, permission gate |
| `ui.rs`       | ANSI coloring helpers (TTY- and `NO_COLOR`-aware)                     |
| `main.rs`     | CLI parsing and the interactive REPL                                  |

## Notes

- `deepseek-v4-flash` is used as the default model ID; set `DEEPSEEK_MODEL` to whatever
  ID your endpoint actually serves (e.g. `deepseek-chat`).
- The `bash` tool runs commands in the working directory but still resolves file
  paths through the sandbox; it is gated behind the permission prompt by default.
