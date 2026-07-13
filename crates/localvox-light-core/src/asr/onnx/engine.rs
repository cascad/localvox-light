//! `ort::Session` + an adapter: loading the model and end-to-end inference.
//!
//! Pipeline: PCM 16k mono f32 → log-mel ([`crate::asr::onnx::mel`]) → ONNX inference
//! ([`ort`]) → greedy CTC ([`crate::asr::onnx::ctc`]) → detokenization (the adapter).

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use super::adapter::{InputLayout, OnnxAdapter};
use super::ctc::greedy_decode_timed;
use super::mel::log_mel_spectrogram;

/// A handy shortcut: `ort::Error<R>` does not implement `std::error::Error`, because the
/// generic `R != ()`. That is why `?` / `anyhow::Context` do not work directly.
/// We convert via `Display`.
fn ort_err<E: std::fmt::Display>(ctx: &'static str) -> impl FnOnce(E) -> anyhow::Error {
    move |e| anyhow::anyhow!("{ctx}: {e}")
}

pub struct OnnxEngine {
    /// `Session::run` takes `&mut self`, while `AsrEngine::transcribe` has `&self`.
    /// `Mutex` gives interior mutability + `Sync`.
    session: Mutex<Session>,
    adapter: Arc<dyn OnnxAdapter>,
}

impl OnnxEngine {
    /// Load an ONNX model from `model_path` and bind it to an adapter.
    pub fn new(model_path: &Path, adapter: Arc<dyn OnnxAdapter>) -> Result<Self> {
        // ONNX Runtime keeps quiet on its side: otherwise it buries the log in the
        // allocator's internal bookkeeping on every inference (see `crate::onnx`).
        crate::onnx::init();

        let session = Session::builder()
            .map_err(ort_err("ort SessionBuilder"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err("ort optimization level"))?
            .commit_from_file(model_path)
            .map_err(ort_err("opening the ONNX model"))
            .with_context(|| format!("file: {}", model_path.display()))?;
        Ok(Self {
            session: Mutex::new(session),
            adapter,
        })
    }

    /// A word with timecodes inside the audio chunk that was fed in (seconds from its
    /// start).
    pub fn transcribe_words_pcm_16k_mono_f32(&self, samples: &[f32]) -> Result<Vec<Word>> {
        let audio_sec = samples.len() as f64 / 16_000.0;
        let (ids_frames, n_logit_frames) = self.decode_timed(samples)?;
        if n_logit_frames == 0 {
            return Ok(Vec::new());
        }
        // The model thins out time (a strided encoder), so «how many seconds are in one
        // frame of logits» is computed from the facts: the duration of the audio divided
        // by the number of frames. That way the formula does not depend on the concrete
        // architecture of the adapter.
        let sec_per_frame = audio_sec / n_logit_frames as f64;
        let vocab = self.adapter.vocab();

        // SentencePiece tokens: a new word starts with ▁. We assemble the words,
        // remembering the frame of the first MEANINGFUL token and of the last one.
        //
        // The start of a word is NOT the frame of the separator. The vocabulary has a
        // separate token «▁» (a bare space), and CTC spits it out well before the speech
        // itself — on frame zero. If it is taken as the start of a word, the word «sticks»
        // to the start of the window: at the acceptance run of 13.07.2026 «Этими» got a
        // duration of 0.00–4.08 s, and the player honestly played four seconds of the
        // PREVIOUS audio.
        // Inside a word the letters go one after another. If half a second gapes between
        // the tokens of a word, the early tokens are aligned wrongly (on int8 CTC spits out
        // the first letter long before the speech), and the start of the word must be taken
        // to be the last CONTINUOUS run. Otherwise the word stretches seconds backwards and
        // the player plays the previous audio.
        let max_gap_frames = (INTRA_WORD_GAP_SEC / sec_per_frame).ceil() as usize;

        let mut words: Vec<Word> = Vec::new();
        let mut cur: Vec<(usize, usize)> = Vec::new(); // (id, frame)

        let mut flush = |cur: &mut Vec<(usize, usize)>, words: &mut Vec<Word>| {
            if cur.is_empty() {
                return;
            }
            let ids: Vec<usize> = cur.iter().map(|(id, _)| *id).collect();
            let text = self.adapter.detokenize(&ids).trim().to_string();
            // The frames of the meaningful tokens (a bare «▁» owns no time).
            let frames: Vec<usize> = cur
                .iter()
                .filter(|(id, _)| {
                    vocab
                        .tokens
                        .get(*id)
                        .map(|p| !p.trim_start_matches(super::vocab::SP_SPACE).is_empty())
                        .unwrap_or(false)
                })
                .map(|(_, f)| *f)
                .collect();
            cur.clear();
            if text.is_empty() || frames.is_empty() {
                return;
            }
            // The start is the first frame of the last continuous run.
            let mut start = frames[0];
            for pair in frames.windows(2) {
                if pair[1].saturating_sub(pair[0]) > max_gap_frames {
                    start = pair[1];
                }
            }
            let end = *frames.last().unwrap_or(&start);
            words.push(Word {
                text,
                start_sec: start as f64 * sec_per_frame,
                // +1: a frame is an interval, not a point
                end_sec: (end.max(start) + 1) as f64 * sec_per_frame,
            });
        };

        for (id, frame) in ids_frames {
            let piece = vocab.tokens.get(id).map(String::as_str).unwrap_or("");
            if piece.starts_with(super::vocab::SP_SPACE) && !cur.is_empty() {
                flush(&mut cur, &mut words);
            }
            cur.push((id, frame));
        }
        flush(&mut cur, &mut words);
        Ok(words)
    }

    /// Transcribe PCM 16 kHz mono f32.
    pub fn transcribe_pcm_16k_mono_f32(&self, samples: &[f32]) -> Result<String> {
        let (ids_frames, _) = self.decode_timed(samples)?;
        let ids: Vec<usize> = ids_frames.into_iter().map(|(id, _)| id).collect();
        Ok(self.adapter.detokenize(&ids))
    }

    /// Inference + greedy CTC: the tokens with their frame numbers and the total number of
    /// logit frames. A single point for both the text and the per-word timecodes — so that
    /// the «words» and the «line» can never drift apart.
    fn decode_timed(&self, samples: &[f32]) -> Result<(Vec<(usize, usize)>, usize)> {
        let cfg = self.adapter.mel_config();
        let spec = log_mel_spectrogram(samples, cfg);
        if spec.n_frames == 0 {
            return Ok((Vec::new(), 0));
        }

        let feat_data: Vec<f32> = match self.adapter.input_layout() {
            InputLayout::BatchFeatTime => {
                let mut data = vec![0.0f32; spec.n_mels * spec.n_frames];
                for t in 0..spec.n_frames {
                    for m in 0..spec.n_mels {
                        data[m * spec.n_frames + t] = spec.data[t * spec.n_mels + m];
                    }
                }
                data
            }
            InputLayout::BatchTimeFeat => spec.data.clone(),
        };
        let feat_shape: [usize; 3] = match self.adapter.input_layout() {
            InputLayout::BatchFeatTime => [1, spec.n_mels, spec.n_frames],
            InputLayout::BatchTimeFeat => [1, spec.n_frames, spec.n_mels],
        };

        let feat_name = self.adapter.input_name();
        let len_name = self.adapter.length_input_name();
        let out_name = self.adapter.output_name();

        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("session mutex poisoned"))?;

        let feat_tensor = Tensor::<f32>::from_array((feat_shape, feat_data))
            .map_err(ort_err("creating the feature tensor"))?;
        let outputs = if let Some(ln) = len_name {
            let len_tensor = Tensor::<i64>::from_array(([1_usize], vec![spec.n_frames as i64]))
                .map_err(ort_err("creating the length tensor"))?;
            session
                .run(ort::inputs![feat_name => feat_tensor, ln => len_tensor])
                .map_err(ort_err("session.run"))?
        } else {
            session
                .run(ort::inputs![feat_name => feat_tensor])
                .map_err(ort_err("session.run"))?
        };

        let logits_value = outputs.get(out_name).ok_or_else(|| {
            anyhow::anyhow!("there is no tensor «{out_name}» among the model outputs")
        })?;
        let extracted = logits_value
            .try_extract_array::<f32>()
            .map_err(ort_err("extracting the logits as f32"))?;
        let shape = extracted.shape();
        if shape.len() != 3 || shape[0] != 1 {
            anyhow::bail!(
                "unexpected shape of the logits: {:?} (expected [1, time, vocab])",
                shape
            );
        }
        let n_logit_frames = shape[1];
        let vocab_size = shape[2];
        let flat: Vec<f32> = extracted.iter().copied().collect();
        let timed = greedy_decode_timed(&flat, vocab_size, self.adapter.vocab().blank_id);
        Ok((timed, n_logit_frames))
    }
}

/// The gap INSIDE a word beyond which the early tokens are treated as garbage alignment:
/// the letters of one word do not sound that far apart from each other.
const INTRA_WORD_GAP_SEC: f64 = 0.5;

/// A word with timecodes inside the inference window (seconds from the start of the
/// window).
#[derive(Debug, Clone)]
pub struct Word {
    pub text: String,
    pub start_sec: f64,
    pub end_sec: f64,
}

impl crate::asr::AsrEngine for OnnxEngine {
    fn name(&self) -> &str {
        self.adapter.name()
    }

    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        self.transcribe_pcm_16k_mono_f32(samples)
    }
}

/// The description of one input/output of an ONNX graph.
pub struct OnnxIoInfo {
    pub name: String,
    pub type_repr: String,
}

/// Load an ONNX model and return the names + types of all its inputs and outputs.
/// A utility for debugging/verifying adapters.
pub fn inspect_model_io(model_path: &Path) -> Result<(Vec<OnnxIoInfo>, Vec<OnnxIoInfo>)> {
    let session = Session::builder()
        .map_err(ort_err("ort SessionBuilder"))?
        .commit_from_file(model_path)
        .map_err(ort_err("opening the ONNX model"))
        .with_context(|| format!("file: {}", model_path.display()))?;
    let inputs = session
        .inputs()
        .iter()
        .map(|i| OnnxIoInfo {
            name: i.name().to_string(),
            type_repr: format!("{:?}", i.dtype()),
        })
        .collect();
    let outputs = session
        .outputs()
        .iter()
        .map(|o| OnnxIoInfo {
            name: o.name().to_string(),
            type_repr: format!("{:?}", o.dtype()),
        })
        .collect();
    Ok((inputs, outputs))
}
