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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub original_lines: Option<Vec<PartialLine>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_identity: Option<String>,
    pub audio_end: u64,
    pub publication_end: u64,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Published {
    pub snapshot: String,
    pub input: FrozenInput,
}
impl Published {
    pub fn snapshot(session: &Path, id: &str) -> Result<Snapshot> {
        ensure!(is_hash(id), "invalid partial snapshot key");
        let path = session.join(format!("incremental/snapshots/{id}.json"));
        let bytes = bytes_file(&path, 16 * 1024 * 1024)?;
        ensure!(hash(&bytes) == id, "partial snapshot checksum changed");
        let value: Snapshot = serde_json::from_slice(&bytes)?;
        Self {
            snapshot: id.into(),
            input: value.input,
        }
        .read(session)
    }
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
        match (&value.original_lines, &value.text_identity) {
            (None, None) => {}
            (Some(raw), Some(identity)) => {
                ensure!(is_hash(identity), "invalid text processor identity");
                crate::text_processing::validate_partial(raw, &value.lines)?;
            }
            _ => anyhow::bail!("incomplete text processor provenance"),
        }
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
        self.publish_records(session, horizon, checkpoint, records)
    }
    /// Commit and publish one verified batch while the caller owns execution.
    pub fn checkpoint_and_publish(
        &self,
        session: &Path,
        horizon: &Horizon,
        new: &[Accepted],
    ) -> Result<Checkpoint> {
        let (checkpoint, records) = self.commit_checkpoint(horizon, new)?;
        self.publish_records(session, horizon, checkpoint.clone(), records)?;
        Ok(checkpoint)
    }
    fn publish_records(
        &self,
        session: &Path,
        horizon: &Horizon,
        checkpoint: Checkpoint,
        records: Vec<Accepted>,
    ) -> Result<Published> {
        self.publish_records_with(session, horizon, checkpoint, records, |_| Ok(()))
    }
    fn publish_records_with(
        &self,
        session: &Path,
        horizon: &Horizon,
        checkpoint: Checkpoint,
        mut records: Vec<Accepted>,
        project: impl FnOnce(&mut Snapshot) -> Result<()>,
    ) -> Result<Published> {
        records.retain(|r| r.descriptor().start_sample < checkpoint.publication_end);
        records.sort_by_key(|r| (r.descriptor().start_sample, r.descriptor().source));
        let input = horizon.freeze(session)?;
        // Only fully window-verified audio is exposed. Withhold a whole older-start
        // line (and its successors) if another track has not closed enough PCM yet.
        let audio_end = checkpoint.common_end;
        let publication_end = records
            .iter()
            .filter(|r| r.descriptor().end_sample > audio_end)
            .map(|r| r.descriptor().start_sample)
            .min()
            .unwrap_or(checkpoint.publication_end);
        let lines = records
            .into_iter()
            .filter(|r| r.descriptor().start_sample < publication_end)
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
        let mut value = Snapshot {
            version: 1,
            input: input.clone(),
            checkpoint,
            captured: horizon.input.clone(),
            plan: horizon.plan.clone(),
            lines,
            original_lines: None,
            text_identity: None,
            audio_end,
            publication_end,
        };
        let raw = value.clone();
        let projection_error = project(&mut value)
            .and_then(|()| {
                match (&value.original_lines, &value.text_identity) {
                    (None, None) => ensure!(
                        value.lines == raw.lines,
                        "changed text without processor provenance"
                    ),
                    (Some(original), Some(identity)) => {
                        ensure!(
                            *original == raw.lines && is_hash(identity),
                            "invalid original text or processor identity"
                        );
                        crate::text_processing::validate_partial(&raw.lines, &value.lines)?;
                    }
                    _ => anyhow::bail!("incomplete text processor provenance"),
                }
                Ok(())
            })
            .err();
        if projection_error.is_some() {
            value = raw;
        }
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
        if let Some(error) = projection_error {
            return Err(error.context(
                "исходная готовая часть сохранена; обработчик текста завершился ошибкой",
            ));
        }
        Ok(published)
    }
}

impl Transaction<'_> {
    /// Consume exactly the previously verified request snapshot; new records are
    /// revalidated at commit. Recovery and the next request start with disk reads.
    pub fn finish(self, session: &Path, new: &[Accepted]) -> Result<Checkpoint> {
        self.finish_with_projection(session, new, |_| Ok(()))
    }
    pub fn finish_with_projection(
        self,
        session: &Path,
        new: &[Accepted],
        project: impl FnOnce(&mut Snapshot) -> Result<()>,
    ) -> Result<Checkpoint> {
        let (state, records) = self.cache.commit_records(self.horizon, self.records, new)?;
        self.cache
            .publish_records_with(session, self.horizon, state.clone(), records, project)?;
        Ok(state)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::incremental::test_support::Fixture;

    #[test]
    fn optional_projection_survives_reopen_and_never_poisons_the_acoustic_cache() {
        let f = Fixture::new([&[600, 1200], &[800, 1200]], false);
        let cache = Cache::open(&f.session, &f.identity).unwrap();
        let records: Vec<_> = (0..2)
            .flat_map(|source| (0..2).map(move |n| (source, n)))
            .map(|(source, n)| {
                cache
                    .resolve(f.window(source, n), |_| Ok("fixture words 123".into()))
                    .unwrap()
                    .0
            })
            .collect();
        let state = cache.checkpoint(&f.horizon, &records).unwrap();
        let (_, accepted) = cache.read_checkpoint(&f.horizon).unwrap();
        let receipt = cache
            .publish_records_with(&f.session, &f.horizon, state, accepted, |snapshot| {
                snapshot.original_lines = Some(snapshot.lines.clone());
                snapshot.text_identity = Some(hash(b"processor"));
                for line in &mut snapshot.lines {
                    line.text = "Fixture words 123.".into();
                }
                Ok(())
            })
            .unwrap();
        let styled = receipt.read(&f.session).unwrap();
        assert_eq!(styled.lines.len(), 2);
        assert_eq!(
            styled.original_lines.as_ref().unwrap()[0].text,
            "fixture words 123"
        );
        drop(cache);
        let cache = Cache::open(&f.session, &f.identity).unwrap();
        let (state, accepted) = cache.read_checkpoint(&f.horizon).unwrap();
        assert!(accepted
            .iter()
            .filter_map(|record| record.text())
            .all(|text| text == "fixture words 123"));
        // Both an explicit error and an invalid processor output publish readable raw text.
        for corrupt in [false, true] {
            let result = cache.publish_records_with(
                &f.session,
                &f.horizon,
                state.clone(),
                accepted.clone(),
                |snapshot| {
                    snapshot.original_lines = Some(snapshot.lines.clone());
                    snapshot.text_identity = Some(hash(b"processor"));
                    snapshot.lines[0].text = "wrong words 999".into();
                    if corrupt {
                        Ok(())
                    } else {
                        anyhow::bail!("fixture processor failed")
                    }
                },
            );
            assert!(result.is_err());
            let current = Published::current(&f.session)
                .unwrap()
                .unwrap()
                .read(&f.session)
                .unwrap();
            assert!(current.original_lines.is_none());
            assert!(current
                .lines
                .iter()
                .all(|line| line.text == "fixture words 123"));
            assert_eq!(current.checkpoint, styled.checkpoint);
            assert_eq!(
                receipt.read(&f.session).unwrap().lines[0].text,
                "Fixture words 123."
            );
            for source in [0, 1] {
                for n in 0..2 {
                    cache
                        .resolve(f.window(source, n), |_| anyhow::bail!("ASR must not rerun"))
                        .unwrap();
                }
            }
        }
    }
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
