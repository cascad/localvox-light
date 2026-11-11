use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Output {
    Text,
    Empty,
    Silence,
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    version: u32,
    input: planning::Descriptor,
    recognizer: String,
    outcome: Output,
    raw_text: Option<String>,
    text: Option<String>,
}
impl Record {
    fn validate(&self, input: &planning::Descriptor, identity: &str) -> Result<()> {
        ensure!(
            self.version == 1 && self.input == *input && self.recognizer == identity,
            "foreign ASR cache record"
        );
        let (outcome, text) = output(input.kind, self.raw_text.as_deref())?;
        ensure!(
            self.outcome == outcome && self.text == text,
            "ASR cache output/filter mismatch"
        );
        Ok(())
    }
}
fn output(kind: WindowKind, raw: Option<&str>) -> Result<(Output, Option<String>)> {
    match kind {
        WindowKind::Silence => {
            ensure!(raw.is_none(), "silent window has an invented ASR answer");
            Ok((Output::Silence, None))
        }
        WindowKind::Speech => {
            let text = raw.context("speech window has no ASR outcome")?.trim();
            Ok(if crate::asr::has_real_word(text) {
                (Output::Text, Some(text.into()))
            } else {
                (Output::Empty, None)
            })
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope<T> {
    sha256: String,
    value: T,
}
fn envelope<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    Ok(serde_json::to_vec(&Envelope {
        sha256: hash(&serde_json::to_vec(value)?),
        value,
    })?)
}
fn read_envelope<T: serde::de::DeserializeOwned + Serialize>(path: &Path) -> Result<(T, String)> {
    decode_envelope(&bytes_file(path, 8 * 1024 * 1024)?)
}
fn decode_envelope<T: serde::de::DeserializeOwned + Serialize>(
    bytes: &[u8],
) -> Result<(T, String)> {
    let e: Envelope<T> = serde_json::from_slice(bytes)?;
    ensure!(
        is_hash(&e.sha256) && hash(&serde_json::to_vec(&e.value)?) == e.sha256,
        "ASR cache checksum mismatch"
    );
    Ok((e.value, e.sha256))
}
fn optional_envelope<T: serde::de::DeserializeOwned + Serialize>(
    path: &Path,
) -> Result<Option<(T, String)>> {
    match read_envelope(path) {
        Ok(record) => Ok(Some(record)),
        Err(error) => {
            if let Some(io) = error.downcast_ref::<std::io::Error>() {
                if io.kind() != std::io::ErrorKind::NotFound {
                    return Err(error);
                }
            }
            Ok(None) // missing, partial, checksum-invalid or unsupported derived data
        }
    }
}

/// Only verified PCM can produce a cache hit or new accepted result.
pub struct VerifiedWindow<'a> {
    input: &'a planning::Descriptor,
    pcm: &'a [i16],
}
impl<'a> VerifiedWindow<'a> {
    pub fn descriptor(&self) -> &planning::Descriptor {
        self.input
    }
    pub fn pcm(&self) -> &[i16] {
        self.pcm
    }
    pub fn new(input: &'a planning::Descriptor, pcm: &'a [i16]) -> Result<Self> {
        validate_descriptor(input)?;
        ensure!(
            input.end_sample - input.start_sample == pcm.len() as u64
                && pcm_hash(pcm) == input.pcm_sha256,
            "window PCM differs from the planned input"
        );
        Ok(Self { input, pcm })
    }
}

#[derive(Clone, Debug)]
pub struct Accepted {
    key: String,
    sha256: String,
    record: Record,
}
impl Accepted {
    pub fn descriptor(&self) -> &planning::Descriptor {
        &self.record.input
    }
    pub fn text(&self) -> Option<&str> {
        self.record.text.as_deref()
    }
    pub fn outcome(&self) -> &Output {
        &self.record.outcome
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reference {
    key: String,
    sha256: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceCheckpoint {
    pub end_sample: u64,
    pub eof: bool,
    records: Vec<Reference>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub version: u32,
    pub generation: String,
    pub recognizer: String,
    pub plan_recipe: String,
    pub capture_revision: u64,
    pub sources: BTreeMap<u8, SourceCheckpoint>,
    pub common_end: u64,
    /// Stable ordering watermark, additionally constrained by available audio.
    pub publication_end: u64,
    pub complete: bool,
}
impl Checkpoint {
    pub fn fingerprint(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(self)?))
    }
}

/// Like VersionStore, writes require the caller's session execution ownership.
/// Managed workers are covered by their supervising coordinator's lease.
pub struct Cache {
    root: PathBuf,
    identity: String,
    wal: std::sync::Mutex<super::wal::Store>,
}

/// A verified disk snapshot for one held execution lease and one immutable
/// horizon. It is neither serializable nor retained by the resident ASR engine.
pub struct Transaction<'a> {
    pub(super) cache: &'a Cache,
    pub(super) horizon: &'a Horizon,
    pub(super) state: Checkpoint,
    pub(super) records: Vec<Accepted>,
}
impl Transaction<'_> {
    pub fn state(&self) -> &Checkpoint {
        &self.state
    }
}
impl Cache {
    pub fn open(session: &Path, identity: &Identity) -> Result<Self> {
        let metadata = serde_json::to_vec(identity)?;
        let identity = identity.fingerprint()?;
        let root = session.join("incremental").join(&identity);
        let created = !root.exists();
        fs::create_dir_all(root.join("windows"))?;
        let metadata_path = root.join("identity.json");
        if !fs::read(&metadata_path).is_ok_and(|bytes| bytes == metadata) {
            crate::artifacts::atomic_write(&metadata_path, &metadata)?;
        }
        let wal = std::sync::Mutex::new(super::wal::Store::open(root.join("results"))?);
        #[cfg(unix)]
        if created {
            File::open(root.parent().context("cache parent")?)?.sync_all()?;
            File::open(session)?.sync_all()?;
        }
        #[cfg(not(unix))]
        let _ = created;
        Ok(Self {
            root,
            identity,
            wal,
        })
    }
    fn key(&self, d: &planning::Descriptor) -> Result<String> {
        validate_descriptor(d)?;
        Ok(hash(&serde_json::to_vec(&(1, &self.identity, d))?))
    }
    fn record(&self, d: &planning::Descriptor) -> Result<Option<Accepted>> {
        let key = self.key(d)?;
        if let Some(bytes) = self
            .wal
            .lock()
            .map_err(|_| anyhow::anyhow!("result journal lock poisoned"))?
            .get(&key)?
        {
            if let Ok((record, sha256)) = decode_envelope::<Record>(&bytes) {
                if record.validate(d, &self.identity).is_ok() {
                    return Ok(Some(Accepted {
                        key,
                        sha256,
                        record,
                    }));
                }
            }
        }
        let Some((record, sha256)) =
            optional_envelope::<Record>(&self.root.join("windows").join(format!("{key}.json")))?
        else {
            return Ok(None);
        };
        if record.validate(d, &self.identity).is_err() {
            return Ok(None);
        }
        Ok(Some(Accepted {
            key,
            sha256,
            record,
        }))
    }
    /// Returns whether actual ASR was avoided. Silence does not call the recognizer.
    /// A decode/recognizer/publish failure never becomes an Empty result.
    pub fn resolve(
        &self,
        window: VerifiedWindow<'_>,
        mut recognize: impl FnMut(&[i16]) -> Result<String>,
    ) -> Result<(Accepted, bool)> {
        if let Some(accepted) = self.lookup(&window)? {
            return Ok((accepted, true));
        }
        let raw = match window.input.kind {
            WindowKind::Speech => Some(recognize(window.pcm)?),
            WindowKind::Silence => None,
        };
        let (outcome, text) = output(window.input.kind, raw.as_deref())?;
        let record = Record {
            version: 1,
            input: window.input.clone(),
            recognizer: self.identity.clone(),
            outcome,
            raw_text: raw,
            text,
        };
        record.validate(window.input, &self.identity)?;
        let key = self.key(window.input)?;
        // Only missing/corrupt derived data is replaced; a valid record returned above is immutable.
        let bytes = envelope(&record)?;
        ensure!(
            bytes.len() <= 8 * 1024 * 1024,
            "ASR cache record exceeds its limit"
        );
        self.wal
            .lock()
            .map_err(|_| anyhow::anyhow!("result journal lock poisoned"))?
            .put(&key, &bytes)?;
        let accepted = self
            .record(window.input)?
            .context("published ASR result failed verification")?;
        Ok((accepted, false))
    }
    /// Read-only lookup against already verified actual PCM; never repairs a miss.
    pub fn lookup(&self, window: &VerifiedWindow<'_>) -> Result<Option<Accepted>> {
        self.record(window.input)
    }
    fn empty(&self, horizon: &Horizon) -> Result<Checkpoint> {
        Ok(Checkpoint {
            version: 1,
            generation: horizon.generation().into(),
            recognizer: self.identity.clone(),
            plan_recipe: horizon.plan.recipe.fingerprint()?,
            capture_revision: horizon.plan.capture_revision,
            sources: [
                (0, SourceCheckpoint::default()),
                (1, SourceCheckpoint::default()),
            ]
            .into(),
            common_end: 0,
            publication_end: 0,
            complete: false,
        })
    }
    fn previous(&self, horizon: &Horizon) -> Result<Option<Checkpoint>> {
        let Some((state, _)) = optional_envelope::<Checkpoint>(&self.root.join("checkpoint.json"))?
        else {
            return Ok(None);
        };
        Ok((state.version == 1
            && state.generation == horizon.generation()
            && state.recognizer == self.identity
            && state.plan_recipe == horizon.plan.recipe.fingerprint()?
            && state.sources.keys().copied().collect::<Vec<_>>() == [0, 1])
        .then_some(state))
    }
    /// Carries only previously committed, checksum-verified prefix records. It never
    /// adopts a result left before checkpoint commit without fresh VerifiedWindow.
    fn restore(&self, horizon: &Horizon) -> Result<Vec<Accepted>> {
        self.wal
            .lock()
            .map_err(|_| anyhow::anyhow!("result journal lock poisoned"))?
            .refresh()?;
        let Some(previous) = self.previous(horizon)? else {
            return Ok(Vec::new());
        };
        let mut accepted = Vec::new();
        for (id, windows) in &horizon.windows {
            for (window, reference) in windows.iter().zip(&previous.sources[id].records) {
                let Some(record) = self
                    .record(window)?
                    .filter(|r| r.key == reference.key && r.sha256 == reference.sha256)
                else {
                    break;
                };
                accepted.push(record);
            }
        }
        Ok(accepted)
    }
    fn derive(&self, horizon: &Horizon, records: &[Accepted]) -> Result<Checkpoint> {
        let mut state = self.empty(horizon)?;
        let by_key: BTreeMap<_, _> = records.iter().map(|r| (r.key.as_str(), r)).collect();
        for (id, windows) in &horizon.windows {
            let source = state.sources.get_mut(id).unwrap();
            for window in windows {
                let key = self.key(window)?;
                let Some(record) = by_key.get(key.as_str()) else {
                    break;
                };
                ensure!(
                    record.record.input == *window && record.record.recognizer == self.identity,
                    "foreign accepted ASR record"
                );
                ensure!(
                    window.start_sample == source.end_sample,
                    "ASR checkpoint coverage gap"
                );
                source.end_sample = window.end_sample;
                source.records.push(Reference {
                    key,
                    sha256: record.sha256.clone(),
                });
            }
            source.eof = horizon.plan.sources[id].eof
                && source.end_sample == horizon.plan.sources[id].input_end;
        }
        state.common_end = common_frontier(state.sources.values().map(|s| (s.end_sample, s.eof)));
        state.complete = horizon.complete() && state.sources.values().all(|s| s.eof);
        // Publish by window start, not by end: different source window lengths must
        // not insert an older-starting line into an already published text batch.
        let audio = horizon.audio_end();
        state.publication_end = records
            .iter()
            .map(|r| &r.record.input)
            .filter(|w| w.start_sample < state.common_end && w.end_sample > audio)
            .map(|w| w.start_sample)
            .min()
            .unwrap_or(state.common_end);
        Ok(state)
    }
    pub fn checkpoint(&self, horizon: &Horizon, new: &[Accepted]) -> Result<Checkpoint> {
        Ok(self.commit_checkpoint(horizon, new)?.0)
    }
    /// One authoritative read per record at commit; the returned batch is local to
    /// this operation and never used as a process-wide cache of mutable files.
    pub(super) fn commit_checkpoint(
        &self,
        horizon: &Horizon,
        new: &[Accepted],
    ) -> Result<(Checkpoint, Vec<Accepted>)> {
        self.commit_records(horizon, self.restore(horizon)?, new)
    }
    pub fn begin<'a>(&'a self, horizon: &'a Horizon) -> Result<Transaction<'a>> {
        let records = self.restore(horizon)?;
        let state = self.derive(horizon, &records)?;
        Ok(Transaction {
            cache: self,
            horizon,
            state,
            records,
        })
    }
    pub(super) fn commit_records(
        &self,
        horizon: &Horizon,
        mut all: Vec<Accepted>,
        new: &[Accepted],
    ) -> Result<(Checkpoint, Vec<Accepted>)> {
        if let Some(previous) = self.previous(horizon)? {
            ensure!(
                previous.capture_revision <= horizon.plan.capture_revision,
                "stale incremental horizon"
            );
        }
        for record in new {
            ensure!(
                record.record.recognizer == self.identity,
                "foreign ASR identity at checkpoint"
            );
            let id = record.descriptor().source;
            ensure!(
                horizon
                    .windows
                    .get(&id)
                    .is_some_and(|windows| windows.contains(record.descriptor())),
                "ASR result is outside the finite horizon"
            );
            all.retain(|r| r.key != record.key);
            let disk = self
                .record(record.descriptor())?
                .context("accepted ASR record is missing/corrupt")?;
            ensure!(
                disk.sha256 == record.sha256,
                "accepted ASR result changed before checkpoint"
            );
            all.push(disk);
        }
        let state = self.derive(horizon, &all)?;
        crate::artifacts::atomic_write(&self.root.join("checkpoint.json"), &envelope(&state)?)?;
        Ok((state, all))
    }
    /// Revalidates referenced results and derives frontiers; forged saved counters do not advance them.
    pub fn read_checkpoint(&self, horizon: &Horizon) -> Result<(Checkpoint, Vec<Accepted>)> {
        let records = self.restore(horizon)?;
        let state = self.derive(horizon, &records)?;
        let mut published: Vec<_> = records
            .into_iter()
            .filter(|r| r.descriptor().start_sample < state.publication_end)
            .collect();
        published.sort_by_key(|r| (r.descriptor().start_sample, r.descriptor().source));
        Ok((state, published))
    }
}
