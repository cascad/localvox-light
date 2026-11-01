//! Bounded metadata only: observation must never govern queue execution.
use super::*;
use std::sync::{
    atomic::AtomicUsize,
    mpsc::{sync_channel, Receiver, SyncSender},
};

#[derive(Clone)]
pub struct Observer {
    sender: SyncSender<Transition>,
    lost: Arc<AtomicUsize>,
}

#[derive(Clone, Debug)]
pub struct Transition {
    pub kind: &'static str,
    pub job_id: u64,
    pub session: String,
    pub phase: JobPhase,
    pub execution_id: Option<String>,
    pub queued_at: Option<String>,
    pub started_at: Option<String>,
    pub ended_at: Option<String>,
    pub attempt: u32,
    pub state_after: JobState,
    pub phase_after: JobPhase,
    pub after_has_error: bool,
}

impl Transition {
    pub fn as_json(&self) -> serde_json::Value {
        serde_json::json!({"kind":self.kind,"job_id":self.job_id,"session":self.session,
            "phase":self.phase,"execution_id":self.execution_id,"queued_at":self.queued_at,
            "started_at":self.started_at,"ended_at":self.ended_at,"attempt":self.attempt,
            "state_after":self.state_after,"phase_after":self.phase_after,"after_has_error":self.after_has_error})
    }
}

impl Observer {
    pub fn channel(capacity: usize) -> (Self, Receiver<Transition>) {
        let (sender, receiver) = sync_channel(capacity);
        (
            Self {
                sender,
                lost: Arc::new(AtomicUsize::new(0)),
            },
            receiver,
        )
    }

    pub fn lost_count(&self) -> usize {
        self.lost.load(Ordering::Relaxed)
    }

    pub(super) fn emit(&self, kind: &'static str, before: &Job, after: &Job) {
        let ended_at = if kind == "claimed" {
            None
        } else if matches!(
            after.state,
            JobState::Done | JobState::Failed | JobState::Cancelled
        ) {
            after.finished_at.clone()
        } else {
            after.phase_queued_at.clone()
        };
        let event = Transition {
            kind,
            job_id: before.id,
            session: before.session.clone(),
            phase: before.phase,
            execution_id: before.execution_id.clone(),
            queued_at: before.phase_queued_at.clone(),
            started_at: before.phase_started_at.clone(),
            ended_at,
            attempt: before.attempts,
            state_after: after.state,
            phase_after: after.phase,
            after_has_error: after.last_error.is_some(),
        };
        if self.sender.try_send(event).is_err() {
            self.lost.fetch_add(1, Ordering::Relaxed);
        }
    }
}
