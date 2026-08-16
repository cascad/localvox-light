//! Operations over the session archive — the shared core for the MCP server and the
//! HTTP API (F6). Works only with work_dir files (P5): the recording engine is not
//! needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use localvox_light_core::versions::{read_transcript_lines, VersionStore};
use localvox_light_integrations::SlotRegistry;
use localvox_light_search::SearchIndex;

/// A request to «спросить у LLM»: the material plus how to treat it. Deserialized from the POST
/// body; every field but `text` is optional (a bare paste with the default prompt is valid).
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AskRequest {
    /// The material — pasted text, or a dropped file's text read on the client.
    pub text: String,
    /// The person's own instruction; empty/absent → the default prompt.
    #[serde(default)]
    pub prompt: Option<String>,
    /// `claude` (default) — subscription via the CLI; anything else — the OpenAI-compatible client.
    #[serde(default)]
    pub provider: Option<String>,
    /// A label for the input (file name / URL), for the list. Purely cosmetic.
    #[serde(default)]
    pub input_name: Option<String>,
}

/// How much of the pipeline a re-cook throws away and remakes — chosen by the tab the person is
/// looking at, so ONE «Переварить» does the right thing per section (WP: granular re-cook).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RecookScope {
    /// Only the summary (`summary.md`). Transcript and cleaned text are kept.
    Summary,
    /// The cleaned text (processed.json = «Реплики»/«Текст», one artifact) and the summary that
    /// derives from it. The transcript version is kept.
    Text,
    /// Everything, from the audio up: transcript, cleanup, summary. Forces past an existing version.
    All,
}

impl RecookScope {
    /// From the UI tab name. Unknown → the safe full redo.
    pub fn from_tab(s: &str) -> Self {
        match s {
            "summary" => Self::Summary,
            "processed" | "transcript" | "text" => Self::Text,
            _ => Self::All,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Summary => "только сводка",
            Self::Text => "текст и сводка",
            Self::All => "всё заново",
        }
    }
}

/// A cook counts as stalled when its progress has not moved for this long — above any normal quiet
/// stretch (a summary ~4 min, a model load ~2.5 min), and far above the per-chunk/per-batch
/// heartbeat interval, so only a dead process trips it.
const COOK_STALL_SEC: i64 = 360;

pub struct Archive {
    work_dir: PathBuf,
}

#[derive(Serialize)]
pub struct Hit {
    pub session: String,
    pub kind: String,
    pub start_sec: Option<f64>,
    /// The END of the fragment: a transcript line is a WINDOW of 8-15 seconds, not a moment.
    /// Without it the result does not say what «▶» will play and when it will stop.
    pub end_sec: Option<f64>,
    pub snippet: String,
    /// How close the MEANING is (0..1) — for a semantic hit. `None` for a purely lexical one:
    /// there the match IS the word, and a number would add nothing.
    ///
    /// Without this number a semantic result cannot be judged: the human sees a line with not
    /// a single word of his query and no way to tell whether a meaning was found or the
    /// machine simply returned the nearest of the nonsense.
    pub score: Option<f32>,
    /// The words that ACTUALLY matched (morphology included) — the UI highlights
    /// them. Empty for a semantic hit: there is NO word match there at all, and we
    /// MUST NOT lie about it.
    #[serde(default)]
    pub matched: Vec<String>,
    /// Why this record is here: `слова` | `смысл` | `слова + смысл`
    /// (words | meaning | words + meaning).
    ///
    /// Without this field the result looked random: a person saw a line, could not
    /// find their query in it and did not understand what had been found (owner's
    /// complaint). The answer "found by MEANING, not by words" is honest and settles
    /// the question.
    pub why: String,
}

#[derive(Serialize)]
pub struct SessionInfo {
    pub name: String,
    pub transcript_versions: usize,
    pub has_summary: bool,
    pub has_processed: bool,
    /// The engine is writing this session right now — so it cannot have been cooked.
    /// Without this flag a live recording looked in the archive like "not cooked",
    /// that is, like a failure.
    pub recording: bool,
    /// Drafts: the LLM result is not confirmed by the recording. The work is not
    /// lost, but it is not passed off as fact. The flags are SEPARATE — otherwise the
    /// draft of the cleaned-up text was unreachable from the web (the tab led only to
    /// the summary).
    /// What the check doubted about — and the human has NOT yet said it is fine.
    ///
    /// There is no quarantine any more: the document is always written. The doubts are a
    /// MARK («worth double-checking, may be a lie or an imprecision»), not a verdict — our
    /// check compares literally and will always have false positives (measured: «Сергей» and
    /// «муж» flagged as invented although both were said out loud). The person reads the
    /// document, and one button removes the mark.
    pub summary_doubts: Option<String>,
    pub processed_doubts: Option<String>,
    /// Recording boundaries (RFC3339) and its duration — INCLUDING pauses and
    /// silence: a person needs "July 13, 00:34–00:52", not a directory name.
    pub started_at: Option<String>,
    pub stopped_at: Option<String>,
    pub duration_sec: f64,
    /// The session was cut off by a person (the "Завершить"/Finish button), not by a
    /// failure.
    pub stopped_reason: Option<String>,
    /// Not a single line in the transcript. ATTENTION: "empty" and "not processed"
    /// are DIFFERENT things, and MUST NOT be confused. Empty after cooking means no
    /// speech was found; such a session may be hidden. Not cooked means nobody has
    /// looked at it yet, and it MUST NEVER be hidden: the person will not find their
    /// recording and will decide that it is gone.
    pub empty: bool,
    /// Cooked: a transcript exists (even if empty).
    pub cooked: bool,
    /// What the secretary is doing with it right now: `running` (cooking) | `pending`
    /// (queued) | `failed` (broke down). Empty — the queue does not know about it.
    pub job: Option<String>,
    /// The language chosen by the PERSON (empty — "auto"), and the language detected
    /// from the recognized text. Different things: the first decides which model to
    /// recognize with, the second is only a guess about the finished text.
    pub lang: Option<String>,
    pub lang_detected: Option<String>,
    /// The title given by the person, and the "this is a meeting, not background"
    /// marker. They make the meeting visible in the list, instead of forcing a search
    /// for it among the day's nameless stretches.
    pub title: Option<String>,
    pub meeting: bool,
}

/// One transcript version — for the "Versions" list of a session.
#[derive(Serialize)]
pub struct VersionInfo {
    pub id: u32,
    /// `gigaam-int8` (cooked from audio) / `refined` (combed by the LLM) / custom.
    pub label: String,
    pub model: String,
    pub created_at: String,
    /// Built from the text of another version, not from audio.
    pub derived: bool,
    pub best: bool,
    pub lines: usize,
}

#[derive(Serialize)]
pub struct TranscriptDoc {
    pub session: String,
    pub label: String,
    pub model: String,
    pub lines: Vec<TranscriptLineOut>,
}

#[derive(Serialize)]
pub struct RouteHint {
    pub slot: String,
    pub reason: String,
}

/// State of the auto-cook queue — "what the secretary is doing right now".
#[derive(Serialize, Default)]
pub struct JobsStatus {
    pub pending: usize,
    pub running: usize,
    pub done: usize,
    pub failed: usize,
    /// The session being cooked right now.
    pub current: Option<String>,
    pub last_error: Option<String>,
}

/// One line of the QUEUE — a session waiting for the pot, in the order it will get there.
///
/// Counts alone («варится: 1, в очереди: 7») answer «is it working», not «what is it working on
/// and what is after that». Clicking them used to jump to the stage chain of one session, which
/// showed its LAST FINISHED run — all green, all done, while the cook was still going. A queue is
/// a list with an order; that is the whole point of it.
#[derive(Serialize)]
pub struct QueueItem {
    pub session: String,
    /// What a person calls it, not the directory name.
    pub title: String,
    pub started_at: Option<String>,
    /// `running` | `pending` | `failed`. Done jobs are not a queue — they are history.
    pub state: String,
    /// `cook` — the audio is here; `ingest` — it still has to be fetched from a link.
    pub kind: String,
    /// Place in the line. 1 — next. `None` for what is already running.
    pub position: Option<usize>,
    pub attempts: u32,
    /// Why it failed, if it did — the reason belongs next to the fact, not in a log nobody opens.
    pub last_error: Option<String>,
    /// It has burned its retry budget: it will not move again on its own, only by a human's
    /// «переварить заново». Saying that plainly beats a queue that silently never advances.
    pub stuck: bool,
}

/// Why a record ended up in the results. Strings, not an enum: this is a caption for
/// a human, and it goes into the JSON as is.
const WHY_WORDS: &str = "слова";
const WHY_MEANING: &str = "смысл";
const WHY_BOTH: &str = "слова + смысл";

/// LLM client from the `LOCALVOX_LLM_*` env (shared with `localvox-process`).
///
/// The timeout is a PARAMETER, not a constant: "where do I file this?" is one phrase
/// and a short wait, whereas a question to the archive carries thousands of
/// characters of found fragments with it and thinks noticeably longer. A single
/// timeout for both cases either cuts an honest answer off mid-word, or lets a hung
/// LLM hold an HTTP thread for ten minutes.
fn llm_client_from_env_with(timeout_sec: u64) -> localvox_light_llm::LlmClient {
    llm_client_tuned(timeout_sec, None)
}

/// The same client, but with the temperature under our control.
///
/// A question to the archive is a RETRIEVAL task, not creative writing: at temperature 0.2
/// the model tosses a coin. Measured (13.07.2026): on one and the same prompt — with
/// fragments that literally talk about a cake — it answered correctly once and refused
/// («В записях этого нет») the next time. An answer that depends on a dice roll is not an
/// answer.
/// The same, with a ceiling on the answer. A bounded output is not a nicety: without it a model
/// that has started looping runs until the timeout, and the person gets a page of noise.
fn llm_client_capped(
    timeout_sec: u64,
    temperature: Option<f32>,
    max_tokens: u32,
) -> localvox_light_llm::LlmClient {
    let mut client = llm_client_tuned(timeout_sec, temperature);
    client.set_max_tokens(max_tokens);
    client
}

fn llm_client_tuned(timeout_sec: u64, temperature: Option<f32>) -> localvox_light_llm::LlmClient {
    let base_url = std::env::var("LOCALVOX_LLM_BASE_URL")
        .unwrap_or_else(|_| "http://localhost:11434/v1".into());
    let model = std::env::var("LOCALVOX_LLM_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into());
    let api_key = std::env::var("LOCALVOX_LLM_API_KEY_ENV")
        .ok()
        .and_then(|name| std::env::var(name).ok());
    let default = localvox_light_llm::LlmProfile::default();
    localvox_light_llm::LlmClient::new(localvox_light_llm::LlmProfile {
        base_url,
        model,
        api_key,
        timeout_sec,
        temperature: temperature.unwrap_or(default.temperature),
        ..default
    })
}

/// 16 kHz — the sample rate of the whole archive.
const SR: f64 = 16_000.0;

/// Ceiling for a single clip. Not a whim but memory: 10 minutes of mono 16 kHz is
/// 19 MB per request, and it is assembled in RAM. An hour in one piece (115 MB) would
/// take the API down with several listeners. The player takes the recording in
/// windows — that is its job, not the server's.
const MAX_CLIP_SEC: f64 = 600.0;

/// PCM of a single track over the given range of the recording timeline (s16le).
fn source_pcm(
    dir: &Path,
    meta: &localvox_light_core::chunks::SessionMeta,
    source_id: u8,
    want_start: u64,
    want_end: u64,
) -> Result<Vec<u8>> {
    let audio = dir.join("audio");
    let mut chunks: Vec<_> = meta
        .chunks
        .iter()
        .filter(|c| c.source_id == source_id)
        .collect();
    chunks.sort_by(|a, b| a.start_offset_sec.total_cmp(&b.start_offset_sec));

    let mut pcm: Vec<u8> = Vec::new();
    let mut emitted_end = want_start; // abs. timeline position up to which it is glued
    let mut saw_overlap = false;
    for c in chunks {
        let c_start = (c.start_offset_sec * SR) as u64;
        // Overlap is computed over the DECLARED (in meta) range — we do not silently
        // skip a chunk whose file cannot be read (otherwise the clip would come out
        // shorter/shifted with no diagnostics).
        let decl_end = c_start + (c.duration_sec * SR) as u64;
        if decl_end <= want_start || c_start >= want_end {
            continue;
        }
        saw_overlap = true;
        let from = emitted_end.saturating_sub(c_start); // do not rewrite what is glued
        let to = want_end.saturating_sub(c_start);
        if to == 0 {
            continue;
        }
        let slice = chunk_pcm_range(&audio, &c.file, from, to).with_context(|| {
            format!(
                "чанк {} недоступен (FLAC-декод/удалён) — фрагмент не собрать целиком",
                c.file
            )
        })?;
        if !slice.is_empty() {
            emitted_end = c_start + from + (slice.len() / 2) as u64;
            pcm.extend_from_slice(&slice);
        }
    }
    anyhow::ensure!(
        saw_overlap && !pcm.is_empty(),
        "нет аудио для фрагмента (источник {source_id})"
    );
    Ok(pcm)
}

/// Sum two tracks. WITH CLIPPING PROTECTION: the sum of two loud samples overflows
/// i16 and "wraps" into the opposite sign — that is not "a bit louder", that is
/// crackle instead of speech.
fn mix(a: &[u8], b: &[u8]) -> Vec<u8> {
    let n = a.len().max(b.len()) / 2 * 2;
    let mut out = Vec::with_capacity(n);
    let get = |src: &[u8], i: usize| -> i32 {
        if i + 1 < src.len() {
            i32::from(i16::from_le_bytes([src[i], src[i + 1]]))
        } else {
            0
        }
    };
    for i in (0..n).step_by(2) {
        let s = (get(a, i) + get(b, i)).clamp(i32::from(i16::MIN), i32::from(i16::MAX)) as i16;
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

fn env_secs(name: &str, default: u64) -> u64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn env_f64(name: &str, default: f64) -> f64 {
    std::env::var(name)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// A byte slice of the recording's WAV, for a Range request. `total` is the whole file's length
/// (for `Content-Range`); `start..end` is what `bytes` covers.
pub struct AudioSlice {
    pub total: u64,
    pub start: u64,
    pub end: u64,
    pub bytes: Vec<u8>,
}

/// What the recorder is doing right now.
///
/// `engine` and `recording` are DIFFERENT facts, and the app needs both: with a dead
/// daemon a "record" button would look pressable and lead nowhere, and the person would
/// walk away believing they are being recorded. That is the one lie this system must
/// never tell.
#[derive(serde::Serialize)]
pub struct RecordState {
    /// Is the engine alive at all (it holds a lock on the work directory for its lifetime).
    pub engine: bool,
    pub recording: bool,
    /// The session being written into right now.
    pub session: Option<String>,
    /// Seconds of the past held in memory: press "record" and they enter the session.
    pub preroll_sec: f64,
    /// What the voice module is doing — a DIFFERENT thing from `recording`, and both can be true
    /// at once: a note can be dictated in the middle of a meeting.
    ///
    /// A session is the microphone going to disk here; a note is a phrase on its way into a slot —
    /// someone else's file, on another disk, written in silence. This carries whether the module is
    /// alive at all, what it is dictating, and the receipt for the last note that landed. The first
    /// version carried only the dictation in flight, and a module that had heard nothing looked
    /// exactly like a module that was not running.
    pub voice: Option<localvox_light_core::voice_note::VoiceStatus>,
}

/// Is the recording engine alive AT ALL.
///
/// The `.recording_session` marker survives a daemon crash: a killed process does not
/// clear it, and the session would forever stay "● recording" — the archive would lie
/// about the state. Previously we guarded against this with file freshness ("a chunk
/// younger than two minutes"), and that was a crutch: a new session is born EMPTY
/// (the directory and the meta exist, the first chunk does not yet) and by such a
/// rule looked dead.
///
/// We ask directly: is anybody holding the lock on the work directory. The engine
/// takes the lock for its whole lifetime — if it is free, there is no engine, and the
/// marker is stale. This is not a heuristic but an operating-system fact.
fn engine_alive(work_dir: &Path) -> bool {
    match localvox_light_core::session::try_instance_lock(work_dir) {
        Ok(Some(_lock)) => false, // we took the lock — so there is no engine
        Ok(None) => true,         // busy: the engine is running
        // We could not even ask — we do not invent: we assume the engine is alive and
        // trust the marker. Lying "recording in progress" is more harmless than lying
        // "there is no session": the first a person will check with their own eyes,
        // the second they will not notice.
        Err(_) => true,
    }
}

/// Ceiling on concurrent semantic searches: indexing is serialized by a
/// cross-process lock anyway (semantic.rs); 2 means "one indexes, one waits", and
/// the rest get a fast "busy" instead of a queue on the lock.
const MAX_SEMANTIC_INFLIGHT: usize = 2;

static SEMANTIC_INFLIGHT: AtomicUsize = AtomicUsize::new(0);

/// RAII counter of concurrent semantic requests (the analogue of RouteGuard in
/// http.rs).
struct SemanticGuard;
impl SemanticGuard {
    fn try_acquire() -> Option<Self> {
        if SEMANTIC_INFLIGHT.fetch_add(1, Ordering::AcqRel) >= MAX_SEMANTIC_INFLIGHT {
            SEMANTIC_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
            None
        } else {
            Some(Self)
        }
    }
}
impl Drop for SemanticGuard {
    fn drop(&mut self) {
        SEMANTIC_INFLIGHT.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Somebody the text attributes lines to. Assembled from those lines — see `Archive::speakers`.
#[derive(Serialize)]
pub struct ParticipantOut {
    /// As the text says it right now.
    pub name: String,
    /// «отдельный голос» / «микрофон» / «системный звук» / «звук из источника» — WHY this entry
    /// is called what it is called.
    pub what: String,
    /// A human (or a profile) has given this one a name; «Участник 2» and «Собеседники» have not.
    pub named: bool,
    /// What has to be renamed for this row, and by which mechanism. A voice is renamed by its
    /// roster label (re-labels the lines and remembers the print); a source by its id (changes
    /// only the fallback). A merged row carries both, and renaming must do all of them.
    pub voices: Vec<String>,
    pub sources: Vec<u8>,
    /// Measured ON THE LINES of this very document, not on diarization's own segments — the two
    /// disagree (32.7 min against 18.6 on the owner's video), and the honest number is the one a
    /// reader can count for themselves.
    pub speech_sec: f64,
    pub lines: usize,
}

/// A voice: a participant of the recording (`owner` — the owner, «Я») or a name we
/// know.
#[derive(Serialize)]
pub struct SpeakerOut {
    pub label: String,
    /// How many seconds of speech are attributed to them: this shows how far to trust
    /// the attribution.
    pub speech_sec: f64,
    pub owner: bool,
}

#[derive(Serialize)]
pub struct TranscriptLineOut {
    /// Who said it: a name (diarization + profile), «Участник N» (the voices are
    /// separated, but we do not know the people) or the audio source — if we do not
    /// know at all.
    pub who: String,
    /// 0 — own microphone, 1 — the other participants (loopback); the player needs it
    /// for the clip.
    pub source_id: u8,
    pub start_sec: f64,
    pub end_sec: f64,
    pub text: String,
}

impl Archive {
    pub fn new(work_dir: PathBuf) -> Self {
        Self { work_dir }
    }

    fn sessions_root(&self) -> PathBuf {
        self.work_dir.join("sessions")
    }

    fn asks_root(&self) -> PathBuf {
        self.work_dir.join(localvox_light_core::asks::DIR)
    }

    /// CREATE an ad-hoc «спросить у LLM» request — saved as `Pending` and returned AT ONCE. The
    /// model is NOT called here: that is the worker's job (see [`Self::process_ask`]). This is the
    /// whole point of the async rework — the button returns instantly, and the answer arrives in the
    /// background, surviving a navigation away or a daemon restart, exactly like a session's cook.
    pub fn create_ask(&self, req: AskRequest) -> Result<localvox_light_core::asks::Ask> {
        use localvox_light_core::asks::{self, Ask, AskStatus};

        // A cap that is generous for documents but keeps one request from swallowing memory or
        // blowing past the CLI's stdin limit (~10 MB). Well past a long PDF's text.
        const MAX_INPUT_CHARS: usize = 400_000;
        let input = req.text;
        anyhow::ensure!(!input.trim().is_empty(), "нечего отправлять: пустой материал");
        anyhow::ensure!(
            input.chars().count() <= MAX_INPUT_CHARS,
            "материал слишком большой (> {MAX_INPUT_CHARS} символов) — сократите или разбейте"
        );

        let provider = req
            .provider
            .as_deref()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .unwrap_or("claude")
            .to_string();

        let now = chrono::Local::now();
        let ask = Ask {
            id: asks::new_id(now),
            status: AskStatus::Pending,
            created_at: now.to_rfc3339(),
            provider,
            model: None,
            prompt: asks::effective_prompt(req.prompt.as_deref()),
            input_kind: "text".into(),
            input_name: req.input_name.filter(|s| !s.trim().is_empty()),
            input_chars: input.chars().count(),
            answer: None,
            cost_usd: None,
            error: None,
        };
        asks::save(&self.asks_root(), &ask, &input).context("сохранение запроса в asks/")?;
        Ok(ask)
    }

    /// PROCESS one pending request — the worker's single step. Marks it `Running`, calls the model,
    /// then writes the answer (or the error) and the terminal status. A FAILURE is saved, not
    /// thrown: the person's material and the reason are kept, and the worker moves on.
    pub fn process_ask(&self, id: &str) -> Result<localvox_light_core::asks::Ask> {
        use localvox_light_core::asks::{self, AskStatus};
        let root = self.asks_root();
        let mut ask = asks::load(&root, id)?;
        let input = asks::input(&root, id)?;

        ask.status = AskStatus::Running;
        asks::update(&root, &ask).context("отметить запрос выполняющимся")?;

        // Dispatch. `claude` — the subscription via the official CLI (claude_cli). Anything else —
        // the OpenAI-compatible client (Ollama / Gemini free-tier / an API key), which the rest of
        // the product already speaks; no new provider code for those.
        let outcome: Result<(String, Option<f64>, Option<String>)> = if ask.provider == "claude" {
            let cfg = localvox_light_llm::claude_cli::ClaudeCliConfig::from_env();
            localvox_light_llm::claude_cli::run(&cfg, &ask.prompt, Some(&input))
                .map(|a| (a.text, a.cost_usd, cfg.model.clone()))
        } else {
            let client = llm_client_from_env_with(300);
            let model = client.model().to_string();
            client
                .chat(&[
                    localvox_light_llm::system(ask.prompt.clone()),
                    localvox_light_llm::user(input),
                ])
                .map(|text| (text, None, Some(model)))
        };

        match outcome {
            Ok((text, cost, model)) => {
                ask.answer = Some(text);
                ask.cost_usd = cost;
                ask.model = model;
                ask.error = None;
                ask.status = AskStatus::Done;
            }
            Err(e) => {
                ask.error = Some(format!("{e:#}"));
                ask.status = AskStatus::Failed;
            }
        }
        asks::update(&root, &ask).context("сохранение ответа")?;
        Ok(ask)
    }

    /// The oldest request still waiting — the worker's next unit of work. `None` — the queue is dry.
    pub fn next_pending_ask(&self) -> Option<String> {
        localvox_light_core::asks::pending(&self.asks_root()).into_iter().next()
    }

    /// At startup, put any request left `Running` (the daemon died mid-call) back into the queue.
    pub fn reclaim_running_asks(&self) -> usize {
        localvox_light_core::asks::reclaim_running(&self.asks_root())
    }

    /// The list of past requests, newest first (for the section's sidebar).
    pub fn list_asks(&self) -> Vec<localvox_light_core::asks::AskSummary> {
        localvox_light_core::asks::list(&self.asks_root())
    }

    /// Delete one request entirely (input, answer, record). Returns the id so the caller can echo it.
    pub fn delete_ask(&self, id: &str) -> Result<String> {
        localvox_light_core::asks::delete(&self.asks_root(), id)?;
        Ok(id.to_string())
    }

    /// One request in full: the record plus the material it was made about.
    pub fn get_ask(&self, id: &str) -> Result<Value> {
        let ask = localvox_light_core::asks::load(&self.asks_root(), id)?;
        let input = localvox_light_core::asks::input(&self.asks_root(), id).unwrap_or_default();
        let mut v = serde_json::to_value(&ask)?;
        if let Value::Object(ref mut m) = v {
            m.insert("input".into(), Value::String(input));
        }
        Ok(v)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        let index = SearchIndex::open_or_build(&self.work_dir, false)?;
        Ok(index
            .search(query, limit.clamp(1, 50))?
            .into_iter()
            .map(|h| Hit {
                session: h.session,
                kind: h.kind,
                start_sec: h.start_sec,
                end_sec: h.end_sec,
                snippet: h.snippet,
                score: None,
                matched: h.matched,
                why: WHY_WORDS.into(),
            })
            .collect())
    }

    /// Search with a mode: `lexical` (tantivy, default), `semantic` (vectors via
    /// Ollama), `hybrid` (RRF merge of both — exact words AND meaning).
    pub fn search_mode(&self, query: &str, limit: usize, mode: &str) -> Result<Vec<Hit>> {
        // Empty query: do not drive Ollama and do not return top-k "matches" for a
        // meaningless vector (parity with the localvox-search CLI).
        if query.trim().is_empty() {
            return Ok(Vec::new());
        }
        let limit = limit.clamp(1, 50);
        match mode {
            "semantic" => Ok(self.semantic_hits(query, limit)?),
            "hybrid" => {
                // Each channel yields more candidates than needed; RRF merge:
                // score = Σ 1/(60+rank) — the standard RRF constant.
                //
                // The lexical channel can fail too — and not only by "crashing": the
                // tantivy query parser rejects whole classes of queries ("Only
                // excluding terms given"), and the hybrid used to fail ENTIRELY
                // because of that, even though semantics would have answered the very
                // same question perfectly. Degradation must be symmetric: as long as
                // at least one channel is alive, search works.
                let lex = self.search(query, limit * 2).unwrap_or_else(|e| {
                    tracing::warn!("hybrid: lexical unavailable, degrading to semantic: {e:#}");
                    Vec::new()
                });
                // Semantics is an optional module (Ollama may be turned off): the
                // hybrid degrades to lexical instead of failing entirely.
                let sem = self.semantic_hits(query, limit * 2).unwrap_or_else(|e| {
                    tracing::warn!("hybrid: semantics unavailable, degrading to lexical: {e:#}");
                    Vec::new()
                });
                let mut scored: Vec<(f64, Hit)> = Vec::new();
                // A hit that came from BOTH channels is the most reliable one: same
                // words and same meaning. A person needs to see that, not deduce it
                // from the ordering.
                let add = |hits: Vec<Hit>, scored: &mut Vec<(f64, Hit)>| {
                    // RRF: one contribution per channel per document (the best rank).
                    // Duplicates WITHIN a channel (summary and processed with the same
                    // paragraph) are merged for display but do not sum their scores —
                    // otherwise the md duplicate would unfairly outrank transcript
                    // hits.
                    let mut contributed: Vec<usize> = Vec::new();
                    for (rank, h) in hits.into_iter().enumerate() {
                        let rrf = 1.0 / (60.0 + rank as f64);
                        // dedup: same session + same line (start_sec)
                        if let Some(i) = scored.iter().position(|(_, e)| {
                            e.session == h.session
                                && match (e.start_sec, h.start_sec) {
                                    (Some(a), Some(b)) => (a - b).abs() < 0.5,
                                    (None, None) => e.snippet == h.snippet,
                                    _ => false,
                                }
                        }) {
                            if !contributed.contains(&i) {
                                contributed.push(i);
                                scored[i].0 += rrf;
                                if scored[i].1.why != h.why {
                                    scored[i].1.why = WHY_BOTH.into();
                                }
                                // We take the highlighting from wherever it exists:
                                // the semantic channel does not look for words, but it
                                // found the same line.
                                if scored[i].1.matched.is_empty() {
                                    scored[i].1.matched = h.matched.clone();
                                }
                            }
                        } else {
                            contributed.push(scored.len());
                            scored.push((rrf, h));
                        }
                    }
                };
                add(lex, &mut scored);
                add(sem, &mut scored);
                scored.sort_by(|a, b| b.0.total_cmp(&a.0));
                Ok(scored.into_iter().take(limit).map(|(_, h)| h).collect())
            }
            _ => self.search(query, limit),
        }
    }

    fn semantic_hits(&self, query: &str, limit: usize) -> Result<Vec<Hit>> {
        // `open_or_update` (re)indexes synchronously under an exclusive lock: the
        // first request after a cook/refine may hold the thread for a long time, and
        // without a limit parallel requests would queue up on the lock and eat the
        // HTTP server's shared MAX_INFLIGHT — freezing the WHOLE API, including
        // /api/health (the same motive as RouteGuard for /api/route). On "busy" the
        // hybrid degrades to lexical (see search_mode).
        let _guard = SemanticGuard::try_acquire()
            .context("занято: семантический индекс обновляется, повторите")?;
        let idx = localvox_light_search::semantic::SemanticIndex::open_or_update(&self.work_dir)?;
        Ok(idx
            .search(query, limit)?
            .into_iter()
            .map(|h| Hit {
                session: h.session,
                // the same vocabulary of kinds as the lexical channel (transcript/
                // summary/processed) — the front end and the RRF dedup work uniformly
                kind: h.kind,
                start_sec: h.start_sec,
                end_sec: h.end_sec,
                // The closeness of meaning — the ONLY thing a human can judge a semantic hit
                // by: there are no words of his query in it.
                score: Some(h.score),
                // THE SAME formatter as the lexical channel: the RRF dedup of md hits
                // (they have start_sec=None) compares exactly the snippets — with
                // different formats one paragraph would be shown twice and would lose
                // the sum of its scores
                snippet: localvox_light_search::make_snippet(&h.text, query, 160),
                // We compute the matched words here as well: a semantic hit MAY contain
                // the query's words (then they are visible), and may contain none of
                // them — and then it is empty, which is the truth about this hit.
                matched: localvox_light_search::matched_terms(&h.text, query),
                why: WHY_MEANING.into(),
            })
            .collect())
    }

    pub fn list_sessions(&self, limit: usize) -> Vec<SessionInfo> {
        let mut names: Vec<String> = fs::read_dir(self.sessions_root())
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names.reverse(); // freshest first
        let live = localvox_light_core::jobs::recording_session(&self.work_dir);
        // We ask about the engine's liveness ONCE for the whole list, not per session:
        // this takes a file lock, and doing it fifty times in a row is silly.
        let engine = engine_alive(&self.work_dir);
        // What the secretary is doing with each session — from the queue, in one read.
        let job_of: std::collections::HashMap<String, String> = {
            let q = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
            use localvox_light_core::jobs::JobState as S;
            q.jobs()
                .iter()
                .filter_map(|j| {
                    let state = match j.state {
                        S::Running => "running",
                        S::Pending => "pending",
                        S::Failed => "failed",
                        S::Done => return None,
                    };
                    Some((j.session.clone(), state.to_string()))
                })
                .collect()
        };
        names
            .into_iter()
            .take(limit.clamp(1, 100))
            .map(|name| {
                let dir = self.sessions_root().join(&name);
                let meta: Option<localvox_light_core::chunks::SessionMeta> =
                    std::fs::read(dir.join("meta.json"))
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok());
                // Duration — from the start of the recording to the end of the last
                // chunk: silence and pauses are included, because that IS the
                // recording time.
                let duration_sec = meta
                    .as_ref()
                    .map(|m| {
                        m.chunks
                            .iter()
                            .map(|c| c.start_offset_sec + c.duration_sec)
                            .fold(0.0f64, f64::max)
                    })
                    .unwrap_or(0.0);
                let store = VersionStore::open(&dir).ok();
                let lines = store
                    .as_ref()
                    .and_then(|s| s.best())
                    .and_then(|v| store.as_ref().and_then(|s| s.resolve(v.id)))
                    .and_then(|p| read_transcript_lines(&p).ok())
                    .map(|l| l.len())
                    .unwrap_or(0);
                SessionInfo {
                    started_at: meta.as_ref().map(|m| m.started_at.clone()),
                    stopped_at: meta.as_ref().and_then(|m| m.stopped_at.clone()),
                    stopped_reason: meta.as_ref().and_then(|m| m.stopped_reason.clone()),
                    duration_sec,
                    empty: lines == 0,
                    lang: meta.as_ref().and_then(|m| m.lang.clone()),
                    lang_detected: meta.as_ref().and_then(|m| m.lang_detected.clone()),
                    title: meta.as_ref().and_then(|m| m.title.clone()),
                    // A meeting is either one marked by a person or a caught call:
                    // both signs are equally real.
                    meeting: meta
                        .as_ref()
                        .map(|m| m.meeting || !m.meetings.is_empty())
                        .unwrap_or(false),
                    transcript_versions: store
                        .as_ref()
                        .map(|s| s.load().versions.len())
                        .unwrap_or(0),
                    has_summary: dir.join("summary.md").exists(),
                    has_processed: localvox_light_core::readable::exists(&dir),
                    // The marker is the engine's own word. We check not "file
                    // freshness" but whether the engine is alive: an empty newborn
                    // session is a recording, not a corpse.
                    recording: live.as_deref() == Some(name.as_str()) && engine,
                    cooked: store.as_ref().map(|s| !s.load().versions.is_empty()).unwrap_or(false),
                    job: job_of.get(&name).cloned(),
                    summary_doubts: localvox_light_core::processing::doubts(
                        &dir,
                        localvox_light_core::processing::SUMMARY,
                    ),
                    processed_doubts: localvox_light_core::processing::doubts(
                        &dir,
                        localvox_light_core::processing::PROCESSED,
                    ),
                    name,
                }
            })
            .collect()
    }

    /// The session directory, guarded against escaping the archive: the name MUST be
    /// exactly one normal path component — this cuts off not only `/`, `\`, `..` but
    /// also Windows exotica like `C:`, `.` and UNC prefixes.
    pub fn session_dir(&self, session: &str) -> Result<PathBuf> {
        use std::path::Component;
        let mut comps = Path::new(session).components();
        if !matches!(
            (comps.next(), comps.next()),
            (Some(Component::Normal(_)), None)
        ) {
            bail!("некорректное имя сессии");
        }
        let dir = self.sessions_root().join(session);
        if !dir.is_dir() {
            bail!("сессия не найдена: {session}");
        }
        Ok(dir)
    }

    /// The lines of the recording, anchored to the audio: the view for LISTENING and checking a
    /// fragment against the sound.
    ///
    /// It shows the CLEANED wording when there is one, and that is a deliberate reversal. It used
    /// to insist on the raw cook, because a wording combed by an LLM is a derivative and the
    /// recording had to stay evidence (acceptance 13.07.2026: the model turned «там кратно превы…»
    /// into «там кратно выручка превышает расходы»). What made that reversal safe is not a change
    /// of mind but a change of mechanism: the cleanup no longer composes a document. It rewords
    /// ONE line at a time, each judged against its OWN original — nothing may be added
    /// (`grounding::check`) and nothing replaced (`keeps_what_was_said`) — and a line that fails
    /// keeps the recognizer's words. The evidence is still there, in the version file, untouched:
    /// this is a rendering of it, and the play button beside every line is the check that matters.
    ///
    /// Both views draw from the same join, so «расшифровка» and «читаемый текст» can never
    /// disagree about what was said. They differ in FORM: lines against the audio here, continuous
    /// prose there.
    pub fn transcript(&self, session: &str) -> Result<TranscriptDoc> {
        let dir = self.session_dir(session)?;
        let store = VersionStore::open(&dir)?;

        // The cleanup pins the version it read. Serving its wording over any OTHER version would
        // put one text's cleaning under another text's line numbers.
        if let Ok(r) = localvox_light_core::readable::load(&dir) {
            if let Ok(lines) = localvox_light_core::readable::lines(&dir) {
                let v = store.load().versions.into_iter().find(|v| v.id == r.version_id);
                return Ok(TranscriptDoc {
                    session: session.to_string(),
                    label: v.as_ref().map(|v| v.label.clone()).unwrap_or_default(),
                    model: v.map(|v| v.model).unwrap_or_default(),
                    lines: lines
                        .into_iter()
                        .map(|l| TranscriptLineOut {
                            who: l.who,
                            source_id: l.source_id,
                            start_sec: l.start_sec,
                            end_sec: l.end_sec,
                            text: l.text,
                        })
                        .collect(),
                });
            }
        }

        // No cleanup yet — the recognizer's own words, which is the honest state and not a
        // fallback for compatibility: nothing has cleaned this recording.
        let manifest = store.load();
        let raw = manifest
            .versions
            .iter()
            .filter(|v| v.parents.is_empty()) // built from audio, not from text
            .max_by_key(|v| v.id)
            .cloned();
        let best = raw
            .or_else(|| store.best())
            .context("у сессии нет транскрипта (сначала localvox-process)")?;
        let path = store.resolve(best.id).context("файл версии не найден")?;
        let lines = read_transcript_lines(&path)?;
        // Who «Я» actually is depends on where the audio came from — a link is not the owner
        // speaking — and on what the human called the source. One place decides: `source_label`.
        let meta: localvox_light_core::chunks::SessionMeta = fs::read(dir.join("meta.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        Ok(TranscriptDoc {
            session: session.to_string(),
            label: best.label,
            model: best.model,
            lines: lines
                .into_iter()
                .map(|l| TranscriptLineOut {
                    // Diarization gives the real speaker; without it — the audio source.
                    who: match l.speaker.clone() {
                        Some(name) => name,
                        None => localvox_light_core::chunks::source_label(&meta, l.source_id),
                    },
                    source_id: l.source_id,
                    start_sec: l.start_sec,
                    end_sec: l.end_sec,
                    text: l.text,
                })
                .collect(),
        })
    }

    /// The summary, as blocks with their sources RESOLVED TO TIMECODES.
    ///
    /// The file stores line indices — that is the durable form, since a timecode belongs to the
    /// transcript and would go stale the moment a recording is cooked again. The client gets
    /// seconds, because what it does with a source is play it, and making every client re-open
    /// the transcript to turn 12 into 01:30 would be handing them our homework.
    ///
    /// A citation whose line no longer exists is dropped here rather than sent as a dead button.
    /// The claim stays: it was checked when it was written, and a re-cooked transcript does not
    /// make it false — only unlocatable.
    pub fn summary_blocks(&self, session: &str) -> Result<serde_json::Value> {
        let dir = self.session_dir(session)?;
        let text = self.artifact(session, "summary.md")?;
        let (prov, body) = localvox_light_core::provenance::split(&text);
        let blocks = localvox_light_core::citations::parse(body);

        let lines = VersionStore::open(&dir)
            .ok()
            .and_then(|s| s.best().and_then(|b| s.resolve(b.id)))
            .and_then(|p| read_transcript_lines(&p).ok())
            .unwrap_or_default();

        let out: Vec<serde_json::Value> = blocks
            .into_iter()
            .map(|b| {
                let sources: Vec<serde_json::Value> = b
                    .lines
                    .iter()
                    .filter_map(|&i| lines.get(i).map(|l| (i, l)))
                    .map(|(i, l)| json!({"line": i, "start_sec": l.start_sec, "end_sec": l.end_sec}))
                    .collect();
                json!({"kind": b.kind, "text": b.text, "sources": sources})
            })
            .collect();
        Ok(json!({"blocks": out, "provenance": prov}))
    }

    /// Audio clip of a fragment (F6 player): sample-accurately glues the needed range
    /// out of the source's chunks → WAV 16 kHz mono s16. For "found it in search →
    /// listened to it". `dur_sec` is bounded, the audio may have been deleted by
    /// retention. A piece of the recording as WAV.
    ///
    /// `source_id`: 0 — microphone, 1 — system audio, `None` — a MIX of both. The mix
    /// is needed so that the recording can simply be listened to: a conversation
    /// consists of both tracks, and listening to half a dialogue is pointless.
    pub fn audio_clip(
        &self,
        session: &str,
        source_id: Option<u8>,
        start_sec: f64,
        dur_sec: f64,
    ) -> Result<Vec<u8>> {
        let dir = self.session_dir(session)?;
        let meta_txt = fs::read_to_string(dir.join("meta.json")).context("нет meta.json сессии")?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_str(&meta_txt).context("meta.json повреждён")?;

        let dur = dur_sec.clamp(0.1, MAX_CLIP_SEC);
        let want_start = (start_sec.max(0.0) * SR) as u64;
        let want_end = ((start_sec.max(0.0) + dur) * SR) as u64;

        let pcm = match source_id {
            Some(id) => source_pcm(&dir, &meta, id, want_start, want_end)?,
            None => {
                // The mix: the tracks are glued sample-to-sample from the start of the
                // recording, so they add up position by position. A missing track (a
                // monologue into the microphone, audio without a microphone) is not an
                // error — we play the one that exists; it is an error only if there is
                // NOT A SINGLE one.
                let mic = source_pcm(&dir, &meta, 0, want_start, want_end).unwrap_or_default();
                let sys = source_pcm(&dir, &meta, 1, want_start, want_end).unwrap_or_default();
                anyhow::ensure!(
                    !mic.is_empty() || !sys.is_empty(),
                    "нет аудио для фрагмента ({:.1}–{:.1} c)",
                    start_sec.max(0.0),
                    start_sec.max(0.0) + dur
                );
                mix(&mic, &sys)
            }
        };
        Ok(wav_16k_mono(&pcm))
    }

    pub fn artifact(&self, session: &str, file: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        if let Ok(text) = fs::read_to_string(dir.join(file)) {
            return Ok(text);
        }
        // We explain the REASON instead of retelling the file-system error: "file not
        // found" tells the user nothing, whereas "the result is in quarantine" or
        // "this flag creates it" does.
        if file.ends_with(".unverified.md") {
            bail!("черновика нет: последний результат подтверждён записью");
        }
        let draft = file.replace(".md", ".unverified.md");
        if dir.join(&draft).exists() {
            bail!(
                "результат не подтверждён записью и сохранён черновиком — \
                 откройте вкладку «⚠ Черновик»"
            );
        }
        // We suggest THE flag that actually creates this file: previously, for a
        // missing processed.md we advised `--summary` — a command that does not make
        // it.
        let flag = match file {
            "summary.md" => "--summary",
            f if f == localvox_light_core::readable::FILE => "--cleanup",
            _ => "--summary --cleanup",
        };
        bail!(
            "нет {file} у этой сессии. Сгенерировать: localvox-process <сессия> {flag} \
             (пусто и после этого — значит, в записи не нашлось содержательной речи)"
        )
    }

    /// "This is bad — re-cook it." Throws away EVERYTHING derived (the summary, the
    /// cleaned-up text, the drafts) and puts the session up for a repeat cook with
    /// `--force`: a new transcript version, a new best, rebuilt indexes.
    ///
    /// The audio and `meta.json` are not touched — they are primary and inviolable,
    /// and everything else is recreated from them. That is why throwing away the
    /// derivatives is not a loss: it is the only way to let a person say "the model
    /// lied" and get the result anew instead of arguing with the archive.
    ///
    /// Old transcript versions stay in the manifest (version history — P2): `--force`
    /// adds a new one and makes it `best` instead of erasing the previous ones.
    /// Has the current run's progress not moved for longer than any normal quiet stretch (a summary
    /// ~4 min, a model load ~2.5 min)? With per-chunk/per-batch heartbeats a live cook updates every
    /// few seconds, so a long silence means a dead process. Shared by the progress view (to show
    /// «оборвалось») and by recook (to let a human override a stalled job).
    fn progress_stale(&self, dir: &Path) -> bool {
        let stages =
            localvox_light_core::progress::fold(&localvox_light_core::progress::read(dir));
        let ts = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp());
        let now = chrono::Local::now().timestamp();
        stages
            .iter()
            .filter_map(|s| s.updated_at.as_deref())
            .filter_map(ts)
            .max()
            .is_some_and(|t| now - t > COOK_STALL_SEC)
    }

    pub fn recook(&self, session: &str, scope: RecookScope) -> Result<String> {
        use localvox_light_core::processing as proc;
        let dir = self.session_dir(session)?;
        // Preconditions first, destruction after.
        self.ensure_recookable(session, &dir)?;

        let readable = localvox_light_core::readable::FILE;
        let mut queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);

        // A human's «Переварить» must WIN over a stalled cook — a job stuck `Running` because the
        // daemon (or the cook) died mid-work. The enqueue methods refuse a Running job to protect a
        // LIVE cook; so if nothing has moved for a long time, we reclaim that job first and let the
        // re-cook proceed. A genuinely-live cook is not stale and is still refused below — its own
        // work finishes, and the message «оборвалось, нажмите Переварить» does what it promises.
        if self.progress_stale(&dir)
            && queue.jobs().iter().any(|j| {
                j.session == session
                    && matches!(j.state, localvox_light_core::jobs::JobState::Running)
            })
        {
            queue.reclaim_session(session);
        }

        // What each scope THROWS AWAY, what records it FORGETS (no file → no «done» record, or
        // discovery would decide there is nothing to remake), and how it re-queues. «Реплики» and
        // «Текст» are one artifact — processed.json — so both mean «cleanup + summary»; «Сводка» is
        // the leaf; «Всё» redoes the transcript too. Summary and Text re-queue WITHOUT force (the
        // transcript stays, only the missing derivatives are remade); All forces from the audio up.
        //
        // We queue the work FIRST and destroy afterwards: a queue refusal («already cooking») must
        // not leave the session stripped of its derivatives with no job to bring them back.
        let (files, forget, queued): (Vec<&str>, Vec<&str>, bool) = match scope {
            RecookScope::Summary => (
                vec!["summary.md", "summary.unverified.md"],
                vec![proc::SUMMARY],
                queue.requeue_derivatives(session, true, false, false),
            ),
            RecookScope::Text => (
                vec!["summary.md", readable, "summary.unverified.md", "processed.unverified.md"],
                vec![proc::PROCESSED, proc::SUMMARY],
                queue.requeue_derivatives(session, true, true, false),
            ),
            RecookScope::All => {
                // Full redo uses the daemon's post-processing flags, from the SHARED place — our own
                // reading of the same env once diverged («no variable = disabled» here, «enabled» for
                // the daemon), so «redo» erased the summary and queued a job that never remade it.
                let (summary, cleanup, refine) =
                    localvox_light_core::jobs::post_processing_from_env();
                (
                    vec!["summary.md", readable, "summary.unverified.md", "processed.unverified.md"],
                    vec![proc::SUMMARY, proc::PROCESSED, proc::TRANSCRIPT],
                    queue.enqueue_recook(session, summary, cleanup, refine),
                )
            }
        };

        if !queued {
            anyhow::bail!("сессия уже варится прямо сейчас — дождитесь окончания");
        }

        // A NEW RUN STARTS HERE, at the press — not when the daemon gets round to it. Until then the
        // chain would show the PREVIOUS run: all green, all finished, while the work has not started.
        localvox_light_core::progress::new_run(&dir);

        let mut dropped = Vec::new();
        for f in files {
            if fs::remove_file(dir.join(f)).is_ok() {
                dropped.push(f);
            }
        }
        proc::forget(&dir, &forget);

        tracing::info!("re-cook on demand: {session} ({}, dropped: {dropped:?})", scope.label());
        Ok(format!(
            "поставлено на переварку ({}); выброшено производных файлов: {}",
            scope.label(),
            dropped.len()
        ))
    }

    /// Whether this session may be re-cooked. Checked BEFORE any destruction —
    /// including before a language change: changing the language and then failing to
    /// re-cook means leaving the person with a summary in the language they have just
    /// rejected.
    fn ensure_recookable(&self, session: &str, dir: &Path) -> Result<()> {
        // A live session MUST NOT be re-cooked: the cook would commit a PARTIAL
        // transcript with the current recipe, and discovery would consider it cooked
        // forever — the rest of the recording would never be transcribed. We check
        // against the marker, not against file freshness: while paused the files are
        // not updated, but the engine is alive and will finish writing the session.
        if localvox_light_core::jobs::recording_session(&self.work_dir).as_deref() == Some(session)
        {
            bail!("сессия записывается прямо сейчас — остановите запись, потом переваривайте");
        }
        // No audio — there is nothing to re-cook from, and all the more reason not to
        // erase the derivatives: they are the last thing left of the session.
        let has_audio = std::fs::read_dir(dir.join("audio"))
            .map(|rd| rd.flatten().any(|e| e.path().is_file()))
            .unwrap_or(false);
        if !has_audio {
            bail!("в сессии нет аудио (удалено по retention?) — переваривать нечего");
        }
        Ok(())
    }

    /// Delete a session ENTIRELY — audio and everything derived from it.
    ///
    /// This is the ONLY irreversible action in the whole system. Everywhere else the audio is the
    /// source of truth and the derivatives are disposable — a bad summary is re-cooked, a wrong
    /// language is redone, all from the audio. Here the audio itself goes, and nothing brings it
    /// back. So the guards are real, not decorative:
    ///
    ///  * a session being RECORDED right now is not deletable — stop it first, or the engine keeps
    ///    writing into a directory we just removed;
    ///  * the caller must pass the session's own name back as `confirm`. A generic "yes" is too
    ///    easy to click through; echoing the name means the deletion was aimed, not fat-fingered.
    ///
    /// The queue entry goes too — a job pointing at a directory that no longer exists would fail
    /// forever. The search index rebuilds itself from the session files, so the deleted session
    /// drops out of it on the next search with no extra work here.
    pub fn delete_session(&self, session: &str, confirm: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        if !dir.exists() {
            bail!("сессия не найдена — возможно, уже удалена");
        }
        if localvox_light_core::jobs::recording_session(&self.work_dir).as_deref() == Some(session) {
            bail!("сессия записывается прямо сейчас — сначала остановите запись");
        }
        if confirm != session {
            // Not a user-facing message: the UI names the session itself. This only fires if a
            // script calls the API by hand, and then it must say exactly what it wants.
            bail!("удаление не подтверждено: передайте имя сессии в поле confirm");
        }

        let mut queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        queue.forget_session(session);

        std::fs::remove_dir_all(&dir).with_context(|| format!("не удалось удалить {session}"))?;
        tracing::info!("session deleted: {session}");
        Ok(session.to_string())
    }

    /// Change the language of the recording. `None` — go back to auto-detection.
    ///
    /// There is no need for separate code to reset the derivatives: the language is
    /// part of the recipes, so the transcript and the summary have already become
    /// "made the wrong way" and will be made anew. We only erase the derived files
    /// right away, without waiting for the queue — so that the person does not read a
    /// summary in a language they rejected.
    pub fn set_lang(&self, session: &str, lang: Option<&str>) -> Result<String> {
        let dir = self.session_dir(session)?;
        self.ensure_recookable(session, &dir)?;

        // The model for the language MUST exist BEFORE we destroy anything. Otherwise
        // one click in the UI looked like this: the language is written down, the
        // summary and the cleaned-up text are deleted, the answer is "ok, queued for
        // re-cook" — and the cook fails on "no model", and the artifacts do not come
        // back. The person was told "done", yet they lost the summary and will not
        // guess why.
        if let Some(code) = lang {
            let code = localvox_light_core::lang::normalize(code)?;
            let default_dir = localvox_light_core::lang::default_model_dir()
                .context("не найден каталог ASR-модели — укажите LOCALVOX_ASR_MODEL_DIR")?;
            if !localvox_light_core::lang::has_model(&code, &default_dir) {
                // The same text as the cook's: one cause — one explanation.
                localvox_light_core::lang::asr_model(&code, &default_dir)?;
            }
        }

        if !localvox_light_core::lang::set(&dir, lang)? {
            return Ok("язык не изменился".into());
        }
        // A language change invalidates EVERYTHING (the transcript picks the model by language), so
        // it is always a full redo.
        let msg = self.recook(session, RecookScope::All)?;
        Ok(match lang {
            Some(code) => format!("язык: {code}; {msg}"),
            None => format!("язык: авто; {msg}"),
        })
    }

    /// What the recorder is doing right now.
    ///
    /// `engine` is a separate fact from `recording`, and the app needs both: with a dead
    /// daemon a "record" button would look pressable and go nowhere, and the person would
    /// be left believing they are being recorded. That is the one lie this system must
    /// never tell.
    pub fn record_state(&self) -> RecordState {
        let session = localvox_light_core::jobs::recording_session(&self.work_dir);
        let engine = engine_alive(&self.work_dir);
        RecordState {
            engine,
            recording: engine && session.is_some(),
            session: session.filter(|_| engine),
            preroll_sec: env_f64("LOCALVOX_LIGHT_PREROLL_SEC", 300.0),
            // Gated on `engine` for the same reason as `session`: the marker is a file, and a
            // killed daemon does not clear it. Without the engine there is nobody listening, and
            // a screen saying otherwise is the one lie this system must never tell.
            voice: engine
                .then(|| localvox_light_core::voice_note::read(&self.work_dir))
                .flatten(),
        }
    }

    /// Start recording. Nothing is written to disk until this is asked for — but the last
    /// minutes are held in memory, and they go into the session, so a conversation that
    /// began before the button is not lost.
    pub fn start_recording(&self, title: &str) -> Result<String> {
        if let Some(name) = localvox_light_core::jobs::recording_session(&self.work_dir) {
            bail!("запись уже идёт: {name}");
        }
        let title = title.trim();
        localvox_light_core::jobs::request_record_start(&self.work_dir, title)
            .context("не удалось попросить движок начать запись")?;
        Ok(if title.is_empty() {
            "запись начата".into()
        } else {
            format!("запись начата: {title}")
        })
    }

    /// Who spoke in the recording: the roster of participants with their captions.
    ///
    /// Empty — either there was no diarization (no model) or no voices were found.
    /// Both are honest: we MUST NOT silently show «Участник 1» where we counted
    /// nobody.
    /// WHO IS IN THIS RECORDING — derived from the lines themselves, never from a register.
    ///
    /// It used to come from the diarization roster, and the roster lies by omission in both
    /// directions. Measured 19.07.2026 on the owner's archive:
    ///
    ///   20260718_230541_youtube  roster: «Участник 1» (1.4 мин) + «Арсен Маркарян» (32.7 мин)
    ///                            text:   «Арсен Маркарян» only. «Участник 1» says NOT ONE LINE.
    ///   20260716_164443          roster lists «Участник 3»; the text has no such line either,
    ///                            and has «Собеседники», which the roster knows nothing about.
    ///
    /// A phantom in this list is not a cosmetic flaw: the owner is offered a rename for somebody
    /// who never speaks in their document, and the count of participants is wrong. So the list is
    /// GROUPED FROM THE LINES. Someone who says nothing cannot appear, because there is nothing to
    /// group them from.
    ///
    /// It also answers the question the old screen could not: «чем «Собеседники» отличается от
    /// «Участник 1»». Both are attributions in the same text, produced by different mechanisms —
    /// a voice diarization separated, versus the input the sound arrived through — and each entry
    /// now says which of the two it is.
    pub fn speakers(&self, session: &str) -> Result<Vec<ParticipantOut>> {
        let dir = self.session_dir(session)?;
        let store = VersionStore::open(&dir)?;
        // The very version the document shows, so the list and the text can never disagree.
        let id = localvox_light_core::readable::load(&dir)
            .map(|r| r.version_id)
            .ok()
            .or_else(|| store.best().map(|v| v.id))
            .context("у сессии нет транскрипта (сначала localvox-process)")?;
        let path = store.resolve(id).context("файл версии не найден")?;
        let lines = read_transcript_lines(&path)?;
        let meta: localvox_light_core::chunks::SessionMeta = fs::read(dir.join("meta.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();

        // Keyed by what PRODUCED the attribution, not by the name it currently shows: a source
        // renamed to the same string as a voice is still a different thing, and merging them would
        // hand one rename to the other.
        let mut order: Vec<(Option<String>, u8)> = Vec::new();
        let mut acc: std::collections::HashMap<(Option<String>, u8), (f64, usize)> =
            std::collections::HashMap::new();
        for l in &lines {
            let key = (l.speaker.clone(), l.source_id);
            let e = acc.entry(key.clone()).or_insert_with(|| {
                order.push(key.clone());
                (0.0, 0)
            });
            e.0 += (l.end_sec - l.start_sec).max(0.0);
            e.1 += 1;
        }

        // ONE ROW PER NAME. The two mechanisms can land on the same name, and then they are one
        // person as far as anyone reading is concerned. Measured on the owner's video: 73 lines
        // carried the separated voice «Арсен Маркарян» and 3 more had no voice separated, so they
        // fell back to the source — which he had renamed to «Арсен Маркарян» too. Listed apart,
        // that reads as the same man twice, and honest data looks like a defect.
        //
        // Merged, the row still states the whole truth in `what`: which mechanisms attributed it.
        // And it keeps BOTH keys, because renaming «этого человека» has to reach every line the
        // screen shows under that name — a rename that fixed 73 lines and left 3 behind would be
        // the same defect wearing a different shape.
        let mut rows: Vec<ParticipantOut> = Vec::new();
        for key in order {
            let (speech_sec, lines) = acc[&key];
            let (voice, source_id) = key;
            let (name, what, named) = match &voice {
                // Diarization separated this voice. «Участник N» means exactly «a voice we could
                // not put a name to» — an honest answer, and the button asks for one.
                Some(label) => (
                    label.clone(),
                    "отдельный голос",
                    !label.starts_with("Участник"),
                ),
                // No voice was separated — the line is attributed to the INPUT it came through,
                // and that is all we honestly know about it.
                None => (
                    localvox_light_core::chunks::source_label(&meta, source_id),
                    if source_id == 0 {
                        if meta.source.is_some() {
                            "звук из источника"
                        } else {
                            "микрофон"
                        }
                    } else {
                        "системный звук"
                    },
                    meta.source_names.contains_key(&source_id.to_string()),
                ),
            };

            let row = match rows.iter().position(|r| r.name == name) {
                Some(i) => {
                    let r = &mut rows[i];
                    r.speech_sec += speech_sec;
                    r.lines += lines;
                    r.named |= named;
                    if !r.what.split(" · ").any(|p| p == what) {
                        r.what = format!("{} · {what}", r.what);
                    }
                    r
                }
                None => {
                    rows.push(ParticipantOut {
                        name,
                        what: what.to_string(),
                        named,
                        voices: Vec::new(),
                        sources: Vec::new(),
                        speech_sec,
                        lines,
                    });
                    rows.last_mut().unwrap()
                }
            };
            match voice {
                Some(label) => row.voices.push(label),
                None => row.sources.push(source_id),
            }
        }
        // Loudest first: the person who carried the recording belongs at the top.
        rows.sort_by(|a, b| b.speech_sec.total_cmp(&a.speech_sec));
        Ok(rows)
    }

    /// What each audio SOURCE is called in this session — with the name in force right now.
    ///
    /// Sources are not speakers: they are «which input the sound came through». Diarization names
    /// voices; this names the fallback used when it has not. The two live in different places
    /// because they are different facts.
    pub fn sources(&self, session: &str) -> Result<Vec<serde_json::Value>> {
        let dir = self.session_dir(session)?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).context("нет meta.json")?)?;
        // Only the sources this recording actually HAS: a session that arrived by link has no
        // second track, and offering to rename one would invent a participant.
        let ids: Vec<u8> = if meta.source.is_some() { vec![0] } else { vec![0, 1] };
        Ok(ids
            .into_iter()
            .map(|id| {
                json!({
                    "source_id": id,
                    "name": localvox_light_core::chunks::source_label(&meta, id),
                    "custom": meta.source_names.contains_key(&id.to_string()),
                    "what": if id == 0 {
                        if meta.source.is_some() { "звук из источника" } else { "микрофон" }
                    } else {
                        "системный звук"
                    },
                })
            })
            .collect())
    }

    /// Rename a source: the narrator of a downloaded video is not «Я».
    ///
    /// ONLY THE SUMMARY IS REBUILT. It is prose the model wrote, with the name inside its
    /// sentences — «Я рассказываю про…» can only become «Арсен рассказывает про…» by writing the
    /// summary again. Leaving it would pass the correction off as taken into account.
    ///
    /// The readable text is NOT rebuilt and NOT deleted, and that is the whole point of storing it
    /// as a delta: it holds no name at all. The label is joined in from `meta.json` on every read,
    /// so it already says the new name — before this function returns. Re-cooking it would burn
    /// minutes of LLM time to arrive at exactly the file that is already on disk.
    pub fn name_source(&self, session: &str, source_id: u8, name: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        let meta_path = dir.join("meta.json");
        let mut meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(&meta_path).context("нет meta.json")?)?;
        let name = name.trim();
        if name.is_empty() {
            // Back to the default — which depends on where the audio came from.
            meta.source_names.remove(&source_id.to_string());
        } else {
            meta.source_names
                .insert(source_id.to_string(), name.to_string());
        }
        localvox_light_core::chunks::save_meta_public(&meta_path, &meta);
        let now = localvox_light_core::chunks::source_label(&meta, source_id);

        // Neither the transcript lines nor the readable text are touched: the label is not stored
        // in either of them, it is rendered from the source id on every read. Only the summary
        // QUOTES it, inside sentences the model composed, and only the summary is rebuilt.
        let (summary, _cleanup, refine) = localvox_light_core::jobs::post_processing_from_env();
        let mut queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        let queued = queue.requeue_for_artifacts(session, summary, false, refine);
        if queued {
            let _ = fs::remove_file(dir.join("summary.md"));
            // No file — no record of it either, or discovery decides there is nothing to do and
            // the artifact never comes back.
            localvox_light_core::processing::forget(
                &dir,
                &[localvox_light_core::processing::SUMMARY],
            );
        }
        Ok(format!(
            "теперь «{now}» — текст и реплики переподписаны сразу{}",
            if queued {
                ", сводка пересобирается"
            } else {
                "; сессия уже в работе — сводка пересоберётся там"
            }
        ))
    }

    /// Name a voice: «Участник 2» is Ivan.
    ///
    /// Does three things, and all three are mandatory:
    ///
    /// 1. **remembers the voice under the name** — in later sessions it will be
    ///    recognized by itself;
    /// 2. **re-labels the lines** in every version of the transcript — WITHOUT
    ///    re-recognition: the audio has not changed, only one caption has to change;
    /// 3. **throws away the summary** and queues it for a rebuild — it says «Участник 2»
    ///    inside sentences the model composed, and leaving that means lying that the name
    ///    was taken into account. The readable text needs nothing: it stores no name, and
    ///    step 2 is already the only place the caption lives.
    ///
    /// A person's voice is biometrics. A profile appears only when it has been named
    /// HERE, by hand: we MUST NOT silently accumulate prints of everyone who got into
    /// the microphone.
    pub fn name_speaker(&self, session: &str, label: &str, name: &str) -> Result<String> {
        let name = name.trim();
        anyhow::ensure!(!name.is_empty(), "имя не может быть пустым");
        let dir = self.session_dir(session)?;

        let mut roster = localvox_light_core::diarize::roster::load(&dir);
        let member = roster
            .by_label(label)
            .cloned()
            .with_context(|| format!("в этой записи нет говорящего «{label}»"))?;
        if member.label == name {
            return Ok(format!("«{name}» — и так его имя"));
        }

        // We remember the voice FIRST: if there was not enough speech for a profile,
        // we have not managed to destroy anything. The order "preconditions first,
        // destruction after" is the same as for the language change, and for the same
        // reason.
        localvox_light_core::diarize::profiles::enroll(
            &self.work_dir,
            name,
            &member.embedding,
            member.speech_sec,
        )?;

        roster.rename(label, name);
        localvox_light_core::diarize::roster::save(&dir, &roster)?;

        let store = VersionStore::open(&dir)?;
        let lines = store.relabel_speaker(label, name)?;

        // Only the SUMMARY is written about «Участник 2» — it is prose, and the name is inside
        // its sentences. The readable text quotes nothing: it renders the caption from the very
        // lines just relabelled, so it already says the new name. The audio is not touched
        // either: re-cooking half an hour of sound for the sake of a name is a mockery, and then
        // names simply would not be used at all.
        let (summary, _cleanup, refine) = localvox_light_core::jobs::post_processing_from_env();
        let mut queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        let queued = queue.requeue_for_artifacts(session, summary, false, refine);
        if queued {
            let _ = fs::remove_file(dir.join("summary.md"));
            // No file — no record of it either: otherwise discovery will decide there
            // is nothing to do, and the artifact will not come back.
            localvox_light_core::processing::forget(
                &dir,
                &[localvox_light_core::processing::SUMMARY],
            );
        }

        Ok(format!(
            "«{label}» → «{name}»: переподписано строк: {lines}{}",
            if queued {
                "; сводка пересобирается"
            } else {
                "; сессия уже в работе — сводка пересоберётся там"
            }
        ))
    }

    /// The voices we know by name (shared across the whole archive).
    pub fn known_speakers(&self) -> Vec<SpeakerOut> {
        localvox_light_core::diarize::profiles::list(&self.work_dir)
            .into_iter()
            .map(|(label, speech_sec)| SpeakerOut {
                label,
                speech_sec,
                owner: false,
            })
            .collect()
    }

    /// Forget a voice. A person MUST be able to erase biometrics — with one button.
    /// The transcripts are not rewritten in the process: what was said was said, and
    /// the caption under it is a fact of the recording. What is erased is the PRINT,
    /// that is, the ability to recognize this voice from now on.
    pub fn forget_speaker(&self, name: &str) -> Result<String> {
        if localvox_light_core::diarize::profiles::forget(&self.work_dir, name)? {
            Ok(format!("голос «{name}» забыт"))
        } else {
            Ok(format!("голоса «{name}» мы и не знали"))
        }
    }

    /// A question to the archive (F4): an answer BASED ON THE FOUND FRAGMENTS, with
    /// citations to them. If nothing was found, the model is not called at all (see
    /// `chat::ask`).
    pub fn ask(&self, question: &str) -> Result<crate::chat::Answer> {
        // Temperature 0: the answer must not depend on a dice roll.
        //
        // And a CEILING on the answer. An answer about your own recordings is a few sentences —
        // the default 8192 tokens is a licence to ramble, and the model took it: asked «про код»,
        // it produced seventy lines of «в записях нет информации о том, как код связан с…»
        // (14.07.2026). The prompt asks for brevity; a ceiling enforces it.
        let client = llm_client_capped(
            env_secs("LOCALVOX_LLM_CHAT_TIMEOUT_SEC", 120),
            Some(0.0),
            env_secs("LOCALVOX_LLM_CHAT_MAX_TOKENS", 800) as u32,
        );
        crate::chat::ask(self, question, &client)
    }

    /// The languages we can REALLY recognize: the ones we have a model for. The web
    /// builds its selector from this list rather than from one hardcoded into the
    /// HTML — offering a language we cannot do means promising the impossible.
    pub fn langs(&self) -> Vec<String> {
        let Some(default_dir) = localvox_light_core::lang::default_model_dir() else {
            return Vec::new();
        };
        // The default one plus those that have a model on disk. A list of short codes,
        // not a sweep of the whole of ISO-639: we look at what is on disk and in the
        // environment.
        let mut out = Vec::new();
        for code in localvox_light_core::lang::candidates(&default_dir) {
            if localvox_light_core::lang::has_model(&code, &default_dir) {
                out.push(code);
            }
        }
        out
    }

    /// The WHOLE recording as one WAV, sliced to a byte range.
    ///
    /// This is what makes the player instant. Instead of us re-fetching a window on every click,
    /// the browser is handed one `audio.wav` with HTTP Range support and does the seeking itself —
    /// it asks for exactly the bytes around the target and lands there at once. The daemon never
    /// holds the whole file: each range request produces only its own slice, mixed on the fly.
    ///
    /// `range` is `(start, end_inclusive)` from the `Range: bytes=` header; `None` — the whole
    /// file (which is still served slice-by-slice, never built entire).
    pub fn audio_wav(&self, session: &str, range: Option<(u64, Option<u64>)>) -> Result<AudioSlice> {
        let dir = self.session_dir(session)?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).context("нет meta.json")?)
                .context("meta.json не читается")?;

        let duration = meta
            .chunks
            .iter()
            .map(|c| c.start_offset_sec + c.duration_sec)
            .fold(0.0_f64, f64::max);
        // Even/whole samples: a WAV frame is 2 bytes, and an odd data length would desync s16.
        let pcm_len = ((duration * SR) as u64) * 2;
        let file_len = 44 + pcm_len;

        let (start, end) = match range {
            Some((s, e)) => {
                let end = e.map(|e| (e + 1).min(file_len)).unwrap_or(file_len);
                (s.min(file_len), end.max(s.min(file_len)))
            }
            None => (0, file_len),
        };

        let mut bytes = Vec::with_capacity((end - start) as usize);
        // The header, if the range reaches into the first 44 bytes.
        if start < 44 {
            let header = wav_header(pcm_len as u32);
            bytes.extend_from_slice(&header[start as usize..end.min(44) as usize]);
        }
        // The PCM part, mixed on the fly for exactly the samples this range covers.
        let pcm_from = start.max(44) - 44;
        let pcm_to = end.max(44) - 44;
        if pcm_to > pcm_from {
            let sample_from = pcm_from / 2;
            let sample_to = pcm_to.div_ceil(2);
            let mic = source_pcm(&dir, &meta, 0, sample_from, sample_to).unwrap_or_default();
            let sys = source_pcm(&dir, &meta, 1, sample_from, sample_to).unwrap_or_default();
            let mixed = mix(&mic, &sys);
            // `mixed` begins at byte `sample_from*2`; take the exact window the range asked for.
            let off = (pcm_from - sample_from * 2) as usize;
            let want = (pcm_to - pcm_from) as usize;
            let avail = mixed.len().saturating_sub(off);
            bytes.extend_from_slice(&mixed[off..off + want.min(avail)]);
            // A track can end before the timeline does (system audio goes quiet). Pad with silence
            // so the file's length always matches its declared duration — or the browser's clock
            // drifts against ours.
            if want > avail {
                bytes.resize(bytes.len() + (want - avail), 0);
            }
        }

        Ok(AudioSlice { total: file_len, start, end, bytes })
    }

    /// The shape of the recording: peak loudness per bucket, for the waveform in the player.
    ///
    /// It is REAL. A drawn wave (a sine, noise, anything "pretty") is worse than no wave at all:
    /// people navigate a recording by it — they aim at the loud part where the argument was — and
    /// a wave that does not match the sound sends them to the wrong place while looking exactly
    /// as trustworthy as one that does.
    ///
    /// One pass over the audio, cached in `peaks.json`. The cache is derived data and therefore
    /// disposable: delete it and it is recomputed from the chunks, which are the truth.
    pub fn peaks(&self, name: &str, buckets: usize) -> Result<Value> {
        let buckets = buckets.clamp(32, 2000);
        let dir = self.session_dir(name)?;
        let meta: localvox_light_core::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(dir.join("meta.json")).context("нет meta.json")?)
                .context("meta.json не читается")?;

        let duration = meta
            .chunks
            .iter()
            .map(|c| c.start_offset_sec + c.duration_sec)
            .fold(0.0_f64, f64::max);
        if duration <= 0.0 {
            return Ok(json!({ "duration_sec": 0.0, "peaks": [] }));
        }

        let cache = dir.join("peaks.json");
        if let Some(v) = fs::read(&cache)
            .ok()
            .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
            .filter(|v| v["buckets"] == json!(buckets))
            .filter(|v| (v["duration_sec"].as_f64().unwrap_or(0.0) - duration).abs() < 0.5)
        {
            return Ok(v);
        }

        // In windows, not all at once: an hour of a track is 115 MB, and holding two of them in
        // the daemon's memory to draw a strip 300 pixels wide is not an honest trade.
        const WINDOW_SEC: f64 = 60.0;
        let mut acc = vec![0f32; buckets];
        let mut from = 0.0_f64;
        while from < duration {
            let to = (from + WINDOW_SEC).min(duration);
            for source_id in 0..2u8 {
                // A chunk that cannot be read must not cost us the whole wave: the player is more
                // useful with a gap in the picture than absent because of one broken file.
                let pcm = source_pcm(
                    &dir,
                    &meta,
                    source_id,
                    (from * SR) as u64,
                    (to * SR) as u64,
                )
                .unwrap_or_default();
                for (i, s) in pcm.chunks_exact(2).enumerate() {
                    let v = f32::from(i16::from_le_bytes([s[0], s[1]]).saturating_abs());
                    let at = from + i as f64 / SR;
                    let b = ((at / duration) * buckets as f64) as usize;
                    let b = b.min(buckets - 1);
                    // The two tracks are NOT summed but taken at their max: the wave answers
                    // "was it loud here", and a sum would make a moment where both speak at once
                    // look twice as loud as the argument that followed it.
                    if v > acc[b] {
                        acc[b] = v;
                    }
                }
            }
            from = to;
        }

        // Normalized against the loudest moment of THIS recording, and through a square root: a
        // quiet conversation must be visible as speech, not as a flat line at the bottom.
        let loudest = acc.iter().copied().fold(0f32, f32::max).max(1.0);
        let peaks: Vec<f32> = acc.iter().map(|v| (v / loudest).sqrt()).collect();

        let out = json!({ "duration_sec": duration, "buckets": buckets, "peaks": peaks });
        let _ = fs::write(&cache, serde_json::to_vec(&out).unwrap_or_default());
        Ok(out)
    }

    /// A link → a session. The same code the voice command uses: one writer, not two.
    pub fn ingest(&self, raw_url: &str) -> Result<String> {
        localvox_light_core::ingest::from_url(&self.work_dir, raw_url)
    }

    /// What is happening with the session, stage by stage. "⚙ cooking" for ten minutes answers
    /// "is anything happening at all" but not "where is it now" — and, when it breaks, not "what
    /// exactly broke".
    /// «Всё верно» — человек прочитал документ и снял пометку сомнения.
    ///
    /// Антоним «переварить заново»: там человек говорит «машина ошиблась, переделай», здесь —
    /// «машина придирается, всё нормально». Наша проверка сверяет буквально и всегда будет
    /// ошибаться (измерено: «Сергей» и «муж» помечены выдумкой, хотя прозвучали вслух). Человек
    /// читал текст и может послушать запись — машина не может. Его слово весомее, и оно
    /// ЗАПОМИНАЕТСЯ: предупреждение, которое возвращается после ответа, перестают читать вообще.
    ///
    /// Кнопка была, а этого метода и его адреса не было вовсе — она стучалась в никуда и молча
    /// ничего не делала.
    pub fn confirm(&self, session: &str, artifact: &str) -> Result<String> {
        // Только то, что мы правда умеем подтверждать: чужое имя артефакта не должно молча
        // создавать запись в журнале.
        let artifact = match artifact {
            "summary" => localvox_light_core::processing::SUMMARY,
            "processed" => localvox_light_core::processing::PROCESSED,
            other => bail!("неизвестный документ: {other}"),
        };
        let dir = self.session_dir(session)?;
        if localvox_light_core::processing::confirm(&dir, artifact) {
            tracing::info!("{session}/{artifact}: человек подтвердил — пометка снята");
            Ok("Отмечено: всё верно".into())
        } else {
            // Не ошибка: подтверждать было нечего (пометки нет или её уже сняли).
            Ok("Пометки и так не было".into())
        }
    }

    pub fn progress(&self, name: &str) -> Result<Value> {
        let dir = self.session_dir(name)?;
        let stages = localvox_light_core::progress::fold(&localvox_light_core::progress::read(&dir));
        let source: Option<localvox_light_core::chunks::Source> =
            std::fs::read(dir.join("meta.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<localvox_light_core::chunks::SessionMeta>(&b).ok())
                .and_then(|m| m.source);
        // WHETHER THE RUN IS STILL GOING cannot be read off the stages, and this is why the chain
        // used to read "finished" mid-cook. A stage only appears once it has spoken, so in the gap
        // between one stage ending and the next starting — 42 seconds while the model loads, in the
        // logs — every stage present says "done" and the picture says the work is over.
        //
        // The stages cannot answer it even in principle: the last one to speak has no idea whether
        // anyone comes after it. The queue knows — it holds the job — so the answer comes from
        // there, and the chain stops guessing.
        let live = localvox_light_core::jobs::JobQueue::load(&self.work_dir)
            .jobs()
            .iter()
            .any(|j| {
                j.session == name
                    && matches!(
                        j.state,
                        localvox_light_core::jobs::JobState::Running
                            | localvox_light_core::jobs::JobState::Pending
                    )
            });

        // Seconds since a timestamp string, or None if it will not parse. Whole seconds across
        // offsets — the log is RFC3339 with a zone, so instants compare correctly.
        let ts = |s: &str| chrono::DateTime::parse_from_rfc3339(s).ok().map(|t| t.timestamp());

        // How long the WHOLE run took, once it is done — the same time we show live, kept for the
        // finished session: earliest a stage started to latest one ended.
        let first = stages
            .iter()
            .filter_map(|s| s.started_at.as_deref())
            .filter_map(ts)
            .min();
        let last = stages
            .iter()
            .filter_map(|s| s.ended_at.as_deref())
            .filter_map(ts)
            .max();
        let elapsed_sec = match (first, last) {
            (Some(a), Some(b)) if !live && b >= a => Some(b - a),
            _ => None,
        };

        // No «stalled»: a live job is «в работе»/«в очереди», period. Abandoned work is re-queued at
        // startup and re-cooked automatically — there is no hung state to wave a person at. (recook
        // still uses `progress_stale` so a human CAN force a redo of a genuinely wedged cook.)
        Ok(json!({
            "stages": stages,
            "source": source,
            "running": live,
            "elapsed_sec": elapsed_sec,
        }))
    }

    /// Stop recording: the engine closes the session, and the cook picks it up. The reason
    /// goes into `meta.json` — the archive must be able to say who ended the recording.
    pub fn stop_recording(&self) -> Result<String> {
        let Some(name) = localvox_light_core::jobs::recording_session(&self.work_dir) else {
            bail!("сейчас ничего не записывается");
        };
        localvox_light_core::jobs::request_record_stop(&self.work_dir, "вручную")
            .context("не удалось попросить движок остановить запись")?;
        Ok(name)
    }

    /// Slots for notes — only the configured ones: a choice from what is offered
    /// rather than free-form input (a typo in a slot name silently sent the note into
    /// the default file).
    pub fn slots(&self) -> Vec<String> {
        let config = SlotRegistry::default_config_path(None);
        SlotRegistry::load(&config)
            .map(|r| r.names().into_iter().map(str::to_string).collect())
            .unwrap_or_default()
    }

    /// Remove one note from a slot.
    ///
    /// This deletes from the person's OWN vault — a file they also edit by hand — so the note is
    /// identified by its text and its file, never by a position in a list that may already be
    /// stale. The slot's configured path is the boundary of what can be touched, and that is
    /// enforced inside the integration rather than trusted from here.
    /// `raw` is the line AS IT IS IN THE FILE — the screen shows a cleaned rendering (no bullet, no
    /// date prefix), and deleting by that rendering would either miss the line or match a different
    /// one that happens to render identically.
    pub fn delete_slot_note(&self, slot: &str, raw: &str, source: &str) -> Result<()> {
        let config = SlotRegistry::default_config_path(None);
        let registry = SlotRegistry::load(&config).context("не прочитан slots.toml")?;
        let target = registry
            .resolve(slot)
            .with_context(|| format!("слота «{slot}» нет"))?;
        target.delete_note(&localvox_light_integrations::StoredNote {
            text: raw.to_string(),
            date: None,
            raw: raw.to_string(),
            source: source.to_string(),
        })
    }

    /// The notes that are actually IN the slots — «полистать посмотреть».
    ///
    /// Until this existed, a voice note left for a file on another disk and the app never mentioned
    /// it again: the only way to check that anything had been written was a file manager. A
    /// secretary that takes dictation and then refuses to show you the notebook is half a
    /// secretary.
    ///
    /// A slot that cannot be read back (an MCP server we only send to) is reported as such, with
    /// its reason — never as an empty list, because «нельзя прочитать» and «здесь пусто» are
    /// opposite facts.
    pub fn slot_notes(&self, limit: usize) -> Vec<serde_json::Value> {
        let config = SlotRegistry::default_config_path(None);
        let Ok(registry) = SlotRegistry::load(&config) else {
            return Vec::new();
        };
        registry
            .slots()
            .iter()
            .map(|s| {
                let (notes, error) = if s.can_read() {
                    match s.read_notes(limit) {
                        Ok(n) => (n, None),
                        // A destination configured but unreachable — an unplugged drive, a path
                        // that moved. Saying which beats an empty list that reads as «нет идей».
                        Err(e) => (Vec::new(), Some(format!("{e:#}"))),
                    }
                } else {
                    (Vec::new(), Some("сюда можно только писать".to_string()))
                };
                json!({
                    "name": s.name,
                    "default": s.default,
                    "readable": s.can_read(),
                    "notes": notes,
                    "error": error,
                })
            })
            .collect()
    }

    /// The transcript versions of a session. Not "×7" on the card — that number
    /// answers nothing; a person needs to know WHAT it was cooked with, when, and
    /// which one is the working one right now.
    pub fn versions(&self, session: &str) -> Result<Vec<VersionInfo>> {
        let dir = self.session_dir(session)?;
        let store = VersionStore::open(&dir)?;
        let m = store.load();
        let best = store.best().map(|v| v.id);
        Ok(m.versions
            .iter()
            .map(|v| VersionInfo {
                id: v.id,
                label: v.label.clone(),
                model: v.model.clone(),
                created_at: v.created_at.clone(),
                derived: !v.parents.is_empty(),
                best: Some(v.id) == best,
                lines: store
                    .resolve(v.id)
                    .and_then(|p| read_transcript_lines(&p).ok())
                    .map(|l| l.len())
                    .unwrap_or(0),
            })
            .collect())
    }

    /// Make a version the working one: the summary, the search and the export are all
    /// computed from it.
    pub fn set_best(&self, session: &str, id: u32) -> Result<()> {
        let dir = self.session_dir(session)?;
        let store = VersionStore::open(&dir)?;
        store
            .set_best(id)
            .with_context(|| format!("нет версии v{id:03}"))?;
        tracing::info!("{session}: v{id:03} set as the working version");
        Ok(())
    }

    /// State of the background cook queue (WP-C7) — visibility of the autopilot:
    /// "N cooking, M queued, K failed". Reads `jobs.json` (P5: files only).
    /// THE QUEUE: what is being cooked now, and what comes after it, in order.
    ///
    /// Order is the queue's whole meaning, and it is the file's own order — the daemon takes
    /// pending jobs exactly as they lie, oldest first. So the list is not sorted by anything
    /// clever here: reordering it for looks would make it a lie about what happens next.
    ///
    /// `Done` is deliberately absent. A queue is what is left to do; what is finished lives in the
    /// archive, with its own card and its own stages.
    pub fn queue(&self) -> Vec<QueueItem> {
        use localvox_light_core::jobs::{JobKind, JobState as S, MAX_REVIVALS};
        let q = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        let mut place = 0usize;
        q.jobs()
            .iter()
            .filter(|j| !matches!(j.state, S::Done))
            .map(|j| {
                // The meta gives the human name; without it the person reads directory stamps.
                let meta: Option<localvox_light_core::chunks::SessionMeta> =
                    fs::read(self.sessions_root().join(&j.session).join("meta.json"))
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok());
                let title = meta
                    .as_ref()
                    .and_then(|m| m.title.clone())
                    .or_else(|| meta.as_ref().and_then(|m| m.source.as_ref().map(|s| s.url.clone())))
                    .unwrap_or_else(|| j.session.clone());
                let position = if j.state == S::Pending {
                    place += 1;
                    Some(place)
                } else {
                    None // what is running is not waiting
                };
                QueueItem {
                    session: j.session.clone(),
                    title,
                    started_at: meta.and_then(|m| Some(m.started_at)),
                    state: match j.state {
                        S::Running => "running",
                        S::Pending => "pending",
                        S::Failed => "failed",
                        S::Done => "done",
                    }
                    .into(),
                    kind: match j.kind {
                        JobKind::Cook => "cook",
                        JobKind::Ingest => "ingest",
                    }
                    .into(),
                    position,
                    attempts: j.attempts,
                    last_error: j.last_error.clone(),
                    stuck: j.state == S::Failed && j.revivals >= MAX_REVIVALS,
                }
            })
            .collect()
    }

    pub fn jobs_status(&self) -> JobsStatus {
        let q = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        let mut st = JobsStatus::default();
        for j in q.jobs() {
            use localvox_light_core::jobs::JobState as S;
            match j.state {
                S::Pending => st.pending += 1,
                S::Running => {
                    st.running += 1;
                    st.current = Some(j.session.clone());
                }
                S::Done => st.done += 1,
                S::Failed => {
                    st.failed += 1;
                    if st.last_error.is_none() {
                        st.last_error = j.last_error.clone();
                    }
                }
            }
        }
        st
    }

    /// F5 routing: suggest which slots to file the text into (via the LLM). The list
    /// of slots and the hints come from the registry; the LLM config — from the env
    /// (the same `LOCALVOX_LLM_*` as `localvox-process`). Existing slots only.
    pub fn route(&self, text: &str) -> Result<Vec<RouteHint>> {
        let config = SlotRegistry::default_config_path(None);
        let registry = SlotRegistry::load(&config)
            .with_context(|| format!("конфиг слотов: {}", config.display()))?;
        let slots: Vec<localvox_light_llm::routing::RouteSlot> = registry
            .slots()
            .iter()
            .map(|s| localvox_light_llm::routing::RouteSlot {
                name: s.name.clone(),
                // the description, otherwise the aliases — at least some hint for the
                // model
                hint: if s.description.trim().is_empty() {
                    s.aliases.join(", ")
                } else {
                    s.description.clone()
                },
            })
            .collect();
        let client = llm_client_from_env_with(env_secs("LOCALVOX_LLM_ROUTE_TIMEOUT_SEC", 30));
        let out = localvox_light_llm::routing::suggest_routing(text, &slots, &client)?;
        Ok(out
            .into_iter()
            .map(|s| RouteHint {
                slot: s.slot,
                reason: s.reason,
            })
            .collect())
    }

    pub fn append_note(&self, slot: Option<&str>, text: &str) -> Result<String> {
        let config = SlotRegistry::default_config_path(None);
        let registry = SlotRegistry::load(&config)
            .with_context(|| format!("конфиг слотов: {}", config.display()))?;
        let target = match slot {
            Some(q) => registry.resolve(q).with_context(|| {
                format!(
                    "слот «{q}» не найден; есть: {}",
                    registry.names().join(", ")
                )
            })?,
            None => registry.default_slot().context("нет ни одного слота")?,
        };
        let dest = target.write_note(text)?;
        Ok(format!("Записано в слот «{}»: {dest}", target.name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pcm(samples: &[i16]) -> Vec<u8> {
        samples.iter().flat_map(|s| s.to_le_bytes()).collect()
    }

    /// The sum of two loud samples overflows i16 and "wraps" into the opposite sign.
    /// That is not "a bit louder" — that is CRACKLE instead of speech, and the person
    /// will decide the recording is broken.
    #[test]
    fn mixing_two_loud_tracks_does_not_wrap_into_noise() {
        let a = pcm(&[30_000, -30_000, 0]);
        let b = pcm(&[30_000, -30_000, 100]);
        let m = mix(&a, &b);
        let out: Vec<i16> = m
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(out, vec![i16::MAX, i16::MIN, 100], "the mix wrapped: {out:?}");
    }

    /// A monologue into the microphone is a recording without a second track. It must
    /// be played, not declared broken.
    #[test]
    fn a_track_that_does_not_exist_is_silence_not_an_error() {
        let a = pcm(&[5, 6, 7]);
        let m = mix(&a, &[]);
        assert_eq!(m, a);
        assert!(mix(&[], &[]).is_empty());
    }

    /// THE BUTTON THAT DID NOTHING. «Всё верно» called `POST /api/sessions/{name}/confirm`, and
    /// neither the route nor this method existed — the click went into the void, the plaque stayed,
    /// and the person kept being told the same doubt about a document he had already read.
    ///
    /// A warning that comes back after it has been answered stops being read altogether. That is
    /// why the human's verdict must be REMEMBERED, and why this is tested end to end from the
    /// archive's own surface, not from the check inside the core.
    #[test]
    fn confirming_a_doubt_removes_the_plaque_for_good() {
        use localvox_light_core::processing::{self, Outcome};
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path().join("sessions/20260716_x");
        std::fs::create_dir_all(&sdir).unwrap();
        let a = Archive::new(dir.path().to_path_buf());

        // The check flagged the summary — exactly as it did on the live recording where «Сергей»
        // and «муж» were called inventions although both were said out loud.
        processing::record(
            &sdir,
            processing::SUMMARY,
            "summary/ru/qwen3.5:9b/p2",
            Outcome::Unverified,
            Some("имена/названия: сергей".into()),
            None,
        );
        assert!(
            processing::doubts(&sdir, processing::SUMMARY).is_some(),
            "the doubt was not recorded — the test proves nothing"
        );

        assert!(a.confirm("20260716_x", "summary").is_ok());

        assert!(
            processing::doubts(&sdir, processing::SUMMARY).is_none(),
            "the plaque survived the click — this is the bug the owner hit"
        );
        // And it must not come back on the next read of the archive.
        assert!(processing::doubts(&sdir, processing::SUMMARY).is_none());

        // Pressing it again is not an error: there is simply nothing left to confirm.
        assert!(a.confirm("20260716_x", "summary").is_ok());
        // An artifact we do not know is refused rather than silently invented in the ledger.
        assert!(a.confirm("20260716_x", "нечто").is_err());
    }

    /// THE GREEN CHAIN. Between two stages the cook says nothing — in the owner's own logs, 42
    /// seconds of it while the model loaded. Every stage that had spoken said "done", so the chain
    /// read as finished while the work was very much going, and he watched a picture that had
    /// nothing left to show him.
    ///
    /// The stages cannot answer this: the last one to speak does not know whether anyone follows.
    /// The queue holds the job, so the queue is asked.
    #[test]
    fn a_chain_between_two_stages_does_not_claim_to_be_finished() {
        use localvox_light_core::jobs::JobQueue;
        use localvox_light_core::progress::{self, Stage, StageState};

        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path().join("sessions/20260716_g");
        std::fs::create_dir_all(&sdir).unwrap();
        let a = Archive::new(dir.path().to_path_buf());

        // The exact shape of the gap: the run is open, transcribe and refine are done, and summary
        // has not started yet — the model is loading.
        progress::new_run(&sdir);
        progress::mark(&sdir, Stage::Transcribe, StageState::Done, Some("149 строк"));
        progress::mark(&sdir, Stage::Refine, StageState::Done, None);

        let mut q = JobQueue::load(dir.path());
        assert!(q.enqueue_cook("20260716_g", true, true, true));
        let id = q.pending_ids()[0];
        q.start(id).expect("the job did not start");

        let p = a.progress("20260716_g").unwrap();
        // Every stage present says done — which is exactly why the chain used to lie.
        assert!(
            p["stages"].as_array().unwrap().iter().all(|s| s["state"] == "done"),
            "the gap was not reproduced, so this test proves nothing: {p}"
        );
        assert_eq!(
            p["running"], true,
            "the chain says the cook is over while the job is still running — the owner's bug"
        );

        // The job finishes: only now is the chain entitled to read as finished.
        let mut q = JobQueue::load(dir.path());
        q.mark_done(id);
        assert_eq!(a.progress("20260716_g").unwrap()["running"], false);
    }

    /// Deletion is the one irreversible action. The wrong `confirm` must never remove anything —
    /// the guard is the difference between "redo" and "gone".
    #[test]
    fn delete_needs_the_session_name_and_is_final() {
        let dir = tempfile::tempdir().unwrap();
        let sdir = dir.path().join("sessions/20260715_x");
        std::fs::create_dir_all(sdir.join("audio")).unwrap();
        std::fs::write(sdir.join("audio/src0.wav"), b"x").unwrap();
        let a = Archive::new(dir.path().to_path_buf());

        // Wrong confirmation: nothing happens, the audio stays.
        assert!(a.delete_session("20260715_x", "не то").is_err());
        assert!(sdir.exists(), "a wrong confirm still deleted the session");

        // The name echoed back: gone, and gone for good.
        assert_eq!(a.delete_session("20260715_x", "20260715_x").unwrap(), "20260715_x");
        assert!(!sdir.exists());
        // Deleting again is an honest error, not a panic.
        assert!(a.delete_session("20260715_x", "20260715_x").is_err());
    }

    #[test]
    fn session_dir_rejects_everything_but_plain_names() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("sessions/20260711_ok")).unwrap();
        let a = Archive::new(dir.path().to_path_buf());
        assert!(a.session_dir("20260711_ok").is_ok());
        for bad in [
            "C:",
            "C:evil",
            ".",
            "..",
            "",
            "a/b",
            "a\\b",
            "..\\x",
            "\\\\srv\\share",
        ] {
            let err = a.session_dir(bad).unwrap_err().to_string();
            assert_eq!(err, "некорректное имя сессии", "not rejected: {bad:?}");
        }
    }

    /// THE DEFECT THIS EXISTS FOR, caught live on 19.07.2026, session
    /// `20260718_230541_youtube`. Renaming the source deleted the readable text and queued it for
    /// a re-cook — the behaviour from when the label was baked into the document at cook time.
    ///
    /// It is not baked in any more: the artifact is a delta holding no name at all, and the label
    /// is joined in on every read. So the rename must reach the text by CHANGING NOTHING, and
    /// burning minutes of LLM time to rebuild a file byte-for-byte identical to the one on disk is
    /// not a small waste — it is the whole reason the artifact was reshaped.
    #[test]
    fn renaming_a_source_does_not_touch_the_readable_text() {
        use localvox_light_core::readable;
        let dir = tempfile::tempdir().unwrap();
        let sess = dir.path().join("sessions/20260718_rename");
        std::fs::create_dir_all(&sess).unwrap();
        std::fs::write(
            sess.join("meta.json"),
            serde_json::json!({"started_at": "t", "sample_rate": 16000, "chunks": []}).to_string(),
        )
        .unwrap();

        let store = VersionStore::open(&sess).unwrap();
        let (id, path) = store.next_version("test").unwrap();
        let line = localvox_light_core::versions::TranscriptLine {
            source_id: 0,
            start_sec: 0.0,
            end_sec: 2.0,
            text: "ну это самое".into(),
            speaker: None,
        };
        std::fs::write(&path, serde_json::to_string(&line).unwrap() + "\n").unwrap();
        store
            .commit(localvox_light_core::versions::VersionEntry {
                id,
                label: "test".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "m".into(),
                params: serde_json::json!({}),
                created_at: localvox_light_core::versions::now_rfc3339(),
                parents: vec![],
            })
            .unwrap();
        readable::save(
            &sess,
            &readable::Readable {
                version_id: id,
                edits: [(0usize, "Это самое.".to_string())].into_iter().collect(),
                ..Default::default()
            },
        )
        .unwrap();
        let before = std::fs::read(readable::path(&sess)).unwrap();

        let a = Archive::new(dir.path().to_path_buf());
        assert_eq!(readable::lines(&sess).unwrap()[0].who, "Я");

        a.name_source("20260718_rename", 0, "Арсен Маркарян").unwrap();

        assert!(
            readable::exists(&sess),
            "the rename deleted the readable text — this is the bug"
        );
        assert_eq!(
            std::fs::read(readable::path(&sess)).unwrap(),
            before,
            "the readable artifact was rewritten by a rename"
        );
        assert_eq!(
            readable::lines(&sess).unwrap()[0].who,
            "Арсен Маркарян",
            "the new name did not reach the text"
        );
    }

    /// THE PHANTOM, reported by the owner on 19.07.2026 while looking at his own archive.
    ///
    /// The list came from the diarization roster, and the roster holds voices the document never
    /// shows: on his video it offered to rename a «Участник 1» with not one line in the text, and
    /// the meeting listed a «Участник 3» the same way. Grouping the LINES makes that structurally
    /// impossible — somebody who says nothing has nothing to be grouped from.
    ///
    /// The second half is what he saw next: the same name reaching the text twice, once through a
    /// separated voice and once through the source label he had set to the same string. That is
    /// one person, and one row.
    #[test]
    fn participants_come_from_the_lines_not_from_a_register() {
        let dir = tempfile::tempdir().unwrap();
        let sess = dir.path().join("sessions/20260719_who");
        std::fs::create_dir_all(&sess).unwrap();
        // A link session named by hand — «Я» would be a wrong statement about who spoke.
        std::fs::write(
            sess.join("meta.json"),
            serde_json::json!({
                "started_at": "t", "sample_rate": 16000, "chunks": [],
                "source": {"url": "https://example/v", "title": "видео"},
                "source_names": {"0": "Арсен Маркарян"}
            })
            .to_string(),
        )
        .unwrap();

        let line = |start: f64, speaker: Option<&str>| localvox_light_core::versions::TranscriptLine {
            source_id: 0,
            start_sec: start,
            end_sec: start + 10.0,
            text: "речь".into(),
            speaker: speaker.map(str::to_owned),
        };
        let store = VersionStore::open(&sess).unwrap();
        let (id, path) = store.next_version("test").unwrap();
        let lines = [
            line(0.0, Some("Арсен Маркарян")),
            line(10.0, Some("Арсен Маркарян")),
            // No voice separated — attributed to the input, which carries the same name.
            line(20.0, None),
        ];
        std::fs::write(
            &path,
            lines
                .iter()
                .map(|l| serde_json::to_string(l).unwrap() + "
")
                .collect::<String>(),
        )
        .unwrap();
        store
            .commit(localvox_light_core::versions::VersionEntry {
                id,
                label: "test".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "m".into(),
                params: serde_json::json!({}),
                created_at: localvox_light_core::versions::now_rfc3339(),
                parents: vec![],
            })
            .unwrap();

        // The register claims a voice the text never shows.
        let mut roster = localvox_light_core::diarize::roster::Roster::default();
        roster.members.push(localvox_light_core::diarize::roster::Member {
            id: 1,
            label: "Участник 1".into(),
            speech_sec: 84.0,
            owner: false,
            embedding: vec![],
        });
        localvox_light_core::diarize::roster::save(&sess, &roster).unwrap();

        let people = Archive::new(dir.path().to_path_buf())
            .speakers("20260719_who")
            .unwrap();

        assert_eq!(
            people.len(),
            1,
            "expected one person; got {:?}",
            people.iter().map(|p| &p.name).collect::<Vec<_>>()
        );
        let p = &people[0];
        assert_eq!(p.name, "Арсен Маркарян");
        assert_eq!(p.lines, 3, "the source-attributed line was dropped from the count");
        // Both keys are kept, or a rename would fix two lines and leave the third behind.
        assert_eq!(p.voices, vec!["Арсен Маркарян".to_string()]);
        assert_eq!(p.sources, vec![0]);
        assert!(
            p.what.contains("отдельный голос") && p.what.contains("звук из источника"),
            "the row must say what it is made of: {}",
            p.what
        );
    }

    #[test]
    fn audio_clip_extracts_sample_accurate_range() {
        let dir = tempfile::tempdir().unwrap();
        let sess = dir.path().join("sessions/20260712_clip");
        let audio = sess.join("audio");
        std::fs::create_dir_all(&audio).unwrap();
        // 2 s of audio from source 0: 32000 samples = 64000 bytes of data
        std::fs::write(
            audio.join("src0_chunk0001.wav"),
            wav_16k_mono(&vec![7u8; 64_000]),
        )
        .unwrap();
        let meta = serde_json::json!({
            "started_at": "t", "sample_rate": 16000, "meetings": [],
            "chunks": [{"file":"src0_chunk0001","source_id":0,"start_offset_sec":0.0,"duration_sec":2.0}]
        });
        std::fs::write(sess.join("meta.json"), meta.to_string()).unwrap();

        let a = Archive::new(dir.path().to_path_buf());
        // [0.5s, 1.5s) = 16000 samples = 32000 bytes of data + 44 header
        let wav = a.audio_clip("20260712_clip", Some(0), 0.5, 1.0).unwrap();
        assert_eq!(&wav[..4], b"RIFF");
        assert_eq!(wav.len(), 44 + 32_000, "wrong clip range");
        // another source — there is no audio
        assert!(a.audio_clip("20260712_clip", Some(1), 0.5, 1.0).is_err());
    }

    #[test]
    fn audio_clip_errors_when_overlapping_chunk_unreadable() {
        // The chunk is declared in meta, but the file is missing (FLAC/deleted) → an
        // honest error, not a silently shortened/shifted clip.
        let dir = tempfile::tempdir().unwrap();
        let sess = dir.path().join("sessions/20260712_gap");
        std::fs::create_dir_all(sess.join("audio")).unwrap();
        let meta = serde_json::json!({
            "started_at": "t", "sample_rate": 16000, "meetings": [],
            "chunks": [{"file":"src0_chunk0001","source_id":0,"start_offset_sec":0.0,"duration_sec":2.0}]
        });
        std::fs::write(sess.join("meta.json"), meta.to_string()).unwrap();
        let a = Archive::new(dir.path().to_path_buf());
        let err = a
            .audio_clip("20260712_gap", Some(0), 0.5, 1.0)
            .unwrap_err()
            .to_string();
        assert!(err.contains("недоступен"), "an honest error was expected: {err}");
    }
}

/// PCM (s16le, 16 kHz mono) of a chunk over the sample range [from, to) (to is
/// clamped to the length). `.flac` is decoded whole via ffmpeg (the same one that
/// created it), `.wav` is read pointwise (seek — we do not pull the whole file for the
/// sake of one piece). `None` — there is no file (or ffmpeg failed): the caller decides
/// whether that is an error or a skip.
fn chunk_pcm_range(audio: &Path, file: &str, from: u64, to: u64) -> Option<Vec<u8>> {
    use std::io::{Read, Seek, SeekFrom};
    let flac = audio.join(format!("{file}.flac"));
    if flac.exists() {
        // the same strict decode as the cook's: -xerror — a partial decode of a broken
        // FLAC MUST NOT silently shift the clip (env → exe-dir → PATH)
        let ffmpeg = localvox_light_core::chunks::resolve_ffmpeg_for_decode();
        let out = std::process::Command::new(ffmpeg)
            .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
            .arg(&flac)
            .args(["-f", "s16le", "-ac", "1", "-ar", "16000", "-"]) // raw PCM to stdout
            .output()
            .ok()?;
        if !out.status.success() {
            tracing::warn!(
                "ffmpeg failed to decode {}: {}",
                flac.display(),
                String::from_utf8_lossy(&out.stderr).trim()
            );
            return None;
        }
        let total = (out.stdout.len() / 2) as u64;
        let (a, b) = (from.min(total), to.min(total));
        return Some(if b > a {
            out.stdout[(a as usize) * 2..(b as usize) * 2].to_vec()
        } else {
            Vec::new()
        });
    }
    let mut f = fs::File::open(audio.join(format!("{file}.wav"))).ok()?;
    let len = f.metadata().ok()?.len();
    if len <= 44 {
        return Some(Vec::new());
    }
    let total = (len - 44) / 2;
    let (a, b) = (from.min(total), to.min(total));
    if b <= a {
        return Some(Vec::new());
    }
    f.seek(SeekFrom::Start(44 + a * 2)).ok()?;
    let mut buf = vec![0u8; ((b - a) * 2) as usize];
    f.read_exact(&mut buf).ok()?;
    Some(buf)
}

/// Wraps s16le PCM (16 kHz mono) into a minimal WAV container.
/// The 44-byte WAV header for `data_len` bytes of PCM (16 kHz mono s16).
fn wav_header(data_len: u32) -> [u8; 44] {
    const SR: u32 = 16_000;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(data_len + 36).to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&SR.to_le_bytes());
    h[28..32].copy_from_slice(&(SR * 2).to_le_bytes()); // byte rate
    h[32..34].copy_from_slice(&2u16.to_le_bytes()); // block align
    h[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data_len.to_le_bytes());
    h
}

fn wav_16k_mono(pcm: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(&wav_header(pcm.len() as u32));
    out.extend_from_slice(pcm);
    out
}

pub fn render_transcript_text(doc: &TranscriptDoc, dir: &Path) -> String {
    let _ = dir;
    let mut out = format!(
        "# {} (версия {}, модель {})\n\n",
        doc.session, doc.label, doc.model
    );
    for l in &doc.lines {
        out.push_str(&format!(
            "[{}] ({:02}:{:02}) {}\n",
            l.who,
            (l.start_sec / 60.0) as u64,
            l.start_sec as u64 % 60,
            l.text
        ));
    }
    out
}
