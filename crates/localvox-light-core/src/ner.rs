//! Entity extraction WITHOUT a generative model (GLiNER).
//!
//! **Why this is not «a model checking a model».** GLiNER is an encoder-classifier: it
//! takes the text, cuts it into words and SCORES PIECES OF THE INPUT ITSELF. Its output is
//! not text but a list of spans «words i through j are a person». It physically cannot
//! invent a name that is not in the input: it has no decoder, it has nothing to generate a
//! new token with. That is exactly why it fits where a generative model must not be let in.
//!
//! **What it replaces.** Lists of names in the code. Previously «is this a person?» was
//! decided by a dictionary — and that is a dead end for two reasons: there are infinitely
//! many names in a language, and a list never resolves homonymy («Роман закроет задачу»
//! versus «дописать роман»), because it does not see the context. GLiNER does see it: the
//! entity types are given as STRINGS («человек», «дата», «сумма»), and the decision is made
//! from the surroundings.
//!
//! **What it does NOT replace.** Numbers. Numerals and dates are a closed class with a
//! finite grammar; on them, deterministic rules beat any model on recall and give
//! hundred-percent explainability. Numbers stay in `num2words`, and that is not a fallback
//! but the right tool.
//!
//! **The model is optional.** If it is not in `models/ner-gliner`, the name check simply
//! does not work, and we SAY SO. Silently pretending that the names have been checked is
//! not allowed.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use ort::session::{builder::GraphOptimizationLevel, Session};
use ort::value::Tensor;

/// `ort` errors are not Send+Sync, so they do not go into anyhow directly.
fn ort_err<E: std::fmt::Display>(ctx: &'static str) -> impl FnOnce(E) -> anyhow::Error {
    move |e| anyhow::anyhow!("{ctx}: {e}")
}


/// The maximum number of words in a window. The model's limit is 384 tokens including the
/// label prompt; on Cyrillic, mDeBERTa gives ~2 tokens per word, so 120 words + the labels
/// fit with a margin. The overlap is there so that an entity is not torn apart by a
/// boundary.
const WINDOW_WORDS: usize = 120;
const WINDOW_OVERLAP: usize = 20;

/// The confidence threshold. It is calibrated on our own minutes; 0.5 is the GLiNER
/// default.
const DEFAULT_THRESHOLD: f32 = 0.5;

/// A found entity — ALWAYS a piece of the input text.
#[derive(Debug, Clone, PartialEq)]
pub struct Entity {
    /// The surface form — exactly the one that stands in the text.
    pub text: String,
    /// The type, as we named it on the input («человек», «дата»…).
    pub label: String,
    pub score: f32,
    /// The bounds in the source text (bytes).
    pub start: usize,
    pub end: usize,
}

/// A word of the source text and its bounds. GLiNER works at the level of WORDS, not
/// subwords — so XLM-R's crooked subword offsets do not concern us at all.
struct Word {
    text: String,
    start: usize,
    end: usize,
}

pub struct Ner {
    session: Mutex<Session>,
    tokenizer: tokenizers::Tokenizer,
    max_width: usize,
    threshold: f32,
}

/// Whether the model is there. The check happens BEFORE any work: promising a name check
/// without a model is not allowed.
pub fn model_dir() -> Option<PathBuf> {
    model_dir_near(None)
}

/// The same, but with an EXPLICITLY KNOWN ASR model directory.
///
/// This is not a convenience but the fix for a real bug (acceptance run 13.07.2026). The
/// daemon passes the path to the ASR model to the child process as a FLAG (`--model-dir`),
/// because on Windows autostart its working directory is `system32`. And NER looked for
/// itself by `cwd` and, of course, did not find it: the name check SILENTLY did not work,
/// and there was nothing to notice it by.
///
/// The same class of bug already happened with languages. The rule is one: if the caller
/// knows the path to the models — ask him, not the current directory.
pub fn model_dir_near(asr_model_dir: Option<&Path>) -> Option<PathBuf> {
    crate::lang::sibling_model_dir(crate::lang::NER, asr_model_dir)
}

impl Ner {
    /// Load the model from a directory: `*.onnx` + `tokenizer.json` + `gliner_config.json`.
    pub fn open(dir: &Path) -> Result<Self> {
        // Which file exactly is NOT all the same. QUANTIZATION CHANGES NOT ONLY THE WEIGHT
        // BUT ALSO THE CALIBRATION OF THE SCORES, and this is measured, not assumed
        // (13.07.2026):
        //
        //   phrase «Роман закроет задачу»   int8 → 0.22   q4f16 → 0.67   fp16 → 0.89
        //   phrase «Marcus will ship it»    int8 → 0.17   q4f16 → 0.88   fp16 → 0.97
        //
        // int8 FALLS APART on mDeBERTa: the signal survives (the ranking is correct), but
        // the absolute values drop fourfold — and at any sane threshold the model silently
        // finds NOTHING. This is the worst kind of breakage: not a crash but a quiet «there
        // are no entities», indistinguishable from an honestly empty recording.
        //
        // That is why the file is chosen by PREFERENCE, not «the first one that turns up».
        const PREFERRED: [&str; 3] = ["model_fp16.onnx", "model_q4f16.onnx", "model.onnx"];
        let model = match std::env::var_os("LOCALVOX_NER_MODEL_FILE") {
            Some(f) => dir.join(f),
            None => PREFERRED
                .iter()
                .map(|f| dir.join(f))
                .find(|p| p.is_file())
                .or_else(|| {
                    std::fs::read_dir(dir)
                        .ok()?
                        .flatten()
                        .map(|e| e.path())
                        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("onnx"))
                })
                .with_context(|| format!("there is not a single *.onnx in {}", dir.display()))?,
        };
        anyhow::ensure!(model.is_file(), "no model file: {}", model.display());

        let name = model.file_name().unwrap_or_default().to_string_lossy();
        if name.contains("int8") || name.contains("uint8") || name.contains("quantized") {
            tracing::warn!(
                "NER: an int8 model was taken ({name}) — on mDeBERTa it falls apart on \
                 calibration and silently finds no entities. Take model_fp16.onnx or model_q4f16.onnx"
            );
        }

        let tokenizer = tokenizers::Tokenizer::from_file(dir.join("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("tokenizer.json: {e}"))?;

        // max_width — how many words one entity may occupy. This is a property of the
        // TRAINED model: get it wrong and the whole layout of the logits shifts.
        let cfg: serde_json::Value = std::fs::read(dir.join("gliner_config.json"))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default();
        let max_width = cfg
            .get("max_width")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(12) as usize;

        // ONNX Runtime keeps quiet on its side: otherwise it buries the log in the
        // allocator's internal bookkeeping on every inference (see `crate::onnx`).
        crate::onnx::init();

        let session = Session::builder()
            .map_err(ort_err("building the ONNX session"))?
            .with_optimization_level(GraphOptimizationLevel::Level3)
            .map_err(ort_err("optimization level"))?
            .commit_from_file(&model)
            .map_err(ort_err("loading the NER model"))?;

        let threshold = std::env::var("LOCALVOX_NER_THRESHOLD")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(DEFAULT_THRESHOLD);

        tracing::info!(
            "NER: {} (max_width {max_width}, threshold {threshold})",
            model.display()
        );
        Ok(Self {
            session: Mutex::new(session),
            tokenizer,
            max_width,
            threshold,
        })
    }

    /// The entities of a text. `labels` are the types as STRINGS («человек», «дата»,
    /// «сумма»): no lists of names, no language binding in the code.
    pub fn extract(&self, text: &str, labels: &[String]) -> Result<Vec<Entity>> {
        self.extract_at(text, labels, self.threshold)
    }

    /// The threshold for the ANSWER (strict). The record is read more generously — see
    /// `people_in_source`.
    pub fn threshold(&self) -> f32 {
        self.threshold
    }

    /// NAMES, separated from ROLES — by two INDEPENDENT passes.
    ///
    /// **Why separate them at all.** Recordings without a single name exist (a voice note, a
    /// monologue), and the minutes have to call the speaker something: «спикер»,
    /// «говорящий», «рассказчик», «автор». That is a ROLE, not a person, and to accuse over
    /// it means sending every honest set of minutes of a nameless recording into quarantine.
    /// There are infinitely many words for a role: enumerating them is as hopeless as a list
    /// of names.
    ///
    /// **Why two passes and not two labels in one.** Labels in one pass TAKE WEIGHT AWAY
    /// FROM EACH OTHER, and recall collapses — measured: with a pair of labels «Роман
    /// закроет задачу» stopped being a person, and homonymy was the whole point of the
    /// exercise. We ask separately and compare afterwards.
    ///
    /// **The rule: the name must OUTWEIGH the role on the same span.** Measured:
    ///
    /// ```text
    ///   Говорящий   name 0.53 < role 0.79  → role
    ///   Рассказчик  name 0.55 < role 0.62  → role
    ///   Роман       name 0.54 > role 0.49  → NAME
    ///   Достоевский name 0.53 > role 0.51  → NAME
    ///   Marcus      name 0.88 > role 0.77  → NAME
    /// ```
    ///
    /// An absolute threshold for the role will not do: the role label fires on names too
    /// (Иван — 0.78). Only the ORDER matters, not the magnitude.
    pub fn names(
        &self,
        text: &str,
        name_label: &str,
        role_label: &str,
        threshold: f32,
    ) -> Result<Vec<Entity>> {
        let names = self.extract_at(text, &[name_label.to_string()], threshold)?;
        if role_label.trim().is_empty() {
            return Ok(names);
        }
        // We collect the roles GENEROUSLY: what matters is their score for the comparison,
        // not the fact itself.
        let roles = self.extract_at(text, &[role_label.to_string()], 0.1)?;
        Ok(names
            .into_iter()
            .filter(|n| {
                roles
                    .iter()
                    .filter(|r| r.start < n.end && n.start < r.end)
                    .all(|r| n.score > r.score)
            })
            .collect())
    }

    /// The same, but with an explicit threshold.
    ///
    /// The threshold for the RECORD and for the ANSWER are different, and that is
    /// fundamental — the same asymmetry as with numbers. A superfluous entity in the RECORD
    /// is harmless: all it can do is ground something in the answer. A superfluous entity in
    /// the ANSWER is a false accusation and the quarantine of honest minutes. So we read the
    /// record generously and the answer strictly.
    pub fn extract_at(&self, text: &str, labels: &[String], threshold: f32) -> Result<Vec<Entity>> {
        let words = split_words(text);
        if words.is_empty() || labels.is_empty() {
            return Ok(Vec::new());
        }

        let mut out: Vec<Entity> = Vec::new();
        let mut start = 0usize;
        while start < words.len() {
            let end = (start + WINDOW_WORDS).min(words.len());
            out.extend(self.extract_window(text, &words[start..end], labels, threshold)?);
            if end == words.len() {
                break;
            }
            start = end.saturating_sub(WINDOW_OVERLAP);
        }

        // The windows overlap — one and the same entity arrives twice.
        out.sort_by(|a, b| a.start.cmp(&b.start).then(a.end.cmp(&b.end)));
        out.dedup_by(|a, b| a.start == b.start && a.end == b.end && a.label == b.label);
        Ok(out)
    }

    fn extract_window(
        &self,
        text: &str,
        words: &[Word],
        labels: &[String],
        threshold: f32,
    ) -> Result<Vec<Entity>> {
        // GLiNER's input: [CLS] <<ENT>> label <<ENT>> label <<SEP>> word word … [SEP]
        // The prompt made of the labels comes FIRST, so the indices of the text words are
        // shifted.
        let mut pieces: Vec<String> = Vec::with_capacity(labels.len() * 2 + 1 + words.len());
        for l in labels {
            pieces.push("<<ENT>>".to_string());
            pieces.push(l.clone());
        }
        pieces.push("<<SEP>>".to_string());
        let prompt_len = pieces.len();
        pieces.extend(words.iter().map(|w| w.text.clone()));

        let enc = self
            .tokenizer
            .encode(tokenizers::EncodeInput::Single(pieces.clone().into()), true)
            .map_err(|e| anyhow::anyhow!("tokenization: {e}"))?;

        let ids: Vec<i64> = enc.get_ids().iter().map(|&x| i64::from(x)).collect();
        let attn: Vec<i64> = enc
            .get_attention_mask()
            .iter()
            .map(|&x| i64::from(x))
            .collect();

        // words_mask: the FIRST subtoken of every word of the text carries its number
        // (starting from one), everything else carries 0. That is how the model learns where
        // the prompt ends and the record begins.
        let mut words_mask: Vec<i64> = Vec::with_capacity(ids.len());
        let mut prev: Option<u32> = None;
        for wid in enc.get_word_ids() {
            match wid {
                Some(w) if Some(*w) != prev && (*w as usize) >= prompt_len => {
                    words_mask.push((*w as usize - prompt_len + 1) as i64);
                }
                _ => words_mask.push(0),
            }
            prev = *wid;
        }

        let n_words = words.len();
        let n_spans = n_words * self.max_width;
        let mut span_idx: Vec<i64> = Vec::with_capacity(n_spans * 2);
        let mut span_mask: Vec<bool> = Vec::with_capacity(n_spans);
        for i in 0..n_words {
            for w in 0..self.max_width {
                span_idx.push(i as i64);
                span_idx.push((i + w) as i64);
                span_mask.push(i + w < n_words);
            }
        }

        let seq = ids.len();
        let (shape, flat) = {
            let mut session = self
                .session
                .lock()
                .map_err(|_| anyhow::anyhow!("the NER session is poisoned"))?;
            let t_ids =
                Tensor::<i64>::from_array(([1usize, seq], ids)).map_err(ort_err("input_ids"))?;
            let t_attn = Tensor::<i64>::from_array(([1usize, seq], attn))
                .map_err(ort_err("attention_mask"))?;
            let t_wm = Tensor::<i64>::from_array(([1usize, seq], words_mask))
                .map_err(ort_err("words_mask"))?;
            let t_len = Tensor::<i64>::from_array(([1usize, 1usize], vec![n_words as i64]))
                .map_err(ort_err("text_lengths"))?;
            let t_span = Tensor::<i64>::from_array(([1usize, n_spans, 2usize], span_idx))
                .map_err(ort_err("span_idx"))?;
            let t_smask = Tensor::<bool>::from_array(([1usize, n_spans], span_mask))
                .map_err(ort_err("span_mask"))?;
            let outputs = session
                .run(ort::inputs![
                    "input_ids" => t_ids,
                    "attention_mask" => t_attn,
                    "words_mask" => t_wm,
                    "text_lengths" => t_len,
                    "span_idx" => t_span,
                    "span_mask" => t_smask,
                ])
                .map_err(ort_err("session.run"))?;
            let logits = outputs
                .get("logits")
                .context("there is no «logits» among the NER outputs")?
                .try_extract_array::<f32>()
                .map_err(ort_err("extracting the logits"))?;
            let shape: Vec<usize> = logits.shape().to_vec();
            let flat: Vec<f32> = logits.as_slice().context("the logits are not contiguous")?.to_vec();
            (shape, flat)
        };
        // [1, words, width, labels] — we check against the shape instead of taking it on
        // trust: mixed-up axes would give plausible but wrong entities, and that would not
        // crash, it would quietly lie.
        anyhow::ensure!(
            shape.len() == 4
                && shape[1] == n_words
                && shape[2] == self.max_width
                && shape[3] == labels.len(),
            "unexpected shape of the logits: {shape:?} (expected [1, {n_words}, {}, {}])",
            self.max_width,
            labels.len()
        );
        let mut found: Vec<Entity> = Vec::new();
        for i in 0..n_words {
            for w in 0..self.max_width {
                if i + w >= n_words {
                    continue;
                }
                for (k, label) in labels.iter().enumerate() {
                    let idx = ((i * self.max_width) + w) * labels.len() + k;
                    let score = sigmoid(flat[idx]);
                    if score < threshold {
                        continue;
                    }
                    let (b, e) = (words[i].start, words[i + w].end);
                    found.push(Entity {
                        // The surface form is A PIECE OF THE INPUT, cut out along word
                        // boundaries. Not «what the model wrote» (it cannot write anything),
                        // but exactly what stands in the text.
                        text: text[b..e].to_string(),
                        label: label.clone(),
                        score,
                        start: b,
                        end: e,
                    });
                }
            }
        }

        // Intersecting spans: we keep the confident ones, greedily.
        found.sort_by(|a, b| b.score.total_cmp(&a.score));
        let mut kept: Vec<Entity> = Vec::new();
        for e in found {
            if kept.iter().any(|k| e.start < k.end && k.start < e.end) {
                continue;
            }
            kept.push(e);
        }
        Ok(kept)
    }
}

fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x).exp())
}

/// The splitting into words is the same as GLiNER's (`words_splitter_type: whitespace`): a
/// word is a sequence of alphanumerics (with hyphens inside), and every single punctuation
/// mark is a separate token. Diverging from the training here is not allowed: the spans
/// would slip.
fn split_words(text: &str) -> Vec<Word> {
    let mut out = Vec::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let ch = match text[i..].chars().next() {
            Some(c) => c,
            None => break,
        };
        let len = ch.len_utf8();
        if ch.is_whitespace() {
            i += len;
            continue;
        }
        if ch.is_alphanumeric() || ch == '_' {
            let start = i;
            let mut end = i;
            let mut rest = &text[i..];
            while let Some(c) = rest.chars().next() {
                if c.is_alphanumeric() || c == '_' {
                    end += c.len_utf8();
                    rest = &text[end..];
                } else if (c == '-' || c == '_')
                    && rest[c.len_utf8()..]
                        .chars()
                        .next()
                        .is_some_and(|n| n.is_alphanumeric())
                {
                    // a hyphen INSIDE a word: «онлайн-встреча» is one word
                    end += c.len_utf8();
                    rest = &text[end..];
                } else {
                    break;
                }
            }
            out.push(Word {
                text: text[start..end].to_string(),
                start,
                end,
            });
            i = end;
        } else {
            // a single punctuation mark
            out.push(Word {
                text: text[i..i + len].to_string(),
                start: i,
                end: i + len,
            });
            i += len;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn words_are_split_the_way_the_model_was_trained() {
        let w = split_words("Привет, онлайн-встреча в 15:00!");
        let got: Vec<&str> = w.iter().map(|x| x.text.as_str()).collect();
        assert_eq!(
            got,
            vec!["Привет", ",", "онлайн-встреча", "в", "15", ":", "00", "!"]
        );
        // the bounds must point into the source text — we return the span by them
        assert_eq!(
            &"Привет, онлайн-встреча в 15:00!"[w[2].start..w[2].end],
            "онлайн-встреча"
        );
    }
}
