//! What is happening with a session right now — stage by stage.
//!
//! The queue can already answer "is anything being worked on at all" (`pending` / `running`),
//! and for a recording that was enough: cooking is one step, either it is going or it is not.
//! Ingestion by URL is not one step — download, track extraction, transcription, cleanup,
//! summary — and "⚙ cooking" for ten minutes tells a person nothing about WHERE it is now and
//! WHAT exactly broke if it broke.
//!
//! The log is APPEND-ONLY: `<session>/progress.jsonl`, one fact per line. Nothing is ever
//! overwritten — a re-cook simply appends new facts on top of the old ones, and the history of
//! what happened to the session stays readable by a human with `type progress.jsonl`. The view
//! shown in the app is DERIVED (see [`fold`]) and therefore disposable: it is recomputed from
//! the facts, never stored.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use serde::{Deserialize, Serialize};

/// The chain a session goes through. The order here IS the order shown to a person — this is
/// the one place that decides what comes after what.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Stage {
    /// yt-dlp fetched the source (URL ingestion only).
    Download,
    /// ffmpeg extracted the audio track and it became session chunks (URL ingestion only).
    Extract,
    /// ASR: audio → transcript.
    Transcribe,
    /// LLM cleaned the transcript up into a new best version.
    Refine,
    /// LLM wrote the summary.
    Summary,
}

impl Stage {
    /// The canonical order of the chain.
    pub const CHAIN: [Stage; 5] = [
        Stage::Download,
        Stage::Extract,
        Stage::Transcribe,
        Stage::Refine,
        Stage::Summary,
    ];
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum StageState {
    Running,
    Done,
    Failed,
    /// Deliberately not done (the summary is off, there is no LLM, the recording is silent).
    /// This is an ANSWER, not a gap: a stage that is simply missing from the chain reads as
    /// "it is still coming", and a person waits for something that will never happen.
    Skipped,
}

/// One fact: at this moment, this stage was in this state.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct StageEvent {
    pub stage: Stage,
    pub state: StageState,
    pub at: String,
    /// What a human needs in order to understand — the size of the download, the number of
    /// lines, the reason for the failure. The reason for a failure is the whole point of the
    /// log: "failed" without it sends one digging through the daemon's log.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// How far into the stage we are — units done out of the total (chunks transcribed, batches
    /// cleaned). The percentage a person watches, AND the heartbeat a stall is read from: a long
    /// stage used to run in total silence, so 3 quiet minutes looked identical to a dead process.
    /// Optional — a stage that has no natural unit (download) simply never sets it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub done: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
}

const FILE: &str = "progress.jsonl";

/// The boundary between runs.
///
/// The log is append-only and keeps everything, but the VIEW must show one run — the one that is
/// happening. Without this line a re-cook showed the previous run's chain: all green, all done,
/// while the work had not even started. A person pressed "re-cook", saw "finished", and waited for
/// movement that could not come.
#[derive(Serialize, Deserialize)]
struct RunMark {
    run: String,
}

/// A new run of the work over this session starts here. Everything logged after this line belongs
/// to it; everything before is history and stays in the file.
pub fn new_run(session_dir: &Path) {
    let mark = RunMark {
        run: crate::versions::now_rfc3339(),
    };
    let Ok(mut line) = serde_json::to_string(&mark) else {
        return;
    };
    line.push('\n');
    let _ = OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_dir.join(FILE))
        .and_then(|mut f| f.write_all(line.as_bytes()));
}

/// Append a fact. Failing to write progress must NEVER break the work itself: the log is a
/// story about the work, not the work.
pub fn mark(session_dir: &Path, stage: Stage, state: StageState, note: Option<&str>) {
    append(
        session_dir,
        StageEvent {
            stage,
            state,
            at: crate::versions::now_rfc3339(),
            note: note.map(str::to_string),
            done: None,
            total: None,
        },
    );
}

/// A heartbeat inside a running stage: `done` of `total` units are finished. Emitted often (per
/// chunk transcribed, per batch cleaned) so a person sees a percentage AND a stall is detectable —
/// the freshness of the last such event is what tells a watcher the process is still alive.
pub fn progress(session_dir: &Path, stage: Stage, done: u32, total: u32) {
    append(
        session_dir,
        StageEvent {
            stage,
            state: StageState::Running,
            at: crate::versions::now_rfc3339(),
            note: None,
            done: Some(done),
            total: Some(total),
        },
    );
}

fn append(session_dir: &Path, event: StageEvent) {
    let Ok(mut line) = serde_json::to_string(&event) else {
        return;
    };
    line.push('\n');
    let written = OpenOptions::new()
        .create(true)
        .append(true)
        .open(session_dir.join(FILE))
        .and_then(|mut f| f.write_all(line.as_bytes()));
    if let Err(e) = written {
        tracing::debug!("progress {}: {e}", session_dir.display());
    }
}

/// Convenience for the shape that repeats everywhere: run the step, mark done or failed.
pub fn step<T, E: std::fmt::Display>(
    session_dir: &Path,
    stage: Stage,
    work: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    mark(session_dir, stage, StageState::Running, None);
    match work() {
        Ok(v) => {
            mark(session_dir, stage, StageState::Done, None);
            Ok(v)
        }
        Err(e) => {
            mark(session_dir, stage, StageState::Failed, Some(&e.to_string()));
            Err(e)
        }
    }
}

/// The facts of the CURRENT run, in the order they happened.
///
/// Everything before the last run boundary stays in the file — the history of what happened to
/// this session is readable with `type progress.jsonl` — but it is not shown: a re-cook that
/// displays the previous run's finished chain is a lie about the work being done.
///
/// A broken line is skipped rather than fatal: a half-written one (the machine lost power
/// mid-append) must not hide the rest of the story.
pub fn read(session_dir: &Path) -> Vec<StageEvent> {
    let Ok(text) = std::fs::read_to_string(session_dir.join(FILE)) else {
        return Vec::new();
    };
    let mut current: Vec<StageEvent> = Vec::new();
    for line in text.lines() {
        if serde_json::from_str::<RunMark>(line).is_ok() {
            current.clear(); // a new run: everything above is history
            continue;
        }
        if let Ok(event) = serde_json::from_str::<StageEvent>(line) {
            current.push(event);
        }
    }
    current
}

/// The stage a run was interrupted in — the last event of the current run is still `Running`, so
/// nothing terminal ever followed. `None` when the run ended cleanly (or never started). Used at
/// daemon startup to reset an abandoned cook's status before replaying it.
pub fn interrupted_stage(session_dir: &Path) -> Option<Stage> {
    let events = read(session_dir);
    match events.last() {
        Some(e) if e.state == StageState::Running => Some(e.stage),
        _ => None,
    }
}

/// The state of one stage, as shown to a person.
#[derive(Serialize, Clone, Debug)]
pub struct StageStatus {
    pub stage: Stage,
    pub state: StageState,
    /// When the stage last started — so "running for 4 minutes" is answerable.
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    /// The timestamp of the MOST RECENT event for this stage — a heartbeat or the terminal mark.
    /// A running stage whose `updated_at` is old is stalled: the freshness answers "is it alive".
    pub updated_at: Option<String>,
    pub note: Option<String>,
    /// Progress within the stage, when it reports it (chunks/batches). `done` of `total`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub done: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u32>,
}

/// The derived view: the LATEST state of every stage that has been heard from, in the order of
/// the chain.
///
/// Stages nobody has said anything about are absent — and that is deliberate. A recording from
/// the microphone has no `download`, and drawing it as "pending" would promise a step that will
/// never come.
pub fn fold(events: &[StageEvent]) -> Vec<StageStatus> {
    let mut out: Vec<StageStatus> = Vec::new();
    for stage in Stage::CHAIN {
        let mine: Vec<&StageEvent> = events.iter().filter(|e| e.stage == stage).collect();
        let Some(last) = mine.last() else { continue };
        // The start of the LAST run, not of the first: a re-cook is a new attempt, and it is the
        // one a person is watching.
        let started_at = mine
            .iter()
            .rev()
            .find(|e| e.state == StageState::Running)
            .map(|e| e.at.clone());
        // The freshest done/total for this stage, even if the very last event was a bare mark
        // without them — a heartbeat carries the count, the terminal Done usually does not.
        let progress = mine.iter().rev().find_map(|e| e.done.zip(e.total));
        // The freshest human note, likewise: heartbeats have none, so a stage that finished with
        // «421 строк» keeps it rather than blanking on a trailing progress event.
        let note = mine.iter().rev().find_map(|e| e.note.clone());
        out.push(StageStatus {
            stage,
            state: last.state,
            started_at,
            ended_at: (last.state != StageState::Running).then(|| last.at.clone()),
            updated_at: Some(last.at.clone()),
            note,
            done: progress.map(|(d, _)| d),
            total: progress.map(|(_, t)| t),
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_last_word_about_a_stage_wins() {
        let d = tempfile::tempdir().unwrap();
        mark(d.path(), Stage::Transcribe, StageState::Running, None);
        mark(d.path(), Stage::Transcribe, StageState::Done, Some("412 строк"));

        let view = fold(&read(d.path()));
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].state, StageState::Done);
        assert_eq!(view[0].note.as_deref(), Some("412 строк"));
        assert!(view[0].started_at.is_some(), "the start of the stage is lost");
        assert!(view[0].ended_at.is_some());
    }

    /// A re-cook does not erase history — it appends to it. But the person is shown the LAST
    /// attempt: they are watching the one that is running now.
    #[test]
    fn a_second_attempt_at_a_stage_appends_and_the_view_shows_it() {
        let d = tempfile::tempdir().unwrap();
        mark(d.path(), Stage::Summary, StageState::Failed, Some("ollama недоступна"));
        mark(d.path(), Stage::Summary, StageState::Running, None);

        assert_eq!(read(d.path()).len(), 2, "the facts must not be overwritten");
        let view = fold(&read(d.path()));
        assert_eq!(view[0].state, StageState::Running);
        assert_eq!(view[0].ended_at, None, "a running stage has not ended");
    }

    /// THE RE-COOK BUG. A person pressed «переварить», the queue took the job — and the chain
    /// still showed the PREVIOUS run: transcribed, cleaned up, summarized, all green, all done.
    /// They waited for movement in a picture that had already finished.
    ///
    /// A new run wipes the VIEW, not the file: the history stays and is readable, but the chain
    /// shows the work that is happening now — and at the start of a run that is honestly nothing.
    #[test]
    fn a_new_run_shows_the_work_that_is_happening_not_the_one_that_ended() {
        let d = tempfile::tempdir().unwrap();
        mark(d.path(), Stage::Transcribe, StageState::Done, Some("217 строк"));
        mark(d.path(), Stage::Summary, StageState::Done, None);
        assert_eq!(fold(&read(d.path())).len(), 2);

        new_run(d.path());
        assert!(
            fold(&read(d.path())).is_empty(),
            "the re-cook still shows the finished chain of the previous run"
        );

        mark(d.path(), Stage::Transcribe, StageState::Running, None);
        let view = fold(&read(d.path()));
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].state, StageState::Running);

        // The history is not lost — it is simply not the current run.
        let text = std::fs::read_to_string(d.path().join(FILE)).unwrap();
        assert!(text.contains("217 строк"), "history was destroyed: {text}");
    }

    /// The chain shows only the stages that actually apply: a microphone recording has no
    /// download, and drawing it as "pending" would promise a step that will never come.
    #[test]
    fn stages_nobody_mentioned_are_not_in_the_chain() {
        let d = tempfile::tempdir().unwrap();
        mark(d.path(), Stage::Transcribe, StageState::Done, None);
        mark(d.path(), Stage::Summary, StageState::Done, None);

        let view = fold(&read(d.path()));
        let stages: Vec<Stage> = view.iter().map(|s| s.stage).collect();
        assert_eq!(stages, vec![Stage::Transcribe, Stage::Summary]);
    }

    /// The order is the chain's, not the order the facts arrived in.
    #[test]
    fn the_chain_keeps_its_order() {
        let d = tempfile::tempdir().unwrap();
        mark(d.path(), Stage::Summary, StageState::Done, None);
        mark(d.path(), Stage::Download, StageState::Done, None);
        mark(d.path(), Stage::Transcribe, StageState::Done, None);

        let stages: Vec<Stage> = fold(&read(d.path())).iter().map(|s| s.stage).collect();
        assert_eq!(stages, vec![Stage::Download, Stage::Transcribe, Stage::Summary]);
    }

    /// A failure without a reason sends a person digging through the daemon's log. The reason is
    /// the entire point of this log.
    #[test]
    fn a_failure_carries_its_reason() {
        let d = tempfile::tempdir().unwrap();
        let r: Result<(), String> = step(d.path(), Stage::Download, || {
            Err("yt-dlp: video unavailable".into())
        });
        assert!(r.is_err());
        let view = fold(&read(d.path()));
        assert_eq!(view[0].state, StageState::Failed);
        assert_eq!(view[0].note.as_deref(), Some("yt-dlp: video unavailable"));
    }

    /// A broken tail (the machine lost power mid-append) must not hide the rest of the story.
    #[test]
    fn a_half_written_line_does_not_hide_the_others() {
        let d = tempfile::tempdir().unwrap();
        mark(d.path(), Stage::Download, StageState::Done, None);
        std::fs::OpenOptions::new()
            .append(true)
            .open(d.path().join(FILE))
            .unwrap()
            .write_all(b"{\"stage\":\"extr")
            .unwrap();

        assert_eq!(read(d.path()).len(), 1);
    }
}
