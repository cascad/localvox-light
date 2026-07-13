//! The voice print: a vector of the VOICE, not of the words.
//!
//! The model is ERes2Net (3D-Speaker, Apache-2.0). It is trained to tell people apart by
//! timbre, so it does not know the language and must not know it: Russian, English and any
//! other language sound the same to it. It cannot invent a person for the same reason
//! GLiNER cannot — it has no decoder, it has nothing to generate anything new with. Its
//! output is 512 numbers describing A PIECE OF THE INPUT.
//!
//! **Quantization is forbidden.** The clustering works by cosine, that is, by the ANGLES
//! between vectors, and the angle is the first thing int8 breaks. We already got burned
//! with int8-mDeBERTa: the scores collapsed fourfold, and the model silently found
//! nothing. The model weighs 27 MB against GigaAM's 240 — there is nothing to save here.
//!
//! **The main risk is not in the model but in the features.** If fbank is computed
//! «almost the same way», the vectors turn into garbage, the clusters fall apart, and
//! there is NOT A SINGLE error in the log. That is why [`super::fbank`] repeats kaldi down
//! to the last constant, and a golden test checks both the features and the vector itself
//! against a reference.

use std::path::Path;
use std::sync::Mutex;

use anyhow::{Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use super::fbank;

fn ort_err<E: std::fmt::Display>(ctx: &'static str) -> impl FnOnce(E) -> anyhow::Error {
    move |e| anyhow::anyhow!("{ctx}: {e}")
}

const FILE: &str = "embedding.onnx";

/// Computing a vector for a piece of speech shorter than this is pointless: you cannot
/// recognise a person from a single syllable. Half a second is 50 fbank frames.
const MIN_SAMPLES: usize = 8_000; // 0.5 s

/// Fewer frames than this and the model SILENTLY RETURNS NaN (measured: at `T <= 8` the
/// graph does not crash and does not complain, it returns NaN, which then poisons the
/// cosine and the whole clustering). The pooling averages over time, and there is simply
/// nothing to average there.
const MIN_FRAMES: usize = 9;

pub struct Voice {
    session: Mutex<Session>,
    input: String,
    output: String,
}

impl Voice {
    pub fn open(dir: &Path) -> Result<Self> {
        let model = match std::env::var_os("LOCALVOX_DIARIZE_EMBEDDING_FILE") {
            Some(f) => dir.join(f),
            None => dir.join(FILE),
        };
        anyhow::ensure!(
            model.is_file(),
            "no embedding model: {} (put embedding.onnx there — ERes2Net/CAM++, fp32)",
            model.display()
        );

        let name = model.file_name().unwrap_or_default().to_string_lossy();
        if name.contains("int8") || name.contains("quantized") || name.contains("fp16") {
            tracing::warn!(
                "diarization: a quantized voice model was taken ({name}). The clustering \
                 works by the ANGLES between vectors, and the angle is the first thing \
                 quantization breaks — the voices will start sticking together. Take fp32"
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
            .map_err(ort_err("loading the embedding model"))?;

        // The tensor names of different exports differ (`x`/`embedding` in 3D-Speaker,
        // `feats`/`embs` in WeSpeaker) — we ask the graph instead of guessing.
        let input = session
            .inputs()
            .first()
            .map(|i| i.name().to_string())
            .context("the embedding model has no inputs")?;
        let output = session
            .outputs()
            .first()
            .map(|o| o.name().to_string())
            .context("the embedding model has no outputs")?;

        tracing::info!(
            "diarization: voice {} ({input} → {output})",
            model.display()
        );
        Ok(Self {
            session: Mutex::new(session),
            input,
            output,
        })
    }

    /// The voice vector for a piece of speech. We do not normalise: normalisation is done
    /// by whoever compares (the cosine in [`super::cluster`]) — and it does it the same way
    /// for everyone.
    pub fn embed(&self, samples: &[f32]) -> Result<Vec<f32>> {
        if samples.len() < MIN_SAMPLES {
            // Not an error: a stretch that is too short simply does not describe a voice.
            return Ok(Vec::new());
        }
        let feats = fbank::compute(samples);
        anyhow::ensure!(!feats.is_empty(), "fbank did not compute");
        if feats.len() < MIN_FRAMES {
            return Ok(Vec::new());
        }

        let t = feats.len();
        let flat: Vec<f32> = feats.into_iter().flatten().collect();
        let tensor = Tensor::<f32>::from_array(([1usize, t, fbank::NUM_MEL_BINS], flat))
            .map_err(ort_err("creating the feature tensor"))?;

        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("session mutex poisoned"))?;
        let outputs = session
            .run(ort::inputs![self.input.as_str() => tensor])
            .map_err(ort_err("embedder session.run"))?;

        let value = outputs
            .get(self.output.as_str())
            .ok_or_else(|| anyhow::anyhow!("no tensor «{}» among the outputs", self.output))?;
        let array = value
            .try_extract_array::<f32>()
            .map_err(ort_err("extracting the voice vector"))?;
        let shape = array.shape();
        anyhow::ensure!(
            shape.len() == 2 && shape[0] == 1,
            "unexpected shape of the voice vector: {shape:?} (expected [1, N])"
        );
        let v: Vec<f32> = array.iter().copied().collect();
        // NaN does not «spoil one vector» — it poisons the WHOLE clustering: the distance
        // to NaN is neither greater nor smaller than anything, and comparisons silently
        // lose their meaning. Better to end up without this stretch than without the
        // labelling.
        anyhow::ensure!(
            v.iter().all(|x| x.is_finite()),
            "the model returned NaN on a stretch of {} frames",
            t
        );
        Ok(v)
    }
}

impl super::Embedder for Voice {
    fn embed(&self, samples: &[f32]) -> Result<Vec<f32>> {
        Voice::embed(self, samples)
    }
}
