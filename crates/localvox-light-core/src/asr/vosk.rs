//! Vosk (Kaldi-based) ASR engine. CPU-friendly, offline.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use vosk::DecodingState;

pub struct VoskEngine {
    model: Arc<vosk::Model>,
}

fn complete_result_text(r: &vosk::CompleteResult) -> String {
    match r {
        vosk::CompleteResult::Single(s) => s.text.to_string(),
        vosk::CompleteResult::Multiple(m) => m
            .alternatives
            .first()
            .map(|a| a.text.to_string())
            .unwrap_or_default(),
    }
}

impl VoskEngine {
    pub fn new(model_dir: &Path) -> Result<Self> {
        let am_dir = model_dir.join("am");
        if !am_dir.is_dir() {
            anyhow::bail!(
                "Нет каталога am/ в {} — укажите корень модели Vosk (после распаковки zip: папка с am/, conf/, graph/). Скачать: https://huggingface.co/mychen76/vosk-models/resolve/main/ru/vosk-model-ru-0.42.zip",
                model_dir.display()
            );
        }
        ::vosk::set_log_level(::vosk::LogLevel::Warn);
        tracing::debug!("Loading Vosk model: {} ...", model_dir.display());
        let t0 = std::time::Instant::now();
        let model = ::vosk::Model::new(model_dir.to_string_lossy().as_ref())
            .context("Failed to load Vosk model")?;
        tracing::debug!("Vosk ready ({:.1}s)", t0.elapsed().as_secs_f64());
        Ok(Self {
            model: Arc::new(model),
        })
    }

    /// 16 kHz mono `f32` PCM произвольной длины: потоковая подача в Vosk (несколько финализированных фраз).
    pub fn transcribe_pcm_16k_mono_f32(&self, samples: &[f32]) -> Result<String> {
        self.transcribe_pcm_16k_mono_f32_with_progress(samples, |_, _| {})
    }

    /// Как [`Self::transcribe_pcm_16k_mono_f32`], после каждого чанка вызывается `progress(done_samples, total_samples)`.
    pub fn transcribe_pcm_16k_mono_f32_with_progress(
        &self,
        samples: &[f32],
        mut progress: impl FnMut(usize, usize),
    ) -> Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let total = samples.len();
        progress(0, total);
        let mut recognizer = ::vosk::Recognizer::new(&self.model, 16000.0)
            .context("Failed to create Vosk recognizer")?;
        const CHUNK: usize = 4096;
        let mut pieces: Vec<String> = Vec::new();
        let mut processed = 0usize;
        for chunk in samples.chunks(CHUNK) {
            let samples_i16: Vec<i16> = chunk
                .iter()
                .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
                .collect();
            let state = recognizer
                .accept_waveform(&samples_i16)
                .map_err(|e| anyhow::anyhow!("Vosk accept_waveform failed: {:?}", e))?;
            if state == DecodingState::Failed {
                anyhow::bail!("Vosk decoding failed mid-stream");
            }
            if state == DecodingState::Finalized {
                let t = complete_result_text(&recognizer.result());
                let t = t.trim();
                if !t.is_empty() {
                    pieces.push(t.to_string());
                }
            }
            processed += chunk.len();
            progress(processed.min(total), total);
        }
        let tail = complete_result_text(&recognizer.final_result());
        let tail = tail.trim();
        if !tail.is_empty() {
            pieces.push(tail.to_string());
        }
        progress(total, total);
        Ok(pieces.join(" "))
    }
}

impl super::AsrEngine for VoskEngine {
    fn name(&self) -> &str {
        "vosk"
    }

    fn transcribe(&self, samples: &[f32]) -> Result<String> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        let samples_i16: Vec<i16> = samples
            .iter()
            .map(|&s| (s * 32767.0).clamp(-32768.0, 32767.0) as i16)
            .collect();

        let mut recognizer = ::vosk::Recognizer::new(&self.model, 16000.0)
            .context("Failed to create Vosk recognizer")?;
        recognizer
            .accept_waveform(&samples_i16)
            .map_err(|e| anyhow::anyhow!("Vosk accept_waveform failed: {:?}", e))?;

        let text = complete_result_text(&recognizer.final_result());
        Ok(text.trim().to_string())
    }
}
