//! One sequential resident per resource lane. It owns no session lease when idle.
use super::*;
use localvox_light_core::jobs::asr_worker::{Reply, Request};
use localvox_light_process::StdioSession;

#[derive(Default)]
pub(super) struct Lane {
    process: Option<StdioSession>,
    last_used: Option<Instant>,
    sequence: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "subprocess helper; spawned only by eviction test"]
    fn idle_worker_helper() {
        std::thread::sleep(Duration::from_secs(30));
    }

    #[test]
    fn keep_alive_zero_idle_expiry_and_shutdown_release_the_owned_worker() {
        for (keep_alive, age, evicted) in [
            (Duration::ZERO, Duration::ZERO, true),
            (Duration::from_secs(1), Duration::from_secs(2), true),
            (Duration::from_secs(600), Duration::ZERO, false),
        ] {
            let mut command = Command::new(std::env::current_exe().unwrap());
            command.args([
                "--exact",
                "autocook::warm::tests::idle_worker_helper",
                "--ignored",
            ]);
            let mut lane = Lane {
                process: Some(StdioSession::spawn(command).unwrap()),
                last_used: Some(Instant::now() - age),
                sequence: 0,
            };
            lane.evict(keep_alive);
            assert_eq!(lane.process.is_none(), evicted);
            if !evicted {
                assert!(!lane.process.as_mut().unwrap().has_exited().unwrap());
                lane.clear();
                assert!(lane.process.is_none() && lane.last_used.is_none());
            }
        }
    }
}
impl Lane {
    pub(super) fn clear(&mut self) {
        self.process = None;
        self.last_used = None;
    }
    pub(super) fn evict(&mut self, keep_alive: Duration) {
        if keep_alive.is_zero()
            || self.last_used.is_some_and(|at| at.elapsed() >= keep_alive)
            || self
                .process
                .as_mut()
                .is_some_and(|p| p.has_exited().unwrap_or(true))
        {
            self.clear();
        }
    }
    pub(super) fn run(
        &mut self,
        cfg: &Config,
        running: &AtomicBool,
        job: &Job,
        owner: Option<&jobs::control::ManagedExecution>,
        timeout: Duration,
    ) -> Result<()> {
        let result = self.exchange(cfg, running, job, owner, timeout);
        if result.is_err() {
            self.clear();
        }
        result
    }
    fn exchange(
        &mut self,
        cfg: &Config,
        running: &AtomicBool,
        job: &Job,
        owner: Option<&jobs::control::ManagedExecution>,
        timeout: Duration,
    ) -> Result<()> {
        self.evict(cfg.asr_keep_alive);
        if self.process.is_none() {
            let mut command = cfg.command();
            command
                .arg("--worker-serve")
                .arg("--work-dir")
                .arg(&cfg.work_dir)
                .env("LOCALVOX_LIGHT_YT_FFMPEG", &cfg.ffmpeg);
            self.process = Some(StdioSession::spawn(command)?);
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .context("ASR request id exhausted")?;
        let session = cfg.work_dir.join("sessions").join(&job.session);
        let mut request = Request::new(self.sequence, session.clone(), cfg.model_dir.clone());
        request.prefix = job.kind == jobs::JobKind::Incremental;
        request.input = if request.prefix {
            job.prefix.as_ref().map(|p| p.requested.clone())
        } else {
            None
        };
        request.force = job.force;
        let process = self.process.as_mut().context("resident ASR missing")?;
        let output = process.request(
            &serde_json::to_vec(&request)?,
            Limits::new(timeout),
            || !running.load(Ordering::Relaxed) || owner.is_some_and(|o| o.cancelled()),
            |pid, elapsed| {
                let events = progress::read(&session);
                if let Some(event) = events
                    .last()
                    .filter(|e| e.state == progress::StageState::Running)
                {
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
                tracing::info!(
                    pid,
                    job_id = job.id,
                    elapsed_sec = elapsed.as_secs(),
                    "autocook: resident ASR heartbeat"
                );
            },
        )?;
        let reply: Reply = serde_json::from_slice(&output.stdout).context("resident ASR reply")?;
        reply.validate(self.sequence)?;
        reply.result.map_err(anyhow::Error::msg)?;
        tracing::info!(
            pid = output.pid,
            job_id = job.id,
            model_loads = reply.model_loads,
            elapsed_sec = output.elapsed.as_secs_f64(),
            "autocook: resident ASR request completed"
        );
        self.last_used = Some(Instant::now());
        Ok(())
    }
}
