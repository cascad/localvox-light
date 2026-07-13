//! Universal MCP adapter (stdio transport): the slot declares the server command,
//! the tool name and the constant arguments — the note is delivered by a
//! `tools/call`. One adapter covers Obsidian-MCP, Notion and everything else
//! without writing separate integrations (owner's decision 2026-07-11).
//!
//! Protocol: JSON-RPC 2.0, messages separated by newlines (MCP stdio). Sequence:
//! `initialize` → `notifications/initialized` → `tools/call`. The server is started
//! for the duration of the write and then killed — for once-a-minute notes that is
//! enough; a persistent connection is for the voice module integration, if the
//! latency ever becomes noticeable.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use crate::{Integration, SlotConfig};

/// Response timeout of the MCP server (env `LOCALVOX_MCP_TIMEOUT_SEC`): a silent
/// server must not hang the voice thread forever.
fn response_timeout() -> Duration {
    let sec = std::env::var("LOCALVOX_MCP_TIMEOUT_SEC")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(20);
    Duration::from_secs(sec.max(1))
}

pub struct McpIntegration {
    command: String,
    args: Vec<String>,
    tool: String,
    text_arg: String,
    static_args: BTreeMap<String, Value>,
}

impl McpIntegration {
    pub fn from_config(slot_name: &str, c: &SlotConfig) -> Result<Self> {
        let command = c
            .command
            .clone()
            .with_context(|| format!("slot «{slot_name}»: type=\"mcp\" needs a command"))?;
        let tool = c
            .tool
            .clone()
            .with_context(|| format!("slot «{slot_name}»: type=\"mcp\" needs a tool"))?;
        let static_args = c
            .static_args
            .iter()
            .map(|(k, v)| Ok((k.clone(), toml_to_json(v)?)))
            .collect::<Result<_>>()?;
        Ok(Self {
            command,
            args: c.args.clone(),
            tool,
            text_arg: c.text_arg.clone(),
            static_args,
        })
    }
}

fn toml_to_json(v: &toml::Value) -> Result<Value> {
    serde_json::to_value(v.clone()).context("static_args → json")
}

impl Integration for McpIntegration {
    fn write_note(&self, text: &str) -> Result<String> {
        let text = text.trim();
        if text.is_empty() {
            bail!("empty note");
        }
        let mut client = McpClient::spawn(&self.command, &self.args)?;
        client.initialize()?;

        let mut arguments = serde_json::Map::new();
        for (k, v) in &self.static_args {
            arguments.insert(k.clone(), v.clone());
        }
        arguments.insert(self.text_arg.clone(), Value::String(text.to_string()));

        let result = client.call(
            "tools/call",
            json!({ "name": self.tool, "arguments": Value::Object(arguments) }),
        )?;
        if result
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false)
        {
            bail!(
                "MCP tool {} returned an error: {}",
                self.tool,
                summarize_content(&result)
            );
        }
        Ok(format!("mcp:{} → {}", self.command, self.tool))
    }
}

fn summarize_content(result: &Value) -> String {
    result
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|i| i.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join(" ")
        })
        .unwrap_or_else(|| result.to_string())
        .chars()
        .take(300)
        .collect()
}

// ─────────────────────── minimal stdio client ───────────────────────

pub struct McpClient {
    child: Child,
    /// The server's stdout lines are read by a separate thread: a blocking pipe read
    /// on Windows cannot be interrupted by a timeout, but a channel recv_timeout can.
    lines: std::sync::mpsc::Receiver<String>,
    next_id: u64,
}

impl McpClient {
    pub fn spawn(command: &str, args: &[String]) -> Result<Self> {
        let mut child = spawn_server(command, args)
            .with_context(|| format!("spawning the MCP server: {command}"))?;
        let stdout = child.stdout.take().context("stdout of the MCP server")?;
        let (tx, lines) = std::sync::mpsc::channel::<String>();
        std::thread::Builder::new()
            .name("mcp-reader".into())
            .spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                loop {
                    line.clear();
                    match reader.read_line(&mut line) {
                        Ok(0) | Err(_) => break, // EOF/error → the channel will close
                        Ok(_) => {
                            if tx.send(line.clone()).is_err() {
                                break;
                            }
                        }
                    }
                }
            })
            .ok();
        Ok(Self {
            child,
            lines,
            next_id: 1,
        })
    }

    pub fn initialize(&mut self) -> Result<()> {
        self.call(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {"name": "localvox", "version": env!("CARGO_PKG_VERSION")},
            }),
        )?;
        self.notify("notifications/initialized", json!({}))?;
        Ok(())
    }

    fn send(&mut self, msg: &Value) -> Result<()> {
        let stdin = self.child.stdin.as_mut().context("stdin of the MCP server")?;
        stdin.write_all(msg.to_string().as_bytes())?;
        stdin.write_all(b"\n")?;
        stdin.flush()?;
        Ok(())
    }

    fn notify(&mut self, method: &str, params: Value) -> Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    /// A request that waits for a response by id (with a timeout); notifications, logs
    /// and server→client requests (they carry a `method` field) are skipped.
    pub fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;

        let deadline = Instant::now() + response_timeout();
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "the MCP server did not answer {method} within {:?} (LOCALVOX_MCP_TIMEOUT_SEC)",
                    response_timeout()
                );
            }
            let line = match self.lines.recv_timeout(remaining) {
                Ok(l) => l,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                    bail!("the MCP server closed the stream without answering {method}")
                }
            };
            let Ok(msg) = serde_json::from_str::<Value>(&line) else {
                continue; // garbage/log line in stdout — skip it
            };
            // the response to our request: the id matches and it is not a server-side request
            if msg.get("id").and_then(Value::as_u64) != Some(id) || msg.get("method").is_some() {
                continue;
            }
            if let Some(err) = msg.get("error") {
                bail!("MCP {method}: {err}");
            }
            return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
        }
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// On Windows `Command::new("npx")` does not find `.cmd`/`.bat` shims (npx, npm and
/// most node wrappers) — on NotFound we retry through `cmd /C`.
fn spawn_server(command: &str, args: &[String]) -> std::io::Result<Child> {
    let direct = Command::new(command)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    #[cfg(windows)]
    {
        if matches!(&direct, Err(e) if e.kind() == std::io::ErrorKind::NotFound) {
            return Command::new("cmd")
                .arg("/C")
                .arg(command)
                .args(args)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
        }
    }
    direct
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    /// Live e2e against a fake MCP server written in Python (if python is unavailable
    /// the test quietly passes: environments without python only check the units above).
    #[test]
    fn mcp_integration_calls_tool_via_fake_server() {
        if Command::new("python").arg("--version").output().is_err() {
            eprintln!("python not found — skipping");
            return;
        }
        let dir = tempdir().unwrap();
        let out = dir.path().join("notes.txt");
        let server = dir.path().join("fake_mcp.py");
        fs::write(
            &server,
            r#"
import json, sys
out_path = sys.argv[1]
for line in sys.stdin:
    msg = json.loads(line)
    mid = msg.get("id")
    if msg.get("method") == "initialize":
        print(json.dumps({"jsonrpc": "2.0", "id": mid, "result": {"protocolVersion": "2024-11-05", "capabilities": {}, "serverInfo": {"name": "fake"}}}), flush=True)
    elif msg.get("method") == "tools/call":
        args = msg["params"]["arguments"]
        with open(out_path, "a", encoding="utf-8") as f:
            f.write(args["note"] + "|" + args["content"] + "\n")
        print(json.dumps({"jsonrpc": "2.0", "id": mid, "result": {"content": [{"type": "text", "text": "ok"}]}}), flush=True)
"#,
        )
        .unwrap();

        let cfg = SlotConfig {
            aliases: vec![],
            default: false,
            description: String::new(),
            kind: "mcp".into(),
            path: None,
            template: String::new(),
            command: Some("python".into()),
            args: vec![
                // Windows Python reads stdin in the system codepage (cp1251) by
                // default — UTF-8 JSON breaks; we force UTF-8.
                "-X".into(),
                "utf8".into(),
                server.display().to_string(),
                out.display().to_string(),
            ],
            tool: Some("append_content".into()),
            text_arg: "content".into(),
            static_args: [("note".to_string(), toml::Value::String("Идеи.md".into()))]
                .into_iter()
                .collect(),
        };
        let integration = McpIntegration::from_config("тест", &cfg).unwrap();
        let dest = integration.write_note("привет из mcp").unwrap();
        assert!(dest.contains("append_content"), "{dest}");
        let body = fs::read_to_string(&out).unwrap();
        assert_eq!(body.trim_end(), "Идеи.md|привет из mcp");
    }
}
