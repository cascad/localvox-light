use super::*;
use crate::{
    artifacts,
    jobs::JobPhase,
    processing::{self, Outcome},
};

fn wanted() -> Wanted {
    Wanted::new(true, false, None, "fixture")
}
fn text_session(root: &Path) -> PathBuf {
    let dir = root.join("sessions/meeting");
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("meta.json"), b"{}").unwrap();
    artifacts::tests::fixture(&dir);
    processing::record(
        &dir,
        processing::TRANSCRIPT,
        "fixture",
        Outcome::Ok,
        None,
        Some(1),
    )
    .unwrap();
    fs::remove_file(dir.join("summary.md")).unwrap();
    dir
}
fn candidate(root: &Path) -> Candidate {
    let mut candidates = scan(root, 0, &wanted());
    assert_eq!(candidates.len(), 1);
    candidates.remove(0)
}
fn publish(root: &Path, candidate: &Candidate, flags: (bool, bool, bool)) -> bool {
    JobQueue::mutate(root, |queue| candidate.publish(queue, flags))
}

#[test]
fn unchanged_candidate_is_published_once_with_live_summary_policy() {
    let root = tempfile::tempdir().unwrap();
    text_session(root.path());
    let found = candidate(root.path());
    assert!(!publish(root.path(), &found, (false, false, false)));
    assert!(publish(root.path(), &found, (true, false, false)));
    assert!(!publish(root.path(), &found, (true, false, false)));
    let queue = JobQueue::load(root.path());
    assert_eq!(queue.jobs().len(), 1);
    assert!(queue.jobs()[0].summary && !queue.jobs()[0].cleanup && !queue.jobs()[0].refine);
    assert!(queue.jobs()[0].follow_settings);
}

#[test]
fn new_manual_work_and_completed_attempts_invalidate_discovery() {
    let root = tempfile::tempdir().unwrap();
    let dir = text_session(root.path());
    let found = candidate(root.path());
    JobQueue::mutate(root.path(), |queue| {
        assert!(queue.requeue_derivatives("meeting", true, false, false));
    });
    assert!(!publish(root.path(), &found, (true, false, false)));
    // A scan of an old Done record must not replay a newer completed attempt.
    JobQueue::mutate(root.path(), |queue| queue.mark_done(0));
    let found = candidate(root.path());
    JobQueue::mutate(root.path(), |queue| {
        assert!(queue.requeue_derivatives("meeting", true, false, false));
        let running = queue.start_phase(0, &[JobPhase::Summary]).unwrap();
        fs::write(dir.join("summary.md"), "finished summary").unwrap();
        processing::record(
            &dir,
            processing::SUMMARY,
            "fixture",
            Outcome::Ok,
            None,
            Some(1),
        )
        .unwrap();
        queue.finish_phase(running.id, running.phase).unwrap();
    });
    assert!(!publish(root.path(), &found, (true, false, false)));
    assert_eq!(JobQueue::load(root.path()).jobs()[0].state, JobState::Done);
}

#[test]
fn deleted_recording_busy_and_changed_sessions_are_deferred() {
    for change in [
        "delete", "record", "lease", "metadata", "version", "receipt",
    ] {
        let root = tempfile::tempdir().unwrap();
        let dir = text_session(root.path());
        let found = candidate(root.path());
        let lease = if change == "lease" {
            super::super::try_execution_lock(&dir).unwrap()
        } else {
            None
        };
        match change {
            "delete" => {
                fs::remove_file(dir.join("meta.json")).unwrap();
                let removed = root.path().join("removed");
                let temporary_root = root.path().canonicalize().unwrap();
                assert!(dir.canonicalize().unwrap().starts_with(&temporary_root));
                assert_eq!(
                    removed.parent().unwrap().canonicalize().unwrap(),
                    temporary_root
                );
                fs::rename(&dir, &removed).unwrap();
            }
            "record" => {
                fs::write(root.path().join(super::super::RECORDING_MARKER), "meeting").unwrap()
            }
            "metadata" => fs::write(dir.join("meta.json"), b"{\"title\":\"changed\"}").unwrap(),
            "version" => {
                let bytes = fs::read(dir.join("versions.json")).unwrap();
                let mut changed = bytes;
                changed.push(b' ');
                fs::write(dir.join("versions.json"), changed).unwrap();
            }
            "receipt" => fs::write(dir.join("processing.json"), b"{}").unwrap(),
            _ => {}
        }
        assert!(
            !publish(root.path(), &found, (true, false, false)),
            "{change}"
        );
        assert!(JobQueue::load(root.path()).jobs().is_empty(), "{change}");
        assert_eq!(
            root.path().join("sessions/meeting").is_dir(),
            change != "delete"
        );
        drop(lease);
    }
}

#[test]
fn corrupt_queue_and_metadata_never_get_replaced_by_discovery() {
    let root = tempfile::tempdir().unwrap();
    text_session(root.path());
    let found = candidate(root.path());
    fs::write(root.path().join("jobs.json"), b"{broken").unwrap();
    assert!(!publish(root.path(), &found, (true, false, false)));
    assert!(scan(root.path(), 0, &wanted()).is_empty());
    assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), b"{broken");
}

#[test]
fn transcription_remains_mandatory_and_new_audio_invalidates_a_cook_candidate() {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("sessions/meeting");
    fs::create_dir_all(dir.join("audio")).unwrap();
    fs::write(
        dir.join("meta.json"),
        b"{\"stopped_at\":\"2026-10-31T00:00:00Z\"}",
    )
    .unwrap();
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut audio = hound::WavWriter::create(dir.join("audio/chunk.wav"), spec).unwrap();
    audio.write_sample(0_i16).unwrap();
    audio.finalize().unwrap();
    let old = candidate(root.path());
    assert_eq!(old.kind, Kind::Cook);
    fs::write(dir.join("audio/new.part"), b"pending audio").unwrap();
    assert!(!publish(root.path(), &old, (false, false, false)));
    fs::remove_file(dir.join("audio/new.part")).unwrap();
    let fresh = candidate(root.path());
    assert!(publish(root.path(), &fresh, (false, false, false)));
    let queue = JobQueue::load(root.path());
    assert_eq!(queue.jobs()[0].phase, JobPhase::Prepare);
    assert!(!queue.jobs()[0].summary && !queue.jobs()[0].refine && !queue.jobs()[0].cleanup);
}

#[test]
fn recording_started_after_publication_prevents_a_worker_claim() {
    let root = tempfile::tempdir().unwrap();
    text_session(root.path());
    let found = candidate(root.path());
    assert!(publish(root.path(), &found, (true, false, false)));
    fs::write(root.path().join(super::super::RECORDING_MARKER), "meeting").unwrap();
    JobQueue::mutate(root.path(), |queue| {
        assert!(queue.start_phase(0, &[JobPhase::Prepare]).is_none());
        assert_eq!(queue.jobs()[0].state, JobState::Pending);
        assert_eq!(queue.jobs()[0].attempts, 0);
    });
    fs::remove_file(root.path().join(super::super::RECORDING_MARKER)).unwrap();
    JobQueue::mutate(root.path(), |queue| {
        assert!(queue.start_phase(0, &[JobPhase::Prepare]).is_some());
        assert_eq!(queue.jobs()[0].attempts, 1);
    });
}

#[test]
fn same_size_metadata_change_with_restored_mtime_invalidates_a_candidate() {
    let root = tempfile::tempdir().unwrap();
    let dir = text_session(root.path());
    let found = candidate(root.path());
    let path = dir.join("meta.json");
    let before = fs::metadata(&path).unwrap();
    let at = before.modified().unwrap();
    fs::write(&path, b"[]").unwrap();
    fs::File::options()
        .write(true)
        .open(&path)
        .unwrap()
        .set_modified(at)
        .unwrap();
    assert_eq!(before.len(), fs::metadata(&path).unwrap().len());
    assert_eq!(at, fs::metadata(&path).unwrap().modified().unwrap());
    assert!(!publish(root.path(), &found, (true, false, false)));
    assert_eq!(fs::read(&path).unwrap(), b"[]");
}
