//! Operations over the session archive — the shared core for the MCP server and the
//! HTTP API (F6). Works only with work_dir files (P5): the recording engine is not
//! needed.

mod config;
mod media;
mod mutations;
mod partial;
use config::{env_f64, env_secs, llm_client_capped, llm_client_from_env_with};
mod catalog;
mod commands;
mod documents;
mod notes;
mod participants;
mod queries;
mod queue;
mod recording;
pub use media::AudioStream;
pub use partial::PartialInfo;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use anyhow::{bail, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

use localvox_light_core::versions::{read_transcript_lines, VersionStore};
use localvox_light_integrations::SlotRegistry;
use localvox_light_search::SearchIndex;

#[derive(Debug)]
pub(crate) struct QueueUnavailable(String);

impl std::fmt::Display for QueueUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Очередь недоступна: {}", self.0)
    }
}

impl std::error::Error for QueueUnavailable {}

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
    pub source_url: Option<String>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub partial: Option<PartialInfo>,
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
    pub cancelling: usize,
    pub cancelled: usize,
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
    pub phase: localvox_light_core::jobs::JobPhase,
    pub retry: Option<localvox_light_core::workflow::RetryPlan>,
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
        let to = want_end.min(decl_end).saturating_sub(c_start);
        if to <= from {
            continue;
        }
        let slice = chunk_pcm_range(&audio, &c.file, from, to).with_context(|| {
            format!(
                "чанк {} недоступен (FLAC-декод/удалён) — фрагмент не собрать целиком",
                c.file
            )
        })?;
        anyhow::ensure!(
            slice.len() as u64 == (to - from) * 2,
            "аудиофрагмент короче подтверждённого диапазона"
        );
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
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
}

#[cfg(test)]
mod tests {
    use super::*;

    fn retained_transcript(work: &Path) -> PathBuf {
        let session = work.join("sessions/retained");
        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("raw").unwrap();
        fs::write(
            &path,
            "{\"source_id\":0,\"start_sec\":0,\"end_sec\":5,\"text\":\"original speech\"}\n",
        )
        .unwrap();
        store
            .commit(localvox_light_core::versions::VersionEntry {
                id,
                label: "raw".into(),
                file: path.file_name().unwrap().to_str().unwrap().into(),
                model: "fixture".into(),
                params: json!({}),
                created_at: "t".into(),
                parents: vec![],
            })
            .unwrap();
        fs::write(session.join("summary.md"), "previous summary").unwrap();
        path
    }

    #[test]
    fn verified_transcript_remains_readable_when_summary_fails() {
        use localvox_light_core::{jobs::JobQueue, processing as proc, progress};
        let work = tempfile::tempdir().unwrap();
        retained_transcript(work.path());
        let session = work.path().join("sessions/retained");
        fs::remove_file(session.join("summary.md")).unwrap();
        proc::record(
            &session,
            proc::TRANSCRIPT,
            "fixture",
            proc::Outcome::Ok,
            None,
            Some(1),
        )
        .unwrap();
        JobQueue::mutate(work.path(), |queue| {
            assert!(queue.requeue_derivatives("retained", true, false, false));
            let job = queue.claim_next().unwrap();
            queue.mark_failed(job.id, "summary service unavailable", 1);
        });
        progress::mark(
            &session,
            progress::Stage::Summary,
            progress::StageState::Failed,
            Some("summary service unavailable"),
        );
        let archive = Archive::new(work.path().to_path_buf());
        assert_eq!(
            archive.transcript("retained").unwrap().lines[0].text,
            "original speech"
        );
        let view = archive.progress("retained").unwrap();
        let rows = view["stages"].as_array().unwrap();
        assert_eq!(
            rows.iter().find(|s| s["stage"] == "transcribe").unwrap()["state"],
            "done"
        );
        assert_eq!(
            rows.iter().find(|s| s["stage"] == "summary").unwrap()["state"],
            "failed"
        );
    }

    #[test]
    fn text_and_summary_recook_without_audio_start_at_the_requested_phase() {
        use localvox_light_core::jobs::{JobPhase, JobQueue};
        use localvox_light_core::processing as proc;
        for (scope, phase) in [
            (RecookScope::Text, JobPhase::Text),
            (RecookScope::Summary, JobPhase::Summary),
        ] {
            let dir = tempfile::tempdir().unwrap();
            let source = retained_transcript(dir.path());
            let before = fs::read(&source).unwrap();
            // Also cover an imported video's old queue entry, which still has kind=ingest.
            JobQueue::mutate(dir.path(), |q| {
                q.enqueue_ingest("retained", true, true, true);
            });
            let archive = Archive::new(dir.path().to_path_buf());
            archive.recook("retained", scope).unwrap();
            let queue = JobQueue::load(dir.path());
            let job = &queue.jobs()[0];
            assert_eq!(job.phase, phase);
            assert!(!job.force);
            assert_eq!(fs::read(&source).unwrap(), before);
            let session = source.parent().unwrap().parent().unwrap();
            assert!(!session.join("audio").exists());
            // Simulate persisted post-processing outputs; acceptance must not demand lost audio
            // or a historical receipt that this legacy transcript never had.
            fs::write(session.join("summary.md"), "new summary").unwrap();
            proc::record(
                session,
                proc::SUMMARY,
                "fixture",
                proc::Outcome::Ok,
                None,
                Some(1),
            )
            .unwrap();
            if phase == JobPhase::Text {
                localvox_light_core::readable::save(
                    session,
                    &localvox_light_core::readable::Readable {
                        version_id: 1,
                        ..Default::default()
                    },
                )
                .unwrap();
                proc::record(
                    session,
                    proc::PROCESSED,
                    "fixture",
                    proc::Outcome::Ok,
                    None,
                    Some(1),
                )
                .unwrap();
            }
            localvox_light_core::jobs::verify_phase(session, job).unwrap();
        }
    }

    #[test]
    fn full_recook_without_audio_preserves_the_last_results() {
        let dir = tempfile::tempdir().unwrap();
        let source = retained_transcript(dir.path());
        let before = fs::read(&source).unwrap();
        let archive = Archive::new(dir.path().to_path_buf());
        assert!(archive.recook("retained", RecookScope::All).is_err());
        assert_eq!(fs::read(source).unwrap(), before);
        assert_eq!(
            fs::read_to_string(dir.path().join("sessions/retained/summary.md")).unwrap(),
            "previous summary"
        );
        assert!(!dir.path().join("jobs.json").exists());
    }

    #[test]
    fn naming_a_short_voice_is_local_and_does_not_enroll_a_shared_profile() {
        use localvox_light_core::diarize::{profiles, roster};
        let dir = tempfile::tempdir().unwrap();
        retained_transcript(dir.path());
        let session = dir.path().join("sessions/retained");
        profiles::enroll(dir.path(), "Historical profile", &[1.0, 0.0], 30.0).unwrap();
        let old_profiles = fs::read(dir.path().join("speakers.json")).unwrap();
        roster::save(
            &session,
            &roster::Roster {
                members: vec![roster::Member {
                    id: 0,
                    label: "Speaker 1".into(),
                    embedding: vec![1.0, 0.0],
                    speech_sec: 1.0,
                    owner: false,
                    manually_named: false,
                }],
            },
        )
        .unwrap();
        let archive = Archive::new(dir.path().to_path_buf());
        archive
            .name_speaker("retained", "Speaker 1", "Local name")
            .unwrap();
        let named = roster::load(&session);
        assert_eq!(named.members[0].label, "Local name");
        assert!(named.members[0].manually_named);
        assert_eq!(
            fs::read(dir.path().join("speakers.json")).unwrap(),
            old_profiles
        );
        let queue = localvox_light_core::jobs::JobQueue::load(dir.path());
        assert_eq!(
            queue.jobs()[0].phase,
            localvox_light_core::jobs::JobPhase::Summary
        );
        assert!(!queue.jobs()[0].refine);
        assert!(!session.join("summary.md").exists());
    }

    /// Opt-in local evidence: never mutate the source archive or call a model/network.
    #[test]
    #[ignore = "requires LOCALVOX_REVIEW_SESSION pointing at a historical session; operates on a temporary copy"]
    fn historical_speaker_correction_on_temporary_copy_preserves_speech() {
        let source = PathBuf::from(std::env::var("LOCALVOX_REVIEW_SESSION").unwrap());
        let work = tempfile::tempdir().unwrap();
        let session = work.path().join("sessions/copied");
        fs::create_dir_all(session.join("transcripts")).unwrap();
        let mut snapshots = Vec::new();
        for relative in [Path::new(""), Path::new("transcripts")] {
            for entry in fs::read_dir(source.join(relative)).unwrap().flatten() {
                if !entry.file_type().unwrap().is_file() {
                    continue;
                }
                let path = entry.path();
                if !matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("json" | "jsonl" | "md")
                ) {
                    continue;
                }
                let bytes = fs::read(&path).unwrap();
                fs::write(session.join(relative).join(entry.file_name()), &bytes).unwrap();
                snapshots.push((path, bytes));
            }
        }
        let versions = VersionStore::open(&session).unwrap().load().versions;
        let content = |path: &Path| -> Vec<serde_json::Value> {
            fs::read_to_string(path)
                .unwrap()
                .lines()
                .filter(|s| !s.trim().is_empty())
                .map(|s| {
                    let mut line: serde_json::Value = serde_json::from_str(s).unwrap();
                    line.as_object_mut().unwrap().remove("speaker");
                    line
                })
                .collect()
        };
        let before: Vec<_> = versions
            .iter()
            .map(|v| content(&session.join("transcripts").join(&v.file)))
            .collect();
        let archive = Archive::new(work.path().to_path_buf());
        archive
            .name_speaker("copied", "Арсен Маркарян", "Участник 1")
            .unwrap();
        for (v, expected) in versions.iter().zip(before) {
            let path = session.join("transcripts").join(&v.file);
            assert_eq!(
                content(&path),
                expected,
                "speech/timing changed in {}",
                v.file
            );
            assert!(!fs::read_to_string(path).unwrap().contains("Арсен Маркарян"));
        }
        assert!(localvox_light_core::diarize::roster::load(&session)
            .members
            .iter()
            .any(|m| m.label == "Участник 1" && m.manually_named));
        for (path, bytes) in snapshots {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
        assert!(!session.join("summary.md").exists());
        assert_eq!(
            localvox_light_core::jobs::JobQueue::load(work.path()).jobs()[0].phase,
            localvox_light_core::jobs::JobPhase::Summary
        );
    }

    #[test]
    fn postprocess_rejects_missing_malformed_and_unconfirmed_empty_transcripts() {
        for damage in [None, Some("broken json"), Some("")] {
            let dir = tempfile::tempdir().unwrap();
            let source = retained_transcript(dir.path());
            if let Some(bytes) = damage {
                fs::write(&source, bytes).unwrap();
            } else {
                fs::remove_file(&source).unwrap();
            }
            let archive = Archive::new(dir.path().to_path_buf());
            assert!(archive.recook("retained", RecookScope::Summary).is_err());
            assert_eq!(
                fs::read_to_string(dir.path().join("sessions/retained/summary.md")).unwrap(),
                "previous summary"
            );
            assert!(!dir.path().join("jobs.json").exists());
        }
    }

    #[test]
    fn naming_reports_queue_failure_after_local_save_without_claiming_a_rebuild() {
        use localvox_light_core::diarize::roster;
        let work = tempfile::tempdir().unwrap();
        let source = retained_transcript(work.path());
        let speech = fs::read(&source).unwrap();
        let session = work.path().join("sessions/retained");
        roster::save(
            &session,
            &roster::Roster {
                members: vec![roster::Member {
                    id: 0,
                    label: "Old".into(),
                    embedding: vec![1.0, 0.0],
                    speech_sec: 1.0,
                    owner: false,
                    manually_named: false,
                }],
            },
        )
        .unwrap();
        fs::create_dir(work.path().join("jobs.json")).unwrap();
        let error = Archive::new(work.path().to_path_buf())
            .name_speaker("retained", "Old", "New")
            .unwrap_err();
        assert!(error
            .to_string()
            .contains("имя сохранено; повтор сводки не поставлен"));
        assert_eq!(roster::load(&session).members[0].label, "New");
        assert_eq!(fs::read(source).unwrap(), speech);
        assert!(!session.join("summary.md").exists());
    }

    #[test]
    fn recook_does_not_invalidate_artifacts_owned_by_a_live_worker() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("sessions/owned");
        fs::create_dir_all(session.join("audio")).unwrap();
        fs::write(session.join("audio/src0_chunk0001.wav"), b"fixture").unwrap();
        fs::write(session.join("summary.md"), b"keep this result").unwrap();
        let archive = Archive::new(dir.path().to_path_buf());
        let lease = localvox_light_core::jobs::try_execution_lock(&session)
            .unwrap()
            .unwrap();
        let error = archive.recook("owned", RecookScope::Summary).unwrap_err();
        assert!(
            error.to_string().contains("обработка ещё выполняется"),
            "{error:#}"
        );
        assert_eq!(
            fs::read(session.join("summary.md")).unwrap(),
            b"keep this result"
        );
        assert!(!dir.path().join("jobs.json").exists());
        drop(lease);
        assert!(localvox_light_core::jobs::try_execution_lock(&session)
            .unwrap()
            .is_some());
    }

    #[test]
    fn every_session_mutation_refuses_a_live_owner_before_changing_artifacts() {
        let dir = tempfile::tempdir().unwrap();
        let transcript = retained_transcript(dir.path());
        let session = dir.path().join("sessions/retained");
        fs::write(session.join("meta.json"), br#"{"lang":"ru"}"#).unwrap();
        let watched = [
            transcript,
            session.join("meta.json"),
            session.join("versions.json"),
            session.join("summary.md"),
        ];
        let before: Vec<_> = watched.iter().map(|p| fs::read(p).unwrap()).collect();
        let _lease = localvox_light_core::jobs::try_execution_lock(&session)
            .unwrap()
            .unwrap();
        let archive = Archive::new(dir.path().to_path_buf());
        for result in [
            archive.name_speaker("retained", "Speaker 1", "Changed"),
            archive.name_source("retained", 0, "Changed"),
            archive.set_lang("retained", Some("en")),
            archive.set_best("retained", 1).map(|_| String::new()),
            archive.delete_session("retained", "retained"),
        ] {
            assert!(result.unwrap_err().to_string().contains("сессия занята"));
            for (path, expected) in watched.iter().zip(&before) {
                assert_eq!(&fs::read(path).unwrap(), expected);
            }
        }
        assert!(!dir.path().join("jobs.json").exists());
    }

    #[test]
    fn language_is_restored_when_recook_cannot_be_queued() {
        let work = tempfile::tempdir().unwrap();
        let transcript = retained_transcript(work.path());
        let session = work.path().join("sessions/retained");
        fs::create_dir(session.join("audio")).unwrap();
        let meta = br#"{"started_at":"t","sample_rate":16000,"lang":"en","lang_detected":"en","chunks":[{"file":"src0_chunk0001","source_id":0,"start_offset_sec":0,"duration_sec":0.001}]}"#;
        fs::write(session.join("meta.json"), meta).unwrap();
        fs::write(
            session.join("audio/src0_chunk0001.wav"),
            wav_16k_mono(&[0; 32]),
        )
        .unwrap();
        localvox_light_core::artifacts::audio(&session).unwrap();
        fs::create_dir(work.path().join("jobs.json")).unwrap();
        let speech = fs::read(&transcript).unwrap();
        let error = Archive::new(work.path().to_path_buf())
            .set_lang("retained", None)
            .unwrap_err();
        assert!(
            error.to_string().contains("очередь не удалось сохранить"),
            "{error:#}"
        );
        assert_eq!(fs::read(session.join("meta.json")).unwrap(), meta);
        assert_eq!(fs::read(transcript).unwrap(), speech);
        assert_eq!(
            fs::read_to_string(session.join("summary.md")).unwrap(),
            "previous summary"
        );
    }

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
        assert_eq!(
            out,
            vec![i16::MAX, i16::MIN, 100],
            "the mix wrapped: {out:?}"
        );
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

        let store = VersionStore::open(&sdir).unwrap();
        let (id, path) = store.next_version("raw").unwrap();
        fs::write(
            &path,
            r#"{"source_id":0,"start_sec":0,"end_sec":5,"text":"hello"}"#,
        )
        .unwrap();
        store
            .commit(localvox_light_core::versions::VersionEntry {
                id,
                label: "raw".into(),
                file: path.file_name().unwrap().to_str().unwrap().into(),
                model: "fixture".into(),
                params: json!({}),
                created_at: "t".into(),
                parents: vec![],
            })
            .unwrap();
        fs::write(sdir.join("summary.md"), "A real summary").unwrap();
        // The check flagged the summary — exactly as it did on the live recording where «Сергей»
        // and «муж» were called inventions although both were said out loud.
        processing::record(
            &sdir,
            processing::SUMMARY,
            "summary/ru/qwen3.5:9b/p2",
            Outcome::Unverified,
            Some("имена/названия: сергей".into()),
            Some(id),
        )
        .unwrap();
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
        progress::mark(
            &sdir,
            Stage::Transcribe,
            StageState::Done,
            Some("149 строк"),
        );
        progress::mark(&sdir, Stage::Refine, StageState::Done, None);

        let mut q = JobQueue::load(dir.path());
        assert!(q.enqueue_cook("20260716_g", true, true, true));
        let id = q.pending_ids()[0];
        q.start(id).expect("the job did not start");

        let p = a.progress("20260716_g").unwrap();
        // Events without output files cannot prove completion; future stages stay blocked.
        assert_eq!(p["stages"][0]["state"], "invalid");
        assert_eq!(p["stages"][1]["state"], "invalid");
        assert_eq!(p["stages"][2]["state"], "skipped"); // automatic cleanup is no longer requested
        assert_eq!(p["stages"][3]["state"], "blocked");
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
        assert_eq!(
            a.delete_session("20260715_x", "20260715_x").unwrap(),
            "20260715_x"
        );
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

        a.name_source("20260718_rename", 0, "Арсен Маркарян")
            .unwrap();

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

        let line =
            |start: f64, speaker: Option<&str>| localvox_light_core::versions::TranscriptLine {
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
                .map(|l| {
                    serde_json::to_string(l).unwrap()
                        + "
"
                })
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
        roster
            .members
            .push(localvox_light_core::diarize::roster::Member {
                id: 1,
                label: "Участник 1".into(),
                speech_sec: 84.0,
                owner: false,
                embedding: vec![],
                manually_named: false,
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
        assert_eq!(
            p.lines, 3,
            "the source-attributed line was dropped from the count"
        );
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
        assert!(
            err.contains("недоступен"),
            "an honest error was expected: {err}"
        );
    }
}

/// PCM (s16le, 16 kHz mono) of a chunk over the sample range [from, to) (to is
/// clamped to the length). `.flac` is decoded over the requested interval by a bounded,
/// supervised ffmpeg; `.wav` is read pointwise (seek — we do not pull the whole file for the
/// sake of one piece). `None` — there is no file (or ffmpeg failed): the caller decides
/// whether that is an error or a skip.
fn chunk_pcm_range(audio: &Path, file: &str, from: u64, to: u64) -> Option<Vec<u8>> {
    localvox_light_core::chunks::codec::with_read(audio, file, |path| {
        localvox_light_core::chunks::codec::range(
            path,
            from,
            to,
            Path::new(&localvox_light_core::chunks::resolve_ffmpeg_for_decode()),
        )
    })
    .map_err(|error| tracing::warn!(%error,"audio range unavailable"))
    .ok()
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
