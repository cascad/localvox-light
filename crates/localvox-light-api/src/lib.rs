//! Archive MCP server (F6, phase C): the recordings database exposed as tools for
//! external agents (Claude Desktop/Code, your own scripts) — parity with Granola,
//! but local.
//!
//! Detachability (P5): the server works only with work_dir files (sessions/,
//! index/, slots.toml) — it does not need the recording engine, and the engine does
//! not need it. Transport — MCP stdio (JSON-RPC 2.0, ndjson): the same protocol as
//! our slots MCP client, only from the other side.
//!
//! Tools: `search_transcripts`, `list_sessions`, `get_transcript`,
//! `get_summary`, `append_note`.

pub mod archive;
pub mod chat;
pub mod http;

/// Tests that touch `LOCALVOX_SLOTS_CONFIG` MUST run one at a time: the environment
/// variable is one per process, while `cargo test` runs tests in threads. Without
/// this one test cleared the variable while another was using it, and the failure
/// looked random (it only failed in a full workspace run).
#[cfg(test)]
pub(crate) static SLOTS_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use serde_json::{json, Value};

use archive::{render_transcript_text, Archive};

pub const PROTOCOL_VERSION: &str = "2024-11-05";

pub struct McpServer {
    archive: Archive,
}

impl McpServer {
    pub fn new(work_dir: PathBuf) -> Self {
        Self {
            archive: Archive::new(work_dir),
        }
    }

    /// Handles a single JSON-RPC message; `None` — no response required
    /// (a notification or someone else's junk).
    pub fn handle(&self, msg: &Value) -> Option<Value> {
        let method = msg.get("method")?.as_str()?;
        let id = msg.get("id").cloned();
        // notifications (no id) require no response
        let id = match id {
            Some(v) if !v.is_null() => v,
            _ => return None,
        };
        let result = self.dispatch(method, msg.get("params").unwrap_or(&Value::Null));
        Some(match result {
            Ok(r) => json!({"jsonrpc": "2.0", "id": id, "result": r}),
            Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": {
                "code": -32000, "message": format!("{e:#}"),
            }}),
        })
    }

    fn dispatch(&self, method: &str, params: &Value) -> Result<Value> {
        match method {
            "initialize" => Ok(json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "localvox", "version": env!("CARGO_PKG_VERSION")},
            })),
            "ping" => Ok(json!({})),
            "tools/list" => Ok(json!({ "tools": tool_definitions() })),
            "tools/call" => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .context("tools/call without name")?;
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let text = self.call_tool(name, &args)?;
                Ok(json!({ "content": [{"type": "text", "text": text}], "isError": false }))
            }
            other => bail!("method not supported: {other}"),
        }
    }

    fn call_tool(&self, name: &str, args: &Value) -> Result<String> {
        match name {
            "search_transcripts" => self.search(args),
            "list_sessions" => self.list_sessions(args),
            "get_transcript" => self.get_transcript(args),
            "get_summary" => self.get_artifact(args, "summary.md"),
            "append_note" => self.append_note(args),
            other => bail!("no such tool: {other}"),
        }
    }

    fn search(&self, args: &Value) -> Result<String> {
        let query = args
            .get("query")
            .and_then(Value::as_str)
            .context("query argument is required")?;
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(10) as usize;
        let hits = self.archive.search(query, limit)?;
        if hits.is_empty() {
            return Ok(format!("Ничего не найдено по запросу: {query}"));
        }
        let mut out = String::new();
        for (i, h) in hits.iter().enumerate() {
            let ts = h
                .start_sec
                .map(|s| format!(" ({:02}:{:02})", (s / 60.0) as u64, s as u64 % 60))
                .unwrap_or_default();
            out.push_str(&format!(
                "{}. [сессия {}] {}{}\n   {}\n",
                i + 1,
                h.session,
                h.kind,
                ts,
                h.snippet
            ));
        }
        Ok(out)
    }

    fn list_sessions(&self, args: &Value) -> Result<String> {
        let limit = args.get("limit").and_then(Value::as_u64).unwrap_or(20) as usize;
        let sessions = self.archive.list_sessions(limit);
        if sessions.is_empty() {
            return Ok("Сессий пока нет".into());
        }
        let mut out = String::new();
        for s in sessions {
            let mut extras = Vec::new();
            if s.transcript_versions > 0 {
                extras.push(format!("транскрипт: {} версии", s.transcript_versions));
            }
            if s.has_summary {
                extras.push("summary".into());
            }
            if s.has_processed {
                extras.push("processed".into());
            }
            out.push_str(&format!(
                "- {}{}
",
                s.name,
                if extras.is_empty() {
                    " (только аудио)".to_string()
                } else {
                    format!(" — {}", extras.join(", "))
                }
            ));
        }
        Ok(out)
    }

    fn get_transcript(&self, args: &Value) -> Result<String> {
        let session = args
            .get("session")
            .and_then(Value::as_str)
            .context("session argument is required (name from list_sessions)")?;
        let doc = self.archive.transcript(session)?;
        Ok(render_transcript_text(&doc, std::path::Path::new("")))
    }

    fn get_artifact(&self, args: &Value, file: &str) -> Result<String> {
        let session = args
            .get("session")
            .and_then(Value::as_str)
            .context("session argument is required (name from list_sessions)")?;
        self.archive.artifact(session, file)
    }

    fn append_note(&self, args: &Value) -> Result<String> {
        let text = args
            .get("text")
            .and_then(Value::as_str)
            .context("text argument is required")?;
        let slot = args.get("slot").and_then(Value::as_str);
        self.archive.append_note(slot, text)
    }
}

fn tool_definitions() -> Value {
    json!([
        {
            "name": "search_transcripts",
            "description": "Полнотекстовый поиск (с русской морфологией) по всем транскриптам, сводкам и заметкам записанных сессий localvox. Возвращает совпадения с именем сессии и таймкодом.",
            "inputSchema": {"type": "object", "properties": {
                "query": {"type": "string", "description": "поисковый запрос"},
                "limit": {"type": "integer", "description": "максимум результатов (по умолчанию 10)"}
            }, "required": ["query"]}
        },
        {
            "name": "list_sessions",
            "description": "Список записанных сессий localvox (свежие первыми) с их артефактами: транскрипт/summary/processed.",
            "inputSchema": {"type": "object", "properties": {
                "limit": {"type": "integer", "description": "максимум (по умолчанию 20)"}
            }}
        },
        {
            "name": "get_transcript",
            "description": "Полный транскрипт сессии (best-версия) с ролями и таймкодами.",
            "inputSchema": {"type": "object", "properties": {
                "session": {"type": "string", "description": "имя сессии из list_sessions"}
            }, "required": ["session"]}
        },
        {
            "name": "get_summary",
            "description": "Сводка сессии (summary.md), если сгенерирована: о чём запись, главное, детали.",
            "inputSchema": {"type": "object", "properties": {
                "session": {"type": "string", "description": "имя сессии из list_sessions"}
            }, "required": ["session"]}
        },
        {
            "name": "append_note",
            "description": "Записать заметку в слот localvox (по умолчанию — default-слот из slots.toml).",
            "inputSchema": {"type": "object", "properties": {
                "text": {"type": "string", "description": "текст заметки"},
                "slot": {"type": "string", "description": "имя/алиас слота (опционально)"}
            }, "required": ["text"]}
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use localvox_light_core::versions::{now_rfc3339, TranscriptLine, VersionEntry, VersionStore};
    use std::fs;
    use std::path::Path;

    fn make_archive(dir: &Path) {
        let session = dir.join("sessions/20260711_test");
        fs::create_dir_all(&session).unwrap();
        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("gigaam-int8").unwrap();
        let line = TranscriptLine {
            source_id: 0,
            start_sec: 65.0,
            end_sec: 70.0,
            text: "обсудили ресурсные колбаски".into(),
            speaker: None,
        };
        fs::write(&path, serde_json::to_string(&line).unwrap() + "\n").unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "gigaam-int8".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "m".into(),
                params: json!({}),
                created_at: now_rfc3339(),
                parents: vec![],
            })
            .unwrap();
        fs::write(session.join("summary.md"), "## Решения\nперенесли релиз\n").unwrap();
    }

    fn call(server: &McpServer, id: u64, method: &str, params: Value) -> Value {
        let msg = json!({"jsonrpc":"2.0","id":id,"method":method,"params":params});
        server.handle(&msg).expect("a response was expected")
    }

    #[test]
    fn initialize_and_tools_list() {
        let dir = tempfile::tempdir().unwrap();
        let s = McpServer::new(dir.path().to_path_buf());
        let init = call(&s, 1, "initialize", json!({}));
        assert_eq!(init["result"]["protocolVersion"], PROTOCOL_VERSION);
        let tools = call(&s, 2, "tools/list", json!({}));
        let names: Vec<&str> = tools["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"search_transcripts"));
        assert!(names.contains(&"append_note"));
        // a notification gets no response
        assert!(s
            .handle(&json!({"jsonrpc":"2.0","method":"notifications/initialized"}))
            .is_none());
    }

    #[test]
    fn transcript_summary_and_search_over_archive() {
        let dir = tempfile::tempdir().unwrap();
        make_archive(dir.path());
        let s = McpServer::new(dir.path().to_path_buf());

        let list = call(
            &s,
            1,
            "tools/call",
            json!({"name":"list_sessions","arguments":{}}),
        );
        let text = list["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("20260711_test"), "{text}");
        assert!(text.contains("summary"));

        let tr = call(
            &s,
            2,
            "tools/call",
            json!({"name":"get_transcript","arguments":{"session":"20260711_test"}}),
        );
        let text = tr["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("[Я] (01:05) обсудили ресурсные колбаски"),
            "{text}"
        );

        let sm = call(
            &s,
            3,
            "tools/call",
            json!({"name":"get_summary","arguments":{"session":"20260711_test"}}),
        );
        assert!(sm["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("перенесли релиз"));

        // morphology: «колбаска» finds «колбаски»
        let sr = call(
            &s,
            4,
            "tools/call",
            json!({"name":"search_transcripts","arguments":{"query":"колбаска"}}),
        );
        let text = sr["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("20260711_test"), "{text}");
    }

    #[test]
    fn errors_are_jsonrpc_errors_and_paths_are_guarded() {
        let dir = tempfile::tempdir().unwrap();
        let s = McpServer::new(dir.path().to_path_buf());
        let bad = call(
            &s,
            1,
            "tools/call",
            json!({"name":"get_transcript","arguments":{"session":"../../etc"}}),
        );
        assert!(bad["error"]["message"]
            .as_str()
            .unwrap()
            .contains("некорректное"));
        let unknown = call(
            &s,
            2,
            "tools/call",
            json!({"name":"launch_rockets","arguments":{}}),
        );
        assert!(unknown["error"]["message"]
            .as_str()
            .unwrap()
            .contains("no such tool"));
    }

    #[test]
    fn append_note_via_slots_config() {
        // the environment variable is one per process — tests run one at a time
        let _env = crate::SLOTS_ENV_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let vault = dir.path().join("vault.md");
        let slots = dir.path().join("slots.toml");
        fs::write(
            &slots,
            format!(
                "[slots.\"идеи\"]\npath = \"{}\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n",
                vault.display().to_string().replace('\\', "/")
            ),
        )
        .unwrap();
        std::env::set_var("LOCALVOX_SLOTS_CONFIG", &slots);
        let s = McpServer::new(dir.path().to_path_buf());
        let r = call(
            &s,
            1,
            "tools/call",
            json!({"name":"append_note","arguments":{"text":"заметка от агента"}}),
        );
        std::env::remove_var("LOCALVOX_SLOTS_CONFIG");
        assert_eq!(r["result"]["isError"], false);
        assert_eq!(fs::read_to_string(&vault).unwrap(), "- заметка от агента\n");
    }
}
