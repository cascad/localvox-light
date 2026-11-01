use super::*;

fn stopped(job: &mut Job) {
    job.state = JobState::Cancelled;
    job.finished_at = Some(now_rfc3339());
    job.force = false;
    job.last_error = None;
    job.failed_at = None;
}

impl JobQueue {
    /// Durable claim/advance fence. Signal the managed owner only after this returns Ok.
    pub fn try_request_cancellation(&mut self, session: &str) -> anyhow::Result<Job> {
        self.ensure_readable()?;
        let index = self
            .file
            .jobs
            .iter()
            .position(|job| job.session == session)
            .ok_or_else(|| anyhow::anyhow!("задание не найдено"))?;
        if matches!(
            self.file.jobs[index].state,
            JobState::Cancelling | JobState::Cancelled
        ) {
            return Ok(self.file.jobs[index].clone());
        }
        let previous = self.file.jobs[index].clone();
        let job = &mut self.file.jobs[index];
        job.state = JobState::Cancelling;
        job.last_error = None;
        job.failed_at = None;
        job.force = false;
        self.commit_jobs(vec![(index, previous)], "запрос остановки")?;
        Ok(self.file.jobs[index].clone())
    }

    /// Caller owns the actual execution lease. No heartbeat/timeout may substitute for it.
    pub fn try_complete_cancellation(&mut self, session: &str) -> anyhow::Result<Job> {
        self.ensure_readable()?;
        if let Some(index) = self.file.jobs.iter().position(|job| job.session == session) {
            if self.file.jobs[index].state == JobState::Cancelled {
                return Ok(self.file.jobs[index].clone());
            }
            let previous = self.file.jobs[index].clone();
            stopped(&mut self.file.jobs[index]);
            self.commit_jobs(vec![(index, previous)], "подтверждение остановки")?;
            return Ok(self.file.jobs[index].clone());
        }
        // Older archives may have no queue row. A tombstone prevents rediscovery after cancel
        // or an unsuccessful delete; publish it directly, never as an intermediate Pending job.
        let id = self.file.next_id;
        let mut job = Job::pending(
            id,
            session,
            JobKind::Cook,
            JobPhase::Prepare,
            (false, false, false),
            false,
        );
        job.phase_queued_at = None;
        stopped(&mut job);
        self.file.jobs.push(job.clone());
        self.file.next_id += 1;
        if !self.save() {
            self.file.jobs.pop();
            self.file.next_id = id;
            return Err(QueueStorageError(
                "не удалось сохранить подтверждение остановки в jobs.json".into(),
            )
            .into());
        }
        Ok(job)
    }

    /// The original owner still holds its lease. Never acknowledge another execution.
    pub fn try_complete_owned_cancellation(&mut self, owner: &Job) -> anyhow::Result<bool> {
        self.ensure_readable()?;
        let Some(job) = self.file.jobs.iter().find(|job| job.id == owner.id) else {
            return Ok(false);
        };
        if job.state != JobState::Cancelling || job.execution_id != owner.execution_id {
            return Ok(false);
        }
        self.try_complete_cancellation(&owner.session)?;
        Ok(true)
    }

    /// Recover an accepted stop intent only after the OS proves there is no live owner.
    /// Called on startup/maintenance; it never converts cancellation into Pending/retry.
    pub fn try_reconcile_cancellations(&mut self, work_dir: &Path) -> anyhow::Result<usize> {
        self.ensure_readable()?;
        let mut previous = Vec::new();
        let mut leases = Vec::new();
        for (index, job) in self.file.jobs.iter_mut().enumerate() {
            if job.state != JobState::Cancelling {
                continue;
            }
            match try_execution_lock(&work_dir.join("sessions").join(&job.session)) {
                Ok(Some(lease)) => {
                    leases.push(lease);
                    previous.push((index, job.clone()));
                    stopped(job);
                }
                Ok(None) => {}
                Err(error) => {
                    tracing::warn!(session = job.session, "stop acknowledgement: {error}")
                }
            }
        }
        self.commit_jobs(previous, "восстановление остановки")
    }

    /// Post-delete queue cleanup; unlike the legacy void wrapper, failure is observable.
    pub fn try_forget_session(&mut self, session: &str) -> anyhow::Result<()> {
        self.ensure_readable()?;
        let previous = self.file.jobs.clone();
        self.file.jobs.retain(|job| job.session != session);
        if previous.len() == self.file.jobs.len() {
            return Ok(());
        }
        if !self.save() {
            self.file.jobs = previous;
            return Err(QueueStorageError(
                "не удалось убрать удалённую сессию из jobs.json".into(),
            )
            .into());
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "cancellation_tests.rs"]
mod tests;
