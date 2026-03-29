//! Vosk (Kaldi-based) ASR engine. CPU-friendly, offline.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

pub struct VoskEngine {
    model: Arc<vosk::Model>,
}

impl VoskEngine {
    pub fn new(model_dir: &Path) -> Result<Self> {
        let am_dir = model_dir.join("am");
        if !am_dir.is_dir() {
            anyhow::bail!(
                "Нет каталога am/ в {} — укажите корень модели Vosk (после распаковки zip: папка с am/, conf/, graph/). Скачать: https://alphacephei.com/vosk/models",
                model_dir.display()
            );
        }
        ::vosk::set_log_level(::vosk::LogLevel::Warn);
        tracing::info!("Loading Vosk model: {} ...", model_dir.display());
        let t0 = std::time::Instant::now();
        let model = ::vosk::Model::new(model_dir.to_string_lossy().as_ref())
            .context("Failed to load Vosk model")?;
        tracing::info!("Vosk ready ({:.1}s)", t0.elapsed().as_secs_f64());
        Ok(Self {
            model: Arc::new(model),
        })
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

        let result = recognizer.final_result();
        let text = match result {
            ::vosk::CompleteResult::Single(s) => s.text.to_string(),
            ::vosk::CompleteResult::Multiple(m) => m
                .alternatives
                .first()
                .map(|a| a.text.to_string())
                .unwrap_or_default(),
        };
        Ok(text.trim().to_string())
    }
}
