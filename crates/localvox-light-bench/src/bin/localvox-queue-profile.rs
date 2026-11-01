//! Run the production coordinator only in an explicitly isolated profiling workspace.
use anyhow::{ensure, Context, Result};
use clap::Parser;
use localvox_light::autocook::{self, Observer, Transition};
use localvox_light_core::jobs::{self, Job, JobQueue, JobState};
use serde_json::{json, Value};
use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

const MARKER: &str = ".localvox-profile-workspace";
const REPORT: &str = "queue-report.json";
const LOCK: &str = ".queue-profile.lock";

#[derive(Parser)]
#[command(about = "Run the real coordinator without capture/API in a marked profiling workspace")]
struct Args {
    #[arg(long)]
    work_dir: PathBuf,
    /// Must be the same sibling executable the production coordinator resolves.
    #[arg(long)]
    worker: PathBuf,
    #[arg(long, default_value_t = 900)]
    timeout_sec: u64,
}

fn worker_name() -> &'static str {
    if cfg!(windows) {
        "localvox-process.exe"
    } else {
        "localvox-process"
    }
}

fn validate(root: &Path, worker: &Path, runner: &Path) -> Result<()> {
    let canonical_root = root.canonicalize()?;
    let root = canonical_root.as_path();
    ensure!(
        fs::read_to_string(root.join(MARKER)).ok().as_deref() == Some("localvox-profile/v1\n"),
        "work directory is not an isolated profiling workspace"
    );
    ensure!(
        !root.join(jobs::RECORDING_MARKER).try_exists()?,
        "recording marker present"
    );
    ensure!(
        !root.join(REPORT).try_exists()?,
        "queue report already exists; use a new workspace"
    );
    let expected = runner
        .parent()
        .context("runner directory")?
        .join(worker_name())
        .canonicalize()
        .context("coordinator sibling worker missing")?;
    ensure!(
        worker.canonicalize()? == expected,
        "worker must match the coordinator sibling"
    );
    let queue = JobQueue::load(root);
    ensure!(queue.error().is_none(), "queue unreadable");
    // The marker is not permission to traverse session names outside the test workspace.
    for job in queue.jobs() {
        ensure!(
            matches!(
                Path::new(&job.session)
                    .components()
                    .collect::<Vec<_>>()
                    .as_slice(),
                [std::path::Component::Normal(_)]
            ),
            "unsafe session path"
        );
        let session = root
            .join("sessions")
            .join(&job.session)
            .canonicalize()
            .context("profiling session missing")?;
        ensure!(
            session.starts_with(root.join("sessions").canonicalize()?) && session.starts_with(root),
            "session escapes profiling workspace"
        );
        ensure!(
            !matches!(job.state, JobState::Running | JobState::Cancelling),
            "workspace already has a running job"
        );
    }
    Ok(())
}

struct WorkspaceLock(PathBuf);
impl WorkspaceLock {
    fn acquire(root: &Path) -> Result<Self> {
        let path = root.join(LOCK);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .context("profiling workspace already claimed; inspect before removing a stale lock")?;
        use std::io::Write;
        writeln!(file, "{}", std::process::id())?;
        Ok(Self(path))
    }
}
impl Drop for WorkspaceLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn metadata(job: &Job) -> Value {
    json!({"id":job.id,"session":job.session,"kind":job.kind,"phase":job.phase,
        "state":job.state,"attempt":job.attempts,"execution_id":job.execution_id,
        "created_at":job.created_at,"enqueued_at":job.enqueued_at,"started_at":job.started_at,
        "queued_at":job.phase_queued_at,"phase_started_at":job.phase_started_at,
        "finished_at":job.finished_at,"has_error":job.last_error.is_some()})
}

fn elapsed(start: Option<&str>, end: Option<&str>) -> Option<f64> {
    let start = chrono::DateTime::parse_from_rfc3339(start?).ok()?;
    let end = chrono::DateTime::parse_from_rfc3339(end?).ok()?;
    let millis = (end - start).num_milliseconds();
    (millis >= 0).then_some(millis as f64 / 1000.0)
}

fn transition(event: Transition) -> Value {
    let mut value = event.as_json();
    value["queue_wait_sec"] = json!(elapsed(
        event.queued_at.as_deref(),
        event.started_at.as_deref()
    ));
    value["phase_wall_sec"] = json!(elapsed(
        event.started_at.as_deref(),
        event.ended_at.as_deref()
    ));
    value
}

fn write_report(root: &Path, report: &Value) -> Result<()> {
    // Intermediate snapshots are explicitly incomplete; a final file is published after join.
    fs::write(
        root.join("queue-report.pending.json"),
        serde_json::to_vec_pretty(report)?,
    )?;
    Ok(())
}

fn run(args: Args, running: Arc<AtomicBool>) -> Result<PathBuf> {
    ensure!(args.timeout_sec > 0, "timeout must be positive");
    let root = args.work_dir.canonicalize()?;
    let _claim = WorkspaceLock::acquire(&root)?;
    validate(&root, &args.worker, &std::env::current_exe()?)?;
    let initial = JobQueue::load(&root)
        .jobs()
        .iter()
        .map(metadata)
        .collect::<Vec<_>>();
    let (observer, events) = Observer::channel(4096);
    let mut report = json!({"version":1,"scope":"isolated_production_coordinator",
        "complete":false,"all_success":false,"events_lost":0,"events":[],"initial_jobs":initial});
    write_report(&root, &report)?;
    let started = Instant::now();
    let handle = autocook::spawn_observed(
        root.to_string_lossy().into_owned(),
        running.clone(),
        observer.clone(),
    )
    .context("coordinator disabled or could not start")?;
    let mut checkpoint = Instant::now();
    let mut report_error = None;
    let reason = loop {
        for event in events.try_iter() {
            report["events"]
                .as_array_mut()
                .unwrap()
                .push(transition(event));
        }
        let queue = JobQueue::load(&root);
        if queue.error().is_some() {
            break "queue_unreadable";
        }
        let state = jobs::coordinator_status().map(|s| s.state);
        if state.as_deref() == Some("ready")
            && queue.jobs().iter().all(|j| {
                matches!(
                    j.state,
                    JobState::Done | JobState::Failed | JobState::Cancelled
                )
            })
        {
            break "terminal";
        }
        if handle.is_finished() {
            break "coordinator_stopped";
        }
        if !running.load(Ordering::Relaxed) {
            break "cancelled";
        }
        if started.elapsed() >= Duration::from_secs(args.timeout_sec) {
            break "timeout";
        }
        if checkpoint.elapsed() >= Duration::from_secs(1) {
            report["elapsed_sec"] = json!(started.elapsed().as_secs_f64());
            report["events_lost"] = json!(observer.lost_count());
            if let Err(error) = write_report(&root, &report) {
                report_error = Some(error);
                break "report_write_failed";
            }
            checkpoint = Instant::now();
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    running.store(false, Ordering::Relaxed);
    let joined = handle.join().is_ok();
    for event in events.try_iter() {
        report["events"]
            .as_array_mut()
            .unwrap()
            .push(transition(event));
    }
    let queue = JobQueue::load(&root);
    report["jobs"] = json!(queue.jobs().iter().map(metadata).collect::<Vec<_>>());
    report["reason"] = json!(reason);
    report["coordinator_joined"] = json!(joined);
    report["events_lost"] = json!(observer.lost_count());
    report["elapsed_sec"] = json!(started.elapsed().as_secs_f64());
    report["complete"] = json!(
        reason == "terminal" && joined && queue.error().is_none() && observer.lost_count() == 0
    );
    report["all_success"] =
        json!(report["complete"] == true && queue.jobs().iter().all(|j| j.state == JobState::Done));
    write_report(&root, &report)?;
    fs::rename(root.join("queue-report.pending.json"), root.join(REPORT))?;
    if let Some(error) = report_error {
        return Err(error);
    }
    ensure!(
        report["all_success"] == true,
        "queue profile incomplete/failed; see queue-report.json"
    );
    Ok(root.join(REPORT))
}

fn main() -> Result<()> {
    let running = Arc::new(AtomicBool::new(true));
    let signal = running.clone();
    ctrlc::set_handler(move || signal.store(false, Ordering::Relaxed))?;
    println!("{}", run(Args::parse(), running)?.display());
    Ok(())
}

#[cfg(test)]
#[path = "queue_profile/tests.rs"]
mod tests;
