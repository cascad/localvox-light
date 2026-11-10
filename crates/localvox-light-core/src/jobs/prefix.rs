//! Coalesced intent lives in the same durable queue and uses its execution lease.
use super::*;
use crate::{
    chunks::{capture, planning},
    incremental::{FrozenInput, Horizon},
};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intent {
    pub requested: FrozenInput,
    pub accepted_work: Option<String>,
    pub finalization_requested: bool,
}
pub struct Observation {
    pub session: String,
    pub generation: String,
    pub input: Option<FrozenInput>,
    pub ended: bool,
    pub enabled: bool,
}
pub fn enabled() -> bool {
    let _guard = post_processing_guard();
    !matches!(
        std::env::var("LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE")
            .unwrap_or_else(
                |_| crate::presets::baked("LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE").into()
            )
            .to_lowercase()
            .as_str(),
        "off" | "false" | "0" | "no"
    )
}
/// Metadata/descriptor reads and immutable horizon publication stay outside queue mutex.
pub fn observe(work_dir: &Path) -> Vec<Observation> {
    let queue = JobQueue::load(work_dir);
    if queue.error().is_some() {
        return vec![];
    }
    let mut names: std::collections::BTreeSet<_> = queue
        .jobs()
        .iter()
        .filter(|j| j.kind == JobKind::Incremental)
        .map(|j| j.session.clone())
        .collect();
    if let Some(active) = recording_session(work_dir) {
        names.insert(active);
    }
    let enabled = enabled();
    names
        .into_iter()
        .filter_map(|session| {
            let dir = work_dir.join("sessions").join(&session);
            let input = capture::read(&dir).ok()?;
            let ended = input.terminal.is_some();
            let horizon = if !ended && enabled && dir.join(planning::FILE).is_file() {
                match Horizon::read(&dir).and_then(|h| {
                    if h.windows().values().all(Vec::is_empty) {
                        Ok(None)
                    } else {
                        h.freeze(&dir).map(Some)
                    }
                }) {
                    Ok(h) => h,
                    Err(e) => {
                        tracing::warn!(session, "ready capture input: {e:#}");
                        None
                    }
                }
            } else {
                None
            };
            Some(Observation {
                session,
                generation: input.generation,
                input: horizon,
                ended,
                enabled,
            })
        })
        .collect()
}
impl JobQueue {
    pub fn offer_prefix(
        &mut self,
        observation: &Observation,
        flags: (bool, bool, bool),
    ) -> anyhow::Result<bool> {
        self.ensure_readable()?;
        let Some(index) = self
            .file
            .jobs
            .iter()
            .position(|j| j.session == observation.session)
        else {
            if observation.ended || !observation.enabled {
                return Ok(false);
            }
            let Some(input) = &observation.input else {
                return Ok(false);
            };
            let id = self.file.next_id;
            let mut job = Job::pending(
                id,
                &observation.session,
                JobKind::Incremental,
                JobPhase::Transcribe,
                flags,
                true,
            );
            job.prefix = Some(Intent {
                requested: input.clone(),
                accepted_work: None,
                finalization_requested: false,
            });
            self.file.jobs.push(job);
            self.file.next_id += 1;
            if !self.save() {
                self.file.jobs.pop();
                self.file.next_id = id;
                return Err(QueueStorageError("не удалось сохранить раннее задание".into()).into());
            }
            return Ok(true);
        };
        let job = &mut self.file.jobs[index];
        if job.kind != JobKind::Incremental
            || matches!(job.state, JobState::Cancelling | JobState::Cancelled)
        {
            return Ok(false);
        }
        let previous = job.clone();
        let intent = job
            .prefix
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("missing incremental intent"))?;
        anyhow::ensure!(
            intent.requested.generation == observation.generation,
            "recording generation changed"
        );
        intent.finalization_requested |= observation.ended;
        if let Some(input) = &observation.input {
            if input.revision >= intent.requested.revision {
                intent.requested = input.clone();
            }
        }
        if job.state != JobState::Running {
            if intent.finalization_requested {
                promote(job)
            } else if job.state != JobState::Failed {
                job.state = if observation.enabled
                    && intent.accepted_work.as_ref() != Some(&intent.requested.work)
                {
                    JobState::Pending
                } else {
                    JobState::WaitingInput
                };
            }
        }
        if *job == previous {
            return Ok(false);
        }
        self.commit_jobs(vec![(index, previous)], "обновление раннего задания")?;
        Ok(true)
    }
    /// Result is verified before acquiring queue mutex. Compare the execution token here.
    pub fn finish_prefix(&mut self, claimed: &Job, enabled: bool) -> anyhow::Result<()> {
        self.ensure_readable()?;
        let index = self
            .file
            .jobs
            .iter()
            .position(|j| j.id == claimed.id)
            .ok_or_else(|| anyhow::anyhow!("missing early job"))?;
        let job = &mut self.file.jobs[index];
        if matches!(job.state, JobState::Cancelling | JobState::Cancelled) {
            return Err(CancellationRequested.into());
        }
        anyhow::ensure!(
            job.kind == JobKind::Incremental
                && job.state == JobState::Running
                && job.execution_id == claimed.execution_id,
            "early execution changed"
        );
        let previous = job.clone();
        let intent = job
            .prefix
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("missing early intent"))?;
        intent.accepted_work = Some(
            claimed
                .prefix
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("missing claimed horizon"))?
                .requested
                .work
                .clone(),
        );
        job.phase_started_at = None;
        job.attempts = 0;
        job.last_error = None;
        job.failed_at = None;
        if intent.finalization_requested {
            promote(job)
        } else {
            job.state = if enabled && intent.accepted_work.as_ref() != Some(&intent.requested.work)
            {
                JobState::Pending
            } else {
                JobState::WaitingInput
            };
            job.phase_queued_at = Some(now_rfc3339());
        }
        self.commit_jobs(vec![(index, previous)], "подтверждение ранней порции")?;
        Ok(())
    }
}
fn promote(job: &mut Job) {
    job.kind = JobKind::Cook;
    job.prefix = None;
    job.phase = JobPhase::Prepare;
    job.state = JobState::Pending;
    job.phase_queued_at = Some(now_rfc3339());
    job.phase_started_at = None;
    job.attempts = 0;
}

#[cfg(test)]
mod tests {
    use super::*;
    fn observation(revision: u64, work: u8) -> Observation {
        Observation {
            session: "recorded".into(),
            generation: "0199a774-0000-7000-8000-000000000003".into(),
            input: Some(FrozenInput {
                generation: "0199a774-0000-7000-8000-000000000003".into(),
                revision,
                plan: format!("{revision:064x}"),
                work: format!("{work:064x}"),
            }),
            ended: false,
            enabled: true,
        }
    }
    fn queue() -> (tempfile::TempDir, JobQueue) {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("sessions/recorded")).unwrap();
        fs::write(root.path().join(RECORDING_MARKER), "recorded").unwrap();
        let q = JobQueue::load(root.path());
        (root, q)
    }
    #[test]
    fn coalesced_horizons_do_not_spawn_again_for_the_same_windows_and_yield_to_ordinary_jobs() {
        let (root, mut q) = queue();
        let first = observation(1, 1);
        q.offer_prefix(&first, (true, false, false)).unwrap();
        for _ in 0..10 {
            assert!(!q.offer_prefix(&first, (true, false, false)).unwrap())
        }
        let claimed = q.start_phase(0, &[JobPhase::Transcribe]).unwrap();
        q.offer_prefix(&observation(2, 2), (true, false, false))
            .unwrap();
        assert_eq!(claimed.prefix.as_ref().unwrap().requested.revision, 1);
        q.finish_prefix(&claimed, true).unwrap();
        assert_eq!(q.jobs()[0].state, JobState::Pending);
        fs::create_dir_all(root.path().join("sessions/ordinary")).unwrap();
        q.enqueue_cook("ordinary", false, false, false);
        q.start_phase(1, &[JobPhase::Prepare]).unwrap();
        q.finish_phase(1, JobPhase::Prepare).unwrap_err();
        // Lane priority does not depend on insertion order.
        q.file.jobs[1].state = JobState::Pending;
        q.file.jobs[1].phase = JobPhase::Transcribe;
        assert_eq!(q.pending_for(&[JobPhase::Transcribe]), [1, 0]);
        let second = q.start_phase(0, &[JobPhase::Transcribe]).unwrap();
        q.finish_prefix(&second, true).unwrap();
        assert_eq!(q.jobs()[0].state, JobState::WaitingInput);
        q.offer_prefix(&observation(3, 2), (true, false, false))
            .unwrap();
        assert_eq!(q.jobs()[0].state, JobState::WaitingInput);
        assert_eq!(q.jobs().len(), 2);
    }
    #[test]
    fn stop_during_claim_waits_for_that_execution_and_cancel_is_sticky_after_restart() {
        for cancel in [false, true] {
            let (root, mut q) = queue();
            q.offer_prefix(&observation(1, 1), (true, false, false))
                .unwrap();
            let claimed = q.start_phase(0, &[JobPhase::Transcribe]).unwrap();
            let mut stopped = observation(2, 2);
            stopped.ended = true;
            stopped.input = None;
            q.offer_prefix(&stopped, (true, false, false)).unwrap();
            assert_eq!(q.jobs()[0].kind, JobKind::Incremental);
            if cancel {
                q.try_request_cancellation("recorded").unwrap();
                assert!(q
                    .finish_prefix(&claimed, true)
                    .unwrap_err()
                    .is::<CancellationRequested>());
                q.try_complete_cancellation("recorded").unwrap();
                q = JobQueue::load(root.path());
                q.offer_prefix(&stopped, (true, false, false)).unwrap();
                assert_eq!(q.jobs()[0].state, JobState::Cancelled);
                assert!(q.retry_failed("recorded"));
                q.offer_prefix(&stopped, (true, false, false)).unwrap();
            } else {
                q.finish_prefix(&claimed, true).unwrap();
            }
            assert_eq!(
                (q.jobs()[0].kind, q.jobs()[0].phase, q.jobs()[0].state),
                (JobKind::Cook, JobPhase::Prepare, JobState::Pending)
            );
        }
    }
    #[test]
    fn disabling_early_work_finishes_the_running_quantum_and_stop_still_finalizes() {
        let (_root, mut q) = queue();
        q.offer_prefix(&observation(1, 1), (false, false, false))
            .unwrap();
        let claimed = q.start_phase(0, &[JobPhase::Transcribe]).unwrap();
        let mut off = observation(2, 2);
        off.enabled = false;
        q.offer_prefix(&off, (false, false, false)).unwrap();
        q.finish_prefix(&claimed, false).unwrap();
        assert_eq!(q.jobs()[0].state, JobState::WaitingInput);
        assert!(q.pending_for(&[JobPhase::Transcribe]).is_empty());
        off.ended = true;
        q.offer_prefix(&off, (false, false, false)).unwrap();
        assert_eq!(q.jobs()[0].kind, JobKind::Cook);
    }
}
