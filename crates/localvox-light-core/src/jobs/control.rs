//! In-process stop signals identify a managed lease, never an arbitrary PID.
//! Durable intent remains in JobQueue; a signal alone is not a stop acknowledgement.
use super::*;
use std::{
    collections::HashMap,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, OnceLock, Weak,
    },
};

struct Entry {
    job_id: u64,
    execution_id: Option<String>,
    stop: Weak<AtomicBool>,
}
fn registry() -> &'static Mutex<HashMap<PathBuf, Entry>> {
    static REGISTRY: OnceLock<Mutex<HashMap<PathBuf, Entry>>> = OnceLock::new();
    REGISTRY.get_or_init(Mutex::default)
}
fn key(session: &Path) -> PathBuf {
    session
        .canonicalize()
        .unwrap_or_else(|_| session.to_path_buf())
}

/// Held across waiting for a managed worker; other API mutations fail immediately.
pub fn try_control_lock(session: &Path) -> std::io::Result<Option<fs::File>> {
    super::try_lock_file(&session.join(".mutation.lock"))
}

/// Both guards remain held until all changes belonging to one API operation finish.
pub struct MutationLease {
    _control: fs::File,
    _execution: fs::File,
}
impl MutationLease {
    pub fn new(control: fs::File, execution: fs::File) -> Self {
        Self {
            _control: control,
            _execution: execution,
        }
    }
    pub fn try_acquire(session: &Path) -> std::io::Result<Option<Self>> {
        let Some(control) = try_control_lock(session)? else {
            return Ok(None);
        };
        Ok(super::try_execution_lock(session)?.map(|execution| Self::new(control, execution)))
    }
}

/// The coordinator owns this guard from saved claim through outcome and process cleanup.
pub struct ManagedExecution {
    session: PathBuf,
    lease: Option<fs::File>,
    stop: Arc<AtomicBool>,
}
impl ManagedExecution {
    /// Caller holds the queue mutation mutex so an API cannot miss a newly saved owner.
    pub fn register(session: &Path, job: &Job, lease: fs::File) -> Self {
        let session = key(session);
        let stop = Arc::new(AtomicBool::new(false));
        registry().lock().unwrap_or_else(|e| e.into_inner()).insert(
            session.clone(),
            Entry {
                job_id: job.id,
                execution_id: job.execution_id.clone(),
                stop: Arc::downgrade(&stop),
            },
        );
        Self {
            session,
            lease: Some(lease),
            stop,
        }
    }
    pub fn cancelled(&self) -> bool {
        self.stop.load(Ordering::Relaxed)
    }
}
impl Drop for ManagedExecution {
    fn drop(&mut self) {
        let mut entries = registry().lock().unwrap_or_else(|e| e.into_inner());
        // Lookup cannot observe a stale owner between releasing the OS lease and unregistering.
        drop(self.lease.take());
        let own = Arc::downgrade(&self.stop);
        if entries
            .get(&self.session)
            .is_some_and(|entry| entry.stop.ptr_eq(&own))
        {
            entries.remove(&self.session);
        }
    }
}

pub struct StopHandle {
    job_id: u64,
    execution_id: Option<String>,
    stop: Arc<AtomicBool>,
}
impl StopHandle {
    pub fn matches(&self, job: &Job) -> bool {
        self.job_id == job.id && self.execution_id == job.execution_id
    }
    /// Only call after the corresponding cancellation fence was durably accepted.
    pub fn signal(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}
pub fn managed_owner(session: &Path) -> Option<StopHandle> {
    let entries = registry().lock().unwrap_or_else(|e| e.into_inner());
    let entry = entries.get(&key(session))?;
    Some(StopHandle {
        job_id: entry.job_id,
        execution_id: entry.execution_id.clone(),
        stop: entry.stop.upgrade()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_handle_matches_one_execution_and_does_not_replace_the_lease() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("sessions/fixture")).unwrap();
        let session = root.path().join("sessions/fixture");
        JobQueue::mutate(root.path(), |queue| {
            queue.enqueue_cook("fixture", false, false, false)
        });
        let job = JobQueue::mutate(root.path(), |queue| {
            queue.start_phase(0, &[JobPhase::Prepare])
        })
        .unwrap();
        let owner = ManagedExecution::register(
            &session,
            &job,
            super::super::try_execution_lock(&session).unwrap().unwrap(),
        );
        let handle = managed_owner(&session).unwrap();
        assert!(handle.matches(&job));
        let mut other = job.clone();
        other.execution_id = Some("other".into());
        assert!(!handle.matches(&other));
        assert!(!owner.cancelled());
        handle.signal();
        assert!(owner.cancelled());
        assert!(super::super::try_execution_lock(&session)
            .unwrap()
            .is_none());
        drop(owner);
        assert!(managed_owner(&session).is_none());
        assert!(super::super::try_execution_lock(&session)
            .unwrap()
            .is_some());
    }
    #[test]
    fn mutation_guard_is_exclusive_without_waiting_for_a_worker() {
        let root = tempfile::tempdir().unwrap();
        let control = try_control_lock(root.path()).unwrap().unwrap();
        assert!(MutationLease::try_acquire(root.path()).unwrap().is_none());
        drop(control);
        let worker = super::super::try_execution_lock(root.path())
            .unwrap()
            .unwrap();
        assert!(MutationLease::try_acquire(root.path()).unwrap().is_none());
        // A failed attempt releases the independent control lease.
        assert!(try_control_lock(root.path()).unwrap().is_some());
        drop(worker);
        assert!(MutationLease::try_acquire(root.path()).unwrap().is_some());
    }
}
