//! Worker transport and execution only. Queue transitions and artifact acceptance stay
//! in the coordinator; the operation guard spans acceptance so its duration is unchanged.
use super::{clean_line, error_tail, warm, Config};
use anyhow::{Context, Result};
use localvox_light_core::{
    jobs::{self, Job, JobPhase},
    progress,
};
use localvox_light_process::{Limits, Termination};
use std::{
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};

pub(super) enum Outcome {
    Completed,
    Cancelled,
    Failed(String),
}
pub(super) struct Attempt<'a> {
    pub(super) outcome: Outcome,
    _operation: Option<localvox_light_core::journal::Operation<'a>>,
}

pub(super) fn run<'a>(
    cfg: &Config,
    session: &'a Path,
    running: &AtomicBool,
    job: &Job,
    timeout: Duration,
    owner: Option<&jobs::control::ManagedExecution>,
    warm: Option<&mut warm::Lane>,
) -> Attempt<'a> {
    let initial_stage = match job.phase {
        JobPhase::Prepare => progress::Stage::Download,
        JobPhase::Transcribe => progress::Stage::Transcribe,
        JobPhase::Text if job.refine => progress::Stage::Refine,
        JobPhase::Text => progress::Stage::Cleanup,
        JobPhase::Summary => progress::Stage::Summary,
    };
    progress::mark(
        &session,
        initial_stage,
        progress::StageState::Running,
        Some("Запуск обработчика"),
    );
    let mut cmd = cfg.command();
    cmd.arg(&session)
        .arg("--worker-phase")
        .arg(job.phase.as_str())
        .arg("--work-dir")
        .arg(&cfg.work_dir)
        .arg("--model-dir")
        .arg(&cfg.model_dir)
        .arg("--llm-model")
        .arg(&cfg.llm_model)
        .arg("--summary-provider")
        .arg(&cfg.summary_provider)
        .env("LOCALVOX_LIGHT_YT_FFMPEG", &cfg.ffmpeg);
    if job.phase == JobPhase::Text {
        if job.cleanup {
            cmd.arg("--cleanup");
        }
        if job.refine {
            cmd.arg("--refine");
        }
    }
    if job.phase == JobPhase::Transcribe && job.force {
        cmd.arg("--force");
    }
    if job.kind == jobs::JobKind::Incremental {
        let input = job.prefix.as_ref().map(|p| &p.requested);
        let prepare = || -> Result<PathBuf> {
            let input = input.context("missing incremental input")?;
            input.horizon(&session)?;
            job.execution_id
                .as_deref()
                .context("missing execution id")?;
            let path = session
                .join("incremental/claims")
                .join(format!("{}.json", job.id));
            std::fs::create_dir_all(path.parent().unwrap())?;
            localvox_light_core::artifacts::atomic_write(&path, &serde_json::to_vec(input)?)?;
            Ok(path)
        };
        match prepare() {
            Ok(path) => {
                cmd.arg("--worker-prefix").arg("--worker-input").arg(path);
            }
            Err(error) => {
                return Attempt {
                    outcome: Outcome::Failed(format!("Подготовка раннего входа: {error:#}")),
                    _operation: None,
                };
            }
        }
    }
    let resident_request =
        job.phase == JobPhase::Transcribe && !cfg.asr_keep_alive.is_zero() && warm.is_some();
    let mut _journal_operation = localvox_light_core::journal::Operation::start(
        &session,
        job.phase.as_str(),
        if resident_request {
            "ASR · обработка порции"
        } else {
            "localvox-process"
        },
        if resident_request {
            None
        } else {
            Some(localvox_light_core::journal::DisplayCommand::from_args(
                "localvox-process",
                &cmd.get_args()
                    .map(|v| v.to_string_lossy().into_owned())
                    .collect::<Vec<_>>(),
            ))
        },
    );
    if let Some(execution) = &job.execution_id {
        if let Err(error) = _journal_operation.bind_execution(execution) {
            tracing::debug!("journal execution binding: {error}");
        }
    }
    if job.phase == JobPhase::Transcribe && !cfg.asr_keep_alive.is_zero() {
        if let Some(warm) = warm {
            let result = warm.run(cfg, running, job, owner, timeout);
            let outcome = match result {
                Ok(()) => Outcome::Completed,
                Err(_)
                    if !running.load(Ordering::Relaxed) || owner.is_some_and(|o| o.cancelled()) =>
                {
                    _journal_operation.cancelled();
                    Outcome::Cancelled
                }
                Err(error) => {
                    _journal_operation.failed();
                    Outcome::Failed(format!("Фаза transcribe: {error:#}"))
                }
            };
            return Attempt {
                outcome,
                _operation: Some(_journal_operation),
            };
        }
    }
    let outcome = std::thread::scope(|scope| {
        let (observer, observed, dropped) = localvox_light_process::OutputObserver::channel(16);
        let listener = scope.spawn(|| {
            for chunk in observed {
                // Raw tool output can contain user text or credentials. Publish only facts
                // about transport; detailed operation labels come from trusted producers.
                _journal_operation.log(&format!(
                    "{:?}: получено {} байт; содержимое остаётся в диагностике обработчика",
                    chunk.stream,
                    chunk.bytes.len()
                ));
            }
        });
        let result = localvox_light_process::run_observed(
            cmd,
            None,
            Limits::new(timeout),
            || !running.load(Ordering::Relaxed) || owner.is_some_and(|owner| owner.cancelled()),
            |pid, elapsed| {
                let events = progress::read(&session);
                if let Some(event) = events
                    .last()
                    .filter(|e| e.state == progress::StageState::Running)
                {
                    // A live owned process can be loading a model before its own
                    // heartbeats start. Keep its stage start/counters intact.
                    let age = chrono::DateTime::parse_from_rfc3339(&event.at)
                        .ok()
                        .map(|at| chrono::Utc::now().signed_duration_since(at).num_seconds());
                    if age.is_some_and(|age| age >= 10) {
                        progress::mark(
                            &session,
                            event.stage,
                            progress::StageState::Running,
                            event.note.as_deref(),
                        );
                    }
                }
                let event = events.last();
                tracing::info!(job_id = job.id, session = %job.session, phase = job.phase.as_str(), attempt = job.attempts,
                pid, elapsed_sec = elapsed.as_secs(), timeout_sec = timeout.as_secs(),
                operation = event.and_then(|e| e.note.as_deref()).unwrap_or("запуск обработчика"),
                last_signal = event.map(|e| e.at.as_str()).unwrap_or("нет"), "autocook: heartbeat");
            },
            Some(observer),
        );
        if listener.join().is_err() {
            tracing::warn!("journal observer stopped");
        }
        let lost = dropped.load(Ordering::Relaxed);
        if lost > 0 {
            _journal_operation.log(&format!("Наблюдение вывода пропустило {lost} байт; обработка и сохранённый результат не изменены"));
        }
        result
    });
    let reason = match outcome {
        Ok(out) => {
            for line in String::from_utf8_lossy(&out.stdout).lines() {
                tracing::info!(session = %job.session, "worker: {}", clean_line(line));
            }
            for line in String::from_utf8_lossy(&out.stderr).lines() {
                tracing::debug!(session = %job.session, "worker: {}", clean_line(line));
            }
            tracing::info!(job_id = job.id, session = %job.session, phase = job.phase.as_str(), attempt = job.attempts,
                pid = out.pid, elapsed_sec = out.elapsed.as_secs(), termination = ?out.termination, "autocook: worker stopped");
            match out.termination {
                Termination::Exited(status) if status.success() => {
                    return Attempt {
                        outcome: Outcome::Completed,
                        _operation: Some(_journal_operation),
                    };
                }
                // Interrupted jobs are recovered on restart and reuse verified artifacts.
                Termination::Cancelled => {
                    _journal_operation.cancelled();
                    return Attempt {
                        outcome: Outcome::Cancelled,
                        _operation: Some(_journal_operation),
                    };
                }
                Termination::TimedOut => format!(
                    "Фаза {}: превышен лимит {} с",
                    job.phase.as_str(),
                    timeout.as_secs()
                ),
                Termination::OutputLimit => format!(
                    "Фаза {}: превышен лимит диагностики 8 MiB",
                    job.phase.as_str()
                ),
                Termination::Exited(status) => format!(
                    "Фаза {}: {status}: {}",
                    job.phase.as_str(),
                    error_tail(&out.stderr)
                ),
            }
        }
        Err(e) => format!("Фаза {}: {e:#}", job.phase.as_str()),
    };
    Attempt {
        outcome: Outcome::Failed(reason),
        _operation: Some(_journal_operation),
    }
}
