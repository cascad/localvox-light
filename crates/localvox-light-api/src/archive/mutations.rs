use super::*;
use localvox_light_core::jobs::{
    self,
    control::{self, MutationLease},
    JobQueue,
};
use std::time::{Duration, Instant};

fn storage_error(error: anyhow::Error) -> anyhow::Error {
    if error.is::<jobs::QueueStorageError>() {
        QueueUnavailable(format!("{error:#}")).into()
    } else {
        error
    }
}

impl Archive {
    /// Archive identifiers come from the directory listing; aliases are not new sessions.
    pub(super) fn checked_mutation_target(&self, session: &str, dir: &Path) -> Result<()> {
        let canonical = dir.canonicalize()?;
        let root = self.sessions_root().canonicalize()?;
        anyhow::ensure!(canonical.parent()==Some(root.as_path()) && canonical.file_name()==Some(std::ffi::OsStr::new(session)),
            "используйте точное имя записи из архива; изменение через ссылку или другое имя не выполнено");
        Ok(())
    }

    /// The returned OS lease, not a timestamp or missing heartbeat, acknowledges the stop.
    pub(super) fn stopped_lease(
        &self,
        session: &str,
        dir: &Path,
        timeout: Duration,
    ) -> Result<MutationLease> {
        self.ensure_not_recording(session)?;
        self.checked_mutation_target(session, dir)?;
        let control = control::try_control_lock(dir)?
            .context("сессия занята другим изменением; повторите после его завершения")?;
        let mut free = None;
        JobQueue::mutate(&self.work_dir, |queue|->Result<()> {
            if let Some(error)=queue.error() {return Err(QueueUnavailable(error.into()).into())}
            if let Some(lease)=jobs::try_execution_lock(dir)? {
                queue.try_complete_cancellation(session)?;
                free=Some(lease);
            } else {
                let owner=control::managed_owner(dir)
                    .context("сессия занята внешним CLI или неуправляемым обработчиком; остановите его, файлы не изменены")?;
                let job=queue.jobs().iter().find(|job|job.session==session)
                    .context("управляемое задание не найдено; файлы не изменены")?;
                anyhow::ensure!(owner.matches(job),"владелец обработки уже изменился; файлы не изменены");
                queue.try_request_cancellation(session)?;
                owner.signal();
            }
            Ok(())
        }).map_err(storage_error)?;
        if let Some(execution) = free {
            return Ok(MutationLease::new(control, execution));
        }
        let started = Instant::now();
        loop {
            if let Some(execution) = jobs::try_execution_lock(dir)? {
                JobQueue::mutate(&self.work_dir, |queue| {
                    queue.try_complete_cancellation(session)
                })
                .map_err(storage_error)?;
                return Ok(MutationLease::new(control, execution));
            }
            anyhow::ensure!(started.elapsed()<timeout,
                "остановка ещё не подтверждена: сессия остаётся занятой; файлы сохранены, удаление не выполнялось");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn cancel(&self, session: &str) -> Result<String> {
        let dir = self.session_dir(session)?;
        let _lease = self.stopped_lease(session, &dir, Duration::from_secs(5))?;
        Ok("Обработка отменена; сохранённые результаты доступны. Продолжение — вручную".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    #[test]
    fn deletion_refuses_a_remaining_capture_writer_after_marker_disappears() {
        let (root, archive, session) = fixture();
        let before = fs::read(root.path().join("jobs.json")).unwrap();
        let _capture = localvox_light_core::chunks::capture::try_mutation_lock(&session)
            .unwrap()
            .unwrap();
        assert!(archive.delete_session("fixture", "fixture").is_err());
        assert!(session.join("retained.txt").is_file());
        assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), before);
    }

    #[test]
    fn deletion_after_stop_waits_for_the_real_capture_planner_owner() {
        use localvox_light_core::chunks::{self, capture, codec, planning};
        use std::sync::{Arc, Mutex};
        let (root, archive, session) = fixture();
        fs::create_dir(session.join("audio")).unwrap();
        let generation = "0199a774-0000-7000-8000-000000000001".to_string();
        fs::write(
            session.join("meta.json"),
            serde_json::to_vec(&chunks::SessionMeta {
                capture_generation: Some(generation.clone()),
                sample_rate: 16000,
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        let mut writer = capture::Writer::begin(&session, generation).unwrap();
        writer.open(0, 1, 0).unwrap();
        fs::write(
            session.join("audio/src0_chunk0001.wav"),
            wav_16k_mono(&[1, 0, 2, 0]),
        )
        .unwrap();
        writer
            .close(
                0,
                capture::Range {
                    sequence: 1,
                    start_sample: 0,
                    samples: 2,
                    kind: capture::Kind::Audio,
                    problem: None,
                },
            )
            .unwrap();
        writer
            .end("test Stop", false, [(0, 2), (1, 0)].into())
            .unwrap();
        codec::with_read(&session.join("audio"), "src0_chunk0001", |_| Ok(())).unwrap();
        let sidecar = fs::read_dir(session.join(".audio-locks"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path();
        let blocked = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(sidecar)
            .unwrap();
        blocked.lock().unwrap();
        let group = planning::Planners::default();
        let handle = group
            .attach(Arc::new(Mutex::new(writer)), "unused-for-wav".into())
            .unwrap();
        drop(handle); // Stop notification; planner still owns capture until the read exits
        let before = fs::read(root.path().join("jobs.json")).unwrap();
        assert!(archive.delete_session("fixture", "fixture").is_err());
        assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), before);
        assert!(session.join("retained.txt").is_file());
        drop(group);
        assert!(capture::try_mutation_lock(&session).unwrap().is_some());
        drop(blocked);
        archive.delete_session("fixture", "fixture").unwrap();
        assert!(!session.exists());
    }

    fn fixture() -> (tempfile::TempDir, Archive, PathBuf) {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions/fixture");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("retained.txt"), b"retained transcript bytes").unwrap();
        JobQueue::mutate(root.path(), |queue| {
            queue.enqueue_cook("fixture", true, false, false)
        });
        let archive = Archive::new(root.path().to_path_buf());
        (root, archive, session)
    }
    fn owner(root: &Path, session: &Path) -> control::ManagedExecution {
        JobQueue::mutate(root, |queue| {
            let job = queue.start(0).unwrap();
            control::ManagedExecution::register(
                session,
                &job,
                jobs::try_execution_lock(session).unwrap().unwrap(),
            )
        })
    }

    #[test]
    fn recording_and_wrong_confirmation_never_signal_a_managed_owner() {
        let (root, archive, session) = fixture();
        let execution = owner(root.path(), &session);
        let before = fs::read(root.path().join("jobs.json")).unwrap();
        assert!(archive.delete_session("fixture", "wrong").is_err());
        fs::write(root.path().join(jobs::RECORDING_MARKER), "fixture").unwrap();
        assert!(archive.cancel("fixture").is_err());
        assert!(archive.delete_session("fixture", "fixture").is_err());
        assert!(!execution.cancelled());
        assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), before);
        assert!(!root.path().join(jobs::RECORD_STOP_MARKER).exists());
        assert!(session.join("retained.txt").exists());
    }

    #[test]
    fn cancellation_waits_for_a_managed_lease_and_preserves_files_then_can_resume() {
        let (root, archive, session) = fixture();
        let execution = owner(root.path(), &session);
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !execution.cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(execution.cancelled());
            drop(execution);
        });
        archive.cancel("fixture").unwrap();
        worker.join().unwrap();
        let job = JobQueue::load(root.path()).jobs()[0].clone();
        assert_eq!(job.state, jobs::JobState::Cancelled);
        assert_eq!(
            fs::read(session.join("retained.txt")).unwrap(),
            b"retained transcript bytes"
        );
        assert!(jobs::try_execution_lock(&session).unwrap().is_some());
        let status = archive.jobs_status().unwrap();
        assert_eq!(status.cancelled, 1);
        assert_eq!(status.failed, 0);
        assert_eq!(status.running, 0);
        archive.retry("fixture").unwrap();
        assert_eq!(
            JobQueue::load(root.path()).jobs()[0].state,
            jobs::JobState::Pending
        );
    }

    #[test]
    fn deletion_stops_its_managed_owner_before_removing_any_source() {
        let (root, archive, session) = fixture();
        let execution = owner(root.path(), &session);
        let source = session.clone();
        let saw_stop = std::sync::Arc::new(AtomicBool::new(false));
        let confirmed = saw_stop.clone();
        let worker = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while !execution.cancelled() && Instant::now() < deadline {
                std::thread::sleep(Duration::from_millis(5));
            }
            assert!(execution.cancelled());
            assert_eq!(
                fs::read(source.join("retained.txt")).unwrap(),
                b"retained transcript bytes"
            );
            confirmed.store(true, Ordering::Relaxed);
            drop(execution);
        });
        archive.delete_session("fixture", "fixture").unwrap();
        worker.join().unwrap();
        assert!(saw_stop.load(Ordering::Relaxed));
        assert!(!session.exists());
        assert!(JobQueue::load(root.path()).jobs().is_empty());
    }

    #[test]
    fn timeout_never_deletes_later_when_the_lease_is_finally_released() {
        let (root, archive, session) = fixture();
        let execution = owner(root.path(), &session);
        assert!(archive
            .stopped_lease("fixture", &session, Duration::from_millis(30))
            .is_err());
        assert!(execution.cancelled());
        assert!(session.join("retained.txt").exists());
        assert_eq!(
            JobQueue::load(root.path()).jobs()[0].state,
            jobs::JobState::Cancelling
        );
        drop(execution);
        JobQueue::mutate(root.path(), |queue| {
            queue.try_reconcile_cancellations(root.path())
        })
        .unwrap();
        assert_eq!(
            JobQueue::load(root.path()).jobs()[0].state,
            jobs::JobState::Cancelled
        );
        assert_eq!(
            fs::read(session.join("retained.txt")).unwrap(),
            b"retained transcript bytes"
        );
    }

    #[test]
    fn unmanaged_cli_and_parallel_api_control_conflicts_leave_queue_and_sources_untouched() {
        let (root, archive, session) = fixture();
        let before = fs::read(root.path().join("jobs.json")).unwrap();
        let cli = jobs::try_execution_lock(&session).unwrap().unwrap();
        assert!(archive
            .cancel("fixture")
            .unwrap_err()
            .to_string()
            .contains("CLI"));
        assert!(archive.delete_session("fixture", "fixture").is_err());
        assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), before);
        drop(cli);
        let pending_control = control::try_control_lock(&session).unwrap().unwrap();
        assert!(archive.cancel("fixture").is_err());
        assert!(archive.retry("fixture").is_err());
        assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), before);
        drop(pending_control);
        assert_eq!(
            fs::read(session.join("retained.txt")).unwrap(),
            b"retained transcript bytes"
        );
    }

    #[cfg(windows)]
    #[test]
    fn unsaved_stop_never_signals_the_owner_or_starts_deletion() {
        use std::os::windows::fs::OpenOptionsExt;
        for managed in [false, true] {
            let (root, archive, session) = fixture();
            let execution = managed.then(|| owner(root.path(), &session));
            let before = fs::read(root.path().join("jobs.json")).unwrap();
            let denied = fs::OpenOptions::new()
                .read(true)
                .share_mode(3)
                .open(root.path().join("jobs.json"))
                .unwrap();
            assert!(archive.cancel("fixture").is_err());
            assert!(archive.delete_session("fixture", "fixture").is_err());
            assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), before);
            if let Some(execution) = &execution {
                assert!(!execution.cancelled())
            }
            assert_eq!(
                fs::read(session.join("retained.txt")).unwrap(),
                b"retained transcript bytes"
            );
            drop(denied);
            drop(execution);
        }
    }

    #[cfg(windows)]
    #[test]
    fn failed_stop_acknowledgement_keeps_files_and_durable_intent_until_recovery() {
        use std::os::windows::fs::OpenOptionsExt;
        for delete in [false, true] {
            let (root, archive, session) = fixture();
            let execution = owner(root.path(), &session);
            let path = root.path().join("jobs.json");
            let (sender, receiver) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !execution.cancelled() && Instant::now() < deadline {
                    std::thread::sleep(Duration::from_millis(5));
                }
                assert!(execution.cancelled());
                let denied = fs::OpenOptions::new()
                    .read(true)
                    .share_mode(3)
                    .open(path)
                    .unwrap();
                drop(execution);
                sender.send(denied).unwrap();
            });
            let result = if delete {
                archive.delete_session("fixture", "fixture")
            } else {
                archive.cancel("fixture")
            };
            assert!(result.unwrap_err().is::<QueueUnavailable>());
            let denied = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            worker.join().unwrap();
            assert_eq!(
                JobQueue::load(root.path()).jobs()[0].state,
                jobs::JobState::Cancelling
            );
            assert_eq!(
                fs::read(session.join("retained.txt")).unwrap(),
                b"retained transcript bytes"
            );
            drop(denied);
            JobQueue::mutate(root.path(), |queue| {
                queue.try_reconcile_cancellations(root.path())
            })
            .unwrap();
            assert_eq!(
                JobQueue::load(root.path()).jobs()[0].state,
                jobs::JobState::Cancelled
            );
            assert!(
                session.join("retained.txt").exists(),
                "recovery must never perform delayed deletion"
            );
        }
    }

    #[cfg(windows)]
    #[test]
    fn failed_physical_delete_keeps_a_cancelled_fence_instead_of_resurrecting_the_job() {
        use std::os::windows::fs::OpenOptionsExt;
        let (root, archive, session) = fixture();
        let retained = fs::OpenOptions::new()
            .read(true)
            .share_mode(3)
            .open(session.join("retained.txt"))
            .unwrap();
        assert!(archive.delete_session("fixture", "fixture").is_err());
        assert_eq!(
            fs::read(session.join("retained.txt")).unwrap(),
            b"retained transcript bytes"
        );
        let mut queue = JobQueue::load(root.path());
        assert_eq!(queue.jobs()[0].state, jobs::JobState::Cancelled);
        assert!(!queue.enqueue_cook("fixture", true, false, false));
        drop(retained);
        archive.delete_session("fixture", "fixture").unwrap();
        assert!(!session.exists());
        assert!(JobQueue::load(root.path()).jobs().is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn alternate_case_cannot_create_a_second_tombstone_for_a_pending_session() {
        let (root, archive, session) = fixture();
        let before = fs::read(root.path().join("jobs.json")).unwrap();
        assert!(archive.cancel("FIXTURE").is_err());
        assert_eq!(fs::read(root.path().join("jobs.json")).unwrap(), before);
        assert!(session.join("retained.txt").exists());
    }
}
