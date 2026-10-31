//! Read-only archive discovery and guarded publication on a fresh queue.
use super::{Job, JobQueue, JobState, Wanted};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs, io,
    path::{Path, PathBuf},
    time::SystemTime,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Cook,
    Artifacts,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Revision {
    metadata: [Option<[u8; 32]>; 3],
    audio: Vec<(String, u64, Option<SystemTime>)>,
    created_at: Option<SystemTime>,
}
impl Revision {
    fn read(dir: &Path) -> io::Result<Self> {
        let directory = fs::metadata(dir)?;
        if !directory.is_dir() {
            return Err(io::ErrorKind::NotADirectory.into());
        }
        let mut metadata = [None; 3];
        for (index, name) in ["meta.json", "versions.json", "processing.json"]
            .iter()
            .enumerate()
        {
            metadata[index] = match fs::read(dir.join(name)) {
                Ok(bytes) => Some(Sha256::digest(bytes).into()),
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(error) => return Err(error),
            };
        }
        let mut audio = vec![];
        match fs::read_dir(dir.join("audio")) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry?;
                    if matches!(
                        entry.path().extension().and_then(|s| s.to_str()),
                        Some("wav" | "flac" | "part")
                    ) {
                        let stamp = entry.metadata()?;
                        audio.push((
                            entry.file_name().to_string_lossy().into_owned(),
                            stamp.len(),
                            stamp.modified().ok(),
                        ));
                    }
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        audio.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(Self {
            metadata,
            audio,
            created_at: directory.created().ok(),
        })
    }
}

#[derive(Debug)]
pub struct Candidate {
    session: String,
    kind: Kind,
    work_dir: PathBuf,
    expected_job: Option<Job>,
    revision: Revision,
}
impl Candidate {
    pub fn session(&self) -> &str {
        &self.session
    }
    pub fn kind(&self) -> Kind {
        self.kind
    }

    /// Called under JobQueue::mutate. No transcript reads or receipt verification
    /// here: compare the observed generation, then publish the unchanged candidate.
    pub fn publish(&self, queue: &mut JobQueue, flags: (bool, bool, bool)) -> bool {
        if queue.path != self.work_dir.join("jobs.json")
            || queue.error().is_some()
            || queue.jobs().iter().find(|job| job.session == self.session)
                != self.expected_job.as_ref()
            || super::recording_session(&self.work_dir).as_deref() == Some(self.session.as_str())
        {
            return false;
        }
        let dir = self.work_dir.join("sessions").join(&self.session);
        // API mutations acquire the same nonblocking lease. A deleted directory
        // cannot be recreated by opening this lock, and a busy session is deferred.
        let Ok(Some(_lease)) = super::try_execution_lock(&dir) else {
            return false;
        };
        if Revision::read(&dir).ok().as_ref() != Some(&self.revision) {
            return false;
        }
        if super::recording_session(&self.work_dir).as_deref() == Some(self.session.as_str()) {
            return false;
        }
        let (summary, cleanup, refine) = flags;
        match self.kind {
            Kind::Cook => queue.enqueue_cook(&self.session, summary, cleanup, refine),
            Kind::Artifacts if summary || cleanup || refine => {
                queue.requeue_for_artifacts(&self.session, summary, cleanup, refine)
            }
            Kind::Artifacts => false,
        }
    }
}

/// Read-only snapshot. Must run outside the queue mutex; publication validates
/// both the job and metadata again, so a concurrent completion cannot be replayed.
pub fn scan(work_dir: &Path, quiescent_secs: u64, wanted: &Wanted) -> Vec<Candidate> {
    let queue = JobQueue::load(work_dir);
    if queue.error().is_some() {
        return vec![];
    }
    let expected: HashMap<_, _> = queue
        .jobs()
        .iter()
        .map(|job| (job.session.clone(), job.clone()))
        .collect();
    let Ok(entries) = fs::read_dir(work_dir.join("sessions")) else {
        return vec![];
    };
    let mut revisions = HashMap::new();
    for entry in entries.flatten() {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if expected
            .get(&name)
            .is_some_and(|job| job.state != JobState::Done)
        {
            continue;
        }
        if let Ok(revision) = Revision::read(&entry.path()) {
            revisions.insert(name, revision);
        }
    }
    let cooks = super::sessions_needing_cook(work_dir, quiescent_secs);
    let artifacts = super::sessions_needing_artifacts(work_dir, wanted);
    let mut seen = HashSet::new();
    cooks
        .into_iter()
        .map(|name| (name, Kind::Cook))
        .chain(artifacts.into_iter().map(|name| (name, Kind::Artifacts)))
        .filter_map(|(session, kind)| {
            if !seen.insert(session.clone()) {
                return None;
            }
            Some(Candidate {
                revision: revisions.remove(&session)?,
                expected_job: expected.get(&session).cloned(),
                session,
                kind,
                work_dir: work_dir.to_path_buf(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests;
