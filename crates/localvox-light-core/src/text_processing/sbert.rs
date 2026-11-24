//! Official SbertPuncCase BERT + token-classification head, CPU safetensors.
use super::TextProcessor;
use anyhow::{ensure, Context, Result};
use candle_core::{DType, Device, Module, Tensor};
use candle_nn::{Linear, VarBuilder};
use candle_transformers::models::bert::{BertModel, Config};
use std::path::Path;
use tokenizers::{EncodeInput, Encoding, Tokenizer};

pub const ENGINE: &str = "sbert-punc-case-ru-v1";
const LIMIT: usize = 512;
const LABELS: usize = 13;

pub struct SbertPuncCase {
    model: BertModel,
    head: Linear,
    tokenizer: Tokenizer,
    pool: rayon::ThreadPool,
}
impl SbertPuncCase {
    pub fn new(directory: &Path) -> Result<Self> {
        let config: Config =
            serde_json::from_slice(&std::fs::read(directory.join("config.json"))?)?;
        ensure!(
            config.max_position_embeddings == LIMIT,
            "unsupported Sbert token limit"
        );
        // Buffered safetensors avoids mmap lifetime/truncation hazards when an installation changes.
        let weights = VarBuilder::from_buffered_safetensors(
            std::fs::read(directory.join("model.safetensors"))?,
            DType::F32,
            &Device::Cpu,
        )?;
        let model = BertModel::load(weights.pp("bert"), &config)?;
        let head = candle_nn::linear(config.hidden_size, LABELS, weights.pp("classifier"))?;
        let mut tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("Sbert tokenizer: {e}"))?;
        tokenizer
            .with_truncation(None)
            .map_err(|e| anyhow::anyhow!("Sbert truncation: {e}"))?;
        tokenizer.with_padding(None);
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(2)
            .thread_name(|i| format!("punctuation-{i}"))
            .build()?;
        Ok(Self {
            model,
            head,
            tokenizer,
            pool,
        })
    }

    fn encode(&self, words: &[String]) -> Result<Encoding> {
        self.tokenizer
            .encode(EncodeInput::Single(words.into()), true)
            .map_err(|e| anyhow::anyhow!("Sbert word tokenization: {e}"))
    }

    fn infer(&self, encoding: &Encoding) -> Result<Vec<Vec<f32>>> {
        let length = encoding.len();
        ensure!(length <= LIMIT, "Sbert sequence exceeds model limit");
        self.pool.install(|| {
            let ids = Tensor::from_vec(encoding.get_ids().to_vec(), (1, length), &Device::Cpu)?;
            let types = Tensor::zeros_like(&ids)?;
            let mask = Tensor::from_vec(
                encoding.get_attention_mask().to_vec(),
                (1, length),
                &Device::Cpu,
            )?;
            let hidden = self.model.forward(&ids, &types, Some(&mask))?;
            let logits = self.head.forward(&hidden)?.squeeze(0)?.to_vec2::<f32>()?;
            Ok(logits)
        })
    }

    fn process_words(&self, words: &[String]) -> Result<Vec<String>> {
        if words.is_empty() {
            return Ok(vec![]);
        }
        let encoding = self.encode(words)?;
        if encoding.len() > LIMIT {
            ensure!(
                words.len() > 1,
                "a single word exceeds the Sbert token limit"
            );
            let middle = words.len().div_ceil(2);
            let mut left = self.process_words(&words[..middle])?;
            left.extend(self.process_words(&words[middle..])?);
            return Ok(left);
        }
        let logits = self.infer(&encoding)?;
        let mut first = vec![None; words.len()];
        for (token, word) in encoding.get_word_ids().iter().enumerate() {
            if let Some(word) = word {
                let slot = first.get_mut(*word as usize).context("Sbert word index")?;
                if slot.is_none() {
                    *slot = Some(token);
                }
            }
        }
        words
            .iter()
            .zip(first)
            .map(|(word, token)| {
                let row = logits
                    .get(token.context("Sbert omitted a word")?)
                    .context("Sbert token logits")?;
                ensure!(
                    row.len() == LABELS && row.iter().all(|v| v.is_finite()),
                    "invalid Sbert classifier output"
                );
                let mut label = 0;
                for i in 1..LABELS {
                    if row[i] > row[label] {
                        label = i;
                    }
                }
                render(word, label)
            })
            .collect()
    }
}
impl TextProcessor for SbertPuncCase {
    fn name(&self) -> &str {
        ENGINE
    }
    fn process(&self, text: &str) -> Result<String> {
        let words: Vec<_> = text.split_whitespace().map(str::to_lowercase).collect();
        Ok(self.process_words(&words)?.join(" "))
    }
}

fn render(word: &str, label: usize) -> Result<String> {
    ensure!(
        (1..LABELS).contains(&label),
        "unmapped Sbert label {label}; source text retained"
    );
    let mut result = match (label - 1) / 4 {
        0 => word.to_owned(),
        1 => {
            let mut chars = word.chars();
            let first = chars
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_default();
            first + chars.as_str()
        }
        2 => word.to_uppercase(),
        _ => unreachable!(),
    };
    let punctuation = match (label - 1) % 4 {
        1 => Some('.'),
        2 => Some(','),
        3 => Some('?'),
        _ => None,
    };
    // Existing punctuation is already source data, never duplicate it.
    if !word.ends_with(['.', ',', '?', '!', ';', ':']) {
        if let Some(mark) = punctuation {
            result.push(mark);
        }
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires pinned local model and make-asr-reference.py output; no devices"]
    fn real_model_matches_transformers_logits_labels_and_long_word_boundaries() {
        let file = std::env::var("LOCALVOX_SBERT_REFERENCE").expect("LOCALVOX_SBERT_REFERENCE");
        let fixture: serde_json::Value =
            serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
        let processor =
            SbertPuncCase::new(Path::new(fixture["model_dir"].as_str().unwrap())).unwrap();
        for case in fixture["cases"].as_array().unwrap() {
            let text = case["text"].as_str().unwrap();
            assert_eq!(
                processor.process(text).unwrap(),
                case["rendered"].as_str().unwrap()
            );
            if !case["logits"].is_null() {
                let words: Vec<_> = text.split_whitespace().map(str::to_lowercase).collect();
                let encoding = processor.encode(&words).unwrap();
                assert_eq!(
                    serde_json::to_value(encoding.get_ids()).unwrap(),
                    case["token_ids"]
                );
                assert_eq!(
                    serde_json::to_value(encoding.get_word_ids()).unwrap(),
                    case["word_ids"]
                );
                let logits = processor.infer(&encoding).unwrap();
                let expected: Vec<Vec<f32>> =
                    serde_json::from_value(case["logits"].clone()).unwrap();
                assert_eq!(logits.len(), expected.len());
                let delta = logits
                    .iter()
                    .flatten()
                    .zip(expected.iter().flatten())
                    .map(|(a, b)| (a - b).abs())
                    .fold(0.0_f32, f32::max);
                assert!(
                    delta < 0.005,
                    "Sbert logits differ from Transformers: {delta}"
                );
            }
        }
        assert_eq!(processor.process("  \n ").unwrap(), "");
    }
    #[test]
    fn labels_only_add_casing_and_punctuation_and_keep_digits_and_existing_marks() {
        assert_eq!(render("ёлка", 6).unwrap(), "Ёлка.");
        assert_eq!(render("api", 11).unwrap(), "API,");
        assert_eq!(render("2026", 4).unwrap(), "2026?");
        assert_eq!(render("3.14", 2).unwrap(), "3.14.");
        assert_eq!(render("привет,", 3).unwrap(), "привет,");
        assert!(render("исходник", 0).is_err());
        assert!(render("исходник", 13).is_err());
    }
}
