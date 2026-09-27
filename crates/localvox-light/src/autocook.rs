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

/// Resolve environment once at the composition boundary, before any job is claimed.
struct Config {
    work_dir: PathBuf,
    exe: PathBuf,
    model_dir: PathBuf,
    ffmpeg: PathBuf,
    interval: Duration,
    quiescent_sec: u64,
    summary: bool,
    cleanup: bool,
    refine: bool,
    llm_model: String,
    summary_template: Option<String>,
    summary_provider: String,
    timeouts: [Duration; 4],
    index_timeout: Duration,
    child_env: Vec<(std::ffi::OsString, std::ffi::OsString)>,
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
        let cwd = std::env::current_dir()?;
        let absolute = |path: PathBuf| {
            if path.is_absolute() {
                path
            } else {
                cwd.join(path)
            }
        };
        let exe_dir = std::env::current_exe()?
            .parent()
            .context("executable directory")?
            .to_path_buf();
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
        let (summary, cleanup, refine) = jobs::post_processing_from_env();
        Ok(Self {
            work_dir: absolute(work_dir.into()),
            exe,
            model_dir: absolute(model_dir),
            ffmpeg: localvox_light_core::chunks::resolve_ffmpeg_for_decode().into(),
            interval: Duration::from_secs(seconds("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC", 5)?),
            quiescent_sec: seconds("LOCALVOX_LIGHT_AUTOCOOK_QUIESCENT_SEC", 15)?,
            summary,
            cleanup,
            refine,
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
    if matches!(
        std::env::var("LOCALVOX_LIGHT_AUTOCOOK")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no"
    ) {
        return None;
    }
    let cfg = match Config::from_env(work_dir) {
        Ok(cfg) => cfg,
        Err(e) => {
            tracing::error!("autocook configuration: {e:#}");
            return None;
        }
    };
    match std::thread::Builder::new()
        .name("autocook".into())
        .spawn(move || {
            if let Err(e) = cfg.preflight(&running) {
                tracing::error!("autocook preflight: {e:#}");
                return;
            }
            coordinate(&cfg, &running);
        }) {
        Ok(handle) => Some(handle),
        Err(e) => {
            tracing::error!("starting autocook coordinator: {e}");
            None
        }
    }
}

fn coordinate(cfg: &Config, running: &AtomicBool) {
    let wd = &cfg.work_dir;
    JobQueue::mutate(wd, |q| {
        let taken = q.reclaim_abandoned(wd);
        if taken > 0 {
            tracing::info!(taken, "autocook: interrupted jobs recovered");
        }
        q.drop_orphans(wd);
    });
    let wanted = jobs::Wanted::new(
        cfg.summary,
        cfg.cleanup,
        cfg.summary_template.clone(),
        &cfg.llm_model,
    );
    let lanes = cfg.lanes();
    tracing::info!(?lanes, timeouts_sec = ?cfg.timeouts.map(|t| t.as_secs()), "autocook: coordinator ready");
    // Startup index maintenance waits until queued processing is done as well.
    let index_dirty = AtomicBool::new(true);
    std::thread::scope(|scope| {
        for phases in &lanes {
            let index_dirty = &index_dirty;
            scope.spawn(move || {
                while running.load(Ordering::Relaxed) {
                    let ids = JobQueue::mutate(wd, |q| q.pending_for(phases));
                    for id in ids {
                        if !running.load(Ordering::Relaxed) {
                            break;
                        }
                        let claim = JobQueue::mutate(wd, |q| {
                            let name = q.jobs().iter().find(|j| j.id == id)?.session.clone();
                            match jobs::try_execution_lock(&wd.join("sessions").join(&name)) {
                                Ok(Some(lock)) => q.start_phase(id, phases).map(|job| (job, lock)),
                                Ok(None) => None,
                                Err(e) => {
                                    tracing::warn!(session = name, "worker ownership: {e}");
                                    None
                                }
                            }
                        });
                        if let Some((job, _execution_lock)) = claim {
                            if run_phase(cfg, running, &job) {
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
            JobQueue::mutate(wd, |q| {
                q.drop_orphans(wd);
                let revived = q.revive_failed(Duration::from_secs(3600));
                if revived > 0 {
                    tracing::info!(revived, "autocook: retrying after backoff");
                }
                for name in jobs::sessions_needing_cook(wd, cfg.quiescent_sec) {
                    if q.enqueue_cook(&name, cfg.summary, cfg.cleanup, cfg.refine) {
                        tracing::info!(session = name, "autocook: enqueued");
                    }
                }
                for name in jobs::sessions_needing_artifacts(wd, &wanted) {
                    if q.requeue_for_artifacts(&name, cfg.summary, cfg.cleanup, cfg.refine) {
                        tracing::info!(session = name, "autocook: queued missing artifacts");
                    }
                }
            });
            let queue = JobQueue::load(wd);
            if let Some(error) = queue.error() {
                tracing::error!("autocook queue unavailable: {error}");
            }
            let idle = queue.error().is_none()
                && !queue
                    .jobs()
                    .iter()
                    .any(|j| matches!(j.state, JobState::Running | JobState::Pending));
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

fn run_phase(cfg: &Config, running: &AtomicBool, job: &Job) -> bool {
    let session = cfg.work_dir.join("sessions").join(&job.session);
    if job.phase == JobPhase::Prepare && job.attempts == 1 {
        progress::new_run(&session);
    }
    if (!job.force || job.phase != JobPhase::Transcribe)
        && jobs::verify_phase(&session, job).is_ok()
    {
        return accept_phase(&cfg.work_dir, job);
    }
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
    let timeout = cfg.timeout(job.phase);
    let outcome = localvox_light_process::run(
        cmd,
        None,
        Limits::new(timeout),
        || !running.load(Ordering::Relaxed),
        |pid, elapsed| {
            let events = progress::read(&session);
            let event = events.last();
            tracing::info!(job_id = job.id, session = %job.session, phase = job.phase.as_str(), attempt = job.attempts,
                pid, elapsed_sec = elapsed.as_secs(), timeout_sec = timeout.as_secs(),
                operation = event.and_then(|e| e.note.as_deref()).unwrap_or("запуск обработчика"),
                last_signal = event.map(|e| e.at.as_str()).unwrap_or("нет"), "autocook: heartbeat");
        },
    );
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
                    return accept_phase(&cfg.work_dir, job)
                }
                // Interrupted jobs are recovered on restart and reuse verified artifacts.
                Termination::Cancelled => return false,
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
    fail_phase(&cfg.work_dir, job, &reason);
    true // A failed phase may still have committed a reusable artifact.
}

fn accept_phase(wd: &Path, job: &Job) -> bool {
    let result = JobQueue::mutate(wd, |q| q.finish_phase(job.id, job.phase));
    match result {
        Ok(()) => {
            tracing::info!(job_id = job.id, session = %job.session, phase = job.phase.as_str(), "autocook: артефакт подтверждён");
            true
        }
        Err(e) => {
            fail_phase(wd, job, &format!("Результат фазы не подтверждён: {e:#}"));
            false
        }
    }
}

fn fail_phase(wd: &Path, job: &Job, reason: &str) {
    use progress::{Stage, StageState};
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
    JobQueue::mutate(wd, |q| q.mark_failed(job.id, reason, MAX_ATTEMPTS));
    tracing::warn!(job_id = job.id, session = %job.session, phase = job.phase.as_str(), attempt = job.attempts, "{reason}");
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
