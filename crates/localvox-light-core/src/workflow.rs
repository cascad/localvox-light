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
    /// Union of recorded stage intervals, including retries, without overlap.
    pub stages_sec: Option<f64>,
    /// Measured wall time outside stages: startup, scheduling, transitions.
    pub between_sec: Option<f64>,
    /// Overlap removed from the sum of stage intervals (not child operations).
    pub overlap_sec: Option<f64>,
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
        ..Default::default()
    }
}

/// A wall-clock partition, not a sum of nested operation timers. Unknown stage
/// boundaries leave the partition unknown rather than attributing lost work to gaps.
#[cfg(test)]
fn account_timing(value: &mut Timing, events: &[progress::StageEvent], live: bool, now: &str) {
    account_timing_with_background(value, events, &[], false, live, now)
}
fn account_timing_with_background(
    value: &mut Timing,
    events: &[progress::StageEvent],
    background: &[(String, Option<String>)],
    incomplete: bool,
    live: bool,
    now: &str,
) {
    if incomplete {
        return;
    }
    let Some((from, to)) = value.started_at.as_deref().and_then(timestamp).zip(
        value
            .ended_at
            .as_deref()
            .or(live.then_some(now))
            .and_then(timestamp),
    ) else {
        return;
    };
    if to < from {
        return;
    }
    let mut intervals = Vec::new();
    for stage in Stage::CHAIN {
        let mut start = None;
        for event in events.iter().filter(|e| e.stage == stage) {
            let Some(at) = timestamp(&event.at) else {
                return;
            };
            if at < from || at > to {
                return;
            }
            match event.state {
                StageState::Running => {
                    start.get_or_insert(at);
                }
                StageState::Done
                | StageState::Failed
                | StageState::Cancelled
                | StageState::Invalid
                | StageState::Skipped => {
                    if let Some(begin) = start.take() {
                        if at < begin {
                            return;
                        }
                        intervals.push((begin, at));
                    } else if event.state != StageState::Skipped {
                        return;
                    }
                }
                _ => {}
            }
        }
        if let Some(begin) = start {
            if !live {
                return;
            }
            intervals.push((begin, to));
        }
    }
    for (start, end) in background {
        let Some(start) = timestamp(start) else {
            return;
        };
        let end = end.as_deref().and_then(timestamp).or(live.then_some(to));
        // Completed historical work outside this processing interval is irrelevant.
        if end.is_some_and(|end| end <= from) || start >= to {
            continue;
        }
        let Some(end) = end else {
            return;
        };
        if end < start {
            return;
        }
        intervals.push((start.max(from), end.min(to)));
    }
    intervals.sort_by(|a, b| a.0.total_cmp(&b.0));
    let raw = intervals
        .iter()
        .map(|(start, end)| end - start)
        .sum::<f64>();
    let mut covered = 0.0;
    let mut boundary = from;
    for (start, end) in intervals {
        covered += (end - start.max(boundary)).max(0.0);
        boundary = boundary.max(end);
    }
    value.stages_sec = Some(covered);
    value.between_sec = Some((to - from - covered).max(0.0));
    value.overlap_sec = Some((raw - covered).max(0.0));
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub current: Option<Stage>,
    pub phase: Option<JobPhase>,
    pub since: Option<String>,
    pub next: Vec<Stage>,
    pub retry_at: Option<String>,
    pub last_signal_at: Option<String>,
}

#[derive(Serialize, Debug)]
pub struct RetryPlan {
    pub state: String,
    pub message: String,
    pub retry_at: Option<String>,
}

/// The same read-only explanation is used by the queue and the selected session's workflow.
pub fn retry_plan(
    job: &Job,
    _coordinator: Option<&crate::jobs::CoordinatorStatus>,
    _now: &str,
) -> Option<RetryPlan> {
    (job.state == JobState::Failed).then(|| RetryPlan {
        state: "failed".into(),
        message: "Обработка остановлена после ошибки; продолжение — вручную".into(),
        retry_at: None,
    })
}

fn execution(
    job: Option<&Job>,
    stages: &[StageStatus],
    now: &str,
    coordinator: Option<&crate::jobs::CoordinatorStatus>,
) -> Execution {
    let active = stages.iter().find(|s| s.state == StageState::Running);
    let requested_here = |stage| {
        job.map_or(!matches!(stage, Stage::Refine | Stage::Cleanup), |j| {
            requested(stage, j)
        })
    };
    let incomplete_results = stages.iter().any(|s| {
        requested_here(s.stage) && !matches!(s.state, StageState::Done | StageState::Skipped)
    });
    let mut value = Execution {
        run: None,
        operation: None,
        state: "untracked".into(),
        message: "Задания в очереди нет; автоматический запуск не подтверждён".into(),
        error: job.and_then(|j| j.last_error.clone()),
        current: None,
        phase: job.map(|j| j.phase),
        since: None,
        retry_at: None,
        last_signal_at: active.and_then(|s| s.updated_at.clone()),
        next: stages
            .iter()
            .filter(|s| !matches!(s.state, StageState::Done | StageState::Skipped))
            .filter(|s| requested_here(s.stage))
            .filter(|s| job.is_none_or(|j| remaining_phase(stage_phase(s.stage), j.phase)))
            .map(|s| s.stage)
            .collect(),
    };
    if let Some(j) = job {
        value.since = j.phase_queued_at.clone().or_else(|| {
            (j.phase == JobPhase::Prepare)
                .then(|| j.enqueued_at.clone().unwrap_or(j.created_at.clone()))
        });
        match j.state {
            JobState::WaitingInput => {
                value.state = "waiting_input".into();
                value.message = "Готовая часть обработана; ожидание следующего закрытого фрагмента или включения раннего распознавания".into();
                value.current = None;
                value.since = None;
            }
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
                        JobPhase::Punctuation => {
                            "Пунктуация: запуск обработчика или проверка результата"
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
            JobState::Cancelling => {
                value.state = "cancelling".into();
                value.message = "Запрошена остановка; ожидается освобождение обработчика".into();
                value.current = active.map(|s| s.stage);
                value.since = None; // There is no independent stop-request clock in legacy Job data.
                value.next.clear();
            }
            JobState::Cancelled => {
                value.state = "cancelled".into();
                value.message =
                    "Обработка отменена; сохранённые результаты доступны, продолжение — вручную"
                        .into();
                value.current = None;
                value.since = j.finished_at.clone();
                value.next.clear();
            }
            JobState::Failed => {
                let plan = retry_plan(j, coordinator, now).expect("Failed has a retry plan");
                value.since = (plan.state != "failed")
                    .then(|| j.finished_at.clone())
                    .flatten();
                value.retry_at = plan.retry_at;
                value.state = plan.state;
                value.message = plan.message;
            }
            JobState::Done => {
                value.phase = None;
                value.since = None;
            }
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
        if !incomplete_results
            && !stages
                .iter()
                .any(|s| matches!(s.state, StageState::Invalid | StageState::Failed))
        {
            value.state = "done".into();
            if let Some(skipped) = stages.iter().find(|s| {
                s.stage == Stage::Summary
                    && s.state == StageState::Skipped
                    && requested_here(s.stage)
            }) {
                value.message = format!(
                    "Обработка завершена; сводка пропущена · {}",
                    skipped
                        .note
                        .as_deref()
                        .map(crate::processing::empty_result_reason)
                        .unwrap_or("этап пропущен")
                );
            } else {
                value.message = "Все запрошенные результаты готовы; следующих этапов нет".into();
            }
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
        Stage::Punctuation => JobPhase::Punctuation,
        Stage::Refine | Stage::Cleanup => JobPhase::Text,
        Stage::Summary => JobPhase::Summary,
    }
}

fn remaining_phase(stage: JobPhase, current: JobPhase) -> bool {
    match current {
        JobPhase::Prepare => true,
        JobPhase::Transcribe => stage != JobPhase::Prepare,
        JobPhase::Punctuation => matches!(
            stage,
            JobPhase::Punctuation | JobPhase::Text | JobPhase::Summary
        ),
        JobPhase::Text => matches!(stage, JobPhase::Text | JobPhase::Summary),
        JobPhase::Summary => stage == JobPhase::Summary,
    }
}

fn requested(stage: Stage, job: &Job) -> bool {
    match stage {
        Stage::Punctuation => job.punctuation,
        Stage::Refine => job.refine,
        Stage::Cleanup => job.cleanup,
        Stage::Summary => job.summary,
        _ => true,
    }
}

pub fn view(session: &Path, job: Option<&Job>, from_link: bool, now: &str) -> View {
    let punctuation_requested = job.map(|job| job.punctuation).unwrap_or_else(|| {
        crate::transcription::Choice::for_session(session).is_ok_and(|choice| choice.punctuation)
    });
    let coordinator = crate::jobs::coordinator_status();
    let executor_available = coordinator
        .as_ref()
        .is_none_or(|c| matches!(c.state.as_str(), "ready" | "starting"));
    let run = progress::read_run(session);
    let events = run.events;
    let folded = progress::fold(&events);
    let live = job.is_some_and(|j| {
        matches!(
            j.state,
            JobState::Pending | JobState::Running | JobState::Cancelling | JobState::WaitingInput
        )
    });
    let queued = job.map(|j| j.enqueued_at.as_ref().unwrap_or(&j.created_at).clone());
    // Run marks predate model/title loading. Older queues have no started_at field.
    let run_start = run.started_at;
    let has_started = job.is_none_or(|j| {
        if j.state == JobState::Cancelled {
            return j.started_at.is_some() || j.attempts > 0;
        }
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
            Stage::Punctuation => Some(processing::PUNCTUATED),
            Stage::Refine => Some(processing::REFINED),
            Stage::Cleanup => Some(processing::PROCESSED),
            Stage::Summary => Some(processing::SUMMARY),
            _ => None,
        };
        // --force replaces transcript/derived results, never already verified input audio.
        let force_pending = live && artifact.is_some() && job.is_some_and(|j| j.force);
        if force_pending && !executing {
            row.state = StageState::Waiting;
            row.started_at = None;
            row.ended_at = None;
        }
        if let Some(artifact) = artifact {
            let proof = if artifact == processing::TRANSCRIPT {
                transcription.clone()
            } else {
                processing::inspect(session, artifact)
            };
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
        if stage == Stage::Punctuation
            && !punctuation_requested
            && !executing
            && !matches!(row.state, StageState::Done | StageState::Invalid)
        {
            row.state = StageState::Skipped;
            row.note = Some("Пунктуация выключена для этой записи · без загрузки модели".into());
        }
        if !executing
            && job.is_some_and(|j| !requested(stage, j))
            && !matches!(row.state, StageState::Done | StageState::Invalid)
        {
            row.state = StageState::Skipped;
            row.note = Some(
                if stage == Stage::Punctuation {
                    "Пунктуация выключена для этой записи · без загрузки модели"
                } else if job.is_some_and(|j| j.follow_settings)
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
                    && matches!(
                        stage,
                        Stage::Punctuation | Stage::Refine | Stage::Cleanup | Stage::Summary
                    )
                {
                    row.state = StageState::Skipped;
                    row.note = Some("Нет речи: подтверждено результатом распознавания".into());
                } else if job.state == JobState::Cancelled {
                    row.state = StageState::Cancelled;
                    row.note = Some("Обработка отменена; этап не объявляется выполненным".into());
                    row.ended_at = job.finished_at.clone();
                } else if job.state == JobState::Cancelling {
                    row.state = StageState::Blocked;
                    row.note = Some("Ожидается подтверждённая остановка обработчика".into());
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
                    if job.last_error.is_some() && row.state == StageState::Queued {
                        row.note = Some(
                            "Повтор после предыдущего отказа; причина — в подробностях ошибки"
                                .into(),
                        );
                    }
                } else if job.state == JobState::Failed {
                    let known_failure = events
                        .iter()
                        .rev()
                        .find(|e| {
                            e.state == StageState::Failed && stage_phase(e.stage) == job.phase
                        })
                        .map(|e| e.stage);
                    row.state = if stage_phase(stage) == job.phase
                        && known_failure.is_none_or(|failed| failed == stage)
                    {
                        StageState::Failed
                    } else {
                        StageState::Blocked
                    };
                    row.note = Some(
                        if row.state == StageState::Failed {
                            "Этап остановлен после ошибки; причина — в подробностях ошибки"
                        } else {
                            "Не начат: ошибка на предыдущем этапе"
                        }
                        .into(),
                    );
                }
            }
        }
        stages.push(row);
    }
    let mut execution = execution(job, &stages, now, coordinator.as_ref());
    (execution.run, execution.operation) =
        crate::journal::execution_identity(session, job.and_then(|j| j.execution_id.as_deref()));
    let mut clocks = timing(queued, started, ended, live, now);
    account_timing_with_background(
        &mut clocks,
        &events,
        &run.background,
        run.background_incomplete,
        live,
        now,
    );
    View {
        at: now.into(),
        stages,
        running: live,
        phase: job.filter(|j| j.state != JobState::Done).map(|j| j.phase),
        timing: clocks,
        execution,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn previous_failure_is_detail_not_running_status_or_every_dependency_note() {
        let temp = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(temp.path());
        queue.enqueue_cook("failed", true, false, false);
        let raw = "yt-dlp: HTTP Error 429: Too Many Requests / full upstream diagnostic";
        for state in [
            crate::jobs::JobState::Failed,
            crate::jobs::JobState::Pending,
            crate::jobs::JobState::Running,
        ] {
            let mut job = queue.jobs()[0].clone();
            job.state = state;
            job.last_error = Some(raw.into());
            let result = super::view(
                &temp.path().join("sessions/failed"),
                Some(&job),
                true,
                "2026-11-25T00:00:00Z",
            );
            assert_eq!(result.execution.error.as_deref(), Some(raw));
            assert!(!result.execution.message.contains(raw));
            assert!(result.stages.iter().all(|s| !s
                .note
                .as_deref()
                .unwrap_or_default()
                .contains(raw)));
        }
    }
    fn clock_event(stage: Stage, state: StageState, second: &str) -> progress::StageEvent {
        progress::StageEvent {
            stage,
            state,
            at: format!("2026-11-21T00:00:{second}Z"),
            note: None,
            done: None,
            total: None,
        }
    }

    #[test]
    fn accounting_partitions_wall_time_with_retries_and_overlapping_stages() {
        use Stage::*;
        use StageState::*;
        let events = vec![
            clock_event(Download, Running, "01.100"),
            clock_event(Download, Running, "02"), // heartbeat is not a start
            clock_event(Download, Done, "05.200"),
            clock_event(Extract, Running, "04.200"), // one second overlaps
            clock_event(Extract, Done, "06.200"),
            clock_event(Transcribe, Running, "07.200"),
            clock_event(Transcribe, Failed, "08.200"),
            clock_event(Transcribe, Running, "10.200"),
            clock_event(Transcribe, Done, "14.200"),
            clock_event(Refine, Skipped, "14.200"),
            clock_event(Summary, Running, "15.200"),
            clock_event(Summary, Done, "19.200"),
        ];
        let mut clocks = timing(
            Some("2026-11-21T00:00:00Z".into()),
            Some("2026-11-21T00:00:01Z".into()),
            Some("2026-11-21T00:00:20Z".into()),
            false,
            "unused",
        );
        account_timing(&mut clocks, &events, false, "unused");
        assert!((clocks.stages_sec.unwrap() - 14.1).abs() < 0.001);
        assert!((clocks.between_sec.unwrap() - 4.9).abs() < 0.001);
        assert!((clocks.overlap_sec.unwrap() - 1.0).abs() < 0.001);
        assert!(
            (clocks.stages_sec.unwrap() + clocks.between_sec.unwrap()
                - clocks.processing_sec.unwrap())
            .abs()
                < 0.001
        );
        assert_eq!(
            clocks.queue_sec.unwrap() + clocks.processing_sec.unwrap(),
            clocks.total_sec.unwrap()
        );
    }

    #[test]
    fn background_work_is_not_a_gap_and_overlaps_are_not_added_twice() {
        let at = |s| format!("2026-11-21T00:00:{s:02}Z");
        let events = vec![
            clock_event(Stage::Summary, StageState::Running, "20"),
            clock_event(Stage::Summary, StageState::Done, "40"),
        ];
        let background = vec![(at(1), Some(at(31))), (at(5), Some(at(29)))];
        let mut clocks = timing(Some(at(0)), Some(at(0)), Some(at(50)), false, "unused");
        account_timing_with_background(&mut clocks, &events, &background, false, false, "unused");
        assert_eq!(clocks.stages_sec, Some(39.0));
        assert_eq!(clocks.between_sec, Some(11.0));
        assert_eq!(
            clocks.stages_sec.unwrap() + clocks.between_sec.unwrap(),
            50.0
        );
        let mut incomplete = timing(Some(at(0)), Some(at(0)), Some(at(50)), false, "unused");
        account_timing_with_background(
            &mut incomplete,
            &events,
            &background,
            true,
            false,
            "unused",
        );
        assert!(incomplete.stages_sec.is_none() && incomplete.between_sec.is_none());
        let mut interrupted = timing(Some(at(0)), Some(at(0)), Some(at(50)), false, "unused");
        account_timing_with_background(
            &mut interrupted,
            &events,
            &[(at(1), None)],
            false,
            false,
            "unused",
        );
        assert!(interrupted.stages_sec.is_none());
    }

    #[test]
    fn accounting_rejects_unknown_or_invalid_boundaries_and_ticks_only_live_work() {
        use Stage::*;
        use StageState::*;
        let start = "2026-11-21T00:00:00Z";
        let end = "2026-11-21T00:00:10Z";
        for events in [
            vec![clock_event(Summary, Done, "05")],
            vec![
                clock_event(Summary, Running, "06"),
                clock_event(Summary, Done, "05"),
            ],
            vec![clock_event(Summary, Running, "01")],
            vec![clock_event(Summary, Running, "oops")],
        ] {
            let mut clocks = timing(
                Some(start.into()),
                Some(start.into()),
                Some(end.into()),
                false,
                end,
            );
            account_timing(&mut clocks, &events, false, end);
            assert!(clocks.stages_sec.is_none());
            assert!(clocks.between_sec.is_none());
        }
        let mut clocks = timing(Some(start.into()), Some(start.into()), None, true, end);
        account_timing(
            &mut clocks,
            &[
                clock_event(Transcribe, Running, "01"),
                clock_event(Transcribe, Running, "09"),
            ],
            true,
            end,
        );
        assert_eq!(clocks.stages_sec, Some(9.0));
        assert_eq!(clocks.between_sec, Some(1.0));
    }

    #[test]
    fn recook_keeps_verified_preparation_without_reusing_old_stt() {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions/video");
        std::fs::create_dir_all(session.join("audio")).unwrap();
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
        let audio = session.join("audio/source.wav");
        let mut writer = hound::WavWriter::create(
            &audio,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..1600 {
            writer.write_sample(1i16).unwrap();
        }
        writer.finalize().unwrap();
        std::fs::write(
            session.join("meta.json"),
            serde_json::to_vec(&serde_json::json!({
                "started_at":"2026-11-11T00:00:00Z", "sample_rate":16000,
                "chunks":[{"file":"source","source_id":0,"start_offset_sec":0,"duration_sec":0.1}]
            }))
            .unwrap(),
        )
        .unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        assert!(queue.enqueue_recook("video", true, false, false));
        let mut job = queue.jobs()[0].clone();
        for phase in [JobPhase::Prepare, JobPhase::Transcribe] {
            job.phase = phase;
            let result = view(&session, Some(&job), true, &crate::versions::now_rfc3339());
            for stage in [Stage::Download, Stage::Extract] {
                let row = result.stages.iter().find(|s| s.stage == stage).unwrap();
                assert_eq!(row.state, StageState::Done, "{phase:?}/{stage:?}");
                assert_eq!(row.files, ["audio/source.wav"]);
                assert!(
                    row.started_at.is_none(),
                    "artifact proof does not invent a start"
                );
                assert!(!result.execution.next.contains(&stage));
            }
            assert_ne!(
                result
                    .stages
                    .iter()
                    .find(|s| s.stage == Stage::Transcribe)
                    .unwrap()
                    .state,
                StageState::Done
            );
            assert_eq!(result.execution.next, [Stage::Transcribe, Stage::Summary]);
        }
        std::fs::write(&audio, b"damaged").unwrap();
        let result = view(&session, Some(&job), true, &crate::versions::now_rfc3339());
        for stage in [Stage::Download, Stage::Extract] {
            assert_ne!(
                result
                    .stages
                    .iter()
                    .find(|s| s.stage == stage)
                    .unwrap()
                    .state,
                StageState::Done
            );
            assert!(
                !result.execution.next.contains(&stage),
                "past failures are not future tasks"
            );
        }
        std::fs::remove_file(audio).unwrap();
        job.phase = JobPhase::Prepare;
        let result = view(&session, Some(&job), true, &crate::versions::now_rfc3339());
        assert_eq!(
            &result.execution.next[..2],
            &[Stage::Download, Stage::Extract]
        );
    }

    #[test]
    fn next_plan_never_schedules_a_past_phase_or_marks_its_failures_done() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        queue.enqueue_cook("fixture", true, true, true);
        let mut job = queue.jobs()[0].clone();
        let stages: Vec<_> = Stage::CHAIN
            .iter()
            .map(|stage| status(*stage, StageState::Invalid, None, None))
            .collect();
        for (phase, expected) in [
            (
                JobPhase::Prepare,
                vec![
                    Stage::Download,
                    Stage::Extract,
                    Stage::Transcribe,
                    Stage::Refine,
                    Stage::Cleanup,
                    Stage::Summary,
                ],
            ),
            (
                JobPhase::Transcribe,
                vec![
                    Stage::Transcribe,
                    Stage::Refine,
                    Stage::Cleanup,
                    Stage::Summary,
                ],
            ),
            (
                JobPhase::Text,
                vec![Stage::Refine, Stage::Cleanup, Stage::Summary],
            ),
            (JobPhase::Summary, vec![Stage::Summary]),
        ] {
            job.phase = phase;
            let result = execution(Some(&job), &stages, &crate::versions::now_rfc3339(), None);
            assert_eq!(result.next, expected, "{phase:?}");
            assert!(stages.iter().all(|s| s.state == StageState::Invalid));
        }
        job.state = JobState::Done;
        let mut incomplete = stages;
        incomplete
            .iter_mut()
            .for_each(|s| s.state = StageState::Done);
        incomplete[2].state = StageState::Waiting;
        let result = execution(
            Some(&job),
            &incomplete,
            &crate::versions::now_rfc3339(),
            None,
        );
        assert_eq!(
            result.state, "attention",
            "missing past output is not success"
        );
        assert!(
            result.next.is_empty(),
            "finished queue has no future past stage"
        );
    }

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
    fn failed_retry_plan_is_manual_with_or_without_legacy_timestamps_and_revivals() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        queue.enqueue_cook("retry", true, false, false);
        let mut job = queue.jobs()[0].clone();
        job.state = JobState::Failed;
        for failed_at in [None, Some(0)] {
            for revivals in [0, 1, 3, u32::MAX] {
                job.failed_at = failed_at;
                job.revivals = revivals;
                let before = serde_json::to_value(&job).unwrap();
                let plan = retry_plan(&job, None, "2026-11-23T00:00:00Z").unwrap();
                assert_eq!(plan.state, "failed");
                assert!(plan.retry_at.is_none() && plan.message.contains("вручную"));
                assert_eq!(serde_json::to_value(&job).unwrap(), before);
            }
        }
        job.state = JobState::Pending;
        assert!(retry_plan(&job, None, "2026-11-23T00:00:00Z").is_none());
    }

    #[test]
    fn completed_with_skipped_summary_does_not_claim_that_all_files_were_created() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        queue.enqueue_cook("short", true, false, false);
        let mut job = queue.jobs()[0].clone();
        job.state = JobState::Done;
        let mut skipped = status(Stage::Summary, StageState::Skipped, None, None);
        skipped.note = Some(crate::processing::SUMMARY_NO_CONTENT.into());
        let stages = [
            status(Stage::Transcribe, StageState::Done, None, None),
            skipped,
        ];
        let result = execution(Some(&job), &stages, "2026-11-23T00:00:00Z", None);
        assert_eq!(result.state, "done");
        assert!(result.message.contains("сводка пропущена"));
        assert!(result
            .message
            .contains(crate::processing::SUMMARY_NO_CONTENT));
        assert!(!result.message.contains("Все запрошенные результаты готовы"));
        assert!(result.next.is_empty());
    }

    #[test]
    fn retry_plan_and_execution_share_coordinator_availability() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        queue.enqueue_cook("retry", true, false, false);
        let mut job = queue.jobs()[0].clone();
        job.state = JobState::Failed;
        job.failed_at = Some(1793404800);
        for (status, at, expected) in [
            ("ready", "2026-10-31T00:00:25Z", "failed"),
            ("starting", "2026-10-31T00:00:25Z", "failed"),
            ("disabled", "2026-10-31T00:00:25Z", "failed"),
            ("blocked", "2026-10-31T00:00:25Z", "failed"),
            ("stopped", "2026-10-31T00:00:25Z", "failed"),
            ("ready", "2026-10-30T23:59:59Z", "failed"),
            ("ready", "invalid", "failed"),
        ] {
            let coordinator = crate::jobs::CoordinatorStatus {
                state: status.into(),
                message: "Причина координатора".into(),
                at: at.into(),
            };
            let plan = retry_plan(&job, Some(&coordinator), "2026-10-31T00:00:30Z").unwrap();
            let view = execution(Some(&job), &[], "2026-10-31T00:00:30Z", Some(&coordinator));
            assert_eq!(plan.state, expected);
            assert_eq!((view.state, view.retry_at), (plan.state, plan.retry_at));
            assert_eq!(view.message, plan.message);
        }
        let plan = retry_plan(&job, None, "2026-10-31T00:00:30Z").unwrap();
        assert_eq!(plan.state, "failed");
        assert!(plan.message.contains("вручную"));
    }

    #[test]
    fn automatic_retry_is_not_promised_when_the_coordinator_is_disabled() {
        let root = tempfile::tempdir().unwrap();
        let mut queue = crate::jobs::JobQueue::load(root.path());
        queue.enqueue_cook("retry", true, false, false);
        let mut job = queue.jobs()[0].clone();
        job.state = JobState::Failed;
        job.phase = JobPhase::Summary;
        job.failed_at = Some(1793404800);
        let coordinator = crate::jobs::CoordinatorStatus {
            state: "disabled".into(),
            message: "Автоматическая обработка выключена".into(),
            at: "2026-10-31T00:00:00Z".into(),
        };
        let result = execution(Some(&job), &[], "2026-10-31T00:00:01Z", Some(&coordinator));
        assert_eq!(result.state, "failed");
        assert!(result.message.contains("вручную"));
        assert!(result.retry_at.is_none());
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
        let coordinator = crate::jobs::CoordinatorStatus {
            state: "ready".into(),
            message: "Координатор работает".into(),
            at: "2026-10-31T00:00:25Z".into(),
        };
        let state = execution(
            Some(&job),
            &stages,
            "2026-10-31T00:00:30Z",
            Some(&coordinator),
        );
        assert_eq!(state.state, "failed");
        assert!(state.retry_at.is_none());
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
                } else if row.stage == Stage::Punctuation {
                    "Пунктуация выключена"
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
