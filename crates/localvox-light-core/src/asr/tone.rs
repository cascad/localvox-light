//! T-one CPU ONNX: raw PCM 8 kHz, recurrent 300 ms frames, official greedy CTC.
//! An inference call owns fresh recurrent state. Audio-window times stay with cook.
use super::AsrEngine;
use anyhow::{ensure, Context, Result};
use half::f16;
use ort::{session::Session, value::Tensor};
use rubato::Resampler;
use std::{path::Path, sync::Mutex};

const FRAME: usize = 2400;
const STATE: usize = 219729;
const CLASSES: usize = 35;
const LABELS: &str = "абвгдеёжзийклмнопрстуфхцчшщъыьэюя ";
pub const ENGINE: &str = "t-one-greedy-v1";

fn ort_error<E: std::fmt::Display>(e: E) -> anyhow::Error {
    anyhow::anyhow!("T-one ONNX: {e}")
}

pub struct TOneEngine {
    session: Mutex<Session>,
}
impl TOneEngine {
    pub fn new(directory: &Path) -> Result<Self> {
        validate_vocabulary(directory)?;
        crate::onnx::init();
        let session = Session::builder()
            .map_err(ort_error)?
            .with_intra_threads(2)
            .map_err(ort_error)?
            .with_inter_threads(1)
            .map_err(ort_error)?
            .commit_from_file(directory.join("model.onnx"))
            .map_err(ort_error)?;
        let inputs: Vec<_> = session.inputs().iter().map(|v| v.name()).collect();
        ensure!(
            inputs == ["signal", "state"],
            "unsupported T-one inputs: {inputs:?}"
        );
        ensure!(
            session.outputs().len() == 2,
            "T-one must return logits and state"
        );
        Ok(Self {
            session: Mutex::new(session),
        })
    }

    /// Native sample rate entry point, also used for parity with the upstream pipeline.
    pub fn transcribe_pcm_8k(&self, samples: &[i16]) -> Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let mut session = self
            .session
            .lock()
            .map_err(|_| anyhow::anyhow!("T-one mutex poisoned"))?;
        let mut state = vec![f16::ZERO; STATE];
        let length = (samples.len() + 2 * FRAME).div_ceil(FRAME) * FRAME;
        let mut decoder = Greedy::default();
        for offset in (0..length).step_by(FRAME) {
            let signal: Vec<i32> = (offset..offset + FRAME)
                .map(|i| {
                    i.checked_sub(FRAME)
                        .and_then(|i| samples.get(i))
                        .copied()
                        .unwrap_or(0) as i32
                })
                .collect();
            let signal = Tensor::from_array(([1_usize, FRAME, 1], signal)).map_err(ort_error)?;
            let previous = Tensor::from_array(([1_usize, STATE], state)).map_err(ort_error)?;
            let outputs = session
                .run(ort::inputs!["signal" => signal, "state" => previous])
                .map_err(ort_error)?;
            let (shape, logits) = outputs[0].try_extract_tensor::<f32>().map_err(ort_error)?;
            ensure!(
                shape.len() == 3 && shape[0] == 1 && shape[2] == CLASSES as i64,
                "unexpected T-one logits shape: {shape:?}"
            );
            for row in logits.chunks_exact(CLASSES) {
                ensure!(row.iter().all(|v| !v.is_nan()), "T-one returned NaN logits");
                // Keep the first index on ties, matching numpy.argmax.
                let mut token = 0;
                for i in 1..CLASSES {
                    if row[i] > row[token] {
                        token = i;
                    }
                }
                decoder.push(token)?;
            }
            let (shape, next) = outputs[1].try_extract_tensor::<f16>().map_err(ort_error)?;
            ensure!(
                shape.as_ref() == [1, STATE as i64],
                "unexpected T-one state shape: {shape:?}"
            );
            state = next.to_vec();
        }
        Ok(decoder.text.trim().into())
    }
}
impl AsrEngine for TOneEngine {
    fn name(&self) -> &str {
        ENGINE
    }
    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        self.transcribe_pcm_8k(&pcm_8k(samples)?)
    }
}

pub fn validate_vocabulary(directory: &Path) -> Result<()> {
    let vocab: serde_json::Value = serde_json::from_slice(
        &std::fs::read(directory.join("vocab.json")).context("T-one vocab.json")?,
    )?;
    for (i, ch) in LABELS.chars().enumerate() {
        let key = if ch == ' ' {
            "|".into()
        } else {
            ch.to_string()
        };
        ensure!(
            vocab[&key].as_u64() == Some(i as u64),
            "unsupported T-one vocabulary: {key}"
        );
    }
    ensure!(
        vocab["[PAD]"].as_u64() == Some(34),
        "unsupported T-one blank token"
    );
    Ok(())
}

#[derive(Default)]
struct Greedy {
    previous: Option<usize>,
    text: String,
}
impl Greedy {
    fn push(&mut self, token: usize) -> Result<()> {
        ensure!(token < CLASSES, "invalid T-one CTC token {token}");
        if self.previous != Some(token) && token != 34 {
            self.text
                .push(LABELS.chars().nth(token).context("T-one label")?);
        }
        self.previous = Some(token);
        Ok(())
    }
}

/// Offline band-limited conversion. Compensate the FFT filter delay and flush the tail.
fn pcm_8k(samples: &[f32]) -> Result<Vec<i16>> {
    ensure!(
        samples.iter().all(|v| v.is_finite()),
        "non-finite ASR samples"
    );
    if samples.is_empty() {
        return Ok(vec![]);
    }
    let mut resampler = rubato::FftFixedIn::<f32>::new(16000, 8000, 512, 1, 1)?;
    let delay = resampler.output_delay();
    let count = samples.len().div_ceil(2);
    let mut converted = Vec::with_capacity(count + delay + 512);
    let mut offset = 0;
    while converted.len() < count + delay {
        let size = resampler.input_frames_next();
        let mut block = vec![0.0; size];
        let available = samples.len().saturating_sub(offset).min(size);
        block[..available].copy_from_slice(
            &samples[offset.min(samples.len())..offset.min(samples.len()) + available],
        );
        converted.extend(resampler.process(&[block], None)?.remove(0));
        offset = offset.saturating_add(size);
    }
    Ok(converted[delay..delay + count]
        .iter()
        .map(|v| (v * 32768.0).round().clamp(-32768.0, 32767.0) as i16)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires pinned local model and make-asr-reference.py output; no devices"]
    fn real_model_matches_upstream_and_each_call_resets_state() {
        let file = std::env::var("LOCALVOX_TONE_REFERENCE").expect("LOCALVOX_TONE_REFERENCE");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let engine = TOneEngine::new(Path::new(fixture["model_dir"].as_str().unwrap())).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let mut wav = hound::WavReader::open(case["wav"].as_str().unwrap()).unwrap();
            assert_eq!(wav.spec().sample_rate, 8000);
            let pcm: Vec<i16> = wav
                .samples()
                .collect::<std::result::Result<_, _>>()
                .unwrap();
            let text = engine.transcribe_pcm_8k(&pcm).unwrap();
            assert!(!text.is_empty(), "speech fixture must contain speech");
            assert_eq!(text, case["text"].as_str().unwrap());
            assert_eq!(engine.transcribe_pcm_8k(&vec![0; 8001]).unwrap(), "");
            assert_eq!(engine.transcribe_pcm_8k(&pcm).unwrap(), text);
        }
    }
    #[test]
    fn ctc_repeats_cross_frames_and_blank_separates_repeated_letters() {
        let mut d = Greedy::default();
        for frame in [&[0, 0, 1][..], &[1, 34, 1, 33, 33, 6][..]] {
            for &token in frame {
                d.push(token).unwrap();
            }
        }
        assert_eq!(d.text, "абб ё");
        assert!(d.push(35).is_err());
    }
    #[test]
    fn resampling_preserves_short_and_odd_lengths_and_rejects_invalid_pcm() {
        for n in [0, 1, 17, 511, 512, 513, 16001] {
            assert_eq!(pcm_8k(&vec![0.0; n]).unwrap(), vec![0; n.div_ceil(2)]);
        }
        assert!(pcm_8k(&[f32::NAN]).is_err());
    }
    #[test]
    fn resampling_filters_audio_above_8k_nyquist_instead_of_aliasing_it() {
        let wave = |hz: f32| {
            (0..16000)
                .map(|i| (i as f32 * hz * std::f32::consts::TAU / 16000.0).sin() * 0.5)
                .collect::<Vec<_>>()
        };
        let rms = |v: Vec<i16>| {
            (v[1000..7000]
                .iter()
                .map(|s| (*s as f64).powi(2))
                .sum::<f64>()
                / 6000.0)
                .sqrt()
        };
        let speech = rms(pcm_8k(&wave(1000.0)).unwrap());
        let alias = rms(pcm_8k(&wave(6000.0)).unwrap());
        assert!(
            speech > 10000.0 && alias < speech * 0.01,
            "speech={speech}, alias={alias}"
        );
    }
}
