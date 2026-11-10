//! Capture-owned input ledger. An audio filename is not a commit or an EOF.
use super::*;
use anyhow::{ensure, Context, Result};
use std::collections::BTreeMap;

pub const FILE: &str = "capture/state.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Audio,
    Empty,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub sequence: u32,
    pub start_sample: u64,
    pub samples: u64,
    pub kind: Kind,
    pub problem: Option<String>,
}
impl Range {
    pub fn stem(&self, source: u8) -> String {
        format!("src{source}_chunk{:04}", self.sequence)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenRange {
    pub sequence: u32,
    pub start_sample: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub ranges: Vec<Range>,
    pub open: Option<OpenRange>,
}
impl Source {
    pub fn end(&self) -> u64 {
        self.ranges.last().map_or(0, |r| r.start_sample + r.samples)
    }
    pub fn confirmed_end(&self) -> u64 {
        self.ranges
            .iter()
            .take_while(|r| r.kind != Kind::Unknown)
            .last()
            .map_or(0, |r| r.start_sample + r.samples)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terminal {
    pub at: String,
    pub reason: String,
    pub interrupted: bool,
    pub ends: BTreeMap<u8, u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub generation: String,
    pub revision: u64,
    pub sample_rate: u32,
    pub paused: bool,
    pub sources: BTreeMap<u8, Source>,
    pub terminal: Option<Terminal>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.sample_rate == SAMPLE_RATE,
            "unsupported capture input"
        );
        uuid::Uuid::parse_str(&self.generation).context("invalid capture generation")?;
        ensure!(
            self.sources.keys().copied().collect::<Vec<_>>() == [0, 1],
            "invalid capture sources"
        );
        for source in self.sources.values() {
            let mut end = 0u64;
            let mut seq = 1u32;
            for range in &source.ranges {
                ensure!(
                    range.sequence == seq && range.start_sample == end,
                    "capture range gap/overlap"
                );
                ensure!(
                    range.kind != Kind::Empty || range.samples == 0,
                    "nonempty empty range"
                );
                ensure!(
                    range.kind != Kind::Audio || range.samples > 0,
                    "empty audio range"
                );
                ensure!(
                    (range.kind == Kind::Unknown) == range.problem.is_some(),
                    "invalid range outcome"
                );
                end = end
                    .checked_add(range.samples)
                    .context("capture sample overflow")?;
                seq = seq.checked_add(1).context("capture sequence overflow")?;
            }
            if let Some(open) = &source.open {
                ensure!(
                    open.sequence == seq && open.start_sample == end,
                    "invalid open range"
                );
            }
        }
        if let Some(terminal) = &self.terminal {
            ensure!(
                !self.paused && terminal.ends.len() == self.sources.len(),
                "invalid capture EOF"
            );
            for (id, source) in &self.sources {
                ensure!(
                    source.open.is_none()
                        && source.ranges.iter().all(|r| r.kind != Kind::Unknown)
                        && terminal.ends.get(id) == Some(&source.end()),
                    "unconfirmed capture EOF"
                );
            }
        }
        Ok(())
    }
}

pub fn read(session: &Path) -> Result<Snapshot> {
    let snapshot: Snapshot = serde_json::from_slice(&fs::read(session.join(FILE))?)?;
    snapshot.validate()?;
    let meta: SessionMeta = serde_json::from_slice(&fs::read(session.join("meta.json"))?)?;
    ensure!(
        meta.capture_generation.as_deref() == Some(snapshot.generation.as_str()),
        "capture generation differs from metadata"
    );
    Ok(snapshot)
}

/// One producer, held through recorder, converter and lightweight planner lifetime.
pub struct Writer {
    session: PathBuf,
    working: Snapshot,
    committed_revision: u64,
    _lease: File,
    plan_signal: Option<crossbeam_channel::Sender<()>>,
}
impl Writer {
    pub fn begin(session: &Path, generation: String) -> Result<Self> {
        let meta: SessionMeta = serde_json::from_slice(&fs::read(session.join("meta.json"))?)?;
        ensure!(
            meta.capture_generation.as_deref() == Some(generation.as_str()),
            "capture generation not declared"
        );
        let lease = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(session.join(".capture.lock"))?;
        lease.try_lock().context("capture already owned")?;
        ensure!(!session.join(FILE).exists(), "capture input already exists");
        let mut writer = Self {
            session: session.into(),
            working: Snapshot {
                version: 1,
                generation,
                revision: 0,
                sample_rate: SAMPLE_RATE,
                paused: false,
                sources: [(0, Source::default()), (1, Source::default())].into(),
                terminal: None,
            },
            committed_revision: 0,
            _lease: lease,
            plan_signal: None,
        };
        writer.working.validate()?;
        crate::journal::begin_run(session, &writer.working.generation);
        // Recording remains primary if publishing auxiliary metadata temporarily fails.
        if let Err(error) = writer.persist() {
            report(session, "Начало подтверждения аудио", &error);
        }
        Ok(writer)
    }
    fn persist(&mut self) -> Result<()> {
        self.working.validate()?;
        let path = self.session.join(FILE);
        if path.exists() {
            let disk = read(&self.session)?;
            ensure!(
                disk.generation == self.working.generation
                    && disk.revision == self.committed_revision,
                "capture input changed outside its owner"
            );
        } else {
            ensure!(
                self.committed_revision == 0,
                "committed capture input disappeared"
            );
        }
        fs::create_dir_all(path.parent().context("capture directory")?)?;
        crate::artifacts::atomic_write(&path, &serde_json::to_vec_pretty(&self.working)?)?;
        self.committed_revision = self.working.revision;
        if let Some(signal) = &self.plan_signal {
            let _ = signal.try_send(()); // disk is truth; at most one pending wakeup
        }
        Ok(())
    }
    pub(super) fn set_plan_signal(&mut self, signal: crossbeam_channel::Sender<()>) {
        let _ = signal.try_send(());
        self.plan_signal = Some(signal);
    }
    pub(super) fn has_planner(&self) -> bool {
        self.plan_signal.is_some()
    }
    pub(super) fn session(&self) -> &Path {
        &self.session
    }
    fn update(&mut self, change: impl FnOnce(&mut Snapshot) -> Result<()>) -> Result<()> {
        let mut next = self.working.clone();
        change(&mut next)?;
        next.revision = next
            .revision
            .checked_add(1)
            .context("capture revision overflow")?;
        next.validate()?;
        // Keep a valid unpublished update in RAM: a later commit can include it in full.
        self.working = next;
        self.persist()
    }
    pub fn open(&mut self, source: u8, sequence: u32, start_sample: u64) -> Result<()> {
        self.update(|state| {
            ensure!(state.terminal.is_none(), "capture already ended");
            let source = state.sources.get_mut(&source).context("unknown source")?;
            ensure!(source.open.is_none(), "previous capture range still open");
            source.open = Some(OpenRange {
                sequence,
                start_sample,
            });
            state.paused = false;
            Ok(())
        })
    }
    pub fn close(&mut self, source: u8, range: Range) -> Result<()> {
        self.update(|state| {
            let source = state.sources.get_mut(&source).context("unknown source")?;
            ensure!(
                source.open.as_ref()
                    == Some(&OpenRange {
                        sequence: range.sequence,
                        start_sample: range.start_sample
                    }),
                "capture close differs from open"
            );
            source.ranges.push(range);
            source.open = None;
            Ok(())
        })
    }
    pub fn pause(&mut self) -> Result<()> {
        self.update(|state| {
            ensure!(
                state.sources.values().all(|s| s.open.is_none()),
                "capture pause with open input"
            );
            state.paused = true;
            Ok(())
        })
    }
    pub fn end(&mut self, reason: &str, interrupted: bool, ends: BTreeMap<u8, u64>) -> Result<()> {
        if let Some(terminal) = &self.working.terminal {
            ensure!(
                terminal.reason == reason
                    && terminal.interrupted == interrupted
                    && terminal.ends == ends,
                "different capture EOF retry"
            );
            return self.persist();
        }
        self.update(|state| {
            state.paused = false;
            state.terminal = Some(Terminal {
                at: crate::versions::now_rfc3339(),
                reason: reason.into(),
                interrupted,
                ends,
            });
            Ok(())
        })
    }
}

pub(crate) fn report(session: &Path, label: &str, error: &anyhow::Error) {
    tracing::error!(%error, "{label}");
    let _ = crate::journal::append(
        session,
        crate::journal::Entry {
            stage: "capture".into(),
            id: uuid::Uuid::now_v7().to_string(),
            at: crate::versions::now_rfc3339(),
            kind: "error".into(),
            label: label.into(),
            status: Some("failed".into()),
            text: Some(format!("{error:#}")),
            ..Default::default()
        },
    );
}

/// New captures must never silently fall back to legacy readiness when their ledger is missing.
pub fn is_finalized(session: &Path) -> Result<bool> {
    let declared = fs::read(session.join("meta.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
        .is_some_and(|meta| !meta["capture_generation"].is_null());
    if !declared && !session.join("capture").exists() {
        // Preserve legacy discovery, including recordings predating meta.json.
        return Ok(true);
    }
    Ok(read(session)?.terminal.is_some())
}

pub fn try_mutation_lock(session: &Path) -> std::io::Result<Option<File>> {
    let file = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(session.join(".capture.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(Some(file)),
        Err(fs::TryLockError::WouldBlock) => Ok(None),
        Err(fs::TryLockError::Error(e)) => Err(e),
    }
}

fn saved_samples(session: &Path, stem: &str, generation: &str) -> Result<u64> {
    codec::with_read(&session.join("audio"), stem, |path| {
        let count = codec::samples(path)?;
        if path.extension().and_then(|s| s.to_str()) == Some("flac") {
            codec::verify_conversion(&session.join("audio"), stem, count, generation)?;
        }
        Ok(count)
    })
}

/// Before fresh full ASR, require every sealed sample, including a missing final file.
/// Existing completed text can still be reused after opt-in audio retention.
pub fn validate_final_input(session: &Path) -> Result<()> {
    ensure!(is_finalized(session)?, "capture input has no confirmed EOF");
    if !session.join("capture").exists() {
        return Ok(()); // legacy/imported audio retains its existing validation path
    }
    let snapshot = read(session)?;
    for (id, source) in &snapshot.sources {
        for range in source.ranges.iter().filter(|r| r.kind == Kind::Audio) {
            ensure!(
                saved_samples(session, &range.stem(*id), &snapshot.generation)? == range.samples,
                "saved capture input missing or changed: {}",
                range.stem(*id)
            );
        }
    }
    for entry in fs::read_dir(session.join("audio"))? {
        let path = entry?.path();
        if !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("wav" | "flac" | "part")
        ) {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("audio filename")?;
        let (id, seq) = parse_src_seq(stem).context("unaccounted capture audio")?;
        let range = snapshot
            .sources
            .get(&id)
            .and_then(|s| s.ranges.iter().find(|r| r.sequence == seq))
            .context("unaccounted capture audio")?;
        ensure!(range.stem(id) == stem, "noncanonical capture audio name");
        if range.kind == Kind::Empty && path.extension().and_then(|s| s.to_str()) == Some("part") {
            ensure!(wav_samples(&path)? == 0, "nonempty audio under empty seal");
        } else {
            ensure!(
                range.kind == Kind::Audio
                    && path.extension().and_then(|s| s.to_str()) != Some("part"),
                "unconfirmed capture audio"
            );
        }
    }
    Ok(())
}

fn repair_meta(session: &Path, id: u8, range: &Range) -> Result<()> {
    let meta_path = session.join("meta.json");
    let mut meta: SessionMeta = serde_json::from_slice(&fs::read(&meta_path)?)?;
    let stem = range.stem(id);
    if let Some(entry) = meta.chunks.iter().find(|c| c.file == stem) {
        ensure!(
            entry.source_id == id
                && (entry.start_offset_sec * 16000.0).round() == range.start_sample as f64
                && (entry.duration_sec * 16000.0).round() == range.samples as f64,
            "metadata range differs from capture seal"
        );
        return Ok(());
    }
    let mut stats = LevelStats::default();
    codec::with_read(&session.join("audio"), &stem, |path| {
        codec::stream(
            path,
            Path::new(&resolve_ffmpeg_for_decode()),
            || false,
            |bytes| {
                let samples = bytes
                    .chunks_exact(2)
                    .map(|c| i16::from_le_bytes([c[0], c[1]]))
                    .collect::<Vec<_>>();
                stats.feed(&samples);
                Ok(())
            },
        )
    })?;
    let (rms, voice_ratio) = stats.finish();
    meta.chunks.push(ChunkEntry {
        file: stem,
        source_id: id,
        start_offset_sec: range.start_sample as f64 / 16000.0,
        duration_sec: range.samples as f64 / 16000.0,
        rms,
        voice_ratio,
    });
    save_meta_checked(&meta_path, &meta)
}

/// Strict verification for the recorder's canonical WAV, not a filename/mtime heuristic.
pub(super) fn wav_samples(path: &Path) -> Result<u64> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut header = [0u8; 44];
    file.read_exact(&mut header)?;
    let data = u64::from(u32::from_le_bytes(header[40..44].try_into()?));
    ensure!(
        size == 44 + data && data % 2 == 0 && header == wav_header(data),
        "invalid capture WAV"
    );
    Ok(data / 2)
}

/// Called only after workspace ownership/recovery; the capture lease excludes live producers.
pub(super) fn recover(session: &Path) -> Result<usize> {
    let meta: SessionMeta = serde_json::from_slice(&fs::read(session.join("meta.json"))?)?;
    let Some(generation) = meta.capture_generation else {
        return Ok(0);
    };
    let lease = OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(session.join(".capture.lock"))?;
    if let Err(error) = lease.try_lock() {
        return match error {
            fs::TryLockError::WouldBlock => Ok(0),
            fs::TryLockError::Error(e) => Err(e.into()),
        };
    }
    let snapshot = read(session)?;
    ensure!(
        snapshot.generation == generation,
        "capture recovery generation changed"
    );
    if snapshot.terminal.is_some() {
        // Completed sessions may legitimately have lost audio to opt-in retention.
        // Only remove proven empty housekeeping; never revalidate/recook their text.
        for (id, source) in &snapshot.sources {
            for range in source.ranges.iter().filter(|r| r.kind == Kind::Empty) {
                let part = session
                    .join("audio")
                    .join(format!("{}.part", range.stem(*id)));
                if part.is_file() {
                    ensure!(wav_samples(&part)? == 0, "nonempty audio under empty seal");
                    fs::remove_file(part)?;
                }
            }
        }
        return Ok(0);
    }
    let mut writer = Writer {
        session: session.into(),
        committed_revision: snapshot.revision,
        working: snapshot,
        _lease: lease,
        plan_signal: None,
    };
    let mut recovered = 0;
    for id in [0, 1] {
        let source = writer.working.sources[&id].clone();
        // An unknown logical interval must not shrink to the bytes that happened to survive.
        for range in source.ranges.iter().filter(|r| r.kind == Kind::Unknown) {
            let wav = session
                .join("audio")
                .join(format!("{}.wav", range.stem(id)));
            let part = wav.with_extension("part");
            if range.samples == 0 {
                ensure!(
                    !wav.exists() && !wav.with_extension("flac").exists(),
                    "unexpected audio for empty capture interval"
                );
                if part.is_file() {
                    ensure!(
                        wav_samples(&part)? == 0,
                        "nonempty audio under empty interval"
                    );
                }
                writer.update(|state| {
                    let saved = &mut state.sources.get_mut(&id).unwrap().ranges
                        [(range.sequence - 1) as usize];
                    saved.kind = Kind::Empty;
                    saved.problem = None;
                    Ok(())
                })?;
                if part.is_file() {
                    fs::remove_file(&part)?;
                }
                continue;
            }
            if part.is_file() {
                recover_part(&part)?;
            }
            ensure!(
                saved_samples(session, &range.stem(id), &generation)? == range.samples
                    && range.samples > 0,
                "capture lost samples in {}",
                range.stem(id)
            );
            writer.update(|state| {
                let saved =
                    &mut state.sources.get_mut(&id).unwrap().ranges[(range.sequence - 1) as usize];
                saved.kind = Kind::Audio;
                saved.problem = None;
                Ok(())
            })?;
            recovered += 1;
        }
        if let Some(open) = source.open {
            let stem = format!("src{id}_chunk{:04}", open.sequence);
            let wav = session.join("audio").join(format!("{stem}.wav"));
            let part = wav.with_extension("part");
            let samples = if part.is_file() {
                let bytes = fs::metadata(&part)?.len();
                ensure!(
                    bytes >= 44 && (bytes - 44) % 2 == 0,
                    "truncated PCM sample in {stem}"
                );
                if bytes == 44 {
                    ensure!(wav_samples(&part)? == 0, "invalid empty capture file");
                    ensure!(
                        !wav.exists() && !wav.with_extension("flac").exists(),
                        "unexpected audio for empty capture interval"
                    );
                    0
                } else {
                    recover_part(&part)?;
                    recovered += 1;
                    wav_samples(&wav)?
                }
            } else {
                saved_samples(session, &stem, &generation)?
            };
            writer.close(
                id,
                Range {
                    sequence: open.sequence,
                    start_sample: open.start_sample,
                    samples,
                    kind: if samples == 0 {
                        Kind::Empty
                    } else {
                        Kind::Audio
                    },
                    problem: None,
                },
            )?;
            // The empty seal must survive a crash before its proof is removed.
            if samples == 0 && part.is_file() {
                fs::remove_file(&part)?;
            }
        }
    }
    for (id, source) in &writer.working.sources {
        for range in source.ranges.iter().filter(|r| r.kind == Kind::Audio) {
            ensure!(
                saved_samples(session, &range.stem(*id), &generation)? == range.samples,
                "saved capture input missing or changed"
            );
            repair_meta(session, *id, range)?;
        }
    }
    // Every on-disk chunk must be accounted for; unrecorded suffixes are not guessed away.
    for entry in fs::read_dir(session.join("audio"))? {
        let path = entry?.path();
        if !matches!(
            path.extension().and_then(|s| s.to_str()),
            Some("wav" | "flac" | "part")
        ) {
            continue;
        }
        let (id, seq) = path
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(parse_src_seq)
            .context("unknown capture audio filename")?;
        let range = writer
            .working
            .sources
            .get(&id)
            .and_then(|s| s.ranges.iter().find(|r| r.sequence == seq))
            .context("unconfirmed capture audio suffix")?;
        if range.kind == Kind::Empty && path.extension().and_then(|s| s.to_str()) == Some("part") {
            ensure!(wav_samples(&path)? == 0, "nonempty audio under empty seal");
            fs::remove_file(path)?;
            continue;
        }
        ensure!(
            range.kind == Kind::Audio && path.extension().and_then(|s| s.to_str()) != Some("part"),
            "unconfirmed capture audio"
        );
        ensure!(
            saved_samples(session, &range.stem(id), &generation)? == range.samples,
            "capture audio changed"
        );
    }
    let ends = writer
        .working
        .sources
        .iter()
        .map(|(id, s)| (*id, s.end()))
        .collect();
    writer.end(
        "запись прервана; сохранённое аудио восстановлено",
        true,
        ends,
    )?;
    Ok(recovered)
}

#[cfg(test)]
#[path = "capture_tests.rs"]
mod tests;
