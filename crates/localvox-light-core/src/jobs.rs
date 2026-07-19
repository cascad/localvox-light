//! A durable queue of background jobs + discovery of uncooked sessions (WP-C7).
//!
//! The daemon (`--tray`) finds closed sessions without a transcript on its own and
//! queues them for cooking — the manual `localvox-process` is no longer mandatory.
//! The queue is persisted in `<work_dir>/jobs.json` (at-least-once: a job left in
//! `Running` when the process crashes → back to `Pending`; the cook is idempotent — a
//! version with the same label is skipped). Detachable (P5): the input is session
//! files only.
//!
//! The execution itself (launching `localvox-process`) lives in the daemon
//! (localvox-light): the core stays free of any dependency on ONNX/ort.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

use crate::versions::{now_rfc3339, VersionStore};

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
pub enum JobState {
    Pending,
    Running,
    Done,
    Failed,
}

/// How many times a `Failed` job may be resurrected before it settles for good.
///
/// Revival exists for TRANSIENT failures: Ollama was down, it came back. Three tries spread over
/// ~7 hours (1 h + 2 h + 4 h) answer that question honestly. What does not heal within that budget
/// is not transient, and a hundredth attempt teaches nobody anything — the job stays `Failed`,
/// visible, with its reason, until a human presses «переварить заново».
///
/// Without a bound this was a perpetual motion machine — see [`Job::revivals`].
pub const MAX_REVIVALS: u32 = 3;

/// What has to be done to the session.
///
/// A job is ALWAYS about a session — including ingestion: the session is created empty the
/// moment a link is given (the link lives in its meta), and the job fills it. That is why the
/// card shows up in the archive at once, with the stages of its own arrival visible, instead of
/// a link disappearing into a void for ten minutes.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
#[serde(rename_all = "lowercase")]
pub enum JobKind {
    /// Audio is already there (a recording) — transcribe and finish it.
    #[default]
    Cook,
    /// There is no audio yet: fetch it from the link in the session's meta, and then cook.
    Ingest,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    pub id: u64,
    /// The session directory name (relative to `<work_dir>/sessions`) — portable.
    pub session: String,
    /// `default` — old queues on disk have no such field, and they are all cooks.
    #[serde(default)]
    pub kind: JobKind,
    pub summary: bool,
    pub cleanup: bool,
    /// Clean the transcript up with an LLM as a new best version (`--refine`).
    #[serde(default)]
    pub refine: bool,
    pub state: JobState,
    pub attempts: u32,
    pub created_at: String,
    #[serde(default)]
    pub last_error: Option<String>,
    /// The moment of the transition to `Failed` (unix seconds) — for reviving with
    /// backoff.
    #[serde(default)]
    pub failed_at: Option<u64>,
    /// How many times this job has been RESURRECTED from `Failed`.
    ///
    /// Without this the queue was a perpetual motion machine: `mark_failed` gave up after
    /// `max_attempts`, and `revive_failed` then reset `attempts` back to 0 and set the job
    /// Pending again — every hour, and on every restart, FOREVER. Any permanent failure (a bug,
    /// a model that always chokes on this recording) meant the archive re-cooked itself on every
    /// launch. Measured on the owner's machine: sessions from two days ago still churning.
    ///
    /// Retries are bounded now. Revivals are for TRANSIENT failures (Ollama was down and came
    /// back), and a few tries over several hours settle that question. What does not heal in
    /// that budget will not heal by being tried a hundredth time — it stays `Failed`, visible,
    /// with its reason, until a HUMAN says «try again» (which resets this counter).
    #[serde(default)]
    pub revivals: u32,
    /// Cook again regardless of an already finished version (`--force`). It is set when
    /// a human said «this is bad, re-cook it»: the derived artifacts (transcript,
    /// summary, indexes) are disposable — they are recreated from the audio, and
    /// throwing them away is no loss. Only the audio itself is untouchable.
    #[serde(default)]
    pub force: bool,
}

#[derive(Serialize, Deserialize, Default)]
struct QueueFile {
    next_id: u64,
    jobs: Vec<Job>,
}

pub struct JobQueue {
    path: PathBuf,
    file: QueueFile,
}

impl JobQueue {
    /// Load the queue from `<work_dir>/jobs.json` (or an empty one).
    ///
    /// READING DOES NOT DECIDE ANYTHING. This function used to reinterpret job states as it read
    /// them, and that was the engine of every churn loop in this archive — because `load()` is not
    /// called on restart, it is called on EVERY autocook cycle, i.e. every few seconds.
    ///
    /// First it resurrected `Failed` (`Pending`, `attempts = 0`): a failure never survived to the
    /// next check, `max_attempts` never stuck, and the archive re-cooked itself forever. Measured
    /// on the owner's machine: 21 runs of one session between 17:29 and 20:27, `attempts` frozen
    /// at 1 because it was zeroed every cycle.
    ///
    /// Then — after that half was fixed — the `Running` reset stayed, and it was the same disease.
    /// Reclaiming a job abandoned by a dead daemon needs EVIDENCE that the daemon is dead. Loading
    /// a file is not evidence of anything. See [`Self::reclaim_abandoned`], which is called once,
    /// at startup, where that evidence exists.
    pub fn load(work_dir: &Path) -> Self {
        let path = work_dir.join("jobs.json");
        let mut file: QueueFile = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        // Trimming the history: we cut only the SUCCESSFUL ones (Done). Pending are
        // active; Failed within a run is a tombstone for the attempt cap (do not lose it).
        const KEEP_DONE: usize = 500;
        let done = file
            .jobs
            .iter()
            .filter(|j| j.state == JobState::Done)
            .count();
        if done > KEEP_DONE {
            let mut drop_left = done - KEEP_DONE;
            file.jobs.retain(|j| {
                if drop_left > 0 && j.state == JobState::Done {
                    drop_left -= 1;
                    false // the oldest Done (at the head of the Vec) get thrown out
                } else {
                    true
                }
            });
        }
        Self { path, file }
    }

    fn save(&self) {
        let tmp = self.path.with_extension("json.tmp");
        let write = fs::write(
            &tmp,
            serde_json::to_vec_pretty(&self.file).unwrap_or_default(),
        )
        .and_then(|()| fs::rename(&tmp, &self.path));
        if let Err(e) = write {
            tracing::warn!("jobs.json was not written: {e}");
        }
    }

    pub fn jobs(&self) -> &[Job] {
        &self.file.jobs
    }

    /// The session was cooked by a stale recipe — put it back into the queue, even if
    /// its job is already `Done`. Without this, dedup by session name would close the
    /// road to a re-cook forever: we improved the cook, but the archive stayed as it was.
    /// A running/waiting job we leave alone — it will finish cooking with the current
    /// recipe anyway.
    pub fn requeue_stale(
        &mut self,
        session: &str,
        summary: bool,
        cleanup: bool,
        refine: bool,
    ) -> bool {
        let Some(job) = self.file.jobs.iter_mut().find(|j| j.session == session) else {
            return self.enqueue_cook(session, summary, cleanup, refine);
        };
        if !matches!(job.state, JobState::Done) {
            return false;
        }
        job.state = JobState::Pending;
        job.attempts = 0;
        job.last_error = None;
        job.failed_at = None;
        job.summary = summary;
        job.cleanup = cleanup;
        job.refine = refine;
        self.save();
        true
    }

    /// «This is bad — re-cook it». A human rejected the result: we throw the derived
    /// artifacts away and cook again from scratch, regardless of dedup, of the recipe
    /// and of the version that is already there. The audio we do not touch — everything
    /// is recreated from it.
    ///
    /// A running job we do not interrupt (it will finish cooking anyway), but `Done` and
    /// `Failed` ones we return to the queue with the `force` flag.
    pub fn enqueue_recook(
        &mut self,
        session: &str,
        summary: bool,
        cleanup: bool,
        refine: bool,
    ) -> bool {
        if let Some(job) = self.file.jobs.iter_mut().find(|j| j.session == session) {
            if matches!(job.state, JobState::Running) {
                return false;
            }
            job.state = JobState::Pending;
            job.attempts = 0;
            job.last_error = None;
            job.failed_at = None;
            // A HUMAN said «try again» — that buys a fresh revival budget. Without this, a job
            // that had burned through its budget would swallow the button: pressed, and nothing
            // ever happens again.
            job.revivals = 0;
            job.summary = summary;
            job.cleanup = cleanup;
            job.refine = refine;
            job.force = true;
            self.save();
            return true;
        }
        let queued = self.enqueue_cook(session, summary, cleanup, refine);
        if queued {
            if let Some(job) = self.file.jobs.last_mut() {
                job.force = true;
            }
            self.save();
        }
        queued
    }

    /// The session is cooked, but the required artifact is missing — finish the job.
    ///
    /// Loop safety lives NOT here but in the processing log (`processing.json`): «there
    /// was nothing to do» is a record, not an empty space, and a session with such a
    /// record no longer shows up in discovery. That is why here we can honestly return
    /// any finished job to the queue.
    pub fn requeue_for_artifacts(
        &mut self,
        session: &str,
        summary: bool,
        cleanup: bool,
        refine: bool,
    ) -> bool {
        let Some(job) = self.file.jobs.iter_mut().find(|j| j.session == session) else {
            return self.enqueue_cook(session, summary, cleanup, refine);
        };
        if !matches!(job.state, JobState::Done) {
            return false; // already in flight — it will finish on its own
        }
        job.state = JobState::Pending;
        job.attempts = 0;
        job.last_error = None;
        job.failed_at = None;
        job.summary = summary;
        job.cleanup = cleanup;
        job.refine = refine;
        self.save();
        true
    }

    /// Queue a session for cooking. Dedup by session name: if a job for it already
    /// exists (in any state) — we do not breed a second one. `true` — a new one was
    /// queued.
    pub fn enqueue_cook(
        &mut self,
        session: &str,
        summary: bool,
        cleanup: bool,
        refine: bool,
    ) -> bool {
        if self.file.jobs.iter().any(|j| j.session == session) {
            return false;
        }
        let id = self.file.next_id;
        self.file.next_id += 1;
        self.file.jobs.push(Job {
            id,
            session: session.to_string(),
            kind: JobKind::Cook,
            summary,
            cleanup,
            refine,
            state: JobState::Pending,
            attempts: 0,
            created_at: now_rfc3339(),
            last_error: None,
            failed_at: None,
            revivals: 0,
            force: false,
        });
        self.save();
        true
    }

    /// A link was given: the session exists but is EMPTY — the audio has yet to be fetched.
    ///
    /// The same queue, the same durability, the same at-least-once as for a cook. What changes
    /// is only where the audio comes from: not from the microphone but from the network, and the
    /// job knows to fetch it first.
    pub fn enqueue_ingest(
        &mut self,
        session: &str,
        summary: bool,
        cleanup: bool,
        refine: bool,
    ) -> bool {
        if self.file.jobs.iter().any(|j| j.session == session) {
            return false;
        }
        let id = self.file.next_id;
        self.file.next_id += 1;
        self.file.jobs.push(Job {
            id,
            session: session.to_string(),
            kind: JobKind::Ingest,
            summary,
            cleanup,
            refine,
            state: JobState::Pending,
            attempts: 0,
            created_at: now_rfc3339(),
            last_error: None,
            failed_at: None,
            revivals: 0,
            force: false,
        });
        self.save();
        true
    }

    /// Ids of all pending jobs (a snapshot taken at the start of the scheduler cycle):
    /// we run each of them exactly once per cycle, so that a failing job does not burn
    /// through its attempts back-to-back in a single pass — a transient failure is
    /// retried on the next cycle.
    /// Take back the jobs a dead daemon left mid-cook: `Running` → `Pending`.
    ///
    /// CALLED ONCE, AT STARTUP, and nowhere else. This is the at-least-once guarantee, and it is
    /// safe only here: we have just started, so nothing of ours is cooking, so a `Running` job can
    /// only be the corpse of a previous daemon (which kills its child and deliberately leaves the
    /// job Running — see the shutdown path in the autocook loop). On any later cycle the same
    /// state means the exact opposite: it is cooking RIGHT NOW.
    ///
    /// **The force flag is dropped.** «Переварить» is a destructive instruction — the cook wipes
    /// the finished documents before redoing them — and destructive instructions are not replayed
    /// on a guess. The owner pressed it ONCE, at 17:21; the job never reached `Done` (the daemon
    /// was killed each time before it could write that down), so the flag survived every replay and
    /// every replay wiped the documents the previous one had finished. Five hours, twenty-one runs,
    /// nothing kept.
    ///
    /// So the replay is a NORMAL job: it fills in what `processing.json` says is missing and skips
    /// what is done. If the forced run was killed before it managed anything, the replay does
    /// nothing and the human presses «переварить» again — one word, one wipe.
    pub fn reclaim_abandoned(&mut self) -> usize {
        let mut taken = 0;
        for j in &mut self.file.jobs {
            if j.state == JobState::Running {
                j.state = JobState::Pending;
                j.force = false;
                taken += 1;
            }
        }
        if taken > 0 {
            self.save();
        }
        taken
    }

    pub fn pending_ids(&self) -> Vec<u64> {
        self.file
            .jobs
            .iter()
            .filter(|j| j.state == JobState::Pending)
            .map(|j| j.id)
            .collect()
    }

    /// Mark a specific job as `Running` (+an attempt). Returns a copy if it really was
    /// `Pending` (otherwise None — already claimed/finished).
    pub fn start(&mut self, id: u64) -> Option<Job> {
        let job = self.file.jobs.iter_mut().find(|j| j.id == id)?;
        if job.state != JobState::Pending {
            return None;
        }
        job.state = JobState::Running;
        job.attempts += 1;
        let claimed = job.clone();
        self.save();
        Some(claimed)
    }

    /// Take the next pending job (FIFO). A thin wrapper around `start`.
    pub fn claim_next(&mut self) -> Option<Job> {
        let id = self
            .file
            .jobs
            .iter()
            .find(|j| j.state == JobState::Pending)?
            .id;
        self.start(id)
    }

    pub fn mark_done(&mut self, id: u64) {
        if let Some(j) = self.file.jobs.iter_mut().find(|j| j.id == id) {
            j.state = JobState::Done;
            j.last_error = None;
            // The re-cook has done its work — we drop force. Otherwise the flag would
            // stay forever and any subsequent revival of the job would cook the session
            // again, round and round.
            j.force = false;
        }
        self.save();
    }

    /// The re-cook has done its job — the version is committed. The `force` flag is not
    /// needed any more: without this, a job failing in post-processing (the LLM is down,
    /// exit 2) would cook the session anew on EVERY attempt and breed one transcript
    /// version per attempt.
    pub fn clear_force(&mut self, id: u64) {
        if let Some(j) = self.file.jobs.iter_mut().find(|j| j.id == id) {
            if j.force {
                j.force = false;
                self.save();
            }
        }
    }

    /// The session is gone — drop any job for it. A job pointing at a directory that no longer
    /// exists would fail on every attempt forever, and the queue would carry a ghost.
    pub fn forget_session(&mut self, session: &str) {
        let before = self.file.jobs.len();
        self.file.jobs.retain(|j| j.session != session);
        if self.file.jobs.len() != before {
            self.save();
        }
    }

    /// Drop every job whose session no longer exists on disk.
    ///
    /// `forget_session` covers deletion THROUGH THE APP, and it works — but it is the only door it
    /// covers. A session can also vanish by hand, or its directory can be removed by a failed
    /// ingest, and then the queue keeps a ghost: measured 18.07.2026, two ingest jobs sat there
    /// «сорвалось, повторю» against directories that were not there any more.
    ///
    /// This is the invariant rather than another cleanup call: THE QUEUE MUST NOT HOLD WORK FOR
    /// SOMETHING THAT DOES NOT EXIST, whatever route the session left by. And it decides on
    /// evidence, not on a guess — the absence of the directory is a fact about the filesystem,
    /// checked at the moment of use.
    ///
    /// A session being recorded right now is never touched: its directory exists from the first
    /// moment, so it cannot look like a ghost.
    pub fn drop_orphans(&mut self, work_dir: &Path) -> usize {
        let sessions = work_dir.join("sessions");
        let before = self.file.jobs.len();
        let mut gone: Vec<String> = Vec::new();
        self.file.jobs.retain(|j| {
            let alive = sessions.join(&j.session).is_dir();
            if !alive {
                gone.push(j.session.clone());
            }
            alive
        });
        if self.file.jobs.len() != before {
            tracing::info!("queue: dropped {} job(s) — session gone: {}", gone.len(), gone.join(", "));
            self.save();
        }
        before - self.file.jobs.len()
    }

    /// A job failure: while the attempts are below `max_attempts` — back to `Pending`
    /// (retry later), otherwise — `Failed` (we do not hammer a broken session forever).
    pub fn mark_failed(&mut self, id: u64, err: &str, max_attempts: u32) {
        if let Some(j) = self.file.jobs.iter_mut().find(|j| j.id == id) {
            j.last_error = Some(err.to_string());
            j.state = if j.attempts >= max_attempts {
                j.failed_at = Some(unix_now());
                JobState::Failed
            } else {
                JobState::Pending
            };
        }
        self.save();
    }

    /// Revive `Failed` jobs older than `min_age`: a transient failure (Ollama was
    /// restarting, the disk was busy) heals itself without a daemon restart. The series
    /// of attempts + the backoff keep the protection against hammering forever — the
    /// cook is idempotent, and a repeat is cheap (`Skipped`).
    pub fn revive_failed(&mut self, min_age: std::time::Duration) -> usize {
        let now = unix_now();
        let mut revived = 0usize;
        for j in &mut self.file.jobs {
            if j.state != JobState::Failed {
                continue;
            }
            // THE BUDGET. Beyond it the job stays Failed for good — no more churn. Only a human
            // («переварить заново») buys it a new budget.
            if j.revivals >= MAX_REVIVALS {
                continue;
            }
            // The wait doubles with every revival: 1 h, 2 h, 4 h. A failure that survives the
            // whole budget is not transient, and hammering it every hour teaches nobody anything.
            let wait = min_age.as_secs().saturating_mul(1u64 << j.revivals.min(16));
            let old_enough = j
                .failed_at
                .map(|t| now.saturating_sub(t) >= wait)
                .unwrap_or(true);
            if !old_enough {
                continue;
            }
            j.state = JobState::Pending;
            j.attempts = 0;
            j.failed_at = None;
            j.revivals += 1;
            revived += 1;
        }
        if revived > 0 {
            self.save();
        }
        revived
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// The marker file of the session being written right now (the pipeline writes it, the
/// auto-cook reads it).
pub const RECORDING_MARKER: &str = ".recording_session";

/// The request «start recording».
///
/// **Recording is not the default.** Capture runs always, but nothing reaches the disk
/// until a human says so: a recorder left running all day through an office writes other
/// people's conversations, and none of them agreed to that. The button is made safe by the
/// pre-roll ring — the last minutes live in memory and enter the session at the press, so a
/// conversation that began before it is not lost.
///
/// A file, not a channel: the request comes from the HTTP thread (the web UI), from the
/// tray and from the voice command, while the session is owned by the recording pipeline.
/// A file is the cheapest way to shout across any boundary, and it survives everything
/// short of the disk being deleted.
///
/// The title goes INTO THE SESSION NAME (`20260713_181500_planerka`) — so the meeting is
/// visible in the list instead of being hunted for among the day's nameless stretches.
pub const RECORD_START_MARKER: &str = ".record_start";

/// Ask the engine to start recording. An empty title is a recording without a name.
pub fn request_record_start(work_dir: &Path, title: &str) -> std::io::Result<()> {
    fs::write(work_dir.join(RECORD_START_MARKER), title.trim())
}

/// Take the request — exactly once. An empty file is a request too.
pub fn take_record_start(work_dir: &Path) -> Option<String> {
    let path = work_dir.join(RECORD_START_MARKER);
    let title = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    Some(title.trim().to_string())
}

/// The request «stop recording». `reason` goes into `meta.json`: the archive must be able
/// to say who ended the recording — a human, a voice command or the silence watchdog.
pub const RECORD_STOP_MARKER: &str = ".record_stop";

pub fn request_record_stop(work_dir: &Path, reason: &str) -> std::io::Result<()> {
    fs::write(work_dir.join(RECORD_STOP_MARKER), reason)
}

/// Take the request (if there is one) — exactly once: the marker is deleted immediately.
pub fn take_record_stop(work_dir: &Path) -> Option<String> {
    let path = work_dir.join(RECORD_STOP_MARKER);
    let reason = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    Some(reason.trim().to_string()).filter(|s| !s.is_empty())
}

/// The name of the session the engine is writing into right now (it must not be cooked —
/// it may continue after a pause / an append). None — the engine is not writing.
pub fn recording_session(work_dir: &Path) -> Option<String> {
    let s = fs::read_to_string(work_dir.join(RECORDING_MARKER)).ok()?;
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

/// The names of closed (quiet) sessions that have audio but no transcript — candidates
/// for cooking. `quiescent_secs` — how long a session must stay unchanged to be
/// considered closed. The active one (the `RECORDING_MARKER` marker) we do not take in
/// any case — it may continue after a pause/rotation.
pub fn sessions_needing_cook(work_dir: &Path, quiescent_secs: u64) -> Vec<String> {
    let root = work_dir.join("sessions");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let active = recording_session(work_dir);
    let mut out = Vec::new();
    for e in entries.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned());
        if name.as_deref() == active.as_deref() {
            continue; // being written right now — do not touch
        }
        if needs_cook(&dir, quiescent_secs) {
            if let Some(name) = name {
                out.push(name);
            }
        }
    }
    out.sort();
    out
}

/// What the daemon WANTS to get from a session: artifact → its recipe.
///
/// «Processed» = for every wanted artifact the session's log holds a record with the SAME
/// recipe. Not «the file is there» — the file may legitimately be missing (there is no
/// speech in the recording), and that very ambiguity gave birth to the endless loop.
/// Which post-processing the daemon does by default: `(summary, cleaned-up text, refined
/// version)`.
///
/// **The single place where this is decided.** There used to be two: the daemon enabled
/// the summary and the cleaned-up text by default, while the «♻ Re-cook» button read the
/// same variables by the rule «no variable — means switched off». As a result the button
/// ERASED the summary and queued a job without the flag to make it: a human pressed
/// «redo it» and got «delete it». Two readings of one setting is not code duplication, it
/// is two different answers to one question.
pub fn post_processing_from_env() -> (bool, bool, bool) {
    let on_unless_off = |name: &str| {
        !matches!(
            std::env::var(name)
                .unwrap_or_default()
                .to_lowercase()
                .as_str(),
            "0" | "off" | "false" | "no"
        )
    };
    let off_unless_on = |name: &str| {
        matches!(
            std::env::var(name)
                .unwrap_or_default()
                .to_lowercase()
                .as_str(),
            "1" | "on" | "true" | "yes"
        )
    };
    (
        on_unless_off("LOCALVOX_LIGHT_AUTOCOOK_SUMMARY"),
        on_unless_off("LOCALVOX_LIGHT_AUTOCOOK_CLEANUP"),
        off_unless_on("LOCALVOX_LIGHT_AUTOCOOK_REFINE"),
    )
}

/// The recipe depends on the LANGUAGE, and every session has its own language, so what
/// lives here is not a ready recipe but the things it is computed from. Both discovery
/// (before the work) and the processor (after it) MUST compute it identically — otherwise
/// the recipes diverge and the artifact is made forever. The common place is
/// [`crate::processing::llm_recipe`].
pub struct Wanted {
    pub summary: bool,
    pub cleanup: bool,
    /// The summary template explicitly chosen by a human. `None` — pick it by language.
    pub summary_template: Option<String>,
    pub model: String,
}

impl Wanted {
    /// From the daemon config: which artifacts are enabled and what makes them.
    pub fn new(
        summary: bool,
        cleanup: bool,
        summary_template: Option<String>,
        model: &str,
    ) -> Self {
        Self {
            summary,
            cleanup,
            summary_template,
            model: model.to_string(),
        }
    }

    fn nothing_wanted(&self) -> bool {
        !self.summary && !self.cleanup
    }

    /// Artifact → the recipe for THIS session (its language, its template).
    fn recipes(&self, session_dir: &Path) -> Vec<(&'static str, String)> {
        use crate::processing::{llm_recipe, llm_style, PROCESSED, SUMMARY};
        let lang = crate::lang::text(session_dir);
        let mut v = Vec::new();
        if self.summary {
            let style = llm_style(self.summary_template.as_deref(), &lang);
            v.push((SUMMARY, llm_recipe(SUMMARY, &style, &self.model)));
        }
        if self.cleanup {
            v.push((PROCESSED, llm_recipe(PROCESSED, &lang, &self.model)));
        }
        v
    }
}

/// Sessions that have a transcript but not yet the wanted artifacts made by THIS recipe.
///
/// An endless loop is impossible by construction: «there was nothing to do» is a record
/// in the log, not an empty space. A record with the same recipe is enough, whatever it
/// ended with (done / nothing to do / unverified).
pub fn sessions_needing_artifacts(work_dir: &Path, want: &Wanted) -> Vec<String> {
    if want.nothing_wanted() {
        return Vec::new();
    }
    let root = work_dir.join("sessions");
    let Ok(entries) = fs::read_dir(&root) else {
        return Vec::new();
    };
    let active = recording_session(work_dir);
    let mut out = Vec::new();
    for e in entries.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        let Some(name) = dir.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if Some(name.as_str()) == active.as_deref() {
            continue;
        }
        // A session without a single line of speech does not deserve artifacts: the cook
        // has already found that out and written it into the log (Outcome::Nothing).
        let has_speech = VersionStore::open(&dir)
            .ok()
            .and_then(|s| s.best().map(|v| (s, v)))
            .and_then(|(s, v)| s.resolve(v.id))
            .and_then(|p| crate::versions::read_transcript_lines(&p).ok())
            .map(|l| !l.is_empty())
            .unwrap_or(false);
        if !has_speech {
            continue;
        }
        // DONE IS DONE — the same rule as for the cook above.
        //
        // This asked `is_current(dir, art, recipe)`: was this document made by the recipe I want
        // RIGHT NOW. Any drift in that recipe — the prompt revision, the model named in `.env`,
        // the language — declared finished work unfinished and re-made the whole archive behind
        // the human's back. It bit hardest across a rebuild: a daemon carrying prompt revision 1
        // reading documents stamped revision 2 re-queued every one of them, on every launch,
        // for ever.
        //
        // Now the ledger decides, and only the ledger: if the document was made — whatever it was
        // made by, whatever it ended with (a result, a doubt, or an honest «there was nothing
        // here») — it is done. `Failed` alone is unfinished, and the queue retries that on its
        // own, with a bounded budget.
        //
        // A better prompt reaches the old archive when the HUMAN presses «переварить заново». It
        // does not happen behind his back, and it does not happen a hundred times.
        if want
            .recipes(&dir)
            .iter()
            .any(|(art, _recipe)| !crate::processing::is_done(&dir, art))
        {
            out.push(name);
        }
    }
    out.sort();
    out
}

fn needs_cook(session_dir: &Path, quiescent_secs: u64) -> bool {
    let newest = newest_wav_mtime(&session_dir.join("audio"));
    let Some(newest) = newest else {
        return false; // no audio — nothing to cook
    };
    // The session was closed EXPLICITLY (the «Finish» button) — there is no point in
    // waiting for «silence just in case»: the engine will not touch it any more. A human
    // pressed the button for the sake of a summary, not for the sake of a minute of
    // waiting.
    let closed_explicitly = std::fs::read(session_dir.join("meta.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .map(|m| !m["stopped_at"].is_null())
        .unwrap_or(false);
    // a session that is being actively written we do not touch
    let quiet = closed_explicitly
        || SystemTime::now()
            .duration_since(newest)
            .map(|d| d.as_secs() >= quiescent_secs)
            .unwrap_or(true);
    if !quiet {
        return false;
    }
    // COOKED IS COOKED. Not «cooked by the recipe I happen to want right now».
    //
    // This asked `has_recipe(session_cook_recipe(dir))` — is there a version made by the CURRENT
    // recipe — and re-cooked everything otherwise. The idea was good: improve the cook, and the
    // archive catches up by itself. The practice was a machine for re-cooking the whole archive
    // on every launch, because that recipe is RECOMPUTED EVERY CYCLE out of things that drift:
    //
    //   * `diarize::enabled()` → `-spk` in the recipe → and it looks for the model RELATIVE TO THE
    //     WORKING DIRECTORY. Start the daemon from the repo root — found, `-spk`. Start it from
    //     anywhere else (autostart runs in system32) — not found, no `-spk`. Meanwhile the child
    //     processor gets an explicit `--model-dir`, always finds it, and writes `-spk` back. The
    //     two ends disagree for ever.
    //   * the language, the ASR defaults, the prompt revision — every one of them a fresh reason
    //     to declare finished work unfinished.
    //
    // The owner watched his archive re-cook itself on every start for days, sessions from two days
    // ago included. So the rule is his, and it is the honest one: A FINISHED JOB IS MATERIALISED.
    // If the ledger says this session has a transcript, it has one. Nothing is redone behind the
    // human's back — a better cook reaches the archive when HE presses «переварить заново», and
    // that button already exists and already forces.
    let cooked = VersionStore::open(session_dir)
        .ok()
        .and_then(|s| s.best())
        .is_some();
    !cooked
}

/// The recipe this session MUST be cooked by: the default parameters plus its language.
/// It is computed without going to the model — from meta only, because it is called on
/// every session in every daemon cycle.
pub fn session_cook_recipe(session_dir: &Path) -> String {
    crate::versions::cook_recipe(
        &crate::lang::label(&crate::lang::asr(session_dir)),
        crate::versions::DEFAULT_MAX_WINDOW_SEC,
        crate::versions::DEFAULT_MIN_CUT_SEC,
        crate::versions::DEFAULT_SILENCE_MS,
        crate::diarize::enabled(),
    )
}

fn newest_wav_mtime(audio_dir: &Path) -> Option<SystemTime> {
    let mut newest: Option<SystemTime> = None;
    for e in fs::read_dir(audio_dir).ok()?.flatten() {
        let p = e.path();
        // We also take into account the `.part` being actively written: while the next
        // chunk is being recorded, the freshest `.wav` is the previous one (frozen), and
        // a multi-chunk session would falsely look quiet. A live `.part` is fsync'ed
        // every ~5 s — its mtime honestly shows activity (an orphan `.part` after a crash
        // is frozen and after `quiescent` correctly becomes a candidate). `.flac` — in
        // case closed chunks have been compressed (then the `.wav` is deleted).
        let is_chunk = matches!(
            p.extension().and_then(|x| x.to_str()),
            Some("wav") | Some("part") | Some("flac")
        );
        if !is_chunk {
            continue;
        }
        if let Ok(t) = p.metadata().and_then(|m| m.modified()) {
            newest = Some(match newest {
                Some(cur) if cur >= t => cur,
                _ => t,
            });
        }
    }
    newest
}

#[cfg(test)]
mod tests {

    /// The request is taken EXACTLY ONCE — otherwise the engine would restart the recording
    /// on every frame for as long as the file lies there.
    #[test]
    fn a_start_request_is_taken_exactly_once() {
        let d = tempfile::tempdir().unwrap();
        assert!(take_record_start(d.path()).is_none(), "there was no request");

        request_record_start(d.path(), "  Планёрка  ").unwrap();
        assert_eq!(take_record_start(d.path()).as_deref(), Some("Планёрка"));
        assert!(
            take_record_start(d.path()).is_none(),
            "the request fired twice"
        );
    }

    /// A recording WITHOUT a title is still a recording. An empty string means «record, I
    /// just cannot name it», not «cancel».
    #[test]
    fn a_recording_without_a_name_is_still_a_recording() {
        let d = tempfile::tempdir().unwrap();
        request_record_start(d.path(), "").unwrap();
        assert_eq!(take_record_start(d.path()).as_deref(), Some(""));
    }

    /// The stop carries a REASON, and it must survive to the meta: the archive has to be able
    /// to say who ended the recording — a human, a voice command, or the silence watchdog.
    #[test]
    fn a_stop_request_carries_its_reason() {
        let d = tempfile::tempdir().unwrap();
        assert!(take_record_stop(d.path()).is_none());
        request_record_stop(d.path(), "тишина 15 мин").unwrap();
        assert_eq!(take_record_stop(d.path()).as_deref(), Some("тишина 15 мин"));
        assert!(take_record_stop(d.path()).is_none(), "the request fired twice");
    }

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn enqueue_dedups_by_session() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        assert!(q.enqueue_cook("20260711_a", false, false, false));
        assert!(
            !q.enqueue_cook("20260711_a", true, true, false),
            "a duplicate must not be queued"
        );
        assert!(q.enqueue_cook("20260711_b", false, false, false));
        assert_eq!(q.jobs().len(), 2);
    }

    #[test]
    fn claim_run_done_lifecycle_persists() {
        let dir = tempdir().unwrap();
        {
            let mut q = JobQueue::load(dir.path());
            q.enqueue_cook("s1", false, false, false);
            let job = q.claim_next().unwrap();
            assert_eq!(job.state, JobState::Running);
            assert_eq!(job.attempts, 1);
            q.mark_done(job.id);
        }
        // a reload sees Done
        let q = JobQueue::load(dir.path());
        assert_eq!(q.jobs()[0].state, JobState::Done);
    }

    #[test]
    fn failed_retries_until_cap() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", false, false, false);
        let j = q.claim_next().unwrap(); // attempts=1
        q.mark_failed(j.id, "boom", 2);
        assert_eq!(
            q.jobs()[0].state,
            JobState::Pending,
            "under the cap — retry"
        );
        let j = q.claim_next().unwrap(); // attempts=2
        q.mark_failed(j.id, "boom", 2);
        assert_eq!(
            q.jobs()[0].state,
            JobState::Failed,
            "the cap is reached — Failed"
        );
        assert!(q.claim_next().is_none());
    }

    #[test]
    fn revive_failed_respects_backoff() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", false, false, false);
        let j = q.claim_next().unwrap();
        q.mark_failed(j.id, "ollama down", 1); // attempts=1 >= cap → Failed
        assert_eq!(q.jobs()[0].state, JobState::Failed);
        // a fresh Failed under backoff — does not revive
        assert_eq!(q.revive_failed(std::time::Duration::from_secs(3600)), 0);
        assert_eq!(q.jobs()[0].state, JobState::Failed);
        // without backoff (0) — revives with the attempts reset
        assert_eq!(q.revive_failed(std::time::Duration::ZERO), 1);
        assert_eq!(q.jobs()[0].state, JobState::Pending);
        assert_eq!(q.jobs()[0].attempts, 0);
    }

    // ─── The queue MUST settle. Simulated failures — the owner's "и так происходит каждый
    //     запуск" (WP-C67) ───

    /// THE BUG THAT CHURNED THE ARCHIVE FOR DAYS.
    ///
    /// `load()` used to reset every `Failed` job to `Pending` — on the theory that "a restart is
    /// an explicit try-again". But `load()` is called on EVERY autocook cycle, i.e. every few
    /// seconds. So a failure never survived: the cap never stuck, the backoff never mattered, and
    /// the archive re-cooked itself forever. Sessions from two days ago were still churning.
    ///
    /// A DELETED SESSION MUST NOT KEEP A JOB. Measured 18.07.2026: two ingest jobs sat in the queue
    /// «сорвалось, повторю» against directories that no longer existed — the app looked broken over
    /// recordings the owner had already thrown away.
    ///
    /// And the sweep must run BEFORE `revive_failed`, or the retry faithfully resurrects the ghost.
    #[test]
    fn a_job_for_a_deleted_session_is_dropped_and_not_revived() {
        let d = tempfile::tempdir().unwrap();
        let wd = d.path();
        std::fs::create_dir_all(wd.join("sessions/alive")).unwrap();

        let mut q = JobQueue::load(wd);
        q.enqueue_cook("alive", true, true, true);
        q.enqueue_cook("deleted", true, true, true);
        // Both failed long enough ago that `revive_failed` would take them back.
        for id in q.jobs().iter().map(|j| j.id).collect::<Vec<_>>() {
            q.mark_failed(id, "boom", 0);
        }

        assert_eq!(q.drop_orphans(wd), 1, "the ghost was not dropped");
        let left: Vec<&str> = q.jobs().iter().map(|j| j.session.as_str()).collect();
        assert_eq!(left, vec!["alive"], "wrong job removed");

        // The revival must not bring back what is gone — there is nothing left for it to find.
        q.revive_failed(std::time::Duration::from_secs(0));
        let after: Vec<&str> = q.jobs().iter().map(|j| j.session.as_str()).collect();
        assert_eq!(after, vec!["alive"], "a deleted session came back through revive_failed");
    }

    /// A session that IS there keeps its work — the sweep must not become a queue-emptier.
    #[test]
    fn a_live_session_keeps_its_job() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("sessions/here")).unwrap();
        let mut q = JobQueue::load(d.path());
        q.enqueue_cook("here", true, true, true);
        assert_eq!(q.drop_orphans(d.path()), 0);
        assert_eq!(q.jobs().len(), 1);
    }

    /// A failure must SURVIVE a reload. Only `revive_failed` (bounded, with backoff) or a human
    /// may bring it back.
    #[test]
    fn a_failed_job_survives_a_reload_and_is_not_resurrected() {
        let dir = tempdir().unwrap();
        {
            let mut q = JobQueue::load(dir.path());
            q.enqueue_cook("s1", true, true, true);
            let j = q.claim_next().unwrap();
            q.mark_failed(j.id, "boom", 0); // cap 0 → straight to Failed
            assert_eq!(q.jobs()[0].state, JobState::Failed);
        }
        // Every autocook cycle reloads the queue. Ten cycles must not resurrect anything.
        for cycle in 0..10 {
            let q = JobQueue::load(dir.path());
            assert_eq!(
                q.jobs()[0].state,
                JobState::Failed,
                "reload #{cycle} resurrected a failed job — the archive will churn forever"
            );
            assert!(
                q.jobs()[0].failed_at.is_some(),
                "the tombstone was lost — the backoff has nothing to count from"
            );
        }
        // And it is not handed out for work.
        let mut q = JobQueue::load(dir.path());
        assert!(q.claim_next().is_none(), "a failed job was handed out again");
    }

    /// A process that crashes mid-cook IS replayed — that is the at-least-once guarantee, and it
    /// must not be lost while fixing the churn above. But it is replayed by the STARTUP, which
    /// knows the previous daemon is gone — not by a file read, which knows nothing.
    #[test]
    fn a_crashed_running_job_is_replayed_when_the_next_daemon_starts() {
        let dir = tempdir().unwrap();
        {
            let mut q = JobQueue::load(dir.path());
            q.enqueue_cook("s1", false, false, false);
            q.claim_next().unwrap(); // Running — and then "the process dies"
            assert_eq!(q.jobs()[0].state, JobState::Running);
        }
        let mut q = JobQueue::load(dir.path());
        assert_eq!(q.reclaim_abandoned(), 1);
        assert_eq!(q.jobs()[0].state, JobState::Pending, "the crashed cook was lost");
        assert!(q.claim_next().is_some());
    }

    /// THE COOK THAT IS COOKING RIGHT NOW MUST BE LEFT ALONE.
    ///
    /// `load()` runs on every autocook cycle — every few seconds. While it reset `Running` to
    /// `Pending`, a job in flight was re-queued and started again underneath itself. This is the
    /// same disease as the `Failed` resurrection, in its second half: reading a file was treated as
    /// proof that the worker had died.
    #[test]
    fn a_cook_in_flight_is_not_restarted_underneath_itself() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", true, true, true);
        let claimed = q.claim_next().unwrap();

        // The cook is running. The daemon keeps living: dozens of cycles, each re-reading the file.
        for _ in 0..50 {
            let q = JobQueue::load(dir.path());
            assert!(
                q.pending_ids().is_empty(),
                "a cook in flight was handed out for work a second time"
            );
            assert_eq!(q.jobs()[0].state, JobState::Running);
        }

        // It finishes normally — nothing has trampled it.
        let mut q = JobQueue::load(dir.path());
        q.mark_done(claimed.id);
        assert_eq!(q.jobs()[0].state, JobState::Done);
        assert_eq!(q.jobs()[0].attempts, 1, "the job was started more than once");
    }

    /// «ПЕРЕВАРИТЬ» — ОДНО СЛОВО, ОДНА ПЕРЕВАРКА.
    ///
    /// The owner pressed it once, at 17:21. The cook wipes the finished documents before redoing
    /// them, and the daemon was killed each time before it could write `Done` — so `force` survived
    /// every replay, and every replay wiped what the previous one had finished. Twenty-one runs
    /// between 17:29 and 20:27 on his machine; nothing kept.
    ///
    /// A destructive instruction is not replayed on a guess. The replay is a normal job: it
    /// finishes what is missing. If the forced run managed nothing, the human says the word again.
    #[test]
    fn an_interrupted_recook_does_not_wipe_the_archive_again_on_every_restart() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", true, true, true);
        q.mark_done(0);
        // The human rejects the result.
        assert!(q.enqueue_recook("s1", true, true, true));
        let job = q.claim_next().unwrap();
        assert!(job.force, "the recook did not force — the test proves nothing");

        // The daemon is killed mid-cook: the job stays Running, exactly as the shutdown path
        // leaves it.
        assert_eq!(q.jobs()[0].state, JobState::Running);

        // Restart after restart, the replay must never carry the wipe again.
        for restart in 1..=5 {
            let mut q = JobQueue::load(dir.path());
            q.reclaim_abandoned();
            let again = q.claim_next().expect("the interrupted cook was lost");
            assert!(
                !again.force,
                "restart {restart}: the replay still wipes the finished documents"
            );
        }
    }

    /// A PERMANENT failure settles. This models what actually happened: a step that always fails
    /// on this recording (a bug, a model that chokes on it). It may be retried a few times over
    /// hours — transient failures deserve that — but the budget is finite, and after it the job
    /// stays Failed instead of eating the machine forever.
    #[test]
    fn a_permanently_failing_job_settles_after_its_revival_budget() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", true, true, true);

        let mut rounds = 0;
        // Simulate: the job is handed out, it fails, it is revived, it fails again…
        loop {
            let Some(j) = q.claim_next() else { break };
            q.mark_failed(j.id, "the LLM chokes on this recording", 0); // always straight to Failed
            assert_eq!(q.jobs()[0].state, JobState::Failed);
            // Zero backoff — as if all the waiting had already passed.
            if q.revive_failed(std::time::Duration::ZERO) == 0 {
                break; // the budget is spent: it settled
            }
            rounds += 1;
            assert!(rounds <= MAX_REVIVALS + 1, "the queue is a perpetual motion machine");
        }

        assert_eq!(rounds, MAX_REVIVALS, "the revival budget is not what it claims");
        assert_eq!(q.jobs()[0].state, JobState::Failed);
        assert_eq!(q.jobs()[0].revivals, MAX_REVIVALS);
        // From here on, nothing revives it — not a reload, not another hour, not ever.
        for _ in 0..5 {
            assert_eq!(q.revive_failed(std::time::Duration::ZERO), 0);
        }
        assert_eq!(JobQueue::load(dir.path()).jobs()[0].state, JobState::Failed);
    }

    /// The backoff doubles: 1 h, 2 h, 4 h. Hammering a broken thing every hour teaches nobody
    /// anything, and the owner sees the churn.
    #[test]
    fn the_revival_backoff_doubles_every_round() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", false, false, false);
        let j = q.claim_next().unwrap();
        q.mark_failed(j.id, "down", 0);

        // 1st revival: needs 1 h. Half an hour is not enough.
        assert_eq!(q.revive_failed(std::time::Duration::from_secs(7200)), 0);
        assert_eq!(q.revive_failed(std::time::Duration::ZERO), 1);
        assert_eq!(q.jobs()[0].revivals, 1);

        // 2nd revival now needs TWICE the base — with a base of 1 s, "1 s old" is not enough.
        let j = q.claim_next().unwrap();
        q.mark_failed(j.id, "down", 0);
        assert_eq!(
            q.revive_failed(std::time::Duration::from_secs(1)),
            0,
            "the second revival must wait twice as long"
        );
    }

    /// The human's word outweighs the budget. He pressed «переварить заново» — the job tries
    /// again, however many times it had failed before. Otherwise a burned-out job would swallow
    /// the button silently.
    #[test]
    fn a_human_recook_buys_a_fresh_revival_budget() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", true, true, true);
        // Burn the whole budget.
        for _ in 0..=MAX_REVIVALS {
            if let Some(j) = q.claim_next() {
                q.mark_failed(j.id, "boom", 0);
            }
            q.revive_failed(std::time::Duration::ZERO);
        }
        assert_eq!(q.jobs()[0].state, JobState::Failed);
        assert_eq!(q.revive_failed(std::time::Duration::ZERO), 0, "budget must be spent");

        assert!(q.enqueue_recook("s1", true, true, true), "the human's re-cook was refused");
        assert_eq!(q.jobs()[0].state, JobState::Pending);
        assert_eq!(q.jobs()[0].revivals, 0, "the button did not buy a fresh budget");
        assert!(q.claim_next().is_some(), "the re-cook was not handed out");
    }

    #[test]
    fn discovery_skips_empty_active_and_cooked() {
        let dir = tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        // an empty session (no audio)
        fs::create_dir_all(sessions.join("empty/audio")).unwrap();
        // a session with audio, quiet → a candidate
        let quiet = sessions.join("quiet/audio");
        fs::create_dir_all(&quiet).unwrap();
        fs::write(quiet.join("src0_chunk0001.wav"), b"RIFFxxxx").unwrap();

        let need = sessions_needing_cook(dir.path(), 0);
        assert!(need.contains(&"quiet".to_string()));
        assert!(!need.contains(&"empty".to_string()));
    }

    /// THIS TEST USED TO DEMAND THE CHURN, and that is why it lived so long: it asserted that a
    /// session cooked by an OLD recipe must be cooked again, so that improvements reach the
    /// archive by themselves.
    ///
    /// The intent was good and the cost was ruinous. That recipe is recomputed on every cycle out
    /// of things that drift on their own — whether the diarization model is found (it is looked up
    /// RELATIVE TO THE WORKING DIRECTORY, so autostart alone flips `-spk`), the language, the ASR
    /// defaults. Every flip declared the whole archive stale, and the owner watched two-day-old
    /// sessions re-cook themselves on every launch, for days.
    ///
    /// The rule is inverted now, and it is the owner's: COOKED IS COOKED. Only a session with NO
    /// transcript is work. A better cook reaches the old archive when a human presses «переварить
    /// заново» — never behind his back.
    #[test]
    fn a_cooked_session_is_never_recooked_by_itself_whatever_the_recipe() {
        use crate::versions::{cook_recipe, default_cook_recipe, VersionEntry, VersionStore};

        let dir = tempdir().unwrap();
        let mk = |name: &str| -> std::path::PathBuf {
            let s = dir.path().join("sessions").join(name);
            fs::create_dir_all(s.join("audio")).unwrap();
            fs::write(s.join("audio/src0_chunk0001.wav"), b"RIFFxxxx").unwrap();
            s
        };
        let cook = |session: &std::path::Path, recipe: String| {
            let store = VersionStore::open(session).unwrap();
            let (id, path) = store.next_version("gigaam-int8").unwrap();
            fs::write(&path, b"{}\n").unwrap();
            store
                .commit(VersionEntry {
                    id,
                    label: "gigaam-int8".into(),
                    file: path.file_name().unwrap().to_string_lossy().into(),
                    model: "gigaam".into(),
                    params: serde_json::json!({ "recipe": recipe }),
                    created_at: "t".into(),
                    parents: vec![],
                })
                .unwrap();
        };

        // Cooked by an ancient recipe (180/150 s windows, no diarization).
        cook(&mk("stale"), cook_recipe("gigaam-int8", 180.0, 150.0, 500, false));
        // Cooked by today's recipe.
        cook(&mk("fresh"), default_cook_recipe());
        // Cooked before recipes existed at all — no recipe field.
        let old = mk("no-recipe");
        let store = VersionStore::open(&old).unwrap();
        let (id, path) = store.next_version("gigaam-int8").unwrap();
        fs::write(&path, b"{}\n").unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "gigaam-int8".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "gigaam".into(),
                params: serde_json::json!({ "max_window_sec": 180.0 }),
                created_at: "t".into(),
                parents: vec![],
            })
            .unwrap();
        // And one that was never cooked: THAT is the only work here.
        mk("raw");

        let need = sessions_needing_cook(dir.path(), 0);
        assert_eq!(
            need,
            vec!["raw".to_string()],
            "only a session with no transcript is work; everything else is done: {need:?}"
        );
    }

    /// Kept from the old contract: a session that has never been cooked IS work, and a re-cook by
    /// hand still forces. The rule above must not turn the archive into a museum.
    #[test]
    fn an_uncooked_session_is_still_work_and_a_human_recook_still_forces() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        assert!(q.enqueue_cook("s1", true, true, true), "new work was not queued");
        let j = q.claim_next().unwrap();
        q.mark_done(j.id);
        // Automatic discovery never touches it again…
        assert!(!q.enqueue_cook("s1", true, true, true), "a done session was queued again");
        // …but the human's button does, and it forces.
        assert!(q.enqueue_recook("s1", true, true, true));
        assert_eq!(q.jobs()[0].state, JobState::Pending);
        assert!(q.jobs()[0].force, "the re-cook must force past the «already cooked» skip");
    }


    /// «This is bad — re-cook it»: a human rejected the result. The job goes back into
    /// the queue with `force`, so that the cook does not skip the session by recipe.
    /// The flag is dropped after a success — otherwise the session would grind itself
    /// round and round.
    #[test]
    fn recook_forces_rebuild_and_force_is_cleared_after_success() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        assert!(q.enqueue_cook("s1", false, false, false));
        let id = q.pending_ids()[0];
        q.start(id);
        q.mark_done(id);

        assert!(q.enqueue_recook("s1", true, false, true), "Done → Pending");
        assert!(
            q.jobs()[0].force,
            "without force the cook will skip it by recipe"
        );
        assert!(matches!(q.jobs()[0].state, JobState::Pending));
        assert_eq!(q.jobs().len(), 1, "a duplicate job is not needed");

        let id = q.pending_ids()[0];
        q.start(id);
        assert!(q.start(id).is_none());
        q.mark_done(id);
        assert!(
            !q.jobs()[0].force,
            "force survived — the session would be cooked forever"
        );
    }

    /// A job failing in post-processing (the LLM is down) must not cook the session anew
    /// on EVERY attempt: the cook has already committed a version, the re-cook has done
    /// its work. Otherwise — an endless re-cook and a new version per attempt.
    #[test]
    fn force_is_cleared_once_the_cook_succeeded_even_if_post_processing_failed() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_recook("s1", true, false, true);
        let id = q.pending_ids()[0];
        q.start(id);
        assert!(q.jobs()[0].force);

        // exit 2: the cook went through, the LLM fell over
        q.clear_force(id);
        q.mark_failed(id, "post-processing (LLM)", 5);
        assert!(
            !q.jobs()[0].force,
            "force survived the failure — the session will be cooked round and round"
        );
    }

    /// The session is cooked (by hand, or back when the summary was switched off), but
    /// there is no summary. The auto-cook used to not see it at all — discovery asked
    /// only «does it need a COOK» — and the summary NEVER appeared.
    ///
    /// Loop safety now lives in the LOG, not in a heuristic: «there was nothing to do» is
    /// a record, not an empty space, and such a session does not show up in discovery.
    #[test]
    fn discovery_uses_the_processing_log_not_guesswork() {
        use crate::processing::{self, Outcome};
        use crate::versions::{VersionEntry, VersionStore};

        let dir = tempdir().unwrap();
        let session = dir.path().join("sessions/s1");
        fs::create_dir_all(session.join("audio")).unwrap();
        // a transcript with speech
        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("gigaam-int8").unwrap();
        fs::write(
            &path,
            "{\"source_id\":0,\"start_sec\":0.0,\"end_sec\":5.0,\"text\":\"привет мир\"}\n",
        )
        .unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "gigaam-int8".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "gigaam".into(),
                params: serde_json::json!({}),
                created_at: "t".into(),
                parents: vec![],
            })
            .unwrap();

        let want = Wanted::new(true, false, None, "qwen3.5:9b");
        assert_eq!(
            sessions_needing_artifacts(dir.path(), &want),
            vec!["s1".to_string()],
            "there is no summary — it has to be made"
        );

        // we made it and found out: there is nothing to do. That is FINISHED work.
        processing::record(
            &session,
            processing::SUMMARY,
            &processing::llm_recipe(processing::SUMMARY, "ru", "qwen3.5:9b"),
            Outcome::Nothing,
            None,
            None,
        );
        assert!(
            sessions_needing_artifacts(dir.path(), &want).is_empty(),
            "an endless loop: we demand a summary from a session there is nothing to make it from"
        );

        // AND CHANGING THE MODEL DOES NOT DRAG IT BACK. This used to assert the opposite — «the
        // model changed, the archive finishes itself off» — and that was one of the doors through
        // which the whole archive re-processed itself behind the owner's back. Swapping the model
        // in `.env`, a rebuild carrying a new prompt revision, a working directory that hides the
        // diarization model: any of them rewrote history for hundreds of sessions.
        //
        // The ledger says the summary is made. It is made. A new model reaches the old archive
        // when a HUMAN presses «переварить заново».
        let newer = Wanted::new(true, false, None, "granite4.1:8b");
        assert!(
            sessions_needing_artifacts(dir.path(), &newer).is_empty(),
            "changing the model re-processed a finished session behind the human's back"
        );
    }

    /// THE GUARANTEE THE OWNER ASKED FOR, in the only form worth anything — a test.
    ///
    /// «Сколько бы я ни открывал и закрывал софт, у обработанных записей статус не меняется.»
    /// Open and close the app fifty times: a session that is done stays done, and NOTHING is
    /// handed out for work. Nothing may re-derive «needs doing» from a guess, a timestamp, a
    /// file's presence or the phase of the moon — only from the ledger, which says it is done.
    ///
    /// This is the invariant the owner lived without for days: he watched the archive re-cook
    /// itself on every launch, sessions from two days ago included. Every fix for that churn was
    /// a fix for ONE cause; this test is the statement of the property itself, and it fails the
    /// moment any future cause appears.
    #[test]
    fn a_processed_session_stays_processed_across_restarts() {
        use crate::processing::{self, Outcome};
        use crate::versions::{VersionEntry, VersionStore};

        let dir = tempdir().unwrap();
        let session = dir.path().join("sessions/s1");
        fs::create_dir_all(session.join("audio")).unwrap();
        // Audio old enough to be a candidate: a live recording is skipped for other reasons, and
        // that would make this test pass for the wrong one.
        fs::write(session.join("audio/src0_chunk0001.wav"), b"pcm").unwrap();
        fs::write(
            session.join("meta.json"),
            r#"{"started_at":"t","sample_rate":16000,"chunks":[],"stopped_at":"t","stopped_reason":"вручную"}"#,
        )
        .unwrap();

        // A cooked transcript with speech, committed under THE CURRENT cook recipe — exactly what
        // the daemon will ask for.
        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("gigaam-int8").unwrap();
        fs::write(
            &path,
            "{\"source_id\":0,\"start_sec\":0.0,\"end_sec\":5.0,\"text\":\"привет мир\"}\n",
        )
        .unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "gigaam-int8".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "gigaam".into(),
                params: serde_json::json!({ "recipe": session_cook_recipe(&session) }),
                created_at: "t".into(),
                parents: vec![],
            })
            .unwrap();

        // PROVE THE TEST IS NOT VACUOUS. Before the ledger says the documents are made, discovery
        // MUST want this session — otherwise everything below would pass for the wrong reason
        // (a session nobody looks at is trivially never queued), and a green test would be worse
        // than none at all.
        let want = Wanted::new(true, true, None, "qwen3.5:9b");
        assert_eq!(
            sessions_needing_artifacts(dir.path(), &want),
            vec!["s1".to_string()],
            "discovery does not even see this session — the test would prove nothing"
        );

        // Both documents made, by the current recipes. One of them carries a doubt mark — an
        // `Unverified` artifact is FINISHED work too, and it must not drag the session back into
        // the queue for ever.
        processing::record(
            &session,
            processing::SUMMARY,
            &processing::llm_recipe(processing::SUMMARY, "ru", "qwen3.5:9b"),
            Outcome::Unverified,
            Some("имена/названия: сергей".into()),
            None,
        );
        processing::record(
            &session,
            processing::PROCESSED,
            &processing::llm_recipe(processing::PROCESSED, "ru", "qwen3.5:9b"),
            Outcome::Ok,
            None,
            None,
        );

        // Fifty launches. Nothing is cooked, nothing is queued, nothing is handed out.
        for run in 1..=50 {
            assert!(
                sessions_needing_cook(dir.path(), 0).is_empty(),
                "launch #{run}: a cooked session was sent for cooking again"
            );
            assert!(
                sessions_needing_artifacts(dir.path(), &want).is_empty(),
                "launch #{run}: a finished session was sent for its documents again"
            );
            let mut q = JobQueue::load(dir.path());
            assert!(
                q.claim_next().is_none(),
                "launch #{run}: the queue handed out work for a session that is done"
            );
        }
    }

    #[test]
    fn a_session_cooked_in_its_own_language_is_not_cooked_forever() {
        // A hole that is easy to miss: discovery used to ask «was it cooked by the
        // RUSSIAN recipe». An English session has a different recipe — the answer is
        // always «no», and it would be queued forever. The recipe MUST be the recipe of
        // the SESSION.
        use crate::versions::{VersionEntry, VersionStore};

        let dir = tempdir().unwrap();
        let session = dir.path().join("s1");
        fs::create_dir_all(session.join("audio")).unwrap();
        fs::write(
            session.join("meta.json"),
            r#"{"started_at":"t","sample_rate":16000,"chunks":[],"lang":"en"}"#,
        )
        .unwrap();

        let recipe = session_cook_recipe(&session);
        assert!(recipe.starts_with("asr-en/"), "{recipe}");

        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("asr-en").unwrap();
        fs::write(&path, "").unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "asr-en".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "vosk-en".into(),
                params: serde_json::json!({ "recipe": recipe }),
                created_at: "t".into(),
                parents: vec![],
            })
            .unwrap();

        assert!(
            !needs_cook(&session, 0),
            "the session is cooked by ITS OWN recipe — there is no point cooking it again"
        );
    }

    /// Changing the language DOES redo the documents — but by an explicit act, not by drift.
    ///
    /// This used to lean on the recipe: the language sat inside it, so a change made every
    /// document «made the wrong way» and discovery re-did them by itself. That implicit path is
    /// gone — it is the same path through which a rebuild, a model swap or a working directory
    /// re-processed the whole archive behind the owner's back.
    ///
    /// Nothing is lost. `Archive::set_lang` calls `recook()` ITSELF (archive.rs:943) — the human
    /// said «this recording is not in that language», which is a command, and the command is
    /// carried out openly. That is the rule: a human's word redoes work; a drifting constant does
    /// not.
    #[test]
    fn changing_the_language_alone_does_not_silently_redo_anything() {
        use crate::processing::{self, Outcome};
        use crate::versions::{VersionEntry, VersionStore};

        let dir = tempdir().unwrap();
        let session = dir.path().join("sessions").join("s1");
        fs::create_dir_all(&session).unwrap();
        fs::write(
            session.join("meta.json"),
            r#"{"started_at":"t","sample_rate":16000,"chunks":[]}"#,
        )
        .unwrap();

        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("gigaam-int8").unwrap();
        fs::write(
            &path,
            "{\"source_id\":0,\"start_sec\":0.0,\"end_sec\":2.0,\"text\":\"привет\"}\n",
        )
        .unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "gigaam-int8".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "gigaam".into(),
                params: serde_json::json!({}),
                created_at: "t".into(),
                parents: vec![],
            })
            .unwrap();

        let want = Wanted::new(true, false, None, "qwen3.5:9b");
        // we made a summary in the default language
        processing::record(
            &session,
            processing::SUMMARY,
            &processing::llm_recipe(processing::SUMMARY, crate::lang::DEFAULT, "qwen3.5:9b"),
            Outcome::Ok,
            None,
            None,
        );
        assert!(sessions_needing_artifacts(dir.path(), &want).is_empty());

        // The human says: the recording is not in that language. The LEDGER still says the summary
        // is made, so discovery keeps its hands off — the redo is `set_lang`'s own explicit
        // `recook()`, not a side effect of a string no longer matching.
        crate::lang::set(&session, Some("en")).unwrap();
        assert!(
            sessions_needing_artifacts(dir.path(), &want).is_empty(),
            "discovery redid the work by itself — the only door left must be the human's"
        );
    }

    #[test]
    fn recook_does_not_interrupt_a_running_job() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        q.enqueue_cook("s1", false, false, false);
        let id = q.pending_ids()[0];
        q.start(id);
        assert!(!q.enqueue_recook("s1", false, false, false));
        assert!(matches!(q.jobs()[0].state, JobState::Running));
    }

    /// Dedup by session name must not lock out a re-cook: a `Done` job of a stale session
    /// goes back to Pending, while a running one is left alone.
    #[test]
    fn requeue_stale_revives_done_job_but_not_running_one() {
        let dir = tempdir().unwrap();
        let mut q = JobQueue::load(dir.path());
        assert!(q.enqueue_cook("s1", false, false, false));
        let id = q.pending_ids()[0];
        q.start(id);
        q.mark_done(id);
        assert!(matches!(q.jobs()[0].state, JobState::Done));

        assert!(q.requeue_stale("s1", true, false, true), "Done → Pending");
        assert!(matches!(q.jobs()[0].state, JobState::Pending));
        assert!(q.jobs()[0].summary, "the job flags got updated");
        assert!(q.jobs()[0].refine);
        assert_eq!(q.jobs()[0].attempts, 0);

        // already in the queue — we do not duplicate it a second time and do not knock
        // its state off
        assert!(!q.requeue_stale("s1", false, false, false));
        assert_eq!(q.jobs().len(), 1);
    }

    #[test]
    fn discovery_skips_active_recording_session() {
        let dir = tempdir().unwrap();
        let quiet = dir.path().join("sessions/live/audio");
        fs::create_dir_all(&quiet).unwrap();
        fs::write(quiet.join("src0_chunk0001.wav"), b"RIFFxxxx").unwrap();
        // the marker: the engine is writing into «live» right now → we do not cook it
        fs::write(dir.path().join(RECORDING_MARKER), "live").unwrap();
        assert!(!sessions_needing_cook(dir.path(), 0).contains(&"live".to_string()));
        // the marker is gone → the session becomes a candidate
        fs::remove_file(dir.path().join(RECORDING_MARKER)).unwrap();
        assert!(sessions_needing_cook(dir.path(), 0).contains(&"live".to_string()));
    }

    #[test]
    fn discovery_skips_session_with_live_part_chunk() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("sessions/rec/audio");
        fs::create_dir_all(&audio).unwrap();
        // the previous chunk is closed (old), the current one is being written (.part is
        // fresh)
        fs::write(audio.join("src0_chunk0001.wav"), b"RIFFxxxx").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(1200));
        fs::write(audio.join("src0_chunk0002.part"), b"RIFFxxxx").unwrap();
        // the .wav is older than 1 s (by it the session is «quiet»), but the live .part is
        // fresh → the detector must see the .part and NOT consider the session closed
        assert!(
            !sessions_needing_cook(dir.path(), 1).contains(&"rec".to_string()),
            "a session with an active .part must not be cooked"
        );
    }

    /// THIS TEST USED TO CODIFY THE BUG, and that is why the bug lived for so long: it asserted
    /// that a reload resurrects a `Failed` job ("a restart = an explicit retry"). The premise was
    /// false — `load()` is not called on restart, it is called on EVERY autocook cycle, every few
    /// seconds. So the rule it protected meant "retry a broken job forever, a dozen times a
    /// minute", and the archive churned for days.
    ///
    /// The rule is inverted now: a reload preserves a failure. Coming back is the job of
    /// `revive_failed` (bounded, with backoff) or of a human.
    #[test]
    fn a_reload_preserves_a_failure_instead_of_retrying_it_forever() {
        let dir = tempdir().unwrap();
        {
            let mut q = JobQueue::load(dir.path());
            q.enqueue_cook("s1", false, false, false);
            let j = q.claim_next().unwrap();
            q.mark_failed(j.id, "boom", 1); // attempts=1>=cap → Failed
            assert_eq!(q.jobs()[0].state, JobState::Failed);
        }
        let mut q = JobQueue::load(dir.path());
        assert_eq!(
            q.jobs()[0].state,
            JobState::Failed,
            "a reload resurrected the failure — this is the churn the owner lived with"
        );
        assert!(q.claim_next().is_none(), "a failed job was handed out by a mere reload");
    }
}
