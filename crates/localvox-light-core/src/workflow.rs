//! The user-facing workflow view joins execution events with verified artifacts.
//! Jobs schedule work, receipts prove results, progress events explain execution.

use crate::{
    jobs::{Job, JobPhase, JobState},
    processing::{self, ArtifactState},
    progress::{self, Stage, StageState, StageStatus},
};
use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Default, Debug)]
pub struct Timing {
    pub queued_at: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub queue_sec: Option<f64>,
    pub processing_sec: Option<f64>,
    pub total_sec: Option<f64>,
}

fn timestamp(s: &str) -> Option<f64> {
    chrono::DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|d| d.timestamp_millis() as f64 / 1000.0)
}

fn duration(from: Option<&str>, to: Option<&str>) -> Option<f64> {
    let a = timestamp(from?)?;
    let b = timestamp(to?)?;
    (b >= a).then_some(b - a)
}

/// Clock is supplied by the caller. Unknown start/end stays unknown, never becomes zero.
pub fn timing(
    queued: Option<String>,
    started: Option<String>,
    ended: Option<String>,
    live: bool,
    now: &str,
) -> Timing {
    let end = ended.as_deref().or(live.then_some(now));
    let queue_sec = duration(queued.as_deref(), started.as_deref().or(end));
    let processing_sec = duration(started.as_deref(), end);
    let total_sec = duration(queued.as_deref(), end);
    Timing {
        queued_at: queued,
        started_at: started,
        ended_at: ended,
        queue_sec,
        processing_sec,
        total_sec,
    }
}

#[derive(Serialize)]
pub struct View {
    pub stages: Vec<StageStatus>,
    pub running: bool,
    pub phase: Option<JobPhase>,
    pub timing: Timing,
}

fn stage_phase(stage: Stage) -> JobPhase {
    match stage {
        Stage::Download | Stage::Extract => JobPhase::Prepare,
        Stage::Transcribe => JobPhase::Transcribe,
        Stage::Refine | Stage::Cleanup => JobPhase::Text,
        Stage::Summary => JobPhase::Summary,
    }
}

fn requested(stage: Stage, job: &Job) -> bool {
    match stage {
        Stage::Refine => job.refine,
        Stage::Cleanup => job.cleanup,
        Stage::Summary => job.summary,
        _ => true,
    }
}

pub fn view(session: &Path, job: Option<&Job>, from_link: bool, now: &str) -> View {
    let events = progress::read(session);
    let folded = progress::fold(&events);
    let live = job.is_some_and(|j| matches!(j.state, JobState::Pending | JobState::Running));
    let queued = job.map(|j| j.enqueued_at.as_ref().unwrap_or(&j.created_at).clone());
    // Run marks predate model/title loading. Older queues have no started_at field.
    let run_start = std::fs::read_to_string(session.join("progress.jsonl"))
        .ok()
        .and_then(|s| {
            s.lines()
                .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
                .filter_map(|v| v["run"].as_str().map(str::to_owned))
                .last()
        });
    let has_started = job.is_none_or(|j| {
        j.started_at.is_some()
            || j.attempts > 0
            || j.phase != JobPhase::Prepare
            || matches!(j.state, JobState::Done | JobState::Failed)
    });
    let started = has_started
        .then(|| {
            job.and_then(|j| j.started_at.clone())
                .or(run_start)
                .or_else(|| {
                    events
                        .iter()
                        .find(|e| e.state == StageState::Running)
                        .map(|e| e.at.clone())
                })
        })
        .flatten();
    let ended = if live {
        None
    } else {
        job.and_then(|j| j.finished_at.clone()).or_else(|| {
            events
                .iter()
                .rev()
                .find(|e| e.state != StageState::Running)
                .map(|e| e.at.clone())
        })
    };
    let transcription = processing::inspect(session, processing::TRANSCRIPT);
    let audio = from_link.then(|| crate::artifacts::audio_overview(session));
    let mut stages = vec![];
    for stage in Stage::CHAIN {
        if !from_link && matches!(stage, Stage::Download | Stage::Extract) {
            continue;
        }
        let mut row = folded
            .iter()
            .find(|r| r.stage == stage)
            .cloned()
            .unwrap_or(StageStatus {
                stage,
                state: StageState::Waiting,
                started_at: None,
                ended_at: None,
                updated_at: None,
                note: None,
                done: None,
                total: None,
                artifact: None,
                files: vec![],
            });
        let executing = row.state == StageState::Running
            && job.is_none_or(|j| j.state == JobState::Running && j.phase == stage_phase(stage));
        let artifact = match stage {
            Stage::Transcribe => Some(processing::TRANSCRIPT),
            Stage::Refine => Some(processing::REFINED),
            Stage::Cleanup => Some(processing::PROCESSED),
            Stage::Summary => Some(processing::SUMMARY),
            _ => None,
        };
        let force_pending = live && job.is_some_and(|j| j.force);
        if force_pending && !executing {
            row.state = StageState::Waiting;
            row.started_at = None;
            row.ended_at = None;
        }
        if let Some(artifact) = artifact {
            let proof = processing::inspect(session, artifact);
            row.files = proof
                .receipt
                .as_ref()
                .map(|p| p.outputs.iter().map(|f| f.path.clone()).collect())
                .unwrap_or_default();
            if !executing && !force_pending {
                match proof.state {
                    ArtifactState::Ready => {
                        row.state = StageState::Done;
                        row.note = Some(match &proof.reason {
                            Some(detail) => format!("Артефакт сохранён и проверен · {detail}"),
                            None => "Артефакт сохранён и проверен".into(),
                        });
                        row.ended_at = row.ended_at.or(proof.at.clone());
                    }
                    ArtifactState::Empty => {
                        row.state = StageState::Skipped;
                        row.note = proof.reason.clone();
                        row.ended_at = row.ended_at.or(proof.at.clone());
                    }
                    ArtifactState::Invalid => {
                        row.state = StageState::Invalid;
                        row.note = proof.reason.clone();
                    }
                    ArtifactState::Failed => {
                        row.state = StageState::Failed;
                        row.note = proof.reason.clone();
                    }
                    ArtifactState::Missing if row.state == StageState::Done => {
                        row.state = StageState::Invalid;
                        row.note = Some(
                            "Есть событие завершения, но нет подтверждённого артефакта".into(),
                        );
                    }
                    _ => {}
                }
            }
            row.artifact = Some(proof);
        } else if !executing && !force_pending {
            if let Some(Ok(files)) = &audio {
                row.files = files.clone();
                row.state = StageState::Done;
                row.note = Some("Звуковая дорожка сохранена и проверена".into());
            } else if row.state == StageState::Done {
                // Retention can remove audio after transcription; that is no evidence that it is still present.
                row.note = Some("Этап завершался; исходное аудио сейчас недоступно".into());
                row.state = StageState::Skipped;
            }
        }
        if !executing
            && !matches!(
                row.state,
                StageState::Done | StageState::Skipped | StageState::Invalid
            )
        {
            if let Some(job) = job {
                if !requested(stage, job) {
                    row.state = StageState::Skipped;
                    row.note = Some("Этап не запрошен".into());
                } else if transcription.state == ArtifactState::Empty
                    && matches!(stage, Stage::Refine | Stage::Cleanup | Stage::Summary)
                {
                    row.state = StageState::Skipped;
                    row.note = Some("Нет речи: подтверждено результатом распознавания".into());
                } else if live {
                    let previous_in_phase = events.last().is_some_and(|e| {
                        e.state == StageState::Running
                            && e.stage != stage
                            && stage_phase(e.stage) == job.phase
                    });
                    row.state = if stage_phase(stage) == job.phase && !previous_in_phase {
                        StageState::Queued
                    } else {
                        StageState::Blocked
                    };
                    row.note = Some(
                        if row.state == StageState::Queued {
                            if job.state == JobState::Running {
                                "Обработчик получен; подготовка этапа"
                            } else {
                                "Ожидает свободного обработчика"
                            }
                        } else {
                            "Ожидает подтверждения предыдущих этапов"
                        }
                        .into(),
                    );
                    if let Some(reason) = &job.last_error {
                        row.note = Some(format!("Повтор после ошибки: {reason}"));
                    }
                } else if job.state == JobState::Failed {
                    row.state = if stage_phase(stage) == job.phase {
                        StageState::Failed
                    } else {
                        StageState::Blocked
                    };
                    row.note = job.last_error.clone();
                }
            }
        }
        stages.push(row);
    }
    View {
        stages,
        running: live,
        phase: job.filter(|j| j.state != JobState::Done).map(|j| j.phase),
        timing: timing(queued, started, ended, live, now),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn actual_video_wait_and_work_are_separate_and_media_length_is_irrelevant() {
        let t = timing(
            Some("2026-09-27T11:08:41+03:00".into()),
            Some("2026-09-27T11:25:01+03:00".into()),
            Some("2026-09-27T11:33:23+03:00".into()),
            false,
            "ignored",
        );
        assert_eq!(t.queue_sec, Some(980.0));
        assert_eq!(t.processing_sec, Some(502.0));
        assert_eq!(t.total_sec, Some(1482.0));
    }
    #[test]
    fn queued_work_has_no_fake_processing_time() {
        let t = timing(
            Some("2026-09-27T11:00:00Z".into()),
            None,
            None,
            true,
            "2026-09-27T11:04:00Z",
        );
        assert_eq!(t.queue_sec, Some(240.0));
        assert_eq!(t.processing_sec, None);
        assert_eq!(t.total_sec, Some(240.0));
    }

    #[test]
    fn a_new_run_mark_does_not_start_processing_while_queued() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("sessions/a");
        std::fs::create_dir_all(&session).unwrap();
        progress::new_run(&session); // recook button, before the worker claims it
        let mut q = crate::jobs::JobQueue::load(dir.path());
        q.enqueue_cook("a", true, true, true);
        let view = view(
            &session,
            Some(&q.jobs()[0]),
            false,
            &crate::versions::now_rfc3339(),
        );
        assert!(view.running);
        assert_eq!(view.timing.processing_sec, None);
        assert_eq!(view.stages[0].state, StageState::Blocked);
    }

    #[test]
    fn recovered_results_are_visible_without_new_completion_events() {
        let dir = tempfile::tempdir().unwrap();
        crate::artifacts::tests::fixture(dir.path());
        processing::record(
            dir.path(),
            processing::TRANSCRIPT,
            "fixture",
            processing::Outcome::Ok,
            None,
            Some(1),
        )
        .unwrap();
        progress::new_run(dir.path());
        let result = view(dir.path(), None, false, "2026-09-27T01:00:00Z");
        assert_eq!(result.stages[0].state, StageState::Done);
        assert_eq!(result.stages[0].files, ["transcripts/v001-raw.jsonl"]);
        assert_eq!(result.stages[0].started_at, None);
        assert_ne!(result.stages[1].state, StageState::Done);
        assert_ne!(result.stages[3].state, StageState::Done);
    }
}
