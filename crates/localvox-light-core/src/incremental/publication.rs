//! Immutable input/result snapshots; current.json is published last.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FrozenInput {
    pub generation: String,
    pub revision: u64,
    pub plan: String,
    /// Changes only when actual closed windows change, not merely a new seal/revision.
    pub work: String,
}
impl FrozenInput {
    pub fn path(&self, session: &Path) -> Result<PathBuf> {
        ensure!(
            is_hash(&self.plan) && is_hash(&self.work),
            "invalid frozen input key"
        );
        Ok(session.join(format!("incremental/inputs/{}.json", self.plan)))
    }
    pub fn horizon(&self, session: &Path) -> Result<Horizon> {
        let path = self.path(session)?;
        let bytes = bytes_file(&path, 8 * 1024 * 1024)?;
        ensure!(hash(&bytes) == self.plan, "frozen input checksum changed");
        let horizon = Horizon::read_plan(session, &path)?;
        ensure!(
            horizon.generation() == self.generation && horizon.revision() == self.revision,
            "frozen input identity changed"
        );
        ensure!(
            horizon.work_key()? == self.work,
            "frozen input windows changed"
        );
        Ok(horizon)
    }
}
impl Horizon {
    fn work_key(&self) -> Result<String> {
        Ok(hash(&serde_json::to_vec(&(
            &self.plan.generation,
            &self.plan.recipe,
            &self.windows,
        ))?))
    }
    /// Called before queue mutation. No audio, model or transcript reads under its mutex.
    pub fn freeze(&self, session: &Path) -> Result<FrozenInput> {
        let bytes = serde_json::to_vec(&self.plan)?;
        let input = FrozenInput {
            generation: self.generation().into(),
            revision: self.revision(),
            plan: hash(&bytes),
            work: self.work_key()?,
        };
        let path = input.path(session)?;
        fs::create_dir_all(path.parent().unwrap())?;
        match fs::read(&path) {
            Ok(existing) => ensure!(existing == bytes, "immutable input was changed"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                crate::artifacts::atomic_write(&path, &bytes)?
            }
            Err(e) => return Err(e.into()),
        }
        Ok(input)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PartialLine {
    pub id: String,
    pub source_id: u8,
    pub start_sample: u64,
    pub end_sample: u64,
    pub text: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub input: FrozenInput,
    pub checkpoint: Checkpoint,
    pub captured: capture::Snapshot,
    pub plan: planning::Snapshot,
    pub lines: Vec<PartialLine>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Published {
    pub snapshot: String,
    pub input: FrozenInput,
}
impl Published {
    pub fn read(&self, session: &Path) -> Result<Snapshot> {
        ensure!(is_hash(&self.snapshot), "invalid partial snapshot key");
        let bytes = bytes_file(
            &session.join(format!("incremental/snapshots/{}.json", self.snapshot)),
            16 * 1024 * 1024,
        )?;
        ensure!(
            hash(&bytes) == self.snapshot,
            "partial snapshot checksum changed"
        );
        let value: Snapshot = serde_json::from_slice(&bytes)?;
        ensure!(
            value.version == 1
                && value.input == self.input
                && value.checkpoint.generation == self.input.generation
                && value.checkpoint.capture_revision == self.input.revision
                && value.plan.generation == self.input.generation
                && value.captured.generation == self.input.generation,
            "partial snapshot identity changed"
        );
        ensure!(
            capture::read(session)?.generation == self.input.generation,
            "stale recording generation"
        );
        Ok(value)
    }
    pub fn for_input(session: &Path, input: &FrozenInput) -> Result<Self> {
        let receipt: Self = json_file(
            &session.join(format!("incremental/results/{}.json", input.plan)),
            16384,
        )?;
        ensure!(
            receipt.input == *input,
            "worker accepted another input horizon"
        );
        let result = receipt.read(session)?;
        for (id, source) in &result.plan.sources {
            ensure!(
                result.checkpoint.sources[id].end_sample == source.closed_end,
                "worker left a ready window unprocessed"
            );
        }
        Ok(receipt)
    }
    pub fn current(session: &Path) -> Result<Option<Self>> {
        match json_file(&session.join("incremental/current.json"), 16384) {
            Ok(value) => Ok(Some(value)),
            Err(e)
                if e.downcast_ref::<std::io::Error>()
                    .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                Ok(None)
            }
            Err(e) => Err(e),
        }
    }
}
impl Cache {
    pub fn publish(&self, session: &Path, horizon: &Horizon) -> Result<Published> {
        let (checkpoint, records) = self.read_checkpoint(horizon)?;
        let input = horizon.freeze(session)?;
        let lines = records
            .into_iter()
            .filter_map(|record| {
                record.text().map(|text| {
                    let d = record.descriptor();
                    PartialLine {
                        id: format!(
                            "{}/{}/{}-{}",
                            d.generation, d.source, d.start_sample, d.end_sample
                        ),
                        source_id: d.source,
                        start_sample: d.start_sample,
                        end_sample: d.end_sample,
                        text: text.into(),
                    }
                })
            })
            .collect();
        let value = Snapshot {
            version: 1,
            input: input.clone(),
            checkpoint,
            captured: horizon.input.clone(),
            plan: horizon.plan.clone(),
            lines,
        };
        let bytes = serde_json::to_vec(&value)?;
        ensure!(
            bytes.len() <= 16 * 1024 * 1024,
            "partial snapshot exceeds limit"
        );
        let published = Published {
            snapshot: hash(&bytes),
            input,
        };
        for directory in ["snapshots", "results"] {
            fs::create_dir_all(session.join("incremental").join(directory))?;
        }
        crate::artifacts::atomic_write(
            &session.join(format!("incremental/snapshots/{}.json", published.snapshot)),
            &bytes,
        )?;
        let receipt = serde_json::to_vec(&published)?;
        crate::artifacts::atomic_write(
            &session.join(format!("incremental/results/{}.json", published.input.plan)),
            &receipt,
        )?;
        published.read(session)?;
        crate::artifacts::atomic_write(&session.join("incremental/current.json"), &receipt)?;
        Ok(published)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::test_support::Fixture;
    #[test]
    fn frozen_input_and_published_audio_text_remain_one_revision_when_current_plan_changes() {
        let f = Fixture::new([&[600, 1200], &[800, 1200]], false);
        let input = f.horizon.freeze(&f.session).unwrap();
        fs::write(f.session.join(planning::FILE), b"{}").unwrap();
        let fixed = input.horizon(&f.session).unwrap();
        let cache = Cache::open(&f.session, &f.identity).unwrap();
        let mut accepted = Vec::new();
        for id in [0, 1] {
            for n in 0..2 {
                accepted.push(
                    cache
                        .resolve(f.window(id, n), |_| Ok("fixture words".into()))
                        .unwrap()
                        .0,
                );
            }
        }
        cache.checkpoint(&fixed, &accepted).unwrap();
        let published = cache.publish(&f.session, &fixed).unwrap();
        assert_eq!(Published::current(&f.session).unwrap().unwrap(), published);
        assert_eq!(Published::for_input(&f.session, &input).unwrap(), published);
        let document = published.read(&f.session).unwrap();
        assert_eq!(document.lines.len(), 2); // second source has explicit Silence outcomes
        assert!(document.lines.iter().all(|line| line.source_id == 0));
        assert_eq!(document.checkpoint.common_end, 1200);
        assert_eq!(
            document
                .lines
                .iter()
                .map(|l| &l.id)
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            2
        );
        fs::write(f.session.join("incremental/current.json"), b"broken").unwrap();
        assert!(Published::current(&f.session).is_err());
        assert_eq!(
            published.read(&f.session).unwrap().lines.len(),
            2,
            "old immutable snapshot was invalidated by pointer failure"
        );
        let path = f
            .session
            .join(format!("incremental/snapshots/{}.json", published.snapshot));
        fs::write(path, b"{}").unwrap();
        assert!(published.read(&f.session).is_err());
    }
}
