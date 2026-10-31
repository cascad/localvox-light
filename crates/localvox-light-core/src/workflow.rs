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
    pub at: String,
    pub stages: Vec<StageStatus>,
    pub running: bool,
    pub phase: Option<JobPhase>,
    pub timing: Timing,
    pub execution: Execution,
}

#[derive(Serialize, Debug)]
pub struct Execution {
    pub run: Option<String>,
    pub operation: Option<String>,
    pub state: String,
    pub message: String,
    pub current: Option<Stage>,
    pub phase: Option<JobPhase>,
    pub since: Option<String>,
    pub next: Vec<Stage>,
    pub retry_at: Option<String>,
    pub last_signal_at: Option<String>,
}

fn execution(
    job: Option<&Job>,
    stages: &[StageStatus],
    now: &str,
    coordinator: Option<&crate::jobs::CoordinatorStatus>,
) -> Execution {
    let active = stages.iter().find(|s| s.state == StageState::Running);
    let mut value = Execution {
        run: None,
        operation: None,
        state: "untracked".into(),
        message: "Задания в очереди нет; автоматический запуск не подтверждён".into(),
        current: None,
        phase: job.map(|j| j.phase),
        since: None,
        retry_at: None,
        last_signal_at: active.and_then(|s| s.updated_at.clone()),
        next: stages
            .iter()
            .filter(|s| !matches!(s.state, StageState::Done | StageState::Skipped))
            .filter(|s| {
                job.map_or(!matches!(s.stage, Stage::Refine | Stage::Cleanup), |j| {
                    requested(s.stage, j)
                })
            })
            .map(|s| s.stage)
            .collect(),
    };
    if let Some(j) = job {
        value.since = j.phase_queued_at.clone().or_else(|| {
            (j.phase == JobPhase::Prepare)
                .then(|| j.enqueued_at.clone().unwrap_or(j.created_at.clone()))
        });
        match j.state {
            JobState::Pending => {
                value.state = "queued".into();
                value.message = if j.last_error.is_some() {
                    "Ожидается повтор после ошибки"
                } else {
                    "Ожидает свободного обработчика для следующего этапа"
                }
                .into();
            }
            JobState::Running => {
                value.state = "running".into();
                value.current = active.map(|s| s.stage);
                value.since = j
                    .phase_started_at
                    .clone()
                    .or_else(|| active.and_then(|s| s.started_at.clone()));
                value
                    .next
                    .retain(|s| active.map_or(stage_phase(*s) != j.phase, |a| a.stage != *s));
                value.message = if active.is_some() {
                    "Этап выполняется"
                } else {
                    match j.phase {
                        JobPhase::Prepare => {
                            "Подготовка источника: запуск обработчика или проверка результата"
                        }
                        JobPhase::Transcribe => {
                            "Транскрибация: запуск обработчика или проверка результата"
                        }
                        JobPhase::Text => {
                            "Ручная очистка: запуск обработчика или проверка результата"
                        }
                        JobPhase::Summary => "Сводка: запуск обработчика или проверка результата",
                    }
                }
                .into();
                if duration(
                    value.last_signal_at.as_deref().or(value.since.as_deref()),
                    Some(now),
                )
                .is_some_and(|age| age > 30.0)
                {
                    value.state = "stalled".into();
                    value.message =
                        "Нет сигнала обработчика более 30 секунд; выполнение не подтверждено"
                            .into();
                }
            }
            JobState::Failed => {
                value.retry_at = crate::jobs::automatic_retry_at(j);
                value.state = if value.retry_at.is_some() {
                    "retry_wait"
                } else {
                    "failed"
                }
                .into();
                value.since = value.retry_at.as_ref().and_then(|_| j.finished_at.clone());
                value.message = if value.retry_at.is_some() {
                    "Обработчик остановлен после ошибки; ожидается автоматический повтор"
                } else {
                    "Обработка остановлена; требуется ручной повтор"
                }
                .into();
            }
            JobState::Done => {
                value.phase = None;
                value.since = None;
            }
        }
        if let Some(reason) = &j.last_error {
            value.message = format!("{} · {reason}", value.message);
        }
        if matches!(j.state, JobState::Pending | JobState::Running) {
            if let Some(coordinator) = coordinator {
                if !matches!(coordinator.state.as_str(), "ready" | "starting") {
                    value.state = "blocked".into();
                    value.current = None;
                    value.message = coordinator.message.clone();
                } else if j.state == JobState::Pending
                    && duration(Some(&coordinator.at), Some(now)).is_some_and(|age| age > 30.0)
                {
                    value.state = "stalled".into();
                    value.message =
                        "Координатор не подаёт сигнал; ожидающие задания сохранены".into();
                } else if j.state == JobState::Pending && coordinator.state == "starting" {
                    value.message = "Координатор проверяет доступность обработчика".into();
                }
            } else if j.state == JobState::Pending {
                value
                    .message
                    .push_str(" · состояние координатора неизвестно");
            }
        }
    } else if active.is_some() {
        value.state = "untracked".into();
        value.message =
            "Есть событие запуска, но задания в очереди нет; выполнение не подтверждено".into();
    }
    if job.is_none_or(|j| j.state == JobState::Done) {
        if value.next.is_empty()
            && !stages
                .iter()
                .any(|s| matches!(s.state, StageState::Invalid | StageState::Failed))
        {
            value.state = "done".into();
            value.message = "Все запрошенные результаты готовы; следующих этапов нет".into();
        } else if job.is_some() {
            value.state = "attention".into();
            value.message = "Задание завершалось, но часть результатов не подтверждена".into();
        }
    }
    value.since = value
        .since
        .filter(|at| duration(Some(at), Some(now)).is_some());
    if job.is_none() {
        value.next.clear();
    }
    value
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
    let coordinator = crate::jobs::coordinator_status();
    let executor_available = coordinator
        .as_ref()
        .is_none_or(|c| matches!(c.state.as_str(), "ready" | "starting"));
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
            && executor_available
            && job.is_some_and(|j| j.state == JobState::Running && j.phase == stage_phase(stage));
        if row.state == StageState::Running && !executing {
            row.state = StageState::Waiting;
            row.note = Some("Есть событие запуска, но текущее выполнение не подтверждено".into());
        }
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
            && job.is_some_and(|j| !requested(stage, j))
            && !matches!(row.state, StageState::Done | StageState::Invalid)
        {
            row.state = StageState::Skipped;
            row.note = Some(
                if job.is_some_and(|j| j.follow_settings)
                    && matches!(stage, Stage::Refine | Stage::Cleanup)
                {
                    "Дополнительная очистка запускается вручную"
                } else if job.is_some_and(|j| j.follow_settings) {
                    "Отключено в настройках · без вызова LLM"
                } else {
                    "Не запрошено в задании · без вызова LLM"
                }
                .into(),
            );
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
                    row.note = Some("Не запрошено в задании · без вызова LLM".into());
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
    let mut execution = execution(job, &stages, now, coordinator.as_ref());
    (execution.run, execution.operation) =
        crate::journal::execution_identity(session, job.and_then(|j| j.execution_id.as_deref()));
    View {
        at: now.into(),
        stages,
        running: live,
        phase: job.filter(|j| j.state != JobState::Done).map(|j| j.phase),
        timing: timing(queued, started, ended, live, now),
        execution,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn orphaned_running_event_is_not_a_running_worker() {
        let dir = tempfile::tempdir().unwrap();
        progress::mark(dir.path(), Stage::Transcribe, StageState::Running, None);
        let result = view(dir.path(), None, false, &crate::versions::now_rfc3339());
        assert!(result.stages.iter().all(|s| s.state != StageState::Running));
        assert_eq!(result.execution.state, "untracked");
    }
    fn status(
        stage: Stage,
        state: StageState,
        start: Option<&str>,
        end: Option<&str>,
    ) -> StageStatus {
        StageStatus {
            stage,
            state,
            started_at: start.map(str::to_owned),
            ended_at: end.map(str::to_owned),
            updated_at: end.or(start).map(str::to_owned),
            note: None,
            done: None,
            total: None,
            artifact: None,
            files: vec![],
        }
    }
    #[test]
    fn between_phases_running_retry_and_completion_have_explicit_plans_and_clocks() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        queue.enqueue_cook("a", true, false, false);
        let mut job = queue.jobs()[0].clone();
        job.phase = JobPhase::Summary;
        job.phase_queued_at = Some("2026-10-31T00:00:05Z".into());
        let mut stages = vec![
            status(
                Stage::Transcribe,
                StageState::Done,
                Some("2026-10-31T00:00:01Z"),
                Some("2026-10-31T00:00:05Z"),
            ),
            status(Stage::Summary, StageState::Queued, None, None),
        ];
        let state = execution(Some(&job), &stages, "2026-10-31T00:00:09Z", None);
        assert_eq!(state.state, "queued");
        assert_eq!(state.next, [Stage::Summary]);
        assert_eq!(
            duration(state.since.as_deref(), Some("2026-10-31T00:00:09Z")),
            Some(4.0)
        );
        job.state = JobState::Running;
        job.phase_started_at = Some("2026-10-31T00:00:10Z".into());
        stages[1] = status(
            Stage::Summary,
            StageState::Running,
            Some("2026-10-31T00:00:10Z"),
            None,
        );
        let state = execution(Some(&job), &stages, "2026-10-31T00:00:15Z", None);
        assert_eq!(state.state, "running");
        assert_eq!(state.current, Some(Stage::Summary));
        assert!(state.next.is_empty());
        job.state = JobState::Failed;
        job.failed_at = timestamp("2026-10-31T00:00:20Z").map(|s| s as u64);
        job.finished_at = Some("2026-10-31T00:00:20Z".into());
        stages[1] = status(
            Stage::Summary,
            StageState::Failed,
            Some("2026-10-31T00:00:10Z"),
            Some("2026-10-31T00:00:20Z"),
        );
        let state = execution(Some(&job), &stages, "2026-10-31T00:00:30Z", None);
        assert_eq!(state.state, "retry_wait");
        assert_eq!(
            timestamp(state.retry_at.as_deref().unwrap()).unwrap() - job.failed_at.unwrap() as f64,
            3600.0
        );
        job.revivals = 3;
        assert_eq!(
            execution(Some(&job), &stages, "2026-10-31T00:00:30Z", None).state,
            "failed"
        );
        job.state = JobState::Done;
        stages[1] = status(
            Stage::Summary,
            StageState::Done,
            Some("2026-10-31T00:00:10Z"),
            Some("2026-10-31T00:00:20Z"),
        );
        let state = execution(Some(&job), &stages, "2026-10-31T00:00:30Z", None);
        assert_eq!(state.state, "done");
        assert!(state.next.is_empty() && state.since.is_none());
    }
    #[test]
    fn missing_executor_stale_signals_and_untracked_work_never_look_successful() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        queue.enqueue_cook("a", true, false, false);
        let mut job = queue.jobs()[0].clone();
        job.phase = JobPhase::Summary;
        job.phase_queued_at = None; // old queue: unknown phase boundary
        let stages = vec![status(Stage::Summary, StageState::Queued, None, None)];
        let coordinator = crate::jobs::CoordinatorStatus {
            state: "disabled".into(),
            message: "Автоматическая обработка выключена".into(),
            at: "2026-10-31T00:00:00Z".into(),
        };
        let state = execution(
            Some(&job),
            &stages,
            "2026-10-31T00:01:00Z",
            Some(&coordinator),
        );
        assert_eq!(state.state, "blocked");
        assert!(state.since.is_none());
        assert_eq!(state.next, [Stage::Summary]);
        let state = execution(None, &stages, "2026-10-31T00:01:00Z", None);
        assert_eq!(state.state, "untracked");
        assert!(state.next.is_empty());
        job.state = JobState::Running;
        job.phase_started_at = Some("2026-10-31T00:00:00Z".into());
        assert_eq!(
            execution(Some(&job), &stages, "2026-10-31T00:01:00Z", None).state,
            "stalled"
        );
    }
    #[test]
    fn disabled_automatic_stages_explain_skips_and_keep_verified_stt() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("sessions/a");
        std::fs::create_dir_all(&session).unwrap();
        crate::artifacts::tests::fixture(&session);
        processing::record(
            &session,
            processing::TRANSCRIPT,
            "fixture",
            processing::Outcome::Ok,
            None,
            Some(1),
        )
        .unwrap();
        let mut q = crate::jobs::JobQueue::load(dir.path());
        q.enqueue_cook("a", false, false, false);
        let result = view(&session, Some(&q.jobs()[0]), false, "2026-10-31T01:00:00Z");
        assert_eq!(result.stages[0].state, StageState::Done);
        for row in &result.stages[1..] {
            assert_eq!(row.state, StageState::Skipped);
            assert!(row
                .note
                .as_ref()
                .unwrap()
                .contains(if row.stage == Stage::Summary {
                    "Отключено в настройках"
                } else {
                    "запускается вручную"
                }));
        }
        processing::record(
            &session,
            processing::SUMMARY,
            "fixture",
            processing::Outcome::Ok,
            None,
            Some(1),
        )
        .unwrap();
        let result = view(&session, Some(&q.jobs()[0]), false, "2026-10-31T01:00:00Z");
        assert_eq!(result.stages.last().unwrap().state, StageState::Done);
        assert_eq!(
            std::fs::read_to_string(session.join("summary.md")).unwrap(),
            "A real summary"
        );
    }
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
