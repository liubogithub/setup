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
    resolve_within(&cwd, path)
}

/// Pure sandbox resolution: join `path` onto `base`, normalize `.`/`..`
/// lexically (so not-yet-created files work), and reject escapes from `base`.
/// Split out from `resolve_sandboxed` so it can be unit-tested without touching
/// the process-wide cwd.
fn resolve_within(base: &std::path::Path, path: &str) -> Result<PathBuf> {
    let joined = if std::path::Path::new(path).is_absolute() {
        PathBuf::from(path)
    } else {
        base.join(path)
    };

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

    if !normalized.starts_with(base) {
        bail!(
            "path `{path}` resolves outside the working directory; access is sandboxed to {}",
            base.display()
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

    let cwd = std::env::current_dir()?;
    let mut hits = Vec::new();
    walk_search(&root_path, &cwd, query, &mut hits);

    if hits.is_empty() {
        Ok(format!("No matches for `{query}`."))
    } else {
        Ok(hits.join("\n"))
    }
}

/// Match cap for a single `search`, to bound output and work.
const SEARCH_MAX_HITS: usize = 200;

/// Recursively walk `dir`, collecting `path:line:text` for lines containing
/// `query`. Paths are reported relative to `cwd`. Stops early once the hit cap
/// is reached.
fn walk_search(dir: &std::path::Path, cwd: &std::path::Path, query: &str, hits: &mut Vec<String>) {
    const SKIP: &[&str] = &[".git", "target", "node_modules"];

    let Ok(read) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read.filter_map(|e| e.ok()) {
        if hits.len() >= SEARCH_MAX_HITS {
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || SKIP.contains(&name.as_str()) {
            continue;
        }
        if path.is_dir() {
            walk_search(&path, cwd, query, hits);
        } else if let Ok(text) = std::fs::read_to_string(&path) {
            let rel = path.strip_prefix(cwd).unwrap_or(&path);
            for (i, line) in text.lines().enumerate() {
                if line.contains(query) {
                    hits.push(format!("{}:{}:{}", rel.display(), i + 1, line.trim_end()));
                    if hits.len() >= SEARCH_MAX_HITS {
                        hits.push(format!("... (stopped at {SEARCH_MAX_HITS} matches)"));
                        return;
                    }
                }
            }
        }
    }
}

/// Wall-clock limit for a single `bash` command. Without this, a hanging command
/// (a server, a REPL, `sleep`) would block the agent forever.
const BASH_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

fn bash(args: &Value) -> Result<String> {
    let command = arg_str(args, "command")?;
    run_bash(command, BASH_TIMEOUT)
}

/// Run a shell command with a wall-clock timeout. Split from `bash` so the
/// timeout can be exercised in tests without waiting the full production limit.
fn run_bash(command: &str, timeout: std::time::Duration) -> Result<String> {
    use std::os::unix::process::CommandExt;

    // Put the shell in its OWN process group so that on timeout we can signal the
    // whole group (grandchildren like a `sleep` inside `sh -c`), not just `sh`.
    let mut cmd = Command::new("sh");
    cmd.arg("-c")
        .arg(command)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    unsafe {
        // setpgid(0, 0): the child becomes the leader of a new process group.
        cmd.pre_exec(|| {
            if libc_setpgid() != 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| anyhow::anyhow!("could not run command: {e}"))?;
    let pgid = child.id() as i32; // group id == leader pid

    // Drain stdout/stderr on background threads. Reading inline after the poll
    // loop would deadlock if the command fills the pipe buffer before exiting.
    let stdout_handle = child.stdout.take().map(spawn_reader);
    let stderr_handle = child.stderr.take().map(spawn_reader);

    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if start.elapsed() >= timeout {
                    kill_group(pgid);
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(std::time::Duration::from_millis(20));
            }
            Err(e) => return Err(anyhow::anyhow!("error waiting on command: {e}")),
        }
    };

    // Reader threads finish once the pipes close (which the kill guarantees).
    let stdout_buf = stdout_handle.map(|h| h.join().unwrap_or_default()).unwrap_or_default();
    let stderr_buf = stderr_handle.map(|h| h.join().unwrap_or_default()).unwrap_or_default();

    let mut out = String::new();
    if !stdout_buf.is_empty() {
        out.push_str(&stdout_buf);
    }
    if !stderr_buf.is_empty() {
        if !out.is_empty() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(&stderr_buf);
    }
    if out.is_empty() {
        out.push_str("(no output)");
    }

    match status {
        Some(s) => Ok(format!("{out}\n[exit code: {}]", s.code().unwrap_or(-1))),
        None => Ok(format!(
            "{out}\n[timed out after {}s and was killed]",
            timeout.as_secs()
        )),
    }
}

/// Read a child pipe to end-of-file on a background thread, returning its text.
fn spawn_reader<R: std::io::Read + Send + 'static>(
    mut reader: R,
) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = reader.read_to_string(&mut buf);
        buf
    })
}

// Minimal libc bindings so we can manage the child's process group without
// pulling in the `libc` crate. Both are async-signal-safe / plain syscalls.
extern "C" {
    fn setpgid(pid: i32, pgid: i32) -> i32;
    fn kill(pid: i32, sig: i32) -> i32;
}

/// setpgid(0, 0) — make the calling process a new process-group leader.
/// Called from the child via pre_exec, before it execs the shell.
fn libc_setpgid() -> i32 {
    unsafe { setpgid(0, 0) }
}

/// SIGKILL every process in group `pgid` (kill(-pgid, SIGKILL)).
fn kill_group(pgid: i32) {
    const SIGKILL: i32 = 9;
    unsafe {
        kill(-pgid, SIGKILL);
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::path::Path;

    #[test]
    fn sandbox_allows_nested_paths() {
        let base = Path::new("/home/user/project");
        let p = resolve_within(base, "src/main.rs").unwrap();
        assert_eq!(p, Path::new("/home/user/project/src/main.rs"));
    }

    #[test]
    fn sandbox_normalizes_curdir() {
        let base = Path::new("/home/user/project");
        let p = resolve_within(base, "./a/./b.txt").unwrap();
        assert_eq!(p, Path::new("/home/user/project/a/b.txt"));
    }

    #[test]
    fn sandbox_rejects_parent_escape() {
        let base = Path::new("/home/user/project");
        assert!(resolve_within(base, "../secret.txt").is_err());
        assert!(resolve_within(base, "a/../../secret.txt").is_err());
    }

    #[test]
    fn sandbox_rejects_absolute_outside() {
        let base = Path::new("/home/user/project");
        assert!(resolve_within(base, "/etc/passwd").is_err());
    }

    #[test]
    fn sandbox_allows_absolute_inside() {
        let base = Path::new("/home/user/project");
        let p = resolve_within(base, "/home/user/project/x").unwrap();
        assert_eq!(p, Path::new("/home/user/project/x"));
    }

    #[test]
    fn truncate_leaves_short_output_untouched() {
        let s = "hello".to_string();
        assert_eq!(truncate_output(s.clone()), s);
    }

    #[test]
    fn truncate_caps_long_output() {
        let s = "x".repeat(MAX_OUTPUT_BYTES + 100);
        let out = truncate_output(s);
        assert!(out.len() < MAX_OUTPUT_BYTES + 100);
        assert!(out.contains("truncated"));
    }

    #[test]
    fn needs_permission_matches_mutating_tools() {
        assert!(needs_permission("write_file"));
        assert!(needs_permission("edit_file"));
        assert!(needs_permission("bash"));
        assert!(!needs_permission("read_file"));
        assert!(!needs_permission("search"));
        assert!(!needs_permission("list_files"));
    }

    #[test]
    fn edit_preview_shows_diff_markers() {
        let args = json!({"path": "f.txt", "old_string": "foo", "new_string": "bar"});
        let preview = preview("edit_file", &args).unwrap();
        assert!(preview.contains("- foo"));
        assert!(preview.contains("+ bar"));
    }

    #[test]
    fn bash_preview_shows_command() {
        let args = json!({"command": "ls -la"});
        let preview = preview("bash", &args).unwrap();
        assert!(preview.contains("ls -la"));
    }

    #[test]
    fn bash_captures_stdout_and_exit_code() {
        let out = bash(&json!({"command": "printf hello"})).unwrap();
        assert!(out.contains("hello"));
        assert!(out.contains("[exit code: 0]"));
    }

    #[test]
    fn bash_reports_nonzero_exit() {
        let out = bash(&json!({"command": "exit 3"})).unwrap();
        assert!(out.contains("[exit code: 3]"));
    }

    #[test]
    fn bash_captures_stderr() {
        let out = bash(&json!({"command": "printf oops 1>&2"})).unwrap();
        assert!(out.contains("oops"));
    }

    #[test]
    fn bash_kills_on_timeout() {
        let start = std::time::Instant::now();
        let out = run_bash("sleep 30", std::time::Duration::from_millis(200)).unwrap();
        // Must return near the timeout, not after the full 30s sleep.
        assert!(start.elapsed() < std::time::Duration::from_secs(5));
        assert!(out.contains("timed out"));
    }
}
