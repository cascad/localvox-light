//! Durable scheduling, resource lanes and artifact acceptance. Heavy work lives in the worker;
//! OS process ownership lives in localvox-light-process. This module never decodes audio.
use std::{
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use localvox_light_core::{
    jobs::{self, Job, JobPhase, JobQueue, JobState},
    progress,
};
use localvox_light_process::{Limits, Termination};

const MAX_ATTEMPTS: u32 = 3;

mod observer;
pub use observer::{Observer, Transition};

struct CoordinatorExit<'a>(&'a AtomicBool);
impl Drop for CoordinatorExit<'_> {
    fn drop(&mut self) {
        if self.0.load(Ordering::Relaxed) {
            jobs::report_coordinator("blocked", "Координатор неожиданно остановился; задания сохранены, требуется перезапуск движка");
        } else {
            jobs::report_coordinator("stopped", "Координатор обработки остановлен");
        }
    }
}

/// Resolve environment once at the composition boundary, before any job is claimed.
struct Config {
    work_dir: PathBuf,
    exe: PathBuf,
    model_dir: PathBuf,
    ffmpeg: PathBuf,
    interval: Duration,
    quiescent_sec: u64,
    llm_model: String,
    summary_template: Option<String>,
    summary_provider: String,
    timeouts: [Duration; 4],
    index_timeout: Duration,
    child_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
    observer: Option<Observer>,
}

fn seconds(name: &str, default: u64) -> Result<u64> {
    match std::env::var(name) {
        Ok(value) => {
            let seconds = value
                .parse::<u64>()
                .with_context(|| format!("{name} must be seconds"))?;
            anyhow::ensure!(seconds > 0, "{name} must be positive");
            Ok(seconds)
        }
        Err(std::env::VarError::NotPresent) => Ok(default),
        Err(e) => Err(e).with_context(|| format!("reading {name}")),
    }
}

impl Config {
    fn from_env(work_dir: String) -> Result<Self> {
        let exe = std::env::current_exe()?;
        Self::from_env_in(work_dir, exe.parent().context("executable directory")?)
    }
    fn from_env_in(work_dir: String, exe_dir: &Path) -> Result<Self> {
        let cwd = std::env::current_dir()?;
        let absolute = |path: PathBuf| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        };
        let exe = exe_dir.join(if cfg!(windows) {
            "localvox-process.exe"
        } else {
            "localvox-process"
        });
        anyhow::ensure!(exe.is_file(), "worker missing: {}", exe.display());
        let model_dir = std::env::var_os("LOCALVOX_ASR_MODEL_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                let bundled = exe_dir.join("models/gigaam-v3-e2e-ctc");
                if bundled.is_dir() {
                    bundled
                } else {
                    cwd.join("models/gigaam-v3-e2e-ctc")
                }
            });
        // A missing ASR model fails the STT phase, not the whole queue (text may already exist).
        Ok(Self {
            work_dir: absolute(work_dir.into()),
            exe,
            model_dir: absolute(model_dir),
            ffmpeg: localvox_light_core::chunks::resolve_ffmpeg_for_decode().into(),
            interval: Duration::from_secs(seconds("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC", 5)?),
            quiescent_sec: seconds("LOCALVOX_LIGHT_AUTOCOOK_QUIESCENT_SEC", 15)?,
            llm_model: std::env::var("LOCALVOX_LLM_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into()),
            summary_template: std::env::var("LOCALVOX_LLM_SUMMARY_TEMPLATE")
                .ok()
                .filter(|v| !v.trim().is_empty()),
            summary_provider: std::env::var("LOCALVOX_SUMMARY_PROVIDER").unwrap_or_default(),
            timeouts: [
                Duration::from_secs(seconds("LOCALVOX_PREPARE_TIMEOUT_SEC", 1800)?),
                Duration::from_secs(seconds("LOCALVOX_TRANSCRIBE_TIMEOUT_SEC", 7200)?),
                Duration::from_secs(seconds("LOCALVOX_TEXT_TIMEOUT_SEC", 7200)?),
                Duration::from_secs(seconds("LOCALVOX_SUMMARY_TIMEOUT_SEC", 1800)?),
            ],
            index_timeout: Duration::from_secs(seconds("LOCALVOX_INDEX_TIMEOUT_SEC", 300)?),
            child_env: std::env::vars_os().collect(),
            observer: None,
        })
    }

    fn lanes(&self) -> Vec<Vec<JobPhase>> {
        let mut lanes = vec![vec![JobPhase::Prepare], vec![JobPhase::Transcribe]];
        if self.summary_provider.trim().eq_ignore_ascii_case("claude") {
            lanes.extend([vec![JobPhase::Text], vec![JobPhase::Summary]]);
        } else {
            // Local summary and cleanup share Ollama/VRAM and must not thrash its models.
            lanes.push(vec![JobPhase::Text, JobPhase::Summary]);
        }
        lanes
    }

    fn timeout(&self, phase: JobPhase) -> Duration {
        self.timeouts[match phase {
            JobPhase::Prepare => 0,
            JobPhase::Transcribe => 1,
            JobPhase::Text => 2,
            JobPhase::Summary => 3,
        }]
    }

    fn command(&self) -> Command {
        let mut command = Command::new(&self.exe);
        command.env_clear().envs(self.child_env.iter().cloned());
        command
    }

    fn preflight(&self, running: &AtomicBool) -> Result<()> {
        let mut command = self.command();
        command.arg("--worker-protocol");
        let out = localvox_light_process::run(
            command,
            None,
            Limits::new(Duration::from_secs(10)),
            || !running.load(Ordering::Relaxed),
            |_, _| {},
        )
        .with_context(|| format!("checking worker {}", self.exe.display()))?;
        let protocol = String::from_utf8_lossy(&out.stdout);
        anyhow::ensure!(
            out.termination.success() && protocol.trim() == jobs::WORKER_PROTOCOL,
            "worker {} is incompatible or failed to start: {:?}; expected protocol {:?}, received {:?}; {}. \
             Queue processing is disabled until the daemon is restarted with a compatible worker. \
             From source on Windows, run scripts/run.ps1 (-Profile dev for debug); \
             cargo run alone does not rebuild the worker. For an installed release, update the whole bundle.",
            self.exe.display(),
            out.termination,
            jobs::WORKER_PROTOCOL,
            protocol.trim().chars().take(100).collect::<String>(),
            String::from_utf8_lossy(&out.stderr).chars().take(500).collect::<String>()
        );
        tracing::info!(worker = %self.exe.display(), protocol = jobs::WORKER_PROTOCOL, "autocook: worker protocol verified");
        Ok(())
    }
}

pub fn spawn(work_dir: String, running: Arc<AtomicBool>) -> Option<std::thread::JoinHandle<()>> {
    spawn_inner(work_dir, running, None)
}

/// Measurement-only observer. The ordinary daemon uses `spawn` without one.
pub fn spawn_observed(
    work_dir: String,
    running: Arc<AtomicBool>,
    observer: Observer,
) -> Option<std::thread::JoinHandle<()>> {
    spawn_inner(work_dir, running, Some(observer))
}

fn spawn_inner(
    work_dir: String,
    running: Arc<AtomicBool>,
    observer: Option<Observer>,
) -> Option<std::thread::JoinHandle<()>> {
    if matches!(
        std::env::var("LOCALVOX_LIGHT_AUTOCOOK")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no"
    ) {
        jobs::report_coordinator(
            "disabled",
            "Автоматическая обработка выключена в настройках",
        );
        return None;
    }
    jobs::report_coordinator("starting", "Проверка обработчика");
    let mut cfg = match Config::from_env(work_dir) {
        Ok(cfg) => cfg,
        Err(e) => {
            jobs::report_coordinator(
                "blocked",
                "Координатор не запущен: проверьте комплект worker и настройки",
            );
            tracing::error!("autocook configuration: {e:#}");
            return None;
        }
    };
    cfg.observer = observer;
    match std::thread::Builder::new()
        .name("autocook".into())
        .spawn(move || {
            if let Err(e) = cfg.preflight(&running) {
                jobs::report_coordinator("blocked", "Обработчик не прошёл проверку протокола; обновите комплект и перезапустите движок");
                tracing::error!("autocook preflight: {e:#}");
                return;
            }
            let _status = CoordinatorExit(&running);
            coordinate(&cfg, &running);
        }) {
        Ok(handle) => Some(handle),
        Err(e) => {
            jobs::report_coordinator("blocked", "Не удалось запустить координатор обработки");
            tracing::error!("starting autocook coordinator: {e}");
            None
        }
    }
}

/// No resource lane exists until startup recovery has been accepted by the queue.
fn recover_before_start(work_dir: &Path, running: &AtomicBool, interval: Duration) -> bool {
    while running.load(Ordering::Relaxed) {
        let recovered = JobQueue::mutate(work_dir, |queue| {
            let count = queue.try_reclaim_abandoned(work_dir)?
                + queue.try_reconcile_cancellations(work_dir)?;
            queue.drop_orphans(work_dir);
            Ok::<_, anyhow::Error>(count)
        });
        match recovered {
            Ok(count) => {
                if count > 0 {
                    tracing::info!(taken = count, "autocook: interrupted jobs recovered");
                }
                return true;
            }
            Err(error) => {
                let message = format!("Не удалось сохранить восстановление очереди; запуск обработчиков отложен: {error:#}");
                tracing::error!("{message}");
                jobs::report_coordinator("blocked", &message);
                pause(running, interval);
            }
        }
    }
    false
}

fn coordinate(cfg: &Config, running: &AtomicBool) {
    let wd = &cfg.work_dir;
    if !recover_before_start(wd, running, cfg.interval) {
        return;
    }
    jobs::report_coordinator("ready", "Координатор работает");
    let lanes = cfg.lanes();
    tracing::info!(?lanes, timeouts_sec = ?cfg.timeouts.map(|t| t.as_secs()), "autocook: coordinator ready");
    // Startup index maintenance waits until queued processing is done as well.
    let index_dirty = AtomicBool::new(true);
    std::thread::scope(|scope| {
        for phases in &lanes {
            let index_dirty = &index_dirty;
            scope.spawn(move || {
                while running.load(Ordering::Relaxed) {
                    let ids = JobQueue::mutate(wd, |q| {
                        if !q.refresh_post_processing(jobs::post_processing_from_env()) {
                            return vec![];
                        }
                        q.pending_for(phases)
                    });
                    for id in ids {
                        if !running.load(Ordering::Relaxed) {
                            break;
                        }
                        let claim = JobQueue::mutate(wd, |q| {
                            if !q.refresh_post_processing(jobs::post_processing_from_env()) {
                                return None;
                            }
                            let name = q.jobs().iter().find(|j| j.id == id)?.session.clone();
                            match jobs::try_execution_lock(&wd.join("sessions").join(&name)) {
                                Ok(Some(lock)) => q.start_phase(id, phases).map(|job| {
                                    let lock = jobs::control::ManagedExecution::register(&wd.join("sessions").join(&name), &job, lock);
                                    if let Some(observer) = &cfg.observer { observer.emit("claimed", &job, &job); }
                                    (job, lock)
                                }),
                                Ok(None) => None,
                                Err(e) => {
                                    tracing::warn!(session = name, "worker ownership: {e}");
                                    None
                                }
                            }
                        });
                        if let Some((job, _execution_lock)) = claim {
                            let changed = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run_phase_controlled(cfg, running, &job, Some(&_execution_lock)))) {
                                Ok(changed) => changed,
                                Err(_) => {
                                    fail_phase_observed(wd, running, &job, "Сбой координатора при выполнении задания; сохранённые результаты остаются доступны", cfg.observer.as_ref());
                                    true
                                }
                            };
                            if _execution_lock.cancelled() {
                                finish_cancellation(cfg, running, &job);
                            }
                            if changed {
                                index_dirty.store(true, Ordering::Relaxed);
                            }
                        }
                    }
                    pause(running, cfg.interval);
                }
            });
        }
        let mut last_idle = Instant::now();
        while running.load(Ordering::Relaxed) {
            let maintenance = JobQueue::mutate(wd, |q| {
                q.drop_orphans(wd);
                for job in q
                    .jobs()
                    .iter()
                    .filter(|job| job.state == JobState::Cancelling)
                {
                    if let Some(owner) =
                        jobs::control::managed_owner(&wd.join("sessions").join(&job.session))
                            .filter(|owner| owner.matches(job))
                    {
                        owner.signal();
                    }
                }
                q.try_reconcile_cancellations(wd)?;
                q.try_revive_failed(Duration::from_secs(3600))
            });
            match maintenance {
                Ok(revived) => {
                    if revived > 0 {
                        tracing::info!(revived, "autocook: retrying after backoff");
                    }
                    jobs::report_coordinator("ready", "Координатор работает");
                }
                Err(error) => {
                    let message = format!("Автоматический повтор не сохранён; очередь будет проверена снова: {error:#}");
                    tracing::error!("{message}");
                    jobs::report_coordinator("blocked", &message);
                    pause(running, cfg.interval);
                    continue;
                }
            }
            let (summary, cleanup, _) = jobs::post_processing_from_env();
            let wanted = jobs::Wanted::new(
                summary,
                cleanup,
                cfg.summary_template.clone(),
                &cfg.llm_model,
            );
            discover_with(
                wd,
                || jobs::discovery::scan(wd, cfg.quiescent_sec, &wanted),
                jobs::post_processing_from_env,
            );
            let queue = JobQueue::load(wd);
            if let Some(error) = queue.error() {
                tracing::error!("autocook queue unavailable: {error}");
            }
            let idle = queue.error().is_none()
                && !queue.jobs().iter().any(|j| {
                    matches!(
                        j.state,
                        JobState::Running | JobState::Pending | JobState::Cancelling
                    )
                });
            if idle && index_dirty.swap(false, Ordering::Relaxed) && running.load(Ordering::Relaxed)
            {
                if !warm_indexes(cfg, running) {
                    index_dirty.store(true, Ordering::Relaxed);
                }
            }
            if idle && last_idle.elapsed().as_secs() >= 60 {
                let failed_jobs = queue
                    .jobs()
                    .iter()
                    .filter(|j| j.state == JobState::Failed)
                    .count();
                tracing::info!(
                    failed_jobs,
                    "autocook: нет выполняемых или ожидающих заданий"
                );
                last_idle = Instant::now();
            }
            pause(running, cfg.interval);
        }
    });
}

fn pause(running: &AtomicBool, duration: Duration) {
    let started = Instant::now();
    while running.load(Ordering::Relaxed) && started.elapsed() < duration {
        std::thread::sleep(Duration::from_millis(100));
    }
}

fn discover_with(
    work_dir: &Path,
    scan: impl FnOnce() -> Vec<jobs::discovery::Candidate>,
    policy: impl Fn() -> (bool, bool, bool),
) -> usize {
    let candidates = scan();
    candidates.into_iter().filter(|candidate| {
        let queued = JobQueue::mutate(work_dir, |queue| candidate.publish(queue, policy()));
        if queued { tracing::info!(session = candidate.session(), kind = ?candidate.kind(), "autocook: discovered work queued"); }
        queued
    }).count()
}

#[cfg(test)]
mod cancellation_tests;
#[cfg(test)]
mod discovery_tests;

#[cfg(test)]
fn run_phase(cfg: &Config, running: &AtomicBool, job: &Job) -> bool {
    run_phase_controlled(cfg, running, job, None)
}

fn run_phase_controlled(
    cfg: &Config,
    running: &AtomicBool,
    job: &Job,
    owner: Option<&jobs::control::ManagedExecution>,
) -> bool {
    run_step(cfg, running, job, true, cfg.timeout(job.phase), owner);
    // Even a failed attempt may have committed reusable artifacts. The queue
    // carries success/failure; this return value only dirties idle indexes.
    running.load(Ordering::Relaxed)
}

fn run_step(
    cfg: &Config,
    running: &AtomicBool,
    job: &Job,
    finish: bool,
    timeout: Duration,
    owner: Option<&jobs::control::ManagedExecution>,
) -> bool {
    if owner.is_some_and(|owner| owner.cancelled()) {
        return false;
    }
    let session = cfg.work_dir.join("sessions").join(&job.session);
    if job.phase == JobPhase::Prepare && job.attempts == 1 {
        progress::new_run(&session);
    }
    if (!job.force || job.phase != JobPhase::Transcribe)
        && jobs::verify_phase(&session, job).is_ok()
    {
        return if finish {
            accept_phase_observed(&cfg.work_dir, running, job, cfg.observer.as_ref())
        } else {
            true
        };
    }
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
    let mut _journal_operation = localvox_light_core::journal::Operation::start(
        &session,
        job.phase.as_str(),
        "localvox-process",
        Some(localvox_light_core::journal::DisplayCommand::from_args(
            "localvox-process",
            &cmd.get_args()
                .map(|v| v.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
        )),
    );
    if let Some(execution) = &job.execution_id {
        if let Err(error) = _journal_operation.bind_execution(execution) {
            tracing::debug!("journal execution binding: {error}");
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
                    if finish {
                        return accept_phase_observed(
                            &cfg.work_dir,
                            running,
                            job,
                            cfg.observer.as_ref(),
                        );
                    }
                    return match jobs::verify_phase(&session, job) {
                        Ok(()) => true,
                        Err(error) => {
                            fail_phase_observed(
                                &cfg.work_dir,
                                running,
                                job,
                                &format!("Результат этапа Text не подтверждён: {error:#}"),
                                cfg.observer.as_ref(),
                            );
                            false
                        }
                    };
                }
                // Interrupted jobs are recovered on restart and reuse verified artifacts.
                Termination::Cancelled => {
                    _journal_operation.cancelled();
                    return false;
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
    fail_phase_observed(&cfg.work_dir, running, job, &reason, cfg.observer.as_ref());
    false
}

/// The caller still owns the session's execution lock. Never hold the queue mutex while waiting.
fn retry_queue_commit(
    wd: &Path,
    running: &AtomicBool,
    job: &Job,
    mut commit: impl FnMut() -> anyhow::Result<()>,
) -> anyhow::Result<bool> {
    let session = wd.join("sessions").join(&job.session);
    let mut waiting = None;
    while running.load(Ordering::Relaxed) {
        match commit() {
            Ok(()) => return Ok(true),
            Err(error) if error.is::<jobs::CancellationRequested>() => return Ok(false),
            Err(error) if error.is::<jobs::QueueStorageError>() => {
                if waiting.is_none() {
                    let operation = localvox_light_core::journal::Operation::start(
                        &session,
                        job.phase.as_str(),
                        "Ожидание сохранения очереди",
                        None,
                    );
                    if let Some(execution) = &job.execution_id {
                        let _ = operation.bind_execution(execution);
                    }
                    operation.log("Хранилище очереди недоступно; повторяется только сохранение исхода, без нового запуска обработчика");
                    waiting = Some(operation);
                }
                tracing::error!(
                    job_id = job.id,
                    phase = job.phase.as_str(),
                    "Ожидание сохранения очереди: {error:#}"
                );
                pause(running, Duration::from_secs(1));
            }
            Err(error) => return Err(error),
        }
    }
    Ok(false)
}

fn finish_cancellation(cfg: &Config, running: &AtomicBool, owner: &Job) {
    let session = cfg.work_dir.join("sessions").join(&owner.session);
    let mut acknowledged = false;
    let result = retry_queue_commit(&cfg.work_dir, running, owner, || {
        JobQueue::mutate(&cfg.work_dir, |queue| {
            let before = queue.jobs().iter().find(|job| job.id == owner.id).cloned();
            if queue.try_complete_owned_cancellation(owner)? {
                acknowledged = true;
                let after = queue.jobs().iter().find(|job| job.id == owner.id).unwrap();
                if let Some(observer) = &cfg.observer {
                    observer.emit("cancelled", &before.unwrap(), after);
                }
            }
            Ok(())
        })
    });
    if acknowledged {
        if let Some(event) = progress::read(&session)
            .last()
            .filter(|event| event.state == progress::StageState::Running)
        {
            progress::mark(
                &session,
                event.stage,
                progress::StageState::Cancelled,
                Some("Обработка отменена; сохранённые результаты доступны"),
            );
        }
    }
    if let Err(error) = result {
        tracing::error!(job_id = owner.id, "stop acknowledgement: {error:#}");
    }
}

#[cfg(test)]
fn accept_phase(wd: &Path, running: &AtomicBool, job: &Job) -> bool {
    accept_phase_observed(wd, running, job, None)
}

fn accept_phase_observed(
    wd: &Path,
    running: &AtomicBool,
    job: &Job,
    observer: Option<&Observer>,
) -> bool {
    let result = retry_queue_commit(wd, running, job, || {
        JobQueue::mutate(wd, |q| {
            q.finish_phase_with_policy(job.id, job.phase, Some(jobs::post_processing_from_env()))?;
            if let Some(observer) = observer {
                observer.emit(
                    "accepted",
                    job,
                    q.jobs()
                        .iter()
                        .find(|current| current.id == job.id)
                        .unwrap(),
                );
            }
            Ok(())
        })
    });
    match result {
        Ok(true) => {
            tracing::info!(job_id = job.id, session = %job.session, phase = job.phase.as_str(), "autocook: артефакт подтверждён");
            true
        }
        Ok(false) => false,
        Err(e) => {
            fail_phase_observed(
                wd,
                running,
                job,
                &format!("Результат фазы не подтверждён: {e:#}"),
                observer,
            );
            false
        }
    }
}

#[cfg(test)]
fn fail_phase(wd: &Path, running: &AtomicBool, job: &Job, reason: &str) -> bool {
    fail_phase_observed(wd, running, job, reason, None)
}

fn fail_phase_observed(
    wd: &Path,
    running: &AtomicBool,
    job: &Job,
    reason: &str,
    observer: Option<&Observer>,
) -> bool {
    use progress::{Stage, StageState};
    match retry_queue_commit(wd, running, job, || {
        JobQueue::mutate(wd, |q| {
            q.try_mark_failed(job.id, reason, MAX_ATTEMPTS)?;
            if let Some(observer) = observer {
                observer.emit(
                    "failed",
                    job,
                    q.jobs()
                        .iter()
                        .find(|current| current.id == job.id)
                        .unwrap(),
                );
            }
            Ok(())
        })
    }) {
        Ok(true) => {}
        Ok(false) => return false,
        Err(error) => {
            tracing::error!(job_id = job.id, "Не удалось принять отказ этапа: {error:#}");
            return false;
        }
    }
    let session = wd.join("sessions").join(&job.session);
    let stages: &[Stage] = match job.phase {
        JobPhase::Prepare => &[Stage::Download, Stage::Extract],
        JobPhase::Transcribe => &[Stage::Transcribe],
        JobPhase::Text => &[Stage::Refine, Stage::Cleanup],
        JobPhase::Summary => &[Stage::Summary],
    };
    if let Some(event) = progress::read(&session)
        .iter()
        .rev()
        .find(|e| stages.contains(&e.stage))
    {
        if event.state == StageState::Running {
            progress::mark(&session, event.stage, StageState::Failed, Some(reason));
        }
    }
    tracing::warn!(job_id = job.id, session = %job.session, phase = job.phase.as_str(), attempt = job.attempts, "{reason}");
    true
}

fn clean_line(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.next() == Some('[') {
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
        } else if !c.is_control() {
            out.push(c);
        }
        if out.len() >= 2048 {
            break;
        }
    }
    out.trim().into()
}

fn error_tail(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    text.lines()
        .rev()
        .take(20)
        .find(|l| l.to_lowercase().contains("error"))
        .or_else(|| text.lines().rev().find(|l| !l.trim().is_empty()))
        .map(clean_line)
        .unwrap_or_else(|| "обработчик не сообщил причину".into())
}

fn warm_indexes(cfg: &Config, running: &AtomicBool) -> bool {
    let mut command = cfg.command();
    command
        .arg("--worker-index")
        .arg("--work-dir")
        .arg(&cfg.work_dir);
    let outcome = localvox_light_process::run(
        command,
        None,
        Limits::new(cfg.index_timeout),
        || {
            !running.load(Ordering::Relaxed)
                || JobQueue::load(&cfg.work_dir)
                    .jobs()
                    .iter()
                    .any(|j| matches!(j.state, JobState::Pending | JobState::Running))
        },
        |pid, elapsed| {
            tracing::info!(
                pid,
                elapsed_sec = elapsed.as_secs(),
                "autocook: updating search indexes"
            )
        },
    );
    match outcome {
        Ok(out) if matches!(out.termination, Termination::Cancelled) => false,
        Ok(out) if out.termination.success() => {
            tracing::info!("autocook: indexes updated");
            true
        }
        Ok(out) => {
            tracing::warn!(termination = ?out.termination, "index maintenance: {}", error_tail(&out.stderr));
            true // Retry after new processing or restart, not in a tight loop while Ollama is down.
        }
        Err(e) => {
            tracing::warn!("index maintenance: {e:#}");
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    static POST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn observer_uses_saved_job_clocks_without_error_or_document_content() {
        let _guard = POST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for valid in [true, false] {
            let (root, job) = outcome_fixture(valid);
            let (observer, events) = Observer::channel(4);
            let running = AtomicBool::new(true);
            assert!(if valid {
                accept_phase_observed(root.path(), &running, &job, Some(&observer))
            } else {
                fail_phase_observed(
                    root.path(),
                    &running,
                    &job,
                    "PRIVATE ERROR CONTENT",
                    Some(&observer),
                )
            });
            let event = events.try_recv().unwrap();
            let saved = JobQueue::load(root.path()).jobs()[0].clone();
            assert_eq!(event.kind, if valid { "accepted" } else { "failed" });
            assert_eq!(event.queued_at, job.phase_queued_at);
            assert_eq!(event.started_at, job.phase_started_at);
            assert_eq!(event.ended_at, saved.phase_queued_at);
            assert_eq!(event.execution_id, job.execution_id);
            assert_eq!(event.attempt, job.attempts);
            assert_eq!(event.phase, job.phase);
            assert_eq!(event.phase_after, saved.phase);
            assert_eq!(event.state_after, saved.state);
            assert_eq!(event.after_has_error, !valid);
            assert!(!event.as_json().to_string().contains("PRIVATE"));
            assert_eq!(observer.lost_count(), 0);
            assert!(events.try_recv().is_err());
        }
    }

    #[test]
    fn observer_overflow_does_not_wait_for_a_consumer_or_change_acceptance() {
        let _guard = POST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (root, job) = outcome_fixture(true);
        let (observer, events) = Observer::channel(1);
        observer.emit("claimed", &job, &job);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let helper = scope.spawn(|| {
                let accepted = accept_phase_observed(
                    root.path(),
                    &AtomicBool::new(true),
                    &job,
                    Some(&observer),
                );
                done_tx.send(accepted).unwrap();
            });
            let accepted_without_consumer = done_rx.recv_timeout(Duration::from_secs(2));
            // Release a mistakenly blocking implementation before failing this test.
            let _ = events.try_recv();
            helper.join().unwrap();
            assert_eq!(accepted_without_consumer.unwrap(), true);
        });
        assert_eq!(observer.lost_count(), 1);
        let saved = JobQueue::load(root.path()).jobs()[0].clone();
        assert_eq!(
            (saved.state, saved.phase),
            (JobState::Pending, JobPhase::Transcribe)
        );
    }

    #[cfg(windows)]
    #[test]
    fn observer_reports_no_outcome_until_its_queue_write_is_accepted() {
        let _guard = POST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for valid in [true, false] {
            let (root, job) = outcome_fixture(valid);
            let session = root.path().join("sessions/result");
            let (observer, events) = Observer::channel(4);
            let denied = deny_outcome_queue(root.path());
            let running = AtomicBool::new(true);
            std::thread::scope(|scope| {
                let _stop = StopRecovery(&running);
                let helper = scope.spawn(|| {
                    if valid {
                        accept_phase_observed(root.path(), &running, &job, Some(&observer))
                    } else {
                        fail_phase_observed(
                            root.path(),
                            &running,
                            &job,
                            "PRIVATE ERROR",
                            Some(&observer),
                        )
                    }
                });
                wait_for_outcome_journal(&session);
                assert!(matches!(
                    events.try_recv(),
                    Err(std::sync::mpsc::TryRecvError::Empty)
                ));
                drop(denied);
                assert!(helper.join().unwrap());
            });
            let event = events.try_recv().unwrap();
            assert_eq!(event.kind, if valid { "accepted" } else { "failed" });
            assert_eq!(observer.lost_count(), 0);
            assert!(events.try_recv().is_err());
        }
    }

    pub(super) fn outcome_fixture(valid: bool) -> (tempfile::TempDir, Job) {
        use localvox_light_core::versions::{VersionEntry, VersionStore};
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions/result");
        std::fs::create_dir_all(&session).unwrap();
        if valid {
            let store = VersionStore::open(&session).unwrap();
            let (id, path) = store.next_version("fixture").unwrap();
            std::fs::write(
                &path,
                "{\"source_id\":0,\"start_sec\":0,\"end_sec\":1,\"text\":\"Synthetic result\"}\n",
            )
            .unwrap();
            store
                .commit(VersionEntry {
                    id,
                    file: path.file_name().unwrap().to_string_lossy().into(),
                    label: "fixture".into(),
                    model: "fixture".into(),
                    params: serde_json::json!({}),
                    created_at: "2026-11-01T00:00:00Z".into(),
                    parents: vec![],
                })
                .unwrap();
            localvox_light_core::processing::record(
                &session,
                localvox_light_core::processing::TRANSCRIPT,
                "fixture",
                localvox_light_core::processing::Outcome::Ok,
                None,
                Some(id),
            )
            .unwrap();
        }
        progress::new_run(&session);
        progress::mark(
            &session,
            progress::Stage::Download,
            progress::StageState::Running,
            None,
        );
        let job = JobQueue::mutate(root.path(), |queue| {
            assert!(queue.enqueue_cook("result", false, false, false));
            queue.start(0).unwrap()
        });
        (root, job)
    }

    #[cfg(windows)]
    fn deny_outcome_queue(root: &Path) -> std::fs::File {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(root.join("jobs.json"))
            .unwrap()
    }

    #[cfg(windows)]
    fn wait_for_outcome_journal(session: &Path) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            // Windows append briefly locks this same file. Poll after that lock as well.
            if localvox_light_core::journal::read(
                session,
                &localvox_light_core::journal::Query {
                    limit: 200,
                    ..Default::default()
                },
            )
            .is_ok_and(|page| {
                page.events.iter().any(|entry| {
                    entry.kind == "operation_start" && entry.label == "Ожидание сохранения очереди"
                })
            }) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "queue save wait was not journaled"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(windows)]
    #[test]
    fn outcome_commit_waits_for_storage_then_accepts_the_same_artifact_without_failure() {
        let _guard = POST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (root, job) = outcome_fixture(true);
        let session = root.path().join("sessions/result");
        let before = std::fs::read(root.path().join("jobs.json")).unwrap();
        let deny = deny_outcome_queue(root.path());
        let running = AtomicBool::new(true);
        let _lease = jobs::try_execution_lock(&session).unwrap().unwrap();
        std::thread::scope(|scope| {
            let _stop = StopRecovery(&running);
            let helper = scope.spawn(|| accept_phase(root.path(), &running, &job));
            wait_for_outcome_journal(&session);
            assert!(!helper.is_finished());
            assert_eq!(
                std::fs::read(root.path().join("jobs.json")).unwrap(),
                before
            );
            // A reader can take the queue mutex while the outcome is waiting.
            assert_eq!(JobQueue::mutate(root.path(), |q| q.jobs()[0].attempts), 1);
            assert!(jobs::try_execution_lock(&session).unwrap().is_none());
            drop(deny);
            assert!(helper.join().unwrap());
        });
        let queue = JobQueue::load(root.path());
        let accepted = &queue.jobs()[0];
        assert_eq!(
            (accepted.state, accepted.phase, accepted.attempts),
            (JobState::Pending, JobPhase::Transcribe, 0)
        );
        assert_eq!(accepted.execution_id, job.execution_id);
        assert!(accepted.last_error.is_none());
        assert!(progress::read(&session)
            .iter()
            .all(|event| event.state != progress::StageState::Failed));
        let events = localvox_light_core::journal::read(
            &session,
            &localvox_light_core::journal::Query {
                limit: 200,
                ..Default::default()
            },
        )
        .unwrap()
        .events;
        for kind in ["operation_start", "operation_end"] {
            assert_eq!(
                events
                    .iter()
                    .filter(
                        |event| event.kind == kind && event.label == "Ожидание сохранения очереди"
                    )
                    .count(),
                1
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn outcome_failure_is_journaled_only_after_the_queue_accepts_its_original_reason() {
        let _guard = POST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (root, job) = outcome_fixture(false);
        let session = root.path().join("sessions/result");
        let before = std::fs::read(root.path().join("jobs.json")).unwrap();
        let deny = deny_outcome_queue(root.path());
        let running = AtomicBool::new(true);
        std::thread::scope(|scope| {
            let _stop = StopRecovery(&running);
            let helper =
                scope.spawn(|| fail_phase(root.path(), &running, &job, "worker timed out"));
            wait_for_outcome_journal(&session);
            assert!(!helper.is_finished());
            assert_eq!(
                std::fs::read(root.path().join("jobs.json")).unwrap(),
                before
            );
            assert_eq!(
                progress::read(&session).last().unwrap().state,
                progress::StageState::Running
            );
            drop(deny);
            assert!(helper.join().unwrap());
        });
        let queue = JobQueue::load(root.path());
        let failed = &queue.jobs()[0];
        assert_eq!(
            (failed.state, failed.phase, failed.attempts, failed.revivals),
            (JobState::Pending, job.phase, 1, 0)
        );
        assert_eq!(failed.last_error.as_deref(), Some("worker timed out"));
        let events = progress::read(&session);
        assert_eq!(
            events
                .iter()
                .filter(|event| event.state == progress::StageState::Failed)
                .count(),
            1
        );
        assert_eq!(
            events.last().unwrap().note.as_deref(),
            Some("worker timed out")
        );
    }

    #[cfg(windows)]
    #[test]
    fn outcome_commit_can_stop_without_accepting_success_or_failure() {
        let _guard = POST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        for success in [true, false] {
            let (root, job) = outcome_fixture(success);
            let session = root.path().join("sessions/result");
            let before = std::fs::read(root.path().join("jobs.json")).unwrap();
            let _deny = deny_outcome_queue(root.path());
            let running = AtomicBool::new(true);
            std::thread::scope(|scope| {
                let _stop = StopRecovery(&running);
                let helper = scope.spawn(|| {
                    if success {
                        accept_phase(root.path(), &running, &job)
                    } else {
                        fail_phase(root.path(), &running, &job, "not yet saved")
                    }
                });
                wait_for_outcome_journal(&session);
                running.store(false, Ordering::Relaxed);
                assert!(!helper.join().unwrap());
            });
            assert_eq!(
                std::fs::read(root.path().join("jobs.json")).unwrap(),
                before
            );
            assert_eq!(
                JobQueue::load(root.path()).jobs()[0].state,
                JobState::Running
            );
            assert_eq!(
                progress::read(&session).last().unwrap().state,
                progress::StageState::Running
            );
        }
    }

    #[test]
    fn outcome_verification_failure_does_not_wait_for_storage_or_accept_the_phase() {
        let _guard = POST_ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let (root, job) = outcome_fixture(false);
        assert!(!accept_phase(root.path(), &AtomicBool::new(true), &job));
        let queue = JobQueue::load(root.path());
        assert_eq!(
            (
                queue.jobs()[0].state,
                queue.jobs()[0].phase,
                queue.jobs()[0].attempts
            ),
            (JobState::Pending, job.phase, 1)
        );
        assert!(queue.jobs()[0]
            .last_error
            .as_deref()
            .unwrap()
            .contains("Результат фазы не подтверждён"));
        let session = root.path().join("sessions/result");
        assert_eq!(
            progress::read(&session).last().unwrap().state,
            progress::StageState::Failed
        );
        assert!(localvox_light_core::journal::read(
            &session,
            &localvox_light_core::journal::Query {
                limit: 200,
                ..Default::default()
            }
        )
        .unwrap()
        .events
        .iter()
        .all(|entry| entry.kind != "operation_start"));
    }

    #[cfg(windows)]
    fn recovery_commit_fixture() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(root.path().join("sessions/resume")).unwrap();
        JobQueue::mutate(root.path(), |queue| {
            assert!(queue.requeue_derivatives("resume", true, false, false));
            queue.start_phase(0, &[JobPhase::Summary]).unwrap();
        });
        root
    }

    #[cfg(windows)]
    fn wait_for_blocked_recovery() {
        let deadline = Instant::now() + Duration::from_secs(5);
        while !jobs::coordinator_status().is_some_and(|state| state.state == "blocked") {
            assert!(
                Instant::now() < deadline,
                "recovery did not report its queue failure"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[cfg(windows)]
    struct StopRecovery<'a>(&'a AtomicBool);
    #[cfg(windows)]
    impl Drop for StopRecovery<'_> {
        fn drop(&mut self) {
            self.0.store(false, Ordering::Relaxed);
        }
    }

    #[cfg(windows)]
    #[test]
    fn startup_recovery_waits_for_a_saved_transition_before_returning_ready() {
        use std::os::windows::fs::OpenOptionsExt;
        let _guard = POST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = recovery_commit_fixture();
        let path = root.path().join("jobs.json");
        let before = std::fs::read(&path).unwrap();
        let deny = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let running = AtomicBool::new(true);
        jobs::report_coordinator("starting", "test recovery");
        std::thread::scope(|scope| {
            let _stop = StopRecovery(&running);
            let helper = scope
                .spawn(|| recover_before_start(root.path(), &running, Duration::from_millis(10)));
            wait_for_blocked_recovery();
            assert!(!helper.is_finished());
            assert_eq!(std::fs::read(&path).unwrap(), before);
            assert_eq!(
                JobQueue::load(root.path()).jobs()[0].state,
                JobState::Running
            );
            assert!(!root.path().join("sessions/resume/progress.jsonl").exists());
            drop(deny);
            assert!(helper.join().unwrap());
        });
        let queue = JobQueue::load(root.path());
        assert_eq!(
            (
                queue.jobs()[0].state,
                queue.jobs()[0].phase,
                queue.jobs()[0].attempts
            ),
            (JobState::Pending, JobPhase::Summary, 1)
        );
    }

    #[cfg(windows)]
    #[test]
    fn startup_recovery_can_stop_while_queue_replacement_is_denied() {
        use std::os::windows::fs::OpenOptionsExt;
        let _guard = POST_ENV_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let root = recovery_commit_fixture();
        let path = root.path().join("jobs.json");
        let before = std::fs::read(&path).unwrap();
        let _deny = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(&path)
            .unwrap();
        let running = AtomicBool::new(true);
        jobs::report_coordinator("starting", "test recovery stop");
        std::thread::scope(|scope| {
            let _stop = StopRecovery(&running);
            let helper = scope
                .spawn(|| recover_before_start(root.path(), &running, Duration::from_millis(10)));
            wait_for_blocked_recovery();
            running.store(false, Ordering::Relaxed);
            assert!(!helper.join().unwrap());
        });
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert!(!root.path().join("sessions/resume/progress.jsonl").exists());
    }
    #[test]
    fn verified_gigaam_finishes_without_starting_a_postprocessing_worker_when_disabled() {
        let _guard = POST_ENV_LOCK.lock().unwrap();
        use localvox_light_core::{
            processing,
            versions::{VersionEntry, VersionStore},
        };
        struct Restore(Vec<(String, Option<String>)>);
        impl Drop for Restore {
            fn drop(&mut self) {
                localvox_light_core::settings::apply_live(&self.0);
            }
        }
        let keys: Vec<_> = ["SUMMARY", "CLEANUP", "REFINE"]
            .map(|s| format!("LOCALVOX_LIGHT_AUTOCOOK_{s}"))
            .into();
        let _restore = Restore(
            keys.iter()
                .map(|key| (key.clone(), std::env::var(key).ok()))
                .collect(),
        );
        localvox_light_core::settings::apply_live(
            &keys
                .into_iter()
                .map(|key| {
                    let value = if key.ends_with("SUMMARY") {
                        "off"
                    } else {
                        "on"
                    };
                    (key, Some(value.into()))
                })
                .collect::<Vec<_>>(),
        );
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions/two-hours");
        std::fs::create_dir_all(&session).unwrap();
        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("gigaam").unwrap();
        std::fs::write(&path, serde_json::json!({"source_id":0,"start_sec":0,"end_sec":7200,"text":"Synthetic long recording"}).to_string()+"\n").unwrap();
        store
            .commit(VersionEntry {
                id,
                file: path.file_name().unwrap().to_string_lossy().into(),
                label: "GigaAM".into(),
                model: "synthetic".into(),
                params: serde_json::json!({}),
                created_at: "2026-10-31T00:00:00Z".into(),
                parents: vec![],
            })
            .unwrap();
        processing::record(
            &session,
            processing::TRANSCRIPT,
            "fixture",
            processing::Outcome::Ok,
            None,
            Some(id),
        )
        .unwrap();
        // A non-executable file makes any accidental child startup fail this test.
        std::fs::write(
            root.path().join(if cfg!(windows) {
                "localvox-process.exe"
            } else {
                "localvox-process"
            }),
            [],
        )
        .unwrap();
        let config =
            Config::from_env_in(root.path().to_string_lossy().into(), root.path()).unwrap();
        let job = JobQueue::mutate(root.path(), |q| {
            q.enqueue_cook("two-hours", true, true, true);
            let mut job = q.start_phase(0, &[JobPhase::Prepare]).unwrap();
            q.finish_phase(0, job.phase).unwrap();
            job = q.start_phase(0, &[JobPhase::Transcribe]).unwrap();
            job
        });
        assert!(run_phase(&config, &AtomicBool::new(true), &job));
        let queue = JobQueue::load(root.path());
        assert_eq!(queue.jobs()[0].state, JobState::Done);
        assert!(!queue.jobs()[0].summary && !queue.jobs()[0].cleanup && !queue.jobs()[0].refine);
        assert!(path.is_file());
        assert!(!session.join("summary.md").exists());
    }
    #[test]
    fn saving_a_model_reports_restart_while_an_existing_coordinator_keeps_its_snapshot() {
        const KEY: &str = "LOCALVOX_LLM_MODEL";
        struct Restore(Option<std::ffi::OsString>);
        impl Drop for Restore {
            fn drop(&mut self) {
                match &self.0 {
                    Some(value) => std::env::set_var(KEY, value),
                    None => std::env::remove_var(KEY),
                }
            }
        }
        let _restore = Restore(std::env::var_os(KEY));
        let work = tempfile::tempdir().unwrap();
        std::fs::write(
            work.path().join(if cfg!(windows) {
                "localvox-process.exe"
            } else {
                "localvox-process"
            }),
            [],
        )
        .unwrap();
        std::env::set_var(KEY, "synthetic-before");
        let before =
            Config::from_env_in(work.path().to_string_lossy().into_owned(), work.path()).unwrap();
        let restart = localvox_light_core::settings::apply_live(&[(
            KEY.into(),
            Some("synthetic-after".into()),
        )]);
        assert!(!restart.is_empty());
        assert_eq!(before.llm_model, "synthetic-before");
        let command = before.command();
        assert_eq!(
            command.get_envs().find(|(key, _)| *key == KEY).unwrap().1,
            Some(std::ffi::OsStr::new("synthetic-before"))
        );
        let restarted =
            Config::from_env_in(work.path().to_string_lossy().into_owned(), work.path()).unwrap();
        assert_eq!(restarted.llm_model, "synthetic-after");
        assert_eq!(
            restarted
                .command()
                .get_envs()
                .find(|(key, _)| *key == KEY)
                .unwrap()
                .1,
            Some(std::ffi::OsStr::new("synthetic-after"))
        );
    }
    #[test]
    fn diagnostics_are_bounded_and_plain_text() {
        assert_eq!(clean_line("\u{1b}[32mготово\u{1b}[0m"), "готово");
        assert!(clean_line(&"x".repeat(100_000)).len() <= 2048);
        assert_eq!(
            error_tail(b"progress\nError: model missing\nshutdown\n"),
            "Error: model missing"
        );
    }
}
