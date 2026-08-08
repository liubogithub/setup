//! Tool definitions and execution.
//!
//! Each tool has a JSON-schema definition sent to the model, and a handler that
//! runs when the model calls it. Handlers return a human/model-readable string;
//! errors are turned into `Error: ...` strings and fed back so the model can
//! recover rather than aborting the loop.
//!
//! All file/shell access is sandboxed to the current working directory: a path
//! that resolves outside the cwd is rejected before any I/O happens.

use anyhow::{bail, Result};
use serde_json::{json, Value};
use std::path::PathBuf;
use std::process::Command;

use crate::api::{FunctionDef, ToolDef};

/// Cap on how much tool output we feed back to the model, to avoid blowing up
/// the context window on a huge file or command output.
const MAX_OUTPUT_BYTES: usize = 30_000;

/// The list of tools advertised to the model.
pub fn definitions() -> Vec<ToolDef> {
    let mut defs = Vec::new();

    defs.push(func(
        "read_file",
        "Read the contents of a file. Returns the full text with 1-based line numbers prefixed.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to read." }
            },
            "required": ["path"]
        }),
    ));

    defs.push(func(
        "write_file",
        "Write content to a file, creating it (and parent directories) or overwriting it \
         entirely. Use edit_file for targeted changes to an existing file.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to write." },
                "content": { "type": "string", "description": "The full new file content." }
            },
            "required": ["path", "content"]
        }),
    ));

    defs.push(func(
        "edit_file",
        "Replace an exact substring in a file with new text. `old_string` must appear exactly \
         once in the file, or the edit is rejected.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Path to the file to edit." },
                "old_string": { "type": "string", "description": "Exact text to replace (must be unique)." },
                "new_string": { "type": "string", "description": "Text to replace it with." }
            },
            "required": ["path", "old_string", "new_string"]
        }),
    ));

    defs.push(func(
        "list_files",
        "List files and directories under a path (defaults to the current directory). \
         Not recursive; directories are suffixed with '/'.",
        json!({
            "type": "object",
            "properties": {
                "path": { "type": "string", "description": "Directory to list. Defaults to '.'." }
            }
        }),
    ));

    defs.push(func(
        "search",
        "Search file contents for a substring (case-sensitive) across the working tree. \
         Returns matching lines as path:line:text. Skips hidden dirs, target/, and .git/.",
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string", "description": "Substring to search for." },
                "path": { "type": "string", "description": "Directory to search under. Defaults to '.'." }
            },
            "required": ["query"]
        }),
    ));

    defs.push(func(
        "bash",
        "Run a shell command in the working directory and return its combined stdout/stderr \
         and exit code. Use for building, running tests, git, etc.",
        json!({
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "The shell command to run." }
            },
            "required": ["command"]
        }),
    ));

    defs
}

fn func(name: &str, description: &str, parameters: Value) -> ToolDef {
    ToolDef {
        kind: "function",
        function: FunctionDef {
            name: name.into(),
            description: description.into(),
            parameters,
        },
    }
}

/// Whether a tool requires user confirmation before running.
pub fn needs_permission(name: &str) -> bool {
    matches!(name, "write_file" | "edit_file" | "bash")
}

/// Dispatch a tool call by name. `args` is the already-parsed arguments object.
pub fn run(name: &str, args: &Value) -> String {
    let result = match name {
        "read_file" => read_file(args),
        "write_file" => write_file(args),
        "edit_file" => edit_file(args),
        "list_files" => list_files(args),
        "search" => search(args),
        "bash" => bash(args),
        other => Err(anyhow::anyhow!("unknown tool: {other}")),
    };
    match result {
        Ok(s) => truncate_output(s),
        Err(e) => format!("Error: {e}"),
    }
}

fn truncate_output(s: String) -> String {
    if s.len() <= MAX_OUTPUT_BYTES {
        return s;
    }
    let mut cut = MAX_OUTPUT_BYTES;
    while !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n... (output truncated at {MAX_OUTPUT_BYTES} bytes)", &s[..cut])
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("missing or non-string argument `{key}`"))
}

fn arg_str_or<'a>(args: &'a Value, key: &str, default: &'a str) -> &'a str {
    args.get(key).and_then(Value::as_str).unwrap_or(default)
}

/// Resolve a user-supplied path against the cwd and reject anything that escapes
/// the sandbox (the current working directory tree).
fn resolve_sandboxed(path: &str) -> Result<PathBuf> {
    let cwd = std::env::current_dir()?;
    let joined = if std::path::Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        cwd.join(path)
    };

    // Normalize `.`/`..` lexically without requiring the path to exist yet
    // (write_file may target a not-yet-created file).
    let mut normalized = PathBuf::new();
    for comp in joined.components() {
        use std::path::Component::*;
        match comp {
            ParentDir => {
                normalized.pop();
            }
            CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }

    if !normalized.starts_with(&cwd) {
        bail!(
            "path `{path}` resolves outside the working directory; access is sandboxed to {}",
            cwd.display()
        );
    }
    Ok(normalized)
}

// ---------------------------------------------------------------------------
// Handlers
// ---------------------------------------------------------------------------

fn read_file(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let target = resolve_sandboxed(path)?;
    let text = std::fs::read_to_string(&target)
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
    let target = resolve_sandboxed(path)?;

    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("could not create parent dirs for {path}: {e}"))?;
    }
    std::fs::write(&target, content)
        .map_err(|e| anyhow::anyhow!("could not write {path}: {e}"))?;
    Ok(format!("Wrote {} bytes to {path}", content.len()))
}

fn edit_file(args: &Value) -> Result<String> {
    let path = arg_str(args, "path")?;
    let old = arg_str(args, "old_string")?;
    let new = arg_str(args, "new_string")?;
    let target = resolve_sandboxed(path)?;

    let text = std::fs::read_to_string(&target)
        .map_err(|e| anyhow::anyhow!("could not read {path}: {e}"))?;

    match text.matches(old).count() {
        0 => bail!("`old_string` not found in {path}"),
        1 => {}
        n => bail!(
            "`old_string` appears {n} times in {path}; it must be unique. \
             Add surrounding context to disambiguate."
        ),
    }

    let updated = text.replacen(old, new, 1);
    std::fs::write(&target, updated)
        .map_err(|e| anyhow::anyhow!("could not write {path}: {e}"))?;
    Ok(format!("Edited {path}"))
}

fn list_files(args: &Value) -> Result<String> {
    let path = arg_str_or(args, "path", ".");
    let target = resolve_sandboxed(path)?;

    let mut entries: Vec<String> = std::fs::read_dir(&target)
        .map_err(|e| anyhow::anyhow!("could not list {path}: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if e.path().is_dir() {
                format!("{name}/")
            } else {
                name
            }
        })
        .collect();
    entries.sort();

    if entries.is_empty() {
        Ok(format!("(directory {path} is empty)"))
    } else {
        Ok(entries.join("\n"))
    }
}

fn search(args: &Value) -> Result<String> {
    let query = arg_str(args, "query")?;
    let root = arg_str_or(args, "path", ".");
    let root_path = resolve_sandboxed(root)?;

    let mut hits = Vec::new();
    walk_search(&root_path, query, &mut hits)?;

    if hits.is_empty() {
        Ok(format!("No matches for `{query}`."))
    } else {
        Ok(hits.join("\n"))
    }
}

/// Recursively walk `dir`, collecting `path:line:text` for lines containing `query`.
fn walk_search(dir: &std::path::Path, query: &str, hits: &mut Vec<String>) -> Result<()> {
    const SKIP: &[&str] = &[".git", "target", "node_modules"];
    let cwd = std::env::current_dir()?;

    let read = match std::fs::read_dir(dir) {
        Ok(r) => r,
        Err(_) => return Ok(()),
    };
    for entry in read.filter_map(|e| e.ok()) {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP.contains(&name.as_str()) {
            continue;
        }
        if path.is_dir() {
            walk_search(&path, query, hits)?;
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            let rel = path.strip_prefix(&cwd).unwrap_or(&path);
            for (i, line) in text.lines().enumerate() {
                if line.contains(query) {
                    hits.push(format!("{}:{}:{}", rel.display(), i + 1, line.trim_end()));
                    if hits.len() >= 200 {
                        hits.push("... (stopped at 200 matches)".into());
                        return Ok(());
                    }
                }
            }
        }
    }
    Ok(())
}

fn bash(args: &Value) -> Result<String> {
    let command = arg_str(args, "command")?;
    let output = Command::new("sh")
        .arg("-c")
        .arg(command)
        .output()
        .map_err(|e| anyhow::anyhow!("could not run command: {e}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let code = output.status.code().unwrap_or(-1);

    let mut out = String::new();
    if !stdout.is_empty() {
        out.push_str(&stdout);
    }
    if !stderr.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&stderr);
    }
    if out.is_empty() {
        out.push_str("(no output)");
    }
    Ok(format!("{out}\n[exit code: {code}]"))
}

// ---------------------------------------------------------------------------
// Preview support (used by the permission gate to show what a call will do)
// ---------------------------------------------------------------------------

/// Produce a short preview of a mutating call for the confirmation prompt:
/// a unified-ish diff for edits, a size note for writes, the command for bash.
pub fn preview(name: &str, args: &Value) -> Option<String> {
    match name {
        "edit_file" => {
            let path = args.get("path").and_then(Value::as_str)?;
            let old = args.get("old_string").and_then(Value::as_str)?;
            let new = args.get("new_string").and_then(Value::as_str)?;
            let mut out = format!("edit {path}:\n");
            for line in old.lines() {
                out.push_str(&format!("  - {line}\n"));
            }
            for line in new.lines() {
                out.push_str(&format!("  + {line}\n"));
            }
            Some(out)
        }
        "write_file" => {
            let path = args.get("path").and_then(Value::as_str)?;
            let content = args.get("content").and_then(Value::as_str).unwrap_or("");
            let existed = resolve_sandboxed(path).map(|p| p.exists()).unwrap_or(false);
            let verb = if existed { "overwrite" } else { "create" };
            Some(format!("{verb} {path} ({} bytes)", content.len()))
        }
        "bash" => args
            .get("command")
            .and_then(Value::as_str)
            .map(|c| format!("run: {c}")),
        _ => None,
    }
}
