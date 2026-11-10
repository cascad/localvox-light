//! Capture-owned lightweight planning. No ASR/LLM or processing execution lease.
use super::*;
use crate::windowing::{Window, WindowKind, Windower};
use anyhow::{ensure, Context, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use webrtc_vad::{SampleRate, Vad, VadMode};

pub const FILE: &str = "capture/plan/state.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipe {
    pub revision: u32,
    pub max_samples: u64,
    pub min_cut_samples: u64,
    pub silence_ms: u32,
}
impl Default for Recipe {
    fn default() -> Self {
        Self {
            revision: 1, // WebRTC LowBitrate, 320-sample frames, current quietest cut
            max_samples: (crate::versions::DEFAULT_MAX_WINDOW_SEC * SAMPLE_RATE as f64) as u64,
            min_cut_samples: (crate::versions::DEFAULT_MIN_CUT_SEC * SAMPLE_RATE as f64) as u64,
            silence_ms: crate::versions::DEFAULT_SILENCE_MS,
        }
    }
}
impl Recipe {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.revision == 1
                && self.max_samples > 0
                && self.max_samples <= 200 * SAMPLE_RATE as u64,
            "unsupported capture planner recipe"
        );
        Ok(())
    }
    pub fn fingerprint(&self) -> Result<String> {
        self.validate()?;
        Ok(format!("{:x}", Sha256::digest(serde_json::to_vec(self)?)))
    }
    fn windower(&self) -> Windower {
        Windower::new(
            self.max_samples as f64 / SAMPLE_RATE as f64,
            self.min_cut_samples as f64 / SAMPLE_RATE as f64,
            self.silence_ms,
        )
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Descriptor {
    pub version: u32,
    pub generation: String,
    pub recipe: String,
    pub source: u8,
    pub start_sample: u64,
    pub end_sample: u64,
    pub kind: WindowKind,
    pub pcm_sha256: String,
}
impl Descriptor {
    pub fn from_pcm(
        generation: &str,
        recipe: &str,
        source: u8,
        start_sample: u64,
        pcm: &[i16],
        kind: WindowKind,
    ) -> Result<Self> {
        ensure!(!pcm.is_empty(), "empty planned window");
        Ok(Self {
            version: 1,
            generation: generation.into(),
            recipe: recipe.into(),
            source,
            start_sample,
            end_sample: start_sample
                .checked_add(pcm.len() as u64)
                .context("planned sample overflow")?,
            kind,
            pcm_sha256: crate::windowing::pcm_sha256(pcm),
        })
    }
    pub fn id(&self) -> String {
        format!(
            "src{}_{}-{}",
            self.source, self.start_sample, self.end_sample
        )
    }
    pub fn path(&self) -> String {
        format!("capture/plan/{}/{}.json", self.recipe, self.id())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    pub start_sample: u64,
    pub end_sample: u64,
    pub sha256: String,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourcePlan {
    pub sequence: u32,
    pub input_end: u64,
    pub closed_end: u64,
    pub eof: bool,
    pub windows: Vec<Reference>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Snapshot {
    pub version: u32,
    pub generation: String,
    pub capture_revision: u64,
    pub recipe: Recipe,
    pub sources: BTreeMap<u8, SourcePlan>,
    pub common_end: u64,
    pub complete: bool,
}

struct SourceState {
    windower: Windower,
    vad: Vad,
    plan: SourcePlan,
}

/// Construct inside its owning thread: native VAD state never crosses threads/serializes.
struct Planner {
    session: PathBuf,
    generation: String,
    recipe: Recipe,
    fingerprint: String,
    ffmpeg: PathBuf,
    revision: Option<u64>,
    sources: BTreeMap<u8, SourceState>,
}
impl Planner {
    fn new(session: &Path, generation: String, recipe: Recipe, ffmpeg: PathBuf) -> Result<Self> {
        let fingerprint = recipe.fingerprint()?;
        let sources = [0, 1]
            .into_iter()
            .map(|id| {
                (
                    id,
                    SourceState {
                        windower: recipe.windower(),
                        vad: Vad::new_with_rate_and_mode(
                            SampleRate::Rate16kHz,
                            VadMode::LowBitrate,
                        ),
                        plan: SourcePlan::default(),
                    },
                )
            })
            .collect();
        Ok(Self {
            session: session.into(),
            generation,
            recipe,
            fingerprint,
            ffmpeg,
            revision: None,
            sources,
        })
    }
    fn advance(&mut self, cancelled: &impl Fn() -> bool) -> Result<bool> {
        let input = capture::read(&self.session)?;
        ensure!(
            input.generation == self.generation,
            "capture planner generation changed"
        );
        if self.revision == Some(input.revision) {
            return Ok(input.terminal.is_some());
        }
        ensure!(
            self.revision.is_none_or(|r| r < input.revision),
            "capture revision moved backwards"
        );
        for (id, state) in &mut self.sources {
            let source = &input.sources[id];
            let mut emit = |window: Window<'_>| -> Result<()> {
                ensure!(!cancelled(), "capture planner cancelled");
                ensure!(
                    window.start_sample == state.plan.closed_end,
                    "planner coverage gap/overlap"
                );
                let record = Descriptor::from_pcm(
                    &self.generation,
                    &self.fingerprint,
                    *id,
                    window.start_sample,
                    window.pcm,
                    window.kind,
                )?;
                let bytes = serde_json::to_vec(&record)?;
                let path = self.session.join(record.path());
                match fs::read(&path) {
                    Ok(old) => ensure!(old == bytes, "immutable planner window changed"),
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                        fs::create_dir_all(path.parent().unwrap())?;
                        crate::artifacts::atomic_write(&path, &bytes)?;
                    }
                    Err(error) => return Err(error.into()),
                }
                state.plan.windows.push(Reference {
                    start_sample: record.start_sample,
                    end_sample: record.end_sample,
                    sha256: format!("{:x}", Sha256::digest(bytes)),
                });
                state.plan.closed_end = record.end_sample;
                Ok(())
            };
            let mut classify = |frame: &[i16]| state.vad.is_voice_segment(frame).unwrap_or(true);
            for range in source.ranges.iter().skip(state.plan.sequence as usize) {
                ensure!(!cancelled(), "capture planner cancelled");
                if range.kind == capture::Kind::Unknown {
                    break;
                }
                ensure!(
                    range.sequence == state.plan.sequence + 1
                        && range.start_sample == state.windower.input_end(),
                    "capture planner input discontinuity"
                );
                if range.kind == capture::Kind::Audio {
                    let mut consumed = 0;
                    codec::with_read_cancelled(
                        &self.session.join("audio"),
                        &range.stem(*id),
                        cancelled,
                        |path| {
                            ensure!(
                                codec::samples(path)? == range.samples,
                                "sealed capture length changed"
                            );
                            if path.extension().and_then(|s| s.to_str()) == Some("flac") {
                                codec::verify_conversion_cancelled(
                                    &self.session.join("audio"),
                                    &range.stem(*id),
                                    range.samples,
                                    &self.generation,
                                    cancelled,
                                )?;
                            }
                            codec::stream(path, &self.ffmpeg, cancelled, |bytes| {
                                let pcm: Vec<_> = bytes
                                    .chunks_exact(2)
                                    .map(|b| i16::from_le_bytes([b[0], b[1]]))
                                    .collect();
                                state.windower.feed_all(&pcm, &mut classify, &mut emit)?;
                                consumed += pcm.len() as u64;
                                Ok(())
                            })
                        },
                    )?;
                    ensure!(
                        consumed == range.samples,
                        "sealed capture decode incomplete"
                    );
                }
                state.plan.sequence = range.sequence;
                state.plan.input_end = state.windower.input_end();
            }
            if input.terminal.is_some() && !state.plan.eof {
                ensure!(
                    state.plan.sequence as usize == source.ranges.len()
                        && state.windower.input_end() == source.end(),
                    "capture EOF has unplanned samples"
                );
                state.windower.finish_all(&mut emit)?;
                state.plan.eof = true;
            }
        }
        ensure!(!cancelled(), "capture planner cancelled");
        let sources: BTreeMap<_, _> = self
            .sources
            .iter()
            .map(|(id, s)| (*id, s.plan.clone()))
            .collect();
        let complete = sources.values().all(|s| s.eof);
        let common_end = sources
            .values()
            .filter(|s| !s.eof)
            .map(|s| s.closed_end)
            .min()
            .unwrap_or_else(|| sources.values().map(|s| s.closed_end).max().unwrap_or(0));
        let snapshot = Snapshot {
            version: 1,
            generation: self.generation.clone(),
            capture_revision: input.revision,
            recipe: self.recipe.clone(),
            sources,
            common_end,
            complete,
        };
        fs::create_dir_all(self.session.join("capture/plan"))?;
        crate::artifacts::atomic_write(&self.session.join(FILE), &serde_json::to_vec(&snapshot)?)?;
        self.revision = Some(input.revision);
        Ok(complete)
    }
}

/// Session drop signals the final disk horizon; it never joins on the capture path.
pub struct Handle {
    stopped: Arc<AtomicBool>,
    signal: crossbeam_channel::Sender<()>,
}
impl Drop for Handle {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let _ = self.signal.try_send(());
    }
}

/// Two bounded owners let a previous session finish decoding while a new one records.
#[derive(Default)]
pub struct Planners {
    tasks: Mutex<Vec<std::thread::JoinHandle<()>>>,
    cancelled: Arc<AtomicBool>,
}
impl Planners {
    fn spawn(
        &self,
        session: PathBuf,
        owner: Arc<Mutex<capture::Writer>>,
        ffmpeg: PathBuf,
    ) -> Result<Handle> {
        let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let mut pending = Vec::new();
        for task in tasks.drain(..) {
            if task.is_finished() {
                let _ = task.join();
            } else {
                pending.push(task);
            }
        }
        *tasks = pending;
        ensure!(
            tasks.len() < 2,
            "capture planners busy; early work waits for final recovery"
        );
        let generation = capture::read(&session)?.generation;
        let (signal, receiver) = crossbeam_channel::bounded(1);
        let stopped = Arc::new(AtomicBool::new(false));
        let closed = stopped.clone();
        let cancelled = self.cancelled.clone();
        let wakeup = signal.clone();
        let task = std::thread::Builder::new()
            .name("capture-plan".into())
            .spawn(move || {
                let _owner = owner;
                let run = || -> Result<()> {
                    let mut planner =
                        Planner::new(&session, generation, Recipe::default(), ffmpeg)?;
                    loop {
                        if cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        match receiver.recv_timeout(Duration::from_millis(250)) {
                            Err(crossbeam_channel::RecvTimeoutError::Timeout)
                                if !closed.load(Ordering::Acquire) =>
                            {
                                continue
                            }
                            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => return Ok(()),
                            _ => {}
                        }
                        if cancelled.load(Ordering::Acquire) {
                            return Ok(());
                        }
                        // Read stopped BEFORE the snapshot, so Stop cannot race away its last horizon.
                        let final_horizon = closed.load(Ordering::Acquire);
                        let complete = planner.advance(&|| cancelled.load(Ordering::Acquire))?;
                        if complete || final_horizon {
                            return Ok(());
                        }
                    }
                };
                if let Err(error) = run() {
                    if !cancelled.load(Ordering::Acquire) {
                        capture::report(
                            &session,
                            "Раннее планирование остановлено; запись аудио продолжается",
                            &error,
                        );
                    }
                }
            })
            .context("start capture planner")?;
        tasks.push(task);
        // The first wakeup also covers seals published before notifier installation.
        let _ = wakeup.try_send(());
        Ok(Handle { stopped, signal })
    }
    pub fn attach(&self, owner: Arc<Mutex<capture::Writer>>, ffmpeg: PathBuf) -> Result<Handle> {
        let mut primary = owner.lock().unwrap_or_else(|e| e.into_inner());
        ensure!(!primary.has_planner(), "capture planner already attached");
        let handle = self.spawn(primary.session().to_path_buf(), owner.clone(), ffmpeg)?;
        primary.set_plan_signal(handle.signal.clone());
        Ok(handle)
    }
}
impl Drop for Planners {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Release);
        for task in self
            .tasks
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            let _ = task.join();
        }
    }
}

#[cfg(test)]
mod tests;
