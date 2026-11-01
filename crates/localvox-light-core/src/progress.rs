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

use std::io::BufRead;
#[cfg(test)]
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
    /// Readable wording, after recognition corrections.
    Cleanup,
    /// LLM wrote the summary.
    Summary,
}

impl Stage {
    /// The canonical order of the chain.
    pub const CHAIN: [Stage; 6] = [
        Stage::Download,
        Stage::Extract,
        Stage::Transcribe,
        Stage::Refine,
        Stage::Cleanup,
        Stage::Summary,
    ];
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum StageState {
    Waiting,
    Queued,
    Blocked,
    Invalid,
    Running,
    Done,
    Failed,
    Cancelled,
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
#[serde(deny_unknown_fields)]
struct RunMark {
    run: String,
}

/// A new run of the work over this session starts here. Everything logged after this line belongs
/// to it; everything before is history and stays in the file.
pub fn new_run(session_dir: &Path) {
    let mark = RunMark {
        run: crate::versions::now_rfc3339(),
    };
    crate::journal::begin_run(session_dir, &mark.run);
    let Ok(mut line) = serde_json::to_string(&mark) else {
        return;
    };
    line.push('\n');
    let _ = crate::journal::append_line(session_dir, line.as_bytes());
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
    let written = crate::journal::append_line(session_dir, line.as_bytes());
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

/// Report a bounded operation while a blocking model/HTTP call is in progress.
/// The heartbeat means the worker is alive and waiting, not that the model made progress.
/// Joining before return ensures no late Running can overwrite the terminal event.
pub fn activity<T>(session: &Path, stage: Stage, note: &str, work: impl FnOnce() -> T) -> T {
    let _operation =
        crate::journal::Operation::start(session, &format!("{stage:?}").to_lowercase(), note, None);
    use std::sync::mpsc;
    use std::time::{Duration, Instant};
    struct Stop(mpsc::Sender<()>);
    impl Drop for Stop {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    let started = Instant::now();
    mark(session, stage, StageState::Running, Some(note));
    tracing::info!(?stage, operation = note, "processing operation started");
    std::thread::scope(|scope| {
        let (tx, rx) = mpsc::channel();
        scope.spawn(move || {
            while matches!(
                rx.recv_timeout(Duration::from_secs(5)),
                Err(mpsc::RecvTimeoutError::Timeout)
            ) {
                mark(
                    session,
                    stage,
                    StageState::Running,
                    Some(&format!(
                        "{note} · ожидание/работа {} с",
                        started.elapsed().as_secs()
                    )),
                );
            }
        });
        let _stop = Stop(tx);
        let result = work();
        tracing::info!(
            ?stage,
            operation = note,
            elapsed_sec = started.elapsed().as_secs_f64(),
            "processing operation finished"
        );
        result
    })
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
    read_run(session_dir).events
}

#[derive(Default)]
pub struct Run {
    pub started_at: Option<String>,
    pub events: Vec<StageEvent>,
}

/// The marker and events come from the same pass. Journal payloads are skipped
/// without allocating their text; only a standalone marker resets the view.
pub fn read_run(session_dir: &Path) -> Run {
    #[derive(Deserialize)]
    struct Kind {
        journal: Option<serde::de::IgnoredAny>,
    }
    let Ok(file) = std::fs::File::open(session_dir.join(FILE)) else {
        return Run::default();
    };
    let mut current = Run::default();
    let mut reader = std::io::BufReader::new(file);
    let mut line = Vec::new();
    loop {
        line.clear();
        match reader.read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {}
        }
        if serde_json::from_slice::<Kind>(&line).is_ok_and(|kind| kind.journal.is_some()) {
            continue;
        }
        if let Ok(mark) = serde_json::from_slice::<RunMark>(&line) {
            current.started_at = Some(mark.run);
            current.events.clear();
        } else if let Ok(event) = serde_json::from_slice::<StageEvent>(&line) {
            current.events.push(event);
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact: Option<crate::processing::ArtifactStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub files: Vec<String>,
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
        // A heartbeat is Running too. The attempt starts at the FIRST Running after the
        // preceding terminal event, never at the latest heartbeat.
        let mut start = 0;
        for i in 1..mine.len() {
            if mine[i].state == StageState::Running && mine[i - 1].state != StageState::Running {
                start = i;
            }
        }
        let attempt = &mine[start..];
        let started_at = attempt
            .iter()
            .find(|e| e.state == StageState::Running)
            .map(|e| e.at.clone());
        let progress = attempt.iter().rev().find_map(|e| e.done.zip(e.total));
        let note = attempt.iter().rev().find_map(|e| e.note.clone());
        out.push(StageStatus {
            stage,
            state: last.state,
            started_at,
            ended_at: (last.state != StageState::Running).then(|| last.at.clone()),
            updated_at: Some(last.at.clone()),
            note,
            done: progress.map(|(d, _)| d),
            total: progress.map(|(_, t)| t),
            artifact: None,
            files: vec![],
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_operations_do_not_erase_stage_events_or_hide_an_interruption() {
        let dir = tempfile::tempdir().unwrap();
        new_run(dir.path());
        mark(
            dir.path(),
            Stage::Transcribe,
            StageState::Running,
            Some("loading"),
        );
        let worker = crate::journal::Operation::start(dir.path(), "transcribe", "worker", None);
        worker.log("model loaded");
        assert_eq!(interrupted_stage(dir.path()), Some(Stage::Transcribe));
        progress(dir.path(), Stage::Transcribe, 3, 3);
        mark(dir.path(), Stage::Transcribe, StageState::Done, None);
        drop(worker);
        let rows = fold(&read(dir.path()));
        assert_eq!(
            rows.len(),
            1,
            "operation_end must not erase the completed stage"
        );
        assert_eq!(rows[0].state, StageState::Done);
        assert!(rows[0].started_at.is_some() && rows[0].ended_at.is_some());
        assert_eq!(rows[0].done, Some(3));
        assert_eq!(interrupted_stage(dir.path()), None);

        new_run(dir.path());
        let retry = crate::journal::Operation::start(dir.path(), "summary", "retry", None);
        assert!(
            read(dir.path()).is_empty(),
            "a genuine run mark still starts a new view"
        );
        mark(dir.path(), Stage::Summary, StageState::Running, None);
        retry.log("waiting for model");
        assert_eq!(interrupted_stage(dir.path()), Some(Stage::Summary));
    }

    #[test]
    fn run_marker_is_exact_and_broken_records_do_not_erase_valid_events() {
        let dir = tempfile::tempdir().unwrap();
        let bytes = b"{\"run\":\"first\"}\n{\"stage\":\"transcribe\",\"state\":\"running\",\"at\":\"start\"}\n{\"run\":\"not-a-marker\",\"extra\":true}\n{\"journal\":1,\"run\":\"not-a-marker\",\"state\":\"done\",\"stage\":\"summary\",\"at\":\"end\"}\n\xff\n{broken}\n{\"stage\":\"transcribe\",\"state\":\"done\",\"at\":\"end\"}\n{\"run\":\"partial";
        std::fs::write(dir.path().join(FILE), bytes).unwrap();
        let run = read_run(dir.path());
        assert_eq!(run.started_at.as_deref(), Some("first"));
        assert_eq!(run.events.len(), 2);
        let rows = fold(&run.events);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].started_at.as_deref(), Some("start"));
        assert_eq!(rows[0].ended_at.as_deref(), Some("end"));
        assert_eq!(rows[0].state, StageState::Done);
    }

    #[test]
    #[ignore = "synthetic history polling profile; no models, capture or user archive"]
    fn profile_mixed_history_polling() {
        use std::io::{BufWriter, Write};
        use std::time::Instant;
        let dir = tempfile::tempdir().unwrap();
        let transcript = crate::artifacts::tests::fixture(dir.path());
        let mut speech = BufWriter::new(std::fs::File::create(transcript).unwrap());
        for second in 0..7200 {
            serde_json::to_writer(&mut speech, &serde_json::json!({
                "source_id": 0, "start_sec": second, "end_sec": second + 1,
                "text": "Synthetic meeting words for an isolated polling measurement.", "speaker": "S1"
            })).unwrap();
            writeln!(speech).unwrap();
        }
        drop(speech);
        for artifact in [
            crate::processing::TRANSCRIPT,
            crate::processing::PROCESSED,
            crate::processing::SUMMARY,
        ] {
            crate::processing::record(
                dir.path(),
                artifact,
                "fixture",
                crate::processing::Outcome::Ok,
                None,
                Some(1),
            )
            .unwrap();
        }
        let measure = |f: &mut dyn FnMut()| {
            f();
            let mut samples = (0..5)
                .map(|_| {
                    let started = Instant::now();
                    f();
                    started.elapsed().as_secs_f64() * 1000.0
                })
                .collect::<Vec<_>>();
            samples.sort_by(f64::total_cmp);
            serde_json::json!({"median_ms": samples[2], "min_ms": samples[0], "max_ms": samples[4]})
        };
        let mut report = vec![];
        for runs in [1, 12] {
            let mut file = BufWriter::new(std::fs::File::create(dir.path().join(FILE)).unwrap());
            let verbose = "x".repeat(2048);
            for run in 0..runs {
                writeln!(file, "{{\"run\":\"2026-10-31T00:{run:02}:00Z\"}}").unwrap();
                writeln!(file, "{{\"stage\":\"transcribe\",\"state\":\"running\",\"at\":\"2026-10-31T00:00:01Z\"}}").unwrap();
                for _ in 0..400 {
                    serde_json::to_writer(&mut file, &serde_json::json!({"journal": 1, "run": format!("2026-10-31T00:{run:02}:00Z"), "stage": "transcribe", "kind": "log", "text": verbose})).unwrap();
                    writeln!(file).unwrap();
                }
                writeln!(file, "{{\"stage\":\"transcribe\",\"state\":\"done\",\"at\":\"2026-10-31T00:00:11Z\"}}").unwrap();
                writeln!(file, "{{\"journal\":1,\"run\":\"2026-10-31T00:{run:02}:00Z\",\"stage\":\"transcribe\",\"kind\":\"operation_end\"}}").unwrap();
            }
            drop(file);
            let read = measure(&mut || {
                std::hint::black_box(super::read(dir.path()));
            });
            let view = measure(&mut || {
                std::hint::black_box(crate::workflow::view(
                    dir.path(),
                    None,
                    false,
                    "2026-10-31T01:00:00Z",
                ));
            });
            report.push(serde_json::json!({"historical_runs": runs, "history_bytes": std::fs::metadata(dir.path().join(FILE)).unwrap().len(), "events_retained": super::read(dir.path()).len(), "read": read, "workflow": view}));
        }
        let report = serde_json::json!({"profile": if cfg!(debug_assertions) {"dev"} else {"release"}, "samples": 5, "synthetic_transcript_seconds": 7200, "measurements": report});
        if let Some(path) = std::env::var_os("LOCALVOX_PROFILE_OUTPUT") {
            std::fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
        }
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    }

    fn event(state: StageState, at: &str, done: Option<u32>) -> StageEvent {
        StageEvent {
            stage: Stage::Refine,
            state,
            at: at.into(),
            note: None,
            done,
            total: done.map(|_| 10),
        }
    }

    #[test]
    fn heartbeats_preserve_start_and_retry_drops_old_progress() {
        use StageState::*;
        let mut events = vec![
            event(Running, "00:00", Some(0)),
            event(Running, "07:59", Some(10)),
            event(Done, "08:00", None),
        ];
        let view = fold(&events);
        assert_eq!(view[0].started_at.as_deref(), Some("00:00"));
        assert_eq!(view[0].ended_at.as_deref(), Some("08:00"));
        assert_eq!(view[0].done, Some(10));
        events.push(event(Running, "09:00", None));
        events.push(event(Running, "09:05", None));
        let view = fold(&events);
        assert_eq!(view[0].started_at.as_deref(), Some("09:00"));
        assert_eq!(view[0].updated_at.as_deref(), Some("09:05"));
        assert_eq!(view[0].done, None);
        assert_eq!(view[0].ended_at, None);
    }

    #[test]
    fn the_last_word_about_a_stage_wins() {
        let d = tempfile::tempdir().unwrap();
        mark(d.path(), Stage::Transcribe, StageState::Running, None);
        mark(
            d.path(),
            Stage::Transcribe,
            StageState::Done,
            Some("412 строк"),
        );

        let view = fold(&read(d.path()));
        assert_eq!(view.len(), 1);
        assert_eq!(view[0].state, StageState::Done);
        assert_eq!(view[0].note.as_deref(), Some("412 строк"));
        assert!(
            view[0].started_at.is_some(),
            "the start of the stage is lost"
        );
        assert!(view[0].ended_at.is_some());
    }

    /// A re-cook does not erase history — it appends to it. But the person is shown the LAST
    /// attempt: they are watching the one that is running now.
    #[test]
    fn a_second_attempt_at_a_stage_appends_and_the_view_shows_it() {
        let d = tempfile::tempdir().unwrap();
        mark(
            d.path(),
            Stage::Summary,
            StageState::Failed,
            Some("ollama недоступна"),
        );
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
        mark(
            d.path(),
            Stage::Transcribe,
            StageState::Done,
            Some("217 строк"),
        );
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
        assert_eq!(
            stages,
            vec![Stage::Download, Stage::Transcribe, Stage::Summary]
        );
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
