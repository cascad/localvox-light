//! Operations over the session archive — the shared core for the MCP server and the
//! HTTP API (F6). Works only with work_dir files (P5): the recording engine is not
//! needed.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use localvox_light_core::versions::{read_transcript_lines, VersionStore};
use localvox_light_integrations::SlotRegistry;
use localvox_light_search::SearchIndex;

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
                    has_processed: dir.join("processed.md").exists(),
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

    /// The transcript is WHAT WAS SAID, not what the model thinks about it. That is
    /// why in the "Расшифровка"/Transcript tab we show the raw cook, even if `best`
    /// is switched to `refined`: text combed by the LLM is a derivative, its place is
    /// in "Cleaned-up text". Acceptance 13.07.2026: the transcript read "Там кратно
    /// выручка превышает расходы", while the audio said "там кратно превы…" — the
    /// model made it up, and the recording stopped being evidence.
    pub fn transcript(&self, session: &str) -> Result<TranscriptDoc> {
        let dir = self.session_dir(session)?;
        let store = VersionStore::open(&dir)?;
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
                        None if l.source_id == 0 => "Я".into(),
                        None => "Собеседники".into(),
                    },
                    source_id: l.source_id,
                    start_sec: l.start_sec,
                    end_sec: l.end_sec,
                    text: l.text,
                })
                .collect(),
        })
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
            "processed.md" => "--cleanup",
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
    pub fn recook(&self, session: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        // Preconditions first, destruction after.
        self.ensure_recookable(session, &dir)?;

        // The post-processing flags are EXACTLY the same as the auto-cook's, from the
        // shared place. Our own reading of the same variables gave "no variable —
        // disabled", whereas for the daemon it means "enabled": the button erased the
        // summary and queued a job without the flag to make it. A person pressed
        // "redo" — and got "delete", and the summary never came back.
        let (summary, cleanup, refine) = localvox_light_core::jobs::post_processing_from_env();

        // We queue the work FIRST and destroy afterwards. Otherwise a queue refusal
        // ("already cooking") would leave the session without derived files and
        // without the work that would bring them back.
        let mut queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        if !queue.enqueue_recook(session, summary, cleanup, refine) {
            anyhow::bail!("сессия уже варится прямо сейчас — дождитесь окончания");
        }

        let mut dropped = Vec::new();
        for f in [
            "summary.md",
            "processed.md",
            "summary.unverified.md",
            "processed.unverified.md",
        ] {
            if fs::remove_file(dir.join(f)).is_ok() {
                dropped.push(f);
            }
        }
        // Erasing the file and leaving "done by this recipe" in the journal means
        // lying to discovery: it will see the entry, decide there is nothing to do,
        // and the artifact will not come back. Invariant: no file — no record of it
        // either.
        localvox_light_core::processing::forget(
            &dir,
            &[
                localvox_light_core::processing::SUMMARY,
                localvox_light_core::processing::PROCESSED,
                localvox_light_core::processing::TRANSCRIPT,
            ],
        );

        tracing::info!("re-cook on demand: {session} (dropped: {dropped:?})");
        Ok(format!(
            "поставлено на переварку; выброшено производных файлов: {}",
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
        let msg = self.recook(session)?;
        Ok(match lang {
            Some(code) => format!("язык: {code}; {msg}"),
            None => format!("язык: авто; {msg}"),
        })
    }

    /// "This is a meeting": close the current recording and start a new one — with a
    /// title.
    ///
    /// Auto-detection sees a call in an application (who holds the microphone), but it
    /// will never see a face-to-face stand-up around a table: to the system that is
    /// the same background noise as the whole day. There is no reliable automatic sign
    /// of a meeting in a room — which is why the person's word is decisive here.
    pub fn start_meeting(&self, title: &str) -> Result<String> {
        let title = title.trim();
        localvox_light_core::jobs::request_meeting(&self.work_dir, title)
            .context("не удалось попросить движок начать встречу")?;
        Ok(if title.is_empty() {
            "встреча начата".into()
        } else {
            format!("встреча начата: {title}")
        })
    }

    /// Who spoke in the recording: the roster of participants with their captions.
    ///
    /// Empty — either there was no diarization (no model) or no voices were found.
    /// Both are honest: we MUST NOT silently show «Участник 1» where we counted
    /// nobody.
    pub fn speakers(&self, session: &str) -> Result<Vec<SpeakerOut>> {
        let dir = self.session_dir(session)?;
        Ok(localvox_light_core::diarize::roster::load(&dir)
            .members
            .into_iter()
            .map(|m| SpeakerOut {
                label: m.label,
                speech_sec: m.speech_sec,
                owner: m.owner,
            })
            .collect())
    }

    /// Name a voice: «Участник 2» is Ivan.
    ///
    /// Does three things, and all three are mandatory:
    ///
    /// 1. **remembers the voice under the name** — in later sessions it will be
    ///    recognized by itself;
    /// 2. **re-labels the lines** in every version of the transcript — WITHOUT
    ///    re-recognition: the audio has not changed, only one caption has to change;
    /// 3. **throws away the summary and the cleaned-up text** and queues them for a
    ///    rebuild — they say «Участник 2», and leaving that means lying that the name
    ///    was taken into account.
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

        // The summary and the cleaned-up text are written about «Участник 2» — they
        // have to be rebuilt. The audio is not touched: re-cooking half an hour of
        // sound for the sake of a name is a mockery, and then names simply would not
        // be used at all.
        let (summary, cleanup, refine) = localvox_light_core::jobs::post_processing_from_env();
        let mut queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        let queued = queue.requeue_for_artifacts(session, summary, cleanup, refine);
        if queued {
            for f in ["summary.md", "processed.md"] {
                let _ = fs::remove_file(dir.join(f));
            }
            // No file — no record of it either: otherwise discovery will decide there
            // is nothing to do, and the artifact will not come back.
            localvox_light_core::processing::forget(
                &dir,
                &[
                    localvox_light_core::processing::SUMMARY,
                    localvox_light_core::processing::PROCESSED,
                ],
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
        let client = llm_client_tuned(env_secs("LOCALVOX_LLM_CHAT_TIMEOUT_SEC", 120), Some(0.0));
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

    /// "Finish the session": the engine will close the current one and start a new
    /// one, and the cook will pick up the closed one. Previously one had to kill the
    /// daemon just to get a summary.
    pub fn finish_session(&self) -> Result<String> {
        let Some(name) = localvox_light_core::jobs::recording_session(&self.work_dir) else {
            bail!("сейчас ничего не записывается");
        };
        localvox_light_core::jobs::request_finish(&self.work_dir, "вручную")
            .context("не удалось попросить движок закрыть сессию")?;
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
fn wav_16k_mono(pcm: &[u8]) -> Vec<u8> {
    const SR: u32 = 16_000;
    let data_len = pcm.len() as u32;
    let mut out = Vec::with_capacity(44 + pcm.len());
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(data_len + 36).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&SR.to_le_bytes());
    out.extend_from_slice(&(SR * 2).to_le_bytes()); // byte rate
    out.extend_from_slice(&2u16.to_le_bytes()); // block align
    out.extend_from_slice(&16u16.to_le_bytes()); // bits
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
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
