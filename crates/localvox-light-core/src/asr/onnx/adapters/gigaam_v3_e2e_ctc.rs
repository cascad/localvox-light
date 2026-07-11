//! Адаптер для **GigaAM v3 E2E CTC** (`v3_e2e_ctc.onnx` / `.int8.onnx`).
//!
//! Особенности:
//! * Mel-конфиг — общий [`MelConfig::GIGAAM_V3`] (64 mel, win/hop 320/160, HTK).
//! * Словарь — SentencePiece (`v3_e2e_ctc_vocab.txt`, 257 пиесов, blank на индексе 0).
//! * Текст модели уже с пунктуацией и нормализацией («2024», «,», «.» и т.п.).

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::asr::onnx::adapter::{InputLayout, OnnxAdapter};
use crate::asr::onnx::mel::MelConfig;
use crate::asr::onnx::vocab::{self, Vocab};

/// Кол-во классов у GigaAM v3 E2E CTC (см. `v3_e2e_ctc.yaml`: `num_classes: 257`).
const EXPECTED_VOCAB_SIZE: usize = 257;

/// Имя файла со словарём в каталоге модели.
pub const VOCAB_FILENAME: &str = "v3_e2e_ctc_vocab.txt";

pub struct GigaamV3E2eCtc {
    mel: MelConfig,
    vocab: Vocab,
}

impl GigaamV3E2eCtc {
    /// Создать адаптер по каталогу с моделью; ожидается файл `v3_e2e_ctc_vocab.txt` внутри.
    pub fn from_model_dir(dir: &Path) -> Result<Arc<dyn OnnxAdapter>> {
        let vocab_path = dir.join(VOCAB_FILENAME);
        let vocab = Vocab::load(&vocab_path)?;
        if vocab.tokens.len() != EXPECTED_VOCAB_SIZE {
            anyhow::bail!(
                "{}: ожидалось {} классов в словаре, получено {}",
                vocab_path.display(),
                EXPECTED_VOCAB_SIZE,
                vocab.tokens.len()
            );
        }
        Ok(Arc::new(Self {
            mel: MelConfig::GIGAAM_V3,
            vocab,
        }))
    }
}

impl OnnxAdapter for GigaamV3E2eCtc {
    fn name(&self) -> &str {
        "gigaam-v3-e2e-ctc"
    }

    fn mel_config(&self) -> &MelConfig {
        &self.mel
    }

    fn input_layout(&self) -> InputLayout {
        InputLayout::BatchFeatTime
    }

    fn input_name(&self) -> &str {
        // Имя в экспорте gigaam.to_onnx() — `features`; уточним в Stage 5 при загрузке графа.
        "features"
    }

    fn length_input_name(&self) -> Option<&str> {
        // По `--inspect-model`: входы — `features` [batch, 64, seq_len] (f32)
        // и `feature_lengths` [batch] (i64). Множественное число!
        Some("feature_lengths")
    }

    fn output_name(&self) -> &str {
        // `log_probs` для CTC-варианта.
        "log_probs"
    }

    fn vocab_size(&self) -> usize {
        self.vocab.tokens.len()
    }

    fn vocab(&self) -> &Vocab {
        &self.vocab
    }

    fn detokenize(&self, ids: &[usize]) -> String {
        let pieces: Vec<&str> = ids
            .iter()
            .filter_map(|&i| self.vocab.tokens.get(i).map(String::as_str))
            .collect();
        vocab::sp_detokenize(&pieces)
    }
}
