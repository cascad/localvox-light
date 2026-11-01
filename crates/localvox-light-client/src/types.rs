//! Wire data only. Unknown fields are tolerated for additive server upgrades.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Required fields deliberately have no serde defaults: an arbitrary JSON
/// health endpoint is not evidence of a compatible LocalVox service.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Health {
    pub ok: bool,
    pub service: String,
    pub version: String,
    pub api_protocol: u32,
    pub capabilities: Vec<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct BestReply {
    pub ok: bool,
    pub best: u32,
}
impl Health {
    pub fn compatible(&self) -> bool {
        self.ok
            && self.service == "localvox-light"
            && self.api_protocol == 1
            && ["technical-journal-v1", "terminal-workspace-v1"]
                .iter()
                .all(|required| self.capabilities.iter().any(|c| c == required))
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Session {
    pub name: String,
    pub title: Option<String>,
    pub started_at: Option<String>,
    pub source_url: Option<String>,
    pub duration_sec: f64,
    pub recording: bool,
    pub cooked: bool,
    pub empty: bool,
    pub job: Option<String>,
    pub has_summary: bool,
    pub has_processed: bool,
    pub summary_doubts: Option<String>,
    pub processed_doubts: Option<String>,
    pub lang: Option<String>,
}
impl Session {
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.name)
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RecordState {
    pub engine: bool,
    pub recording: bool,
    pub session: Option<String>,
    pub preroll_sec: f64,
    pub voice: Option<VoiceStatus>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct VoiceStatus {
    pub active: bool,
    pub detail: String,
    pub capturing: Option<Capturing>,
    pub last: Option<Written>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Capturing {
    pub slot: String,
    pub text: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Written {
    pub slot: String,
    pub text: String,
    pub dest: String,
    pub at: String,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Jobs {
    pub running: usize,
    pub current: Option<String>,
    pub pending: usize,
    pub failed: usize,
    pub cancelling: usize,
    pub cancelled: usize,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct QueueItem {
    pub session: String,
    pub title: String,
    pub state: String,
    pub kind: String,
    pub position: Option<usize>,
    pub attempts: usize,
    pub phase: Option<String>,
    pub retry: Option<RetryPlan>,
    pub last_error: Option<String>,
    pub stuck: bool,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RetryPlan {
    pub state: String,
    pub message: String,
    pub retry_at: Option<String>,
}

#[cfg(test)]
mod retry_compatibility {
    use super::*;
    #[test]
    fn queue_accepts_legacy_rows_and_additive_retry_metadata() {
        let old: QueueItem =
            serde_json::from_str(r#"{"session":"old","state":"failed","attempts":3}"#).unwrap();
        assert!(old.phase.is_none() && old.retry.is_none());
        let new: QueueItem = serde_json::from_str(r#"{"session":"new","state":"failed","phase":"summary","retry":{"state":"blocked","message":"coordinator unavailable","retry_at":"2026-10-31T08:00:00Z"}}"#).unwrap();
        assert_eq!(new.phase.as_deref(), Some("summary"));
        assert_eq!(new.retry.unwrap().state, "blocked");
    }
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TranscriptLine {
    pub start_sec: f64,
    pub end_sec: f64,
    pub who: String,
    pub text: String,
    pub source_id: Option<u8>,
    pub original: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Provenance {
    pub template: String,
    pub model: String,
    pub glossary: usize,
    pub at: String,
    pub doubts: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ReadableDoc {
    pub from_transcript: bool,
    pub lines: Vec<TranscriptLine>,
    pub version_id: u32,
    pub provenance: Option<Provenance>,
    pub omitted: usize,
    pub rejected: usize,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Citation {
    pub line: usize,
    pub start_sec: f64,
    pub end_sec: f64,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SummaryBlock {
    pub kind: String,
    pub text: String,
    pub sources: Vec<Citation>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Summary {
    pub blocks: Vec<SummaryBlock>,
    pub provenance: Option<Provenance>,
    pub available: Option<bool>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Version {
    pub id: u32,
    pub best: bool,
    pub derived: bool,
    pub model: String,
    pub created_at: String,
    pub lines: usize,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Participant {
    pub name: String,
    pub what: String,
    pub named: bool,
    pub voices: Vec<String>,
    pub sources: Vec<u8>,
    pub speech_sec: f64,
    pub lines: usize,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SourceName {
    pub source_id: u8,
    pub name: String,
    pub custom: bool,
    pub what: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Hit {
    pub session: String,
    pub kind: String,
    pub snippet: String,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub matched: Vec<String>,
    pub why: Option<String>,
    pub score: Option<f32>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AskSource {
    pub n: usize,
    pub session: String,
    pub kind: String,
    pub text: String,
    pub start_sec: Option<f64>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RouteHint {
    pub slot: String,
    pub reason: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Answer {
    pub text: String,
    pub ungrounded: Vec<String>,
    pub sources: Vec<AskSource>,
    pub verified: bool,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct AskRecord {
    pub id: String,
    pub created_at: String,
    pub provider: String,
    pub model: Option<String>,
    pub prompt: String,
    pub input_kind: String,
    pub input_name: Option<String>,
    pub input_chars: usize,
    pub status: String,
    pub answer: Option<String>,
    pub error: Option<String>,
    pub cost_usd: Option<f64>,
    pub input: Option<String>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct AskRequest {
    pub text: String,
    pub provider: String,
    pub prompt: String,
    pub input_name: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StoredNote {
    pub text: String,
    pub date: Option<String>,
    pub raw: String,
    pub source: String,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SlotNotes {
    pub name: String,
    pub default: bool,
    pub readable: bool,
    pub notes: Vec<StoredNote>,
    pub error: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DeviceChoice {
    pub value: String,
    pub label: String,
    pub is_default: bool,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Setting {
    pub key: String,
    pub group: String,
    pub label: String,
    pub applies: String,
    pub hint: String,
    pub kind: String,
    pub value: Option<String>,
    pub set: bool,
    pub options: Option<Vec<DeviceChoice>>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct SettingsDoc {
    pub path: String,
    pub exists: bool,
    pub settings: Vec<Setting>,
    // Deliberately discard unknown server values (CFG-003 / RV-010).
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Reply {
    pub msg: Option<String>,
    pub session: Option<String>,
    pub deleted: Option<String>,
    pub dest: Option<String>,
    pub saved: Option<usize>,
    pub need_restart: Vec<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Peaks {
    pub duration_sec: f64,
    pub peaks: Vec<f64>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Progress {
    pub at: Option<String>,
    pub execution: Option<Execution>,
    pub running: bool,
    pub phase: Option<String>,
    pub stages: Vec<StageStatus>,
    pub queue_error: Option<String>,
    pub timing: Option<ProgressTiming>,
    pub source: Option<ProgressSource>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct Execution {
    pub run: Option<String>,
    pub operation: Option<String>,
    pub state: String,
    pub message: String,
    pub current: Option<String>,
    pub phase: Option<String>,
    pub since: Option<String>,
    pub next: Vec<String>,
    pub retry_at: Option<String>,
    pub last_signal_at: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ProgressTiming {
    pub queued_at: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub queue_sec: Option<f64>,
    pub processing_sec: Option<f64>,
    pub total_sec: Option<f64>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ProgressSource {
    pub url: String,
    pub title: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct StageStatus {
    pub stage: String,
    pub state: String,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub updated_at: Option<String>,
    pub note: Option<String>,
    pub done: Option<u32>,
    pub total: Option<u32>,
    pub files: Vec<String>,
    pub artifact: Option<Value>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayArgument {
    pub name: String,
    pub value: Option<String>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct DisplayCommand {
    pub program: String,
    pub args: Vec<DisplayArgument>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct JournalEntry {
    pub elapsed_sec: Option<f64>,
    pub id: String,
    pub run: String,
    pub operation: Option<String>,
    pub parent: Option<String>,
    pub stage: String,
    pub kind: String,
    pub at: String,
    pub label: String,
    pub status: Option<String>,
    pub command: Option<DisplayCommand>,
    pub text: Option<String>,
    pub done: Option<u32>,
    pub total: Option<u32>,
}
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct JournalPage {
    pub events: Vec<JournalEntry>,
    pub next_cursor: String,
    pub previous_cursor: String,
    pub has_more: bool,
    pub reset: bool,
    pub truncated: bool,
    pub discarded: usize,
    pub read_bytes: u64,
    pub dropped_verbose: u64,
}
