//! Feature operations behind the stable Archive API facade.
use super::*;

impl Archive {
    /// A link → a session. The same code the voice command uses: one writer, not two.
    pub fn ingest(&self, raw_url: &str) -> Result<String> {
        localvox_light_core::ingest::from_url(&self.work_dir, raw_url)
    }

    pub fn progress(&self, name: &str) -> Result<Value> {
        let dir = self.session_dir(name)?;
        let source: Option<localvox_light_core::chunks::Source> = fs::read(dir.join("meta.json"))
            .ok()
            .and_then(|b| {
                serde_json::from_slice::<localvox_light_core::chunks::SessionMeta>(&b).ok()
            })
            .and_then(|m| m.source);
        let queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        let mut job = queue.jobs().iter().find(|j| j.session == name).cloned();
        if let Some(job) = &mut job {
            if job.follow_settings
                && matches!(
                    job.state,
                    localvox_light_core::jobs::JobState::Pending
                        | localvox_light_core::jobs::JobState::Running
                )
            {
                (job.summary, job.cleanup, job.refine) =
                    localvox_light_core::jobs::post_processing_from_env();
            }
        }
        let view = localvox_light_core::workflow::view(
            &dir,
            job.as_ref(),
            source.is_some(),
            &localvox_light_core::versions::now_rfc3339(),
        );
        let mut value = serde_json::to_value(&view)?;
        if let Some((partial, _)) = self.partial_document(&dir)? {
            value["partial"] = serde_json::to_value(partial)?;
        }
        value["source"] = serde_json::to_value(source)?;
        value["queue_error"] = json!(queue.error());
        if let Some(error) = queue.error() {
            value["execution"]["state"] = json!("unavailable");
            value["execution"]["message"] = json!(format!("Очередь недоступна: {error}"));
            value["execution"]["current"] = Value::Null;
        } else if job
            .as_ref()
            .is_none_or(|j| j.kind != localvox_light_core::jobs::JobKind::Incremental)
            && localvox_light_core::jobs::recording_session(&self.work_dir).as_deref() == Some(name)
        {
            value["execution"]["state"] = json!("recording");
            value["execution"]["message"] =
                json!(if localvox_light_core::jobs::prefix::enabled() {
                    "Идёт запись; ожидание закрытого фрагмента для раннего распознавания"
                } else {
                    "Идёт запись; раннее распознавание выключено, полная обработка — после Stop"
                });
        }
        // Backwards-compatible field; the UI now names queue, processing and total separately.
        value["elapsed_sec"] = if view.running {
            Value::Null
        } else {
            json!(view.timing.processing_sec)
        };
        Ok(value)
    }

    pub fn journal(
        &self,
        name: &str,
        query: &localvox_light_core::journal::Query,
    ) -> Result<Value> {
        let dir = self.session_dir(name)?;
        Ok(serde_json::to_value(localvox_light_core::journal::read(
            &dir, query,
        )?)?)
    }

    pub(super) fn readable_queue(&self) -> Result<localvox_light_core::jobs::JobQueue> {
        let queue = localvox_light_core::jobs::JobQueue::load(&self.work_dir);
        if let Some(error) = queue.error() {
            return Err(QueueUnavailable(error.to_owned()).into());
        }
        Ok(queue)
    }

    /// State of the background cook queue (WP-C7) — visibility of the autopilot:
    /// "N cooking, M queued, K failed". Reads `jobs.json` (P5: files only).
    /// THE QUEUE: what is being cooked now, and what comes after it, in order.
    ///
    /// Order is the queue's whole meaning, and it is the file's own order — the daemon takes
    /// pending jobs exactly as they lie, oldest first. So the list is not sorted by anything
    /// clever here: reordering it for looks would make it a lie about what happens next.
    ///
    /// `Done` is deliberately absent. A queue is what is left to do; what is finished lives in the
    /// archive, with its own card and its own stages.
    pub fn queue(&self) -> Result<Vec<QueueItem>> {
        use localvox_light_core::jobs::{JobKind, JobState as S, MAX_REVIVALS};
        let q = self.readable_queue()?;
        let coordinator = localvox_light_core::jobs::coordinator_status();
        let now = localvox_light_core::versions::now_rfc3339();
        let mut place = 0usize;
        let mut queued: Vec<_> = q.jobs().iter().filter(|j| j.state != S::Done).collect();
        queued.sort_by_key(|j| {
            if j.state == S::WaitingInput {
                2
            } else if j.kind == JobKind::Incremental && j.state == S::Pending {
                1
            } else {
                0
            }
        });
        Ok(queued
            .into_iter()
            .map(|j| {
                // The meta gives the human name; without it the person reads directory stamps.
                let meta: Option<localvox_light_core::chunks::SessionMeta> =
                    fs::read(self.sessions_root().join(&j.session).join("meta.json"))
                        .ok()
                        .and_then(|b| serde_json::from_slice(&b).ok());
                let title = meta
                    .as_ref()
                    .and_then(|m| m.title.clone())
                    .or_else(|| {
                        meta.as_ref()
                            .and_then(|m| m.source.as_ref().map(|s| s.url.clone()))
                    })
                    .unwrap_or_else(|| j.session.clone());
                let position = if j.state == S::Pending {
                    place += 1;
                    Some(place)
                } else {
                    None // what is running is not waiting
                };
                QueueItem {
                    session: j.session.clone(),
                    title,
                    started_at: meta.and_then(|m| Some(m.started_at)),
                    state: match j.state {
                        S::Running => "running",
                        S::Cancelling => "cancelling",
                        S::Cancelled => "cancelled",
                        S::Pending => "pending",
                        S::WaitingInput => "waiting_input",
                        S::Failed => "failed",
                        S::Done => "done",
                    }
                    .into(),
                    kind: match j.kind {
                        JobKind::Cook => "cook",
                        JobKind::Ingest => "ingest",
                        JobKind::Incremental => "incremental",
                    }
                    .into(),
                    position,
                    attempts: j.attempts,
                    phase: j.phase,
                    retry: localvox_light_core::workflow::retry_plan(j, coordinator.as_ref(), &now),
                    last_error: j.last_error.clone(),
                    stuck: j.state == S::Failed && j.revivals >= MAX_REVIVALS,
                }
            })
            .collect())
    }

    pub fn jobs_status(&self) -> Result<JobsStatus> {
        let q = self.readable_queue()?;
        let mut st = JobsStatus::default();
        for j in q.jobs() {
            use localvox_light_core::jobs::JobState as S;
            match j.state {
                S::Pending => st.pending += 1,
                S::WaitingInput => {}
                S::Running => {
                    st.running += 1;
                    st.current = Some(j.session.clone());
                }
                S::Done => st.done += 1,
                S::Cancelling => {
                    st.cancelling += 1;
                    if st.current.is_none() {
                        st.current = Some(j.session.clone());
                    }
                }
                S::Cancelled => st.cancelled += 1,
                S::Failed => {
                    st.failed += 1;
                    if st.last_error.is_none() {
                        st.last_error = j.last_error.clone();
                    }
                }
            }
        }
        Ok(st)
    }
}
