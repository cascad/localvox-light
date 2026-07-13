//! The adapter for **GigaAM v3 E2E CTC** (`v3_e2e_ctc.onnx` / `.int8.onnx`).
//!
//! The specifics:
//! * The mel config is the common [`MelConfig::GIGAAM_V3`] (64 mels, win/hop 320/160, HTK).
//! * The vocabulary is SentencePiece (`v3_e2e_ctc_vocab.txt`, 257 pieces, blank at index 0).
//! * The model's text already comes with punctuation and normalization («2024», «,», «.»
//!   and so on).

use std::path::Path;
use std::sync::Arc;

use anyhow::Result;

use crate::asr::onnx::adapter::{InputLayout, OnnxAdapter};
use crate::asr::onnx::mel::MelConfig;
use crate::asr::onnx::vocab::{self, Vocab};

/// The number of classes of GigaAM v3 E2E CTC (see `v3_e2e_ctc.yaml`: `num_classes: 257`).
const EXPECTED_VOCAB_SIZE: usize = 257;

/// The name of the vocabulary file in the model directory.
pub const VOCAB_FILENAME: &str = "v3_e2e_ctc_vocab.txt";

pub struct GigaamV3E2eCtc {
    mel: MelConfig,
    vocab: Vocab,
}

impl GigaamV3E2eCtc {
    /// Create the adapter from a model directory; the file `v3_e2e_ctc_vocab.txt` is
    /// expected inside.
    pub fn from_model_dir(dir: &Path) -> Result<Arc<dyn OnnxAdapter>> {
        let vocab_path = dir.join(VOCAB_FILENAME);
        let vocab = Vocab::load(&vocab_path)?;
        if vocab.tokens.len() != EXPECTED_VOCAB_SIZE {
            anyhow::bail!(
                "{}: expected {} classes in the vocabulary, got {}",
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
        // The name in the gigaam.to_onnx() export is `features`; we will confirm it in
        // Stage 5 when loading the graph.
        "features"
    }

    fn length_input_name(&self) -> Option<&str> {
        // According to `--inspect-model`: the inputs are `features` [batch, 64, seq_len]
        // (f32) and `feature_lengths` [batch] (i64). Plural!
        Some("feature_lengths")
    }

    fn output_name(&self) -> &str {
        // `log_probs` for the CTC variant.
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
