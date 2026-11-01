use super::*;

fn fixture() -> (tempfile::TempDir, JobQueue, Job) {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir_all(root.path().join("sessions/fixture")).unwrap();
    fs::write(
        root.path().join("sessions/fixture/retained.txt"),
        b"retained transcript bytes",
    )
    .unwrap();
    let mut queue = JobQueue::load(root.path());
    assert!(queue.enqueue_cook("fixture", true, false, false));
    let job = queue.start_phase(0, &[JobPhase::Prepare]).unwrap();
    (root, queue, job)
}

#[test]
fn requested_stop_fences_success_failure_and_claim_without_resetting_phase_clocks() {
    let (root, mut queue, owner) = fixture();
    let requested = queue.try_request_cancellation("fixture").unwrap();
    assert_eq!(requested.state, JobState::Cancelling);
    assert_eq!(requested.phase, owner.phase);
    assert_eq!(requested.phase_started_at, owner.phase_started_at);
    assert_eq!(requested.execution_id, owner.execution_id);
    queue.mark_done(owner.id);
    assert_eq!(queue.jobs()[0].state, JobState::Cancelling);
    assert!(queue
        .finish_phase_with_policy(owner.id, owner.phase, None)
        .unwrap_err()
        .is::<CancellationRequested>());
    assert!(queue
        .try_mark_failed(owner.id, "not a worker failure", 3)
        .unwrap_err()
        .is::<CancellationRequested>());
    assert!(queue.pending_for(&[owner.phase]).is_empty());
    assert!(queue.start_phase(owner.id, &[owner.phase]).is_none());
    assert_eq!(
        JobQueue::load(root.path()).jobs()[0].state,
        JobState::Cancelling
    );
    assert_eq!(
        fs::read(root.path().join("sessions/fixture/retained.txt")).unwrap(),
        b"retained transcript bytes"
    );
}

#[test]
fn owner_acknowledgement_is_execution_scoped_and_cancel_is_not_automatically_revived() {
    let (root, mut queue, owner) = fixture();
    queue.try_request_cancellation("fixture").unwrap();
    let mut wrong = owner.clone();
    wrong.execution_id = Some("another-execution".into());
    assert!(!queue.try_complete_owned_cancellation(&wrong).unwrap());
    assert!(queue.try_complete_owned_cancellation(&owner).unwrap());
    let stopped = queue.jobs()[0].clone();
    assert_eq!(stopped.state, JobState::Cancelled);
    assert!(stopped.finished_at.is_some());
    assert_eq!(stopped.phase, owner.phase);
    assert!(automatic_retry_at(&stopped).is_none());
    assert_eq!(queue.try_reclaim_abandoned(root.path()).unwrap(), 0);
    assert_eq!(
        queue.try_revive_failed(std::time::Duration::ZERO).unwrap(),
        0
    );
    assert!(!queue.enqueue_cook("fixture", true, false, false));
    assert_eq!(queue.try_complete_cancellation("fixture").unwrap(), stopped);
    assert!(queue.retry_failed("fixture"));
    assert_eq!(queue.jobs()[0].state, JobState::Pending);
    assert_eq!(queue.jobs()[0].phase, owner.phase);
    assert!(!queue.jobs()[0].force);
}

#[test]
fn recovery_requires_the_actual_lease_to_be_free_before_confirming_a_stop() {
    let (root, mut queue, _) = fixture();
    queue.try_request_cancellation("fixture").unwrap();
    let lease = try_execution_lock(&root.path().join("sessions/fixture"))
        .unwrap()
        .unwrap();
    assert_eq!(queue.try_reconcile_cancellations(root.path()).unwrap(), 0);
    assert_eq!(queue.jobs()[0].state, JobState::Cancelling);
    drop(lease);
    assert_eq!(queue.try_reconcile_cancellations(root.path()).unwrap(), 1);
    assert_eq!(
        JobQueue::load(root.path()).jobs()[0].state,
        JobState::Cancelled
    );
}

#[test]
fn a_cancelled_legacy_session_gets_a_durable_tombstone_without_a_pending_intermediate() {
    let root = tempfile::tempdir().unwrap();
    let mut queue = JobQueue::load(root.path());
    let cancelled = queue.try_complete_cancellation("old-archive").unwrap();
    assert_eq!(cancelled.state, JobState::Cancelled);
    assert!(cancelled.started_at.is_none());
    assert!(cancelled.phase_queued_at.is_none());
    assert_eq!(JobQueue::load(root.path()).jobs(), &[cancelled]);
    assert!(!queue.enqueue_cook("old-archive", true, false, false));
}

#[test]
fn unreadable_queue_cannot_accept_stop_or_cleanup_and_is_not_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("jobs.json");
    fs::write(&path, b"broken queue").unwrap();
    let mut queue = JobQueue::load(root.path());
    assert!(queue
        .try_request_cancellation("one")
        .unwrap_err()
        .is::<QueueStorageError>());
    assert!(queue
        .try_complete_cancellation("one")
        .unwrap_err()
        .is::<QueueStorageError>());
    assert!(queue
        .try_forget_session("one")
        .unwrap_err()
        .is::<QueueStorageError>());
    assert_eq!(fs::read(path).unwrap(), b"broken queue");
}

#[test]
fn cancellation_has_a_terminal_view_and_keeps_unknown_processing_time_unknown() {
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("sessions/fixture");
    fs::create_dir_all(&session).unwrap();
    let mut queue = JobQueue::load(root.path());
    assert!(queue.enqueue_cook("fixture", true, false, false));
    let job = queue.try_complete_cancellation("fixture").unwrap();
    let view = crate::workflow::view(&session, Some(&job), false, &now_rfc3339());
    assert!(!view.running);
    assert_eq!(view.execution.state, "cancelled");
    assert!(view.execution.next.is_empty());
    assert!(view.timing.processing_sec.is_none());
    let stt = view
        .stages
        .iter()
        .find(|s| s.stage == crate::progress::Stage::Transcribe)
        .unwrap();
    assert_eq!(stt.state, crate::progress::StageState::Cancelled);
    assert!(stt.started_at.is_none());
}

#[cfg(windows)]
#[test]
fn denied_replacement_rolls_back_stop_request_and_acknowledgement() {
    use std::os::windows::fs::OpenOptionsExt;
    let (root, mut queue, _) = fixture();
    let path = root.path().join("jobs.json");
    let before = queue.jobs()[0].clone();
    let denied = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    assert!(queue
        .try_request_cancellation("fixture")
        .unwrap_err()
        .is::<QueueStorageError>());
    assert_eq!(queue.jobs()[0], before);
    drop(denied);
    queue.try_request_cancellation("fixture").unwrap();
    let requested = queue.jobs()[0].clone();
    let denied = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    assert!(queue
        .try_complete_cancellation("fixture")
        .unwrap_err()
        .is::<QueueStorageError>());
    assert_eq!(queue.jobs()[0], requested);
    assert_eq!(JobQueue::load(root.path()).jobs()[0], requested);
    drop(denied);
    queue.try_complete_cancellation("fixture").unwrap();
    let stopped = queue.jobs()[0].clone();
    let denied = fs::OpenOptions::new()
        .read(true)
        .share_mode(3)
        .open(&path)
        .unwrap();
    assert!(queue
        .try_forget_session("fixture")
        .unwrap_err()
        .is::<QueueStorageError>());
    assert_eq!(queue.jobs(), &[stopped]);
    drop(denied);
}
