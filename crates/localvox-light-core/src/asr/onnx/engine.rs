//! `ort::Session` + адаптер: загрузка модели и end-to-end inference.
//!
//! Pipeline: PCM 16k mono f32 → log-mel ([`crate::asr::onnx::mel`]) → ONNX-инференс
//! ([`ort`]) → greedy CTC ([`crate::asr::onnx::ctc`]) → детокенизация (адаптер).

use std::path::Path;
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

use super::adapter::{InputLayout, OnnxAdapter};
use super::ctc::greedy_decode;
use super::mel::log_mel_spectrogram;

/// Удобный шорткат: `ort::Error<R>` не реализует `std::error::Error`, потому что
/// generic `R != ()`. Поэтому `?` / `anyhow::Context` напрямую не работают.
/// Конвертируем через `Display`.
fn ort_err<E: std::fmt::Display>(ctx: &'static str) -> impl FnOnce(E) -> anyhow::Error {
    move |e| anyhow::anyhow!("{ctx}: {e}")
}

pub struct OnnxEngine {
    /// `Session::run` берёт `&mut self`, у `AsrEngine::transcribe` — `&self`.
    /// `Mutex` даёт interior mutability + `Sync`.
    session: Mutex<Session>,
    adapter: Arc<dyn OnnxAdapter>,
}

impl OnnxEngine {
    /// Загрузить ONNX-модель из `model_path` и связать с адаптером.
    pub fn new(model_path: &Path, adapter: Arc<dyn OnnxAdapter>) -> Result<Self> {
        let session = Session::builder()
            .map_err(ort_err("ort SessionBuilder"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err("ort optimization level"))?
            .commit_from_file(model_path)
            .map_err(ort_err("открытие ONNX-модели"))
            .with_context(|| format!("файл: {}", model_path.display()))?;
        Ok(Self {
            session: Mutex::new(session),
            adapter,
        })
    }

    /// Транскрибировать PCM 16 kHz mono f32.
    pub fn transcribe_pcm_16k_mono_f32(&self, samples: &[f32]) -> Result<String> {
        let cfg = self.adapter.mel_config();
        let spec = log_mel_spectrogram(samples, cfg);
        if spec.n_frames == 0 {
            return Ok(String::new());
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
            .map_err(|_| anyhow::anyhow!("session mutex отравлен"))?;

        let feat_tensor = Tensor::<f32>::from_array((feat_shape, feat_data))
            .map_err(ort_err("создание тензора фичей"))?;
        let outputs = if let Some(ln) = len_name {
            let len_tensor = Tensor::<i64>::from_array(([1_usize], vec![spec.n_frames as i64]))
                .map_err(ort_err("создание тензора length"))?;
            session
                .run(ort::inputs![feat_name => feat_tensor, ln => len_tensor])
                .map_err(ort_err("session.run"))?
        } else {
            session
                .run(ort::inputs![feat_name => feat_tensor])
                .map_err(ort_err("session.run"))?
        };

        let logits_value = outputs
            .get(out_name)
            .ok_or_else(|| anyhow::anyhow!("в выходах модели нет тензора «{out_name}»"))?;
        let extracted = logits_value
            .try_extract_array::<f32>()
            .map_err(ort_err("извлечение logits как f32"))?;
        let shape = extracted.shape();
        if shape.len() != 3 || shape[0] != 1 {
            anyhow::bail!(
                "неожиданная форма logits: {:?} (ожидалось [1, time, vocab])",
                shape
            );
        }
        let vocab_size = shape[2];
        let flat: Vec<f32> = extracted.iter().copied().collect();
        let ids = greedy_decode(&flat, vocab_size, self.adapter.vocab().blank_id);
        Ok(self.adapter.detokenize(&ids))
    }
}

impl crate::asr::AsrEngine for OnnxEngine {
    fn name(&self) -> &str {
        self.adapter.name()
    }

    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        self.transcribe_pcm_16k_mono_f32(samples)
    }
}

/// Описание одного входа/выхода ONNX-графа.
pub struct OnnxIoInfo {
    pub name: String,
    pub type_repr: String,
}

/// Загрузить ONNX-модель и вернуть имена + типы всех её входов и выходов.
/// Утилита для отладки/верификации адаптеров.
pub fn inspect_model_io(model_path: &Path) -> Result<(Vec<OnnxIoInfo>, Vec<OnnxIoInfo>)> {
    let session = Session::builder()
        .map_err(ort_err("ort SessionBuilder"))?
        .commit_from_file(model_path)
        .map_err(ort_err("открытие ONNX-модели"))
        .with_context(|| format!("файл: {}", model_path.display()))?;
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
