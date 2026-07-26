//! Ad-hoc LLM requests — «спросить у LLM про файл / текст / ссылку».
//!
//! Stored the SAME way as recordings: one folder per request under `<work_dir>/asks/<id>/`, so the
//! archive and this share a mental model and a human can open either in a file manager. The folder
//! holds three files, split by MUTABILITY on purpose (principle P2, same as sessions):
//!   * `input.txt`  — the material the person handed in. The source of truth; never rewritten.
//!   * `answer.md`  — the model's reply. Derived, disposable, re-creatable by asking again.
//!   * `ask.json`   — the record: what was asked, by whom (provider/model), what it cost, and — if
//!                    it failed — WHY. A failed ask is saved too, so nothing a person typed is lost.
//!
//! This module is pure storage: data types plus filesystem read/write, no LLM and no clock. The id
//! and timestamp are passed IN (the caller owns entropy), so the whole thing is testable on a temp
//! dir with no network and no wall-clock.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The archive sub-directory. Sibling of `sessions/`.
pub const DIR: &str = "asks";

const INPUT_FILE: &str = "input.txt";
const ANSWER_FILE: &str = "answer.md";
const RECORD_FILE: &str = "ask.json";

/// What to do with the handed-in material when the person did not write their own instruction.
/// A retelling-free default: answer about the content, in the content's language.
pub const DEFAULT_PROMPT: &str =
    "Прочитай приложенный материал и ответь по существу на языке материала. \
     Если это документ или страница — дай краткую выжимку главного; \
     если это вопрос — ответь на него.";

/// Where a request is in its (one-stage) pipeline — the same async lifecycle a session has, so the
/// «Спросить» button can return at once and the work finishes in the background, surviving a
/// navigation away or a daemon restart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AskStatus {
    /// Created, waiting for the worker.
    Pending,
    /// The worker is asking the model right now.
    Running,
    /// Answered.
    Done,
    /// The model call failed (see `error`).
    Failed,
}

/// Old records (the synchronous era) carry no status and are all completed.
fn status_done() -> AskStatus {
    AskStatus::Done
}

/// One request and its outcome. Serialized to `ask.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ask {
    /// Folder id — a timestamp, `YYYYMMDD_HHMMSS`, like a session.
    pub id: String,
    /// Where it is in its lifecycle.
    #[serde(default = "status_done")]
    pub status: AskStatus,
    /// When it was made (RFC3339).
    pub created_at: String,
    /// Which provider answered: `claude`, `ollama`, … — provenance, so a re-run is comparable.
    pub provider: String,
    /// The concrete model, when known (e.g. `claude-fable-5`, `qwen3.5:9b`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The instruction actually sent (the person's, or [`DEFAULT_PROMPT`]).
    pub prompt: String,
    /// `text` | `file` | `url` — how the material arrived, for display.
    pub input_kind: String,
    /// A human label for the input: a file name, a URL, or empty for pasted text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_name: Option<String>,
    /// Size of the material in characters — shown in the list without opening `input.txt`.
    pub input_chars: usize,
    /// The reply, when it succeeded. Also mirrored to `answer.md` for clean copying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    /// Usage-equivalent cost the provider reported, if any (provenance).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_usd: Option<f64>,
    /// Why it failed, when it did. Saved so a failed request is inspectable, not vanished.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// A row for the list view — everything shown without reading `input.txt`/`answer.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AskSummary {
    pub id: String,
    pub created_at: String,
    pub provider: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_name: Option<String>,
    pub input_chars: usize,
    /// The lifecycle state — the list shows «спрашиваю…»/«готово»/«ошибка», and the UI polls while
    /// anything is pending or running.
    pub status: AskStatus,
}

/// The person's instruction, or the default when they gave none. Pure.
pub fn effective_prompt(user_prompt: Option<&str>) -> String {
    match user_prompt.map(str::trim).filter(|p| !p.is_empty()) {
        Some(p) => p.to_string(),
        None => DEFAULT_PROMPT.to_string(),
    }
}

/// A folder id from a timestamp. The clock lives in the caller — this only formats.
pub fn new_id(now: chrono::DateTime<chrono::Local>) -> String {
    now.format("%Y%m%d_%H%M%S").to_string()
}

fn ask_dir(root: &Path, id: &str) -> PathBuf {
    root.join(id)
}

/// Write the folder: the immutable input, the derived answer, and the record. `root` is
/// `<work_dir>/asks`. Overwrites an existing id (a re-run of the same request replaces its answer;
/// the input is written again identically).
pub fn save(root: &Path, ask: &Ask, input: &str) -> Result<()> {
    let dir = ask_dir(root, &ask.id);
    std::fs::create_dir_all(&dir)
        .with_context(|| format!("creating ask dir {}", dir.display()))?;
    std::fs::write(dir.join(INPUT_FILE), input).context("writing input.txt")?;
    // answer.md exists only when there is an answer — its absence is a readable signal of failure.
    match &ask.answer {
        Some(a) => std::fs::write(dir.join(ANSWER_FILE), a).context("writing answer.md")?,
        None => {
            let _ = std::fs::remove_file(dir.join(ANSWER_FILE));
        }
    }
    let json = serde_json::to_vec_pretty(ask).context("serializing ask.json")?;
    std::fs::write(dir.join(RECORD_FILE), json).context("writing ask.json")?;
    Ok(())
}

/// Read one request's record.
pub fn load(root: &Path, id: &str) -> Result<Ask> {
    let path = ask_dir(root, id).join(RECORD_FILE);
    let bytes = std::fs::read(&path).with_context(|| format!("reading {}", path.display()))?;
    serde_json::from_slice(&bytes).with_context(|| format!("parsing {}", path.display()))
}

/// The material a request was made about.
pub fn input(root: &Path, id: &str) -> Result<String> {
    let path = ask_dir(root, id).join(INPUT_FILE);
    std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))
}

/// All requests, newest first. A folder without a readable `ask.json` is skipped, not fatal —
/// one corrupt entry must not blank the whole list.
pub fn list(root: &Path) -> Vec<AskSummary> {
    let mut out: Vec<AskSummary> = match std::fs::read_dir(root) {
        Ok(rd) => rd
            .flatten()
            .filter(|e| e.path().is_dir())
            .filter_map(|e| {
                let id = e.file_name().to_string_lossy().into_owned();
                let a = load(root, &id).ok()?;
                Some(AskSummary {
                    id: a.id,
                    created_at: a.created_at,
                    provider: a.provider,
                    input_name: a.input_name,
                    input_chars: a.input_chars,
                    status: a.status,
                })
            })
            .collect(),
        // No asks yet — the directory is created on the first request.
        Err(_) => Vec::new(),
    };
    // Ids are timestamps, so lexical desc == newest first.
    out.sort_by(|a, b| b.id.cmp(&a.id));
    out
}

/// Rewrite the record (and the answer file), leaving `input.txt` untouched — for the status
/// transitions the worker makes (Pending → Running → Done/Failed) without re-writing the material.
pub fn update(root: &Path, ask: &Ask) -> Result<()> {
    let dir = ask_dir(root, &ask.id);
    match &ask.answer {
        Some(a) => std::fs::write(dir.join(ANSWER_FILE), a).context("writing answer.md")?,
        None => {
            let _ = std::fs::remove_file(dir.join(ANSWER_FILE));
        }
    }
    let json = serde_json::to_vec_pretty(ask).context("serializing ask.json")?;
    std::fs::write(dir.join(RECORD_FILE), json).context("writing ask.json")?;
    Ok(())
}

/// Ids of requests waiting for the worker, OLDEST first (FIFO — a queue, not a stack).
pub fn pending(root: &Path) -> Vec<String> {
    let mut ids: Vec<String> = list(root)
        .into_iter()
        .filter(|a| a.status == AskStatus::Pending)
        .map(|a| a.id)
        .collect();
    ids.reverse(); // list() is newest-first; the queue serves oldest-first
    ids
}

/// At daemon startup, a request left `Running` was abandoned mid-model-call (the daemon died). Put
/// it back to `Pending` so the worker retries it — the same self-healing a session's cook has. A
/// re-ask is safe: it just queries again.
pub fn reclaim_running(root: &Path) -> usize {
    let mut n = 0;
    for a in list(root) {
        if a.status == AskStatus::Running {
            if let Ok(mut ask) = load(root, &a.id) {
                ask.status = AskStatus::Pending;
                if update(root, &ask).is_ok() {
                    n += 1;
                }
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effective_prompt_falls_back_to_default() {
        assert_eq!(effective_prompt(Some("сделай выжимку")), "сделай выжимку");
        assert_eq!(effective_prompt(Some("   ")), DEFAULT_PROMPT);
        assert_eq!(effective_prompt(None), DEFAULT_PROMPT);
    }

    fn sample(id: &str, answer: Option<&str>, error: Option<&str>) -> Ask {
        Ask {
            id: id.into(),
            status: if error.is_some() {
                AskStatus::Failed
            } else if answer.is_some() {
                AskStatus::Done
            } else {
                AskStatus::Pending
            },
            created_at: "2026-07-25T14:30:12+03:00".into(),
            provider: "claude".into(),
            model: Some("claude-fable-5".into()),
            prompt: DEFAULT_PROMPT.into(),
            input_kind: "text".into(),
            input_name: None,
            input_chars: 5,
            answer: answer.map(str::to_string),
            cost_usd: Some(0.01),
            error: error.map(str::to_string),
        }
    }

    #[test]
    fn save_and_load_roundtrip_and_files_split_by_mutability() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let a = sample("20260725_143012", Some("это ответ"), None);
        save(root, &a, "входной текст").unwrap();

        let back = load(root, &a.id).unwrap();
        assert_eq!(back.answer.as_deref(), Some("это ответ"));
        assert_eq!(input(root, &a.id).unwrap(), "входной текст");
        // The answer is ALSO a plain file for clean copying.
        let md = std::fs::read_to_string(root.join(&a.id).join("answer.md")).unwrap();
        assert_eq!(md, "это ответ");
    }

    /// A failed request is stored, with no `answer.md`, and shows as failed in the list.
    #[test]
    fn a_failed_ask_is_saved_and_marked() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let a = sample("20260725_150000", None, Some("claude not logged in"));
        save(root, &a, "материал").unwrap();

        assert!(!root.join(&a.id).join("answer.md").exists(), "no answer file on failure");
        let rows = list(root);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, AskStatus::Failed);
        assert_eq!(load(root, &a.id).unwrap().error.as_deref(), Some("claude not logged in"));
    }

    /// The queue serves oldest-first, and only Pending; a startup reclaim revives a stuck Running.
    #[test]
    fn pending_is_fifo_and_reclaim_revives_running() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        save(root, &sample("20260725_100000", None, None), "a").unwrap(); // pending
        save(root, &sample("20260725_110000", None, None), "b").unwrap(); // pending
        let mut running = sample("20260725_090000", None, None);
        running.status = AskStatus::Running;
        save(root, &running, "c").unwrap();

        assert_eq!(pending(root), vec!["20260725_100000", "20260725_110000"], "FIFO, no Running");
        assert_eq!(reclaim_running(root), 1);
        // The revived one is oldest → now first in the queue.
        assert_eq!(pending(root)[0], "20260725_090000");
    }

    /// The list is newest-first and survives a junk folder without an `ask.json`.
    #[test]
    fn list_is_newest_first_and_skips_junk() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        save(root, &sample("20260725_100000", Some("a"), None), "x").unwrap();
        save(root, &sample("20260725_120000", Some("b"), None), "y").unwrap();
        std::fs::create_dir_all(root.join("not-an-ask")).unwrap();

        let rows = list(root);
        assert_eq!(rows.len(), 2, "junk folder skipped");
        assert_eq!(rows[0].id, "20260725_120000", "newest first");
    }

    #[test]
    fn new_id_is_a_session_style_timestamp() {
        use chrono::TimeZone;
        let t = chrono::Local.with_ymd_and_hms(2026, 7, 25, 14, 30, 12).unwrap();
        assert_eq!(new_id(t), "20260725_143012");
    }
}
