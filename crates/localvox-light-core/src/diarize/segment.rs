//! Segmentation: WHO speaks in every frame (pyannote segmentation-3.0, ONNX).
//!
//! The model is local: inside a 10-second window it tells apart no more than THREE voices
//! and no more than TWO at a time. This is not the product's ceiling: how many people are
//! on the recording in total is decided by the clustering of embeddings across windows
//! ([`super::cluster`]).
//!
//! **The model eats the raw waveform** — no mel spectrum, no features. The input is
//! `[1, 1, 160000]` f32 (10 s at 16 kHz), the output is `[1, 589, 7]`.
//!
//! **The output is LOG-PROBABILITIES, not logits.** The last node of the graph is
//! LogSoftmax, even though the tensor is called `logits`. For `argmax` it makes no
//! difference, but anyone who sets out to compare the output with a threshold or to
//! average it MUST remember about `exp()`. The tensor name lies here, and this was
//! verified by running it, not read somewhere.
//!
//! **We do not take quantization.** Measured on our own audio: int8 agrees with fp32 by
//! argmax on only 93% of frames, smears the boundary of a turn from 3.5 s to 6.6 s and
//! invents overlapping speech that is not there. The saving is 4 MB. fp16 on CPU simply
//! crashes with a segfault. We take fp32: 5.7 MB next to GigaAM's 240 MB is noise.

use std::path::{Path, PathBuf};
#[cfg(feature = "onnx")]
use std::sync::Mutex;

#[cfg(feature = "onnx")]
use anyhow::{Context, Result};
#[cfg(feature = "onnx")]
use ort::session::{builder::GraphOptimizationLevel, Session};
#[cfg(feature = "onnx")]
use ort::value::Tensor;

/// `ort` errors are not Send+Sync and do not go into anyhow directly.
#[cfg(feature = "onnx")]
fn ort_err<E: std::fmt::Display>(ctx: &'static str) -> impl FnOnce(E) -> anyhow::Error {
    move |e| anyhow::anyhow!("{ctx}: {e}")
}

#[cfg(feature = "onnx")]
const FILE: &str = "segmentation.onnx";

pub const SAMPLE_RATE: u32 = 16_000;
/// The window the model was trained on: 10 s. Feeding it another window is possible (T is
/// dynamic), but nobody promised any quality outside of the training setup.
pub const WINDOW_SAMPLES: usize = 160_000;
/// The output frame shift — 270 samples (16.875 ms, 58.9 frames per second).
pub const FRAME_SHIFT: usize = 270;
/// The receptive field of a frame — 991 samples; the centre of the first frame lies in the
/// middle of it.
pub const RECEPTIVE_FIELD: usize = 991;
/// How many voices the model tells apart INSIDE a window. Not to be confused with the
/// number of people on the recording: that is not limited by anything.
pub const LOCAL_SPEAKERS: usize = 3;
const CLASSES: usize = 7;

/// The unfolding of a powerset class into the activity of the three local voices.
///
/// The order is confirmed by TWO independent sources — the `config.json` of the re-export
/// and the C++ implementation `InitPowersetMapping()` in sherpa-onnx. Getting it wrong is
/// the worst possible bug: nothing crashes, you simply get plausible garbage in which the
/// turns have been handed out to the wrong people.
const POWERSET: [[bool; LOCAL_SPEAKERS]; CLASSES] = [
    [false, false, false], // 0 — silence
    [true, false, false],  // 1 — A
    [false, true, false],  // 2 — B
    [false, false, true],  // 3 — C
    [true, true, false],   // 4 — A+B
    [true, false, true],   // 5 — A+C
    [false, true, true],   // 6 — B+C
];

/// Who spoke in one frame — the three local voices of the window.
pub type Frame = [bool; LOCAL_SPEAKERS];

/// One window of the recording, labelled frame by frame.
///
/// The local voices of DIFFERENT windows are different people: «A» in the fifth window and
/// «A» in the sixth must not be linked to one another. They will be linked by the
/// clustering of embeddings, not by a permutation of labels; that is why we honestly keep
/// the local labels here.
pub struct Window {
    /// The start of the window in samples from the start of the recording.
    pub offset: usize,
    pub frames: Vec<Frame>,
}

impl Window {
    /// The bounds of frame `i` in samples from the start of the RECORDING.
    pub fn frame_span(&self, i: usize) -> (usize, usize) {
        let start = self.offset + i * FRAME_SHIFT;
        (start, start + RECEPTIVE_FIELD)
    }
}

/// The centre of frame `i` in seconds from the start of the window.
pub fn frame_center_sec(i: usize) -> f64 {
    (i * FRAME_SHIFT + RECEPTIVE_FIELD / 2) as f64 / SAMPLE_RATE as f64
}

/// The model directory: `LOCALVOX_DIARIZE_MODEL_DIR` → next to the ASR model → `models/diarize`.
///
/// The same way of searching as NER's, and for the same reason: on Windows autostart the
/// working directory is `system32`, and a model looked up «by cwd» silently fails to be
/// found.
pub fn model_dir_near(asr_model_dir: Option<&Path>) -> Option<PathBuf> {
    crate::lang::sibling_model_dir(crate::lang::DIARIZE, asr_model_dir)
}

#[cfg(feature = "onnx")]
pub struct Segmenter {
    session: Mutex<Session>,
    input: String,
    output: String,
}

#[cfg(feature = "onnx")]
impl Segmenter {
    pub fn open(dir: &Path) -> Result<Self> {
        let model = match std::env::var_os("LOCALVOX_DIARIZE_MODEL_FILE") {
            Some(f) => dir.join(f),
            None => dir.join(FILE),
        };
        anyhow::ensure!(
            model.is_file(),
            "no segmentation model: {} (put segmentation.onnx there — \
             onnx-community/pyannote-segmentation-3.0, fp32)",
            model.display()
        );

        let name = model.file_name().unwrap_or_default().to_string_lossy();
        if name.contains("int8") || name.contains("quantized") || name.contains("fp16") {
            tracing::warn!(
                "diarization: a quantized model was taken ({name}). MEASURED: int8 diverges \
                 from fp32 on 7% of frames and smears turn boundaries, fp16 on CPU crashes. \
                 Take fp32 — it weighs 5.7 MB"
            );
        }

        // ONNX Runtime keeps quiet on its side: otherwise it buries the log in the
        // allocator's internal bookkeeping on every inference (see `crate::onnx`).
        crate::onnx::init();

        let session = Session::builder()
            .map_err(ort_err("building the ONNX session"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err("optimization level"))?
            .commit_from_file(&model)
            .map_err(ort_err("loading the segmentation model"))?;

        // The tensor names of different re-exports are DIFFERENT (`input_values`/`logits`
        // in onnx-community, `x`/`y` in sherpa). The graph is one and the same, so we ask
        // it for the names instead of hardcoding one of the two and crashing on the other.
        let input = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .context("the segmentation model has no inputs")?;
        let output = session
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .context("the segmentation model has no outputs")?;

        tracing::info!("diarization: segmentation {} ({input} → {output})", model.display());
        Ok(Self {
            session: Mutex::new(session),
            input,
            output,
        })
    }

    /// Label ONE window of exactly [`WINDOW_SAMPLES`] samples.
    ///
    /// The sliding over the recording and the buffering are done by [`super::Runner`]: it
    /// knows nothing about ONNX, and that is why the whole pipeline is proven by tests
    /// without a single weights file on disk.
    pub fn frames(&self, samples: &[f32]) -> Result<Vec<Frame>> {
        anyhow::ensure!(
            samples.len() == WINDOW_SAMPLES,
            "the segmentation window is exactly {WINDOW_SAMPLES} samples, got {}",
            samples.len()
        );
        let tensor = Tensor::<f32>::from_array(([1usize, 1, samples.len()], samples.to_vec()))
            .map_err(ort_err("creating the waveform tensor"))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("session mutex poisoned"))?;
        let outputs = session
            .run(ort::inputs![self.input.as_str() => tensor])
            .map_err(ort_err("segmentation session.run"))?;

        let value = outputs
            .get(self.output.as_str())
            .ok_or_else(|| anyhow::anyhow!("no tensor «{}» among the outputs", self.output))?;
        let array = value
            .try_extract_array::<f32>()
            .map_err(ort_err("extracting the segmentation output"))?;
        let shape = array.shape();
        anyhow::ensure!(
            shape.len() == 3 && shape[0] == 1 && shape[2] == CLASSES,
            "unexpected shape of the segmentation output: {shape:?} (expected [1, frames, 7])"
        );
        let flat: Vec<f32> = array.iter().copied().collect();
        Ok(decode_powerset(&flat, shape[1]))
    }
}

#[cfg(feature = "onnx")]
impl super::Segmentation for Segmenter {
    fn frames(&self, window: &[f32]) -> Result<Vec<Frame>> {
        Segmenter::frames(self, window)
    }
}

/// How many output frames actually describe `samples` samples of the signal.
///
///
/// A frame covers `RECEPTIVE_FIELD` samples, starting at `i * FRAME_SHIFT`. A frame whose
/// CENTRE ended up beyond the end of the recording is already labelling our padding.
pub fn frames_covering(samples: usize) -> usize {
    if samples == 0 {
        return 0;
    }
    let half = RECEPTIVE_FIELD / 2;
    samples.saturating_sub(half).div_ceil(FRAME_SHIFT).max(1)
}

/// Powerset class → the activity of the three local voices, one frame at a time.
///
/// `logits` is a flat `[frames, 7]`. We take the argmax; these are LOG-PROBABILITIES, so
/// the exponent is not needed: it is monotonic and does not change the order.
fn decode_powerset(logits: &[f32], n_frames: usize) -> Vec<Frame> {
    (0..n_frames)
        .map(|f| {
            let row = &logits[f * CLASSES..(f + 1) * CLASSES];
            let cls = row
                .iter()
                .enumerate()
                .max_by(|(_, a), (_, b)| a.total_cmp(b))
                .map(|(i, _)| i)
                .unwrap_or(0);
            POWERSET[cls]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(cls: usize) -> [f32; CLASSES] {
        let mut r = [-9.0f32; CLASSES];
        r[cls] = -0.01; // log-probabilities: all negative, the winner is closer to zero
        r
    }

    /// The order of the seven classes is the place where a mistake DOES NOT CRASH but
    /// quietly hands out the turns to the wrong people. It is pinned down here so that it
    /// cannot be «fixed» in passing.
    #[test]
    fn the_powerset_table_is_the_one_the_model_was_trained_with() {
        let logits: Vec<f32> = (0..CLASSES).flat_map(|c| row(c)).collect();
        let frames = decode_powerset(&logits, CLASSES);
        assert_eq!(frames[0], [false, false, false], "0 — silence");
        assert_eq!(frames[1], [true, false, false], "1 — the first");
        assert_eq!(frames[2], [false, true, false], "2 — the second");
        assert_eq!(frames[3], [false, false, true], "3 — the third");
        assert_eq!(frames[4], [true, true, false], "4 — the first and the second");
        assert_eq!(frames[5], [true, false, true], "5 — the first and the third");
        assert_eq!(frames[6], [false, true, true], "6 — the second and the third");
    }

    /// Overlapping speech is not a failure but a fact: two people speak at once. The model
    /// must say so, not pick «the main one».
    #[test]
    fn two_people_talking_at_once_are_both_reported() {
        let frames = decode_powerset(&row(4), 1);
        assert_eq!(frames[0].iter().filter(|x| **x).count(), 2);
    }

    /// The silence we padded the tail with must not turn into a turn: frames whose centre
    /// lies beyond the end of the recording are already labelling our own silence.
    #[test]
    fn the_silence_we_padded_with_is_not_part_of_the_recording() {
        // 1 s of recording = 16000 samples. A frame steps by 270.
        let n = frames_covering(16_000);
        assert!(
            (57..=60).contains(&n),
            "a second of recording was described by {n} frames (expected ~59)"
        );
        // A whole window — all 589 frames of the model.
        assert!(frames_covering(WINDOW_SAMPLES) >= 589);
        assert_eq!(frames_covering(0), 0);
    }

    #[test]
    fn frame_time_starts_at_the_middle_of_the_receptive_field() {
        assert!((frame_center_sec(0) - 0.0309).abs() < 0.001);
        // Frame 59 is roughly a second.
        assert!((frame_center_sec(59) - 1.026).abs() < 0.01);
    }
}
