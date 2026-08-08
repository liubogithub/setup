//! Tool definitions and execution.
//!
//! The agent exposes three file tools. Each has a JSON-schema definition sent to
//! the model, and a handler that runs when the model calls it. Handlers return a
//! human/model-readable string; errors are turned into `Error: ...` strings and
//! fed back so the model can recover rather than aborting the loop.

use anyhow::Result;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

use crate::api::{FunctionDef, ToolDef};

/// The list of tools advertised to the model.
pub fn definitions() -> Vec<ToolDef> {
    vec![
        ToolDef {
            kind: "function",
            function: FunctionDef {
                name: "read_file".into(),
                description: "Read the contents of a file at the given path. Returns the \
                              full text with 1-based line numbers prefixed."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file to read." }
                    },
                    "required": ["path"]
                }),
            },
        },
        ToolDef {
            kind: "function",
            function: FunctionDef {
                name: "write_file".into(),
                description: "Write content to a file, creating it (and parent directories) \
                              or overwriting it entirely. Use edit_file for targeted changes."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file to write." },
                        "content": { "type": "string", "description": "The full new file content." }
                    },
                    "required": ["path", "content"]
                }),
            },
        },
        ToolDef {
            kind: "function",
            function: FunctionDef {
                name: "edit_file".into(),
                description: "Replace an exact substring in a file with new text. `old_string` \
                              must appear exactly once in the file, or the edit is rejected."
                    .into(),
                parameters: json!({
                    "type": "object",
                    "properties": {
                        "path": { "type": "string", "description": "Path to the file to edit." },
                        "old_string": { "type": "string", "description": "Exact text to replace (must be unique)." },
                        "new_string": { "type": "string", "description": "Text to replace it with." }
                    },
                    "required": ["path", "old_string", "new_string"]
                }),
            },
        },
    ]
}

/// Whether a tool mutates the filesystem (used to gate on permission).
pub fn is_write_tool(name: &str) -> bool {
    matches!(name, "write_file" | "edit_file")
}

/// Dispatch a tool call by name. `args` is the already-parsed arguments object.
pub fn run(name: &str, args: &Value) -> String {
    let result = match name {
        "read_file" => read_file(args),
        "write_file" => write_file(args),
        "edit_file" => edit_file(args),
        other => Err(anyhow::anyhow!("unknown tool: {other}")),
    };
    match result {
        Ok(s) => s,
        Err(e) => format!("Error: {e}"),
    }
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing or non-string argument `{key}`"))
}

/// Resolve a user-supplied path against the current working directory.
fn resolve(path: &str) -> PathBuf {
    Path::new(path).to_path_buf()
}

fn read_file(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let text = std::fs::read_to_string(resolve(path))
        .map_err(|e| anyhow::anyhow!("could not read {path}: {e}"))?;

    if text.is_empty() {
        return Ok(format!("(file {path} is empty)"));
    }
    let numbered = text
        .lines()
        .enumerate()
        .map(|(i, line)| format!("{:>6}\t{}", i + 1, line))
        .collect::<Vec<_>>()
        .join("\n");
    Ok(numbered)
}

fn write_file(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let content = arg_str(args, "content")?;
    let target = resolve(path);

    if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)
                .map_err(|e| anyhow::anyhow!("could not create parent dirs for {path}: {e}"))?;
        }
    }
    std::fs::write(&target, content)
        .map_err(|e| anyhow::anyhow!("could not write {path}: {e}"))?;
    let bytes = content.len();
    Ok(format!("Wrote {bytes} bytes to {path}"))
}

fn edit_file(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let old = arg_str(args, "old_string")?;
    let new = arg_str(args, "new_string")?;
    let target = resolve(path);

    let text = std::fs::read_to_string(&target)
        .map_err(|e| anyhow::anyhow!("could not read {path}: {e}"))?;

    let occurrences = text.matches(old).count();
    match occurrences {
        0 => anyhow::bail!("`old_string` not found in {path}"),
        1 => {}
        n => anyhow::bail!(
            "`old_string` appears {n} times in {path}; it must be unique. \
             Add surrounding context to disambiguate."
        ),
    }

    let updated = text.replacen(old, new, 1);
    std::fs::write(&target, updated)
        .map_err(|e| anyhow::anyhow!("could not write {path}: {e}"))?;
    Ok(format!("Edited {path}"))
}
