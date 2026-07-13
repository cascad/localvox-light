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

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Job {
    pub id: u64,
    /// The session directory name (relative to `<work_dir>/sessions`) — portable.
    pub session: String,
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
    /// Load the queue from `<work_dir>/jobs.json` (or an empty one). Jobs stuck in
    /// `Running` after a process crash are returned to `Pending`.
    pub fn load(work_dir: &Path) -> Self {
        let path = work_dir.join("jobs.json");
        let mut file: QueueFile = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        for j in &mut file.jobs {
            match j.state {
                // the process crashed during the cook — replay it
                JobState::Running => j.state = JobState::Pending,
                // a daemon restart = an explicit «try again» signal (often after fixing
                // the config, e.g. the path to the model): we reset the attempts too.
                JobState::Failed => {
                    j.state = JobState::Pending;
                    j.attempts = 0;
                }
                _ => {}
            }
        }
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
            summary,
            cleanup,
            refine,
            state: JobState::Pending,
            attempts: 0,
            created_at: now_rfc3339(),
            last_error: None,
            failed_at: None,
            force: false,
        });
        self.save();
        true
    }

    /// Ids of all pending jobs (a snapshot taken at the start of the scheduler cycle):
    /// we run each of them exactly once per cycle, so that a failing job does not burn
    /// through its attempts back-to-back in a single pass — a transient failure is
    /// retried on the next cycle.
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
            if j.state == JobState::Failed
                && j.failed_at
                    .map(|t| now.saturating_sub(t) >= min_age.as_secs())
                    .unwrap_or(true)
            {
                j.state = JobState::Pending;
                j.attempts = 0;
                j.failed_at = None;
                revived += 1;
            }
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

/// The request «close the current session and start a new one».
///
/// A file, not a channel: the request comes from the HTTP thread (the button in the web
/// UI) and from the tray, while the session is owned by the recording pipeline. A file
/// is the cheapest way to shout across any boundary, and it survives everything short of
/// the disk being deleted.
///
/// Why this exists at all: while the session is open, the cook does not touch it — and
/// to get a summary one had to KILL THE DAEMON. Now there is a button.
pub const FINISH_MARKER: &str = ".finish_session";

/// Ask the engine to close the current session. `reason` goes into `meta.json`.
pub fn request_finish(work_dir: &Path, reason: &str) -> std::io::Result<()> {
    fs::write(work_dir.join(FINISH_MARKER), reason)
}

/// Take the request (if there is one) — exactly once: the marker is deleted immediately.
pub fn take_finish_request(work_dir: &Path) -> Option<String> {
    let path = work_dir.join(FINISH_MARKER);
    let reason = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    Some(reason.trim().to_string()).filter(|s| !s.is_empty())
}

/// The request «start a MEETING»: close the current session and open a new one — with a
/// title.
///
/// **Why this, if there is auto-detection.** Auto-detection (`detect.rs`) sees a call by
/// the fact that an application is holding the microphone — and that honestly works for
/// Zoom, Teams and the browser. But a face-to-face stand-up around a table it will NEVER
/// see: from the system's point of view that is the same background noise as the rest of
/// the day. There is no reliable automatic sign of a meeting in a room, and pretending
/// there is means lying. So a human must have a button: to say «this is a meeting» with
/// a single motion, before it or at its start.
///
/// The title goes INTO THE SESSION NAME (`20260713_181500_planerka`) — so that the
/// meeting is visible in the list, instead of being hunted for among the day's nameless
/// stretches.
pub const MEETING_MARKER: &str = ".start_meeting";

pub fn request_meeting(work_dir: &Path, title: &str) -> std::io::Result<()> {
    fs::write(work_dir.join(MEETING_MARKER), title.trim())
}

/// Take the request — exactly once. An empty file is a request too: a meeting without a
/// title is still a meeting.
pub fn take_meeting_request(work_dir: &Path) -> Option<String> {
    let path = work_dir.join(MEETING_MARKER);
    let title = fs::read_to_string(&path).ok()?;
    let _ = fs::remove_file(&path);
    Some(title.trim().to_string())
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
        if want
            .recipes(&dir)
            .iter()
            .any(|(art, recipe)| !crate::processing::is_current(&dir, art, recipe))
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
    // We skip only a session cooked by the CURRENT recipe. One cooked by an old one (for
    // example with 180/150 s windows — those very unreadable transcripts) goes for a
    // re-cook: otherwise an improvement of the cook never reaches the archive.
    //
    // The recipe of the SESSION, not the «default recipe»: a session has its own
    // language, and the version label depends on it. Asking an English session about the
    // Russian recipe means never finding it — and queueing that session forever.
    let cooked_now = VersionStore::open(session_dir)
        .map(|s| s.has_recipe(&session_cook_recipe(session_dir)))
        .unwrap_or(false);
    !cooked_now
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

    /// The «New meeting» button is the only RELIABLE way to know that a meeting happened
    /// at all: a face-to-face stand-up around a table auto-detection will never see.
    /// We take the request exactly once — otherwise the engine would cut the session on
    /// every frame for as long as the file lies there.
    #[test]
    fn a_meeting_request_is_taken_exactly_once() {
        let d = tempfile::tempdir().unwrap();
        assert!(
            take_meeting_request(d.path()).is_none(),
            "there was no request"
        );

        request_meeting(d.path(), "  Планёрка  ").unwrap();
        assert_eq!(take_meeting_request(d.path()).as_deref(), Some("Планёрка"));
        assert!(
            take_meeting_request(d.path()).is_none(),
            "the request fired twice"
        );
    }

    /// A meeting WITHOUT a title is still a meeting. An empty string means «mark it, but
    /// I cannot name it», not «cancel».
    #[test]
    fn a_meeting_without_a_name_is_still_a_meeting() {
        let d = tempfile::tempdir().unwrap();
        request_meeting(d.path(), "").unwrap();
        assert_eq!(take_meeting_request(d.path()).as_deref(), Some(""));
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
    fn running_becomes_pending_after_crash() {
        let dir = tempdir().unwrap();
        {
            let mut q = JobQueue::load(dir.path());
            q.enqueue_cook("s1", false, false, false);
            q.claim_next(); // Running, the process «crashes» without mark_done
        }
        let mut q = JobQueue::load(dir.path());
        assert_eq!(
            q.jobs()[0].state,
            JobState::Pending,
            "crashed Running → Pending"
        );
        // and it gets picked up again
        assert!(q.claim_next().is_some());
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

    /// A session cooked with the old windows (180/150 s) MUST go for a re-cook —
    /// otherwise a change of the defaults never reaches the archive, and it stays that
    /// very unreadable transcript forever. One cooked by the current recipe is left
    /// alone (otherwise the re-cook would loop).
    #[test]
    fn discovery_recooks_session_cooked_with_stale_recipe() {
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

        let stale = mk("stale");
        cook(&stale, cook_recipe("gigaam-int8", 180.0, 150.0, 500, false));
        let fresh = mk("fresh");
        cook(&fresh, default_cook_recipe());
        let old = mk("no-recipe"); // cooked before WP-C14: there is no recipe field at all
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

        let need = sessions_needing_cook(dir.path(), 0);
        assert!(
            need.contains(&"stale".to_string()),
            "an old recipe: {need:?}"
        );
        assert!(
            need.contains(&"no-recipe".to_string()),
            "no recipe at all: {need:?}"
        );
        assert!(
            !need.contains(&"fresh".to_string()),
            "the current recipe: {need:?}"
        );
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

        // the model changed — the recipe is different, the archive finishes itself off
        let newer = Wanted::new(true, false, None, "granite4.1:8b");
        assert_eq!(
            sessions_needing_artifacts(dir.path(), &newer),
            vec!["s1".to_string()],
            "the model changed — the previous result is out of date"
        );
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

    #[test]
    fn changing_the_language_makes_the_artifacts_stale() {
        // This is what the language lives in the recipe for: there is no «reset the
        // derived data» code at all, and yet a change of language forces everything to be
        // redone.
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

        // the human says: the recording is not in that language
        crate::lang::set(&session, Some("en")).unwrap();
        assert_eq!(
            sessions_needing_artifacts(dir.path(), &want),
            vec!["s1".to_string()],
            "the language changed — the previous summary was made the wrong way, it will be redone"
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

    #[test]
    fn failed_revives_to_pending_on_restart() {
        let dir = tempdir().unwrap();
        {
            let mut q = JobQueue::load(dir.path());
            q.enqueue_cook("s1", false, false, false);
            let j = q.claim_next().unwrap();
            q.mark_failed(j.id, "boom", 1); // attempts=1>=cap → Failed
            assert_eq!(q.jobs()[0].state, JobState::Failed);
        }
        // a restart = an explicit retry: Failed → Pending with the attempts reset
        let mut q = JobQueue::load(dir.path());
        assert_eq!(q.jobs()[0].state, JobState::Pending);
        assert_eq!(q.jobs()[0].attempts, 0);
        assert!(q.claim_next().is_some());
    }
}
