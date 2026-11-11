//! Verified incremental ASR inputs/results; callers own the session execution lease.
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use crate::chunks::{capture, planning};
use crate::windowing::pcm_sha256 as pcm_hash;
use crate::windowing::WindowKind;

mod audio;
mod cache;
mod media;
mod publication;
mod wal;
pub use audio::{consume_windows, AudioStats};
pub use cache::{Accepted, Cache, Checkpoint, Output, Transaction, VerifiedWindow};
pub use media::AudioReader;
pub use publication::{FrozenInput, PartialLine, Published, Snapshot};

#[cfg(test)]
mod test_support;

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn is_hash(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn bytes_file(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)?.take(limit + 1).read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() as u64 <= limit,
        "incremental metadata exceeds its limit"
    );
    Ok(bytes)
}
fn json_file<T: serde::de::DeserializeOwned>(path: &Path, limit: u64) -> Result<T> {
    Ok(serde_json::from_slice(&bytes_file(path, limit)?)?)
}

/// Exact engine/recipe and content fingerprints. Paths/mtimes are not model identity.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Identity {
    pub revision: u32,
    pub engine: String,
    pub language: String,
    pub recipe: String,
    pub files: BTreeMap<String, String>,
}
impl Identity {
    pub fn from_files(
        engine: String,
        language: String,
        recipe: String,
        files: BTreeMap<String, PathBuf>,
        cancelled: impl Fn() -> bool,
    ) -> Result<Self> {
        let mut hashes = BTreeMap::new();
        for (role, path) in files {
            let mut file = File::open(&path)
                .with_context(|| format!("model dependency {}", path.display()))?;
            let mut digest = Sha256::new();
            let mut buf = [0; 64 * 1024];
            loop {
                ensure!(!cancelled(), "model fingerprint cancelled");
                let n = file.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                digest.update(&buf[..n]);
            }
            hashes.insert(role, format!("{:x}", digest.finalize()));
        }
        let identity = Self {
            revision: 1,
            engine,
            language,
            recipe,
            files: hashes,
        };
        identity.validate()?;
        Ok(identity)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.revision == 1
                && !self.engine.is_empty()
                && !self.language.is_empty()
                && !self.recipe.is_empty()
                && !self.files.is_empty()
                && self
                    .files
                    .iter()
                    .all(|(role, sha)| !role.is_empty() && is_hash(sha)),
            "invalid ASR cache identity"
        );
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<String> {
        self.validate()?;
        Ok(hash(&serde_json::to_vec(self)?))
    }
}

/// Finite, structurally checked capture horizon. PCM is verified separately at use time.
pub struct Horizon {
    input: capture::Snapshot,
    plan: planning::Snapshot,
    windows: BTreeMap<u8, Vec<planning::Descriptor>>,
}
impl Horizon {
    pub fn read(session: &Path) -> Result<Self> {
        Self::read_plan(session, Path::new(planning::FILE))
    }
    pub fn read_plan(session: &Path, plan_path: &Path) -> Result<Self> {
        // The later primary snapshot may include newer seals, never fewer than the plan.
        let plan: planning::Snapshot = json_file(&session.join(plan_path), 8 * 1024 * 1024)?;
        let input = capture::read(session)?;
        ensure!(
            plan.version == 1
                && plan.generation == input.generation
                && plan.capture_revision <= input.revision
                && plan.sources.keys().copied().collect::<Vec<_>>() == [0, 1],
            "plan does not match capture generation/revision"
        );
        let recipe = plan.recipe.fingerprint()?;
        let mut windows = BTreeMap::new();
        for (id, source) in &plan.sources {
            let captured = &input.sources[id];
            ensure!(
                source.sequence as usize <= captured.ranges.len(),
                "plan consumed nonexistent seals"
            );
            let ranges = &captured.ranges[..source.sequence as usize];
            let end = ranges.last().map_or(0, |r| r.start_sample + r.samples);
            ensure!(
                ranges.iter().all(|r| r.kind != capture::Kind::Unknown)
                    && source.input_end == end
                    && source.closed_end <= end,
                "plan crossed unconfirmed input"
            );
            if source.eof {
                ensure!(
                    input.terminal.is_some()
                        && source.sequence as usize == captured.ranges.len()
                        && source.closed_end == end,
                    "plan invented source EOF"
                );
            }
            let mut at = 0;
            let mut records = Vec::new();
            for reference in &source.windows {
                ensure!(
                    reference.start_sample == at
                        && reference.end_sample > at
                        && reference.end_sample <= source.closed_end
                        && is_hash(&reference.sha256),
                    "plan window gap/overlap"
                );
                let path = session.join(format!(
                    "capture/plan/{recipe}/src{id}_{}-{}.json",
                    reference.start_sample, reference.end_sample
                ));
                let bytes = bytes_file(&path, 16384)?;
                ensure!(
                    bytes.len() <= 16384 && hash(&bytes) == reference.sha256,
                    "plan descriptor checksum changed"
                );
                let record: planning::Descriptor = serde_json::from_slice(&bytes)?;
                validate_descriptor(&record)?;
                ensure!(
                    record.generation == plan.generation
                        && record.recipe == recipe
                        && record.source == *id
                        && record.start_sample == at
                        && record.end_sample == reference.end_sample
                        && record.end_sample - record.start_sample <= plan.recipe.max_samples + 320,
                    "plan descriptor identity/range changed"
                );
                at = record.end_sample;
                records.push(record);
            }
            ensure!(
                at == source.closed_end,
                "plan references do not cover its frontier"
            );
            windows.insert(*id, records);
        }
        let common = common_frontier(plan.sources.values().map(|s| (s.closed_end, s.eof)));
        ensure!(
            plan.common_end == common && plan.complete == plan.sources.values().all(|s| s.eof),
            "invalid plan common frontier"
        );
        Ok(Self {
            input,
            plan,
            windows,
        })
    }
    pub fn generation(&self) -> &str {
        &self.input.generation
    }
    pub fn windows(&self) -> &BTreeMap<u8, Vec<planning::Descriptor>> {
        &self.windows
    }
    pub fn complete(&self) -> bool {
        self.plan.complete
    }
    pub fn revision(&self) -> u64 {
        self.plan.capture_revision
    }
    pub fn recipe(&self) -> &planning::Recipe {
        &self.plan.recipe
    }
    fn audio_end(&self) -> u64 {
        common_frontier(
            self.input
                .sources
                .values()
                .map(|s| (s.confirmed_end(), self.input.terminal.is_some())),
        )
    }
}
fn validate_descriptor(d: &planning::Descriptor) -> Result<()> {
    uuid::Uuid::parse_str(&d.generation)?;
    ensure!(
        d.version == 1
            && d.source <= 1
            && d.end_sample > d.start_sample
            && d.end_sample - d.start_sample <= 200 * 16000 + 320
            && is_hash(&d.recipe)
            && is_hash(&d.pcm_sha256),
        "invalid window descriptor"
    );
    Ok(())
}
fn common_frontier(values: impl Iterator<Item = (u64, bool)>) -> u64 {
    let mut active: Option<u64> = None;
    let mut max = 0;
    for (end, ended) in values {
        max = max.max(end);
        if !ended {
            active = Some(active.map_or(end, |v| v.min(end)));
        }
    }
    active.unwrap_or(max)
}
