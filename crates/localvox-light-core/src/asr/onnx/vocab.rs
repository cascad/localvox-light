//! Loading vocabularies in the `onnx-asr` / GigaAM v3 E2E format:
//! one line is `<TOKEN><space><ID>`, where TOKEN may be empty
//! (then the line is just `<ID>`, which corresponds to the blank token).

use std::path::Path;

use anyhow::{Context, Result};

/// The prefix marker of the start of a word in SentencePiece (U+2581, `▁`).
pub const SP_SPACE: &str = "\u{2581}";

#[derive(Clone, Debug)]
pub struct Vocab {
    /// The tokens by index (`tokens.len() == vocab_size`). An empty string means blank.
    pub tokens: Vec<String>,
    /// The index of the blank token.
    pub blank_id: usize,
}

impl Vocab {
    /// Load a vocabulary from a text file.
    ///
    /// The file format (as in `istupakov/gigaam-v3-onnx`):
    /// ```text
    /// 0
    /// ▁ 1
    /// . 2
    /// е 3
    /// ...
    /// ```
    /// The first line `<empty token><space><id>` (visually just `<id>`) is the blank.
    /// If there is no empty token, `blank_id` is taken as `tokens.len() - 1` (for char
    /// vocabs, where `<blk>` comes last).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading vocab {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parsing vocab {}", path.display()))
    }

    /// Parsing a vocabulary from a string (for tests and in-memory vocabs).
    pub fn parse(text: &str) -> Result<Self> {
        let mut entries: Vec<(String, usize)> = Vec::new();
        for (line_no, raw) in text.lines().enumerate() {
            if raw.is_empty() {
                continue;
            }
            // We split at the LAST space: token + " " + id. A whitespace token (`▁`) is ok,
            // as is an empty token (then the line is only `<id>`).
            let (tok, id_str) = match raw.rsplit_once(' ') {
                Some((t, i)) => (t, i),
                None => ("", raw),
            };
            let id: usize = id_str.trim().parse().with_context(|| {
                format!(
                    "vocab line {}: «{}» — failed to parse the id",
                    line_no + 1,
                    raw
                )
            })?;
            entries.push((tok.to_string(), id));
        }

        if entries.is_empty() {
            anyhow::bail!("the vocab is empty");
        }

        let max_id = entries.iter().map(|(_, i)| *i).max().unwrap_or(0);
        let mut tokens = vec![String::new(); max_id + 1];
        let mut seen = vec![false; max_id + 1];
        for (tok, id) in entries {
            if seen[id] {
                anyhow::bail!("vocab: duplicate id {}", id);
            }
            tokens[id] = tok;
            seen[id] = true;
        }
        if !seen.iter().all(|&s| s) {
            let gap = seen.iter().position(|&s| !s).unwrap_or(0);
            anyhow::bail!("vocab: id {} is missing (there is no such line)", gap);
        }

        let blank_id = tokens
            .iter()
            .position(|t| t.is_empty())
            .unwrap_or(tokens.len().saturating_sub(1));

        Ok(Self { tokens, blank_id })
    }

    /// Get the token by index. `None` — if the index is out of bounds.
    pub fn get(&self, idx: usize) -> Option<&str> {
        self.tokens.get(idx).map(String::as_str)
    }
}

/// SentencePiece detokenization: we glue the pieces together, replace `▁` (U+2581) with a
/// space and strip the possible leading space.
pub fn sp_detokenize(pieces: &[&str]) -> String {
    let joined: String = pieces.iter().copied().collect();
    let with_spaces = joined.replace(SP_SPACE, " ");
    with_spaces.trim_start().to_string()
}

/// Simple detokenization for char vocabs (NeMo / GigaAM v3 base): the tokens are already
/// single characters or a space — we glue them as they are.
pub fn char_detokenize(pieces: &[&str]) -> String {
    pieces.concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_e2e_vocab_with_blank_first() {
        let text = "0\n\u{2581} 1\n. 2\nе 3\n";
        let v = Vocab::parse(text).expect("parse");
        assert_eq!(v.tokens.len(), 4);
        assert_eq!(v.tokens[0], "");
        assert_eq!(v.tokens[1], "\u{2581}");
        assert_eq!(v.tokens[2], ".");
        assert_eq!(v.tokens[3], "е");
        assert_eq!(v.blank_id, 0);
    }

    #[test]
    fn parses_char_vocab_with_blank_last() {
        let text = "  0\nа 1\nб 2\n<blk> 3\n";
        let v = Vocab::parse(text).expect("parse");
        assert_eq!(v.tokens.len(), 4);
        assert_eq!(v.tokens[0], " ");
        assert_eq!(v.blank_id, 3);
    }

    #[test]
    fn detects_gaps() {
        let text = "\u{2581} 1\n. 3\n";
        assert!(Vocab::parse(text).is_err());
    }

    #[test]
    fn sp_detokenize_basic() {
        let pieces = ["\u{2581}В", "сегодня", "\u{2581}хорошо", "."];
        assert_eq!(sp_detokenize(&pieces), "Всегодня хорошо.");
    }

    #[test]
    fn sp_detokenize_leading_space() {
        let pieces = ["\u{2581}Привет", "\u{2581}мир"];
        assert_eq!(sp_detokenize(&pieces), "Привет мир");
    }

    #[test]
    fn char_detokenize_basic() {
        assert_eq!(
            char_detokenize(&["п", "р", "и", " ", "в", "е", "т"]),
            "при вет"
        );
    }
}
