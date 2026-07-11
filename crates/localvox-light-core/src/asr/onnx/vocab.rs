//! Загрузка словарей формата `onnx-asr` / GigaAM v3 E2E:
//! одна строка — `<TOKEN><space><ID>`, при этом TOKEN может быть пустым
//! (тогда строка — просто `<ID>`, что соответствует blank-токену).

use std::path::Path;

use anyhow::{Context, Result};

/// Префикс-маркер начала слова у SentencePiece (U+2581, `▁`).
pub const SP_SPACE: &str = "\u{2581}";

#[derive(Clone, Debug)]
pub struct Vocab {
    /// Токены по индексам (`tokens.len() == vocab_size`). Пустая строка означает blank.
    pub tokens: Vec<String>,
    /// Индекс blank-токена.
    pub blank_id: usize,
}

impl Vocab {
    /// Загрузить словарь из текстового файла.
    ///
    /// Формат файла (как у `istupakov/gigaam-v3-onnx`):
    /// ```text
    /// 0
    /// ▁ 1
    /// . 2
    /// е 3
    /// ...
    /// ```
    /// Первая строка `<пустой токен><space><id>` (визуально просто `<id>`) — это blank.
    /// Если пустого токена нет — `blank_id` берётся как `tokens.len() - 1` (для char-вокабов,
    /// где `<blk>` идёт последним).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("чтение vocab {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("разбор vocab {}", path.display()))
    }

    /// Парсинг словаря из строки (для тестов и in-memory вокабов).
    pub fn parse(text: &str) -> Result<Self> {
        let mut entries: Vec<(String, usize)> = Vec::new();
        for (line_no, raw) in text.lines().enumerate() {
            if raw.is_empty() {
                continue;
            }
            // Разделяем по ПОСЛЕДНЕМУ пробелу: token + " " + id. Пробельный токен (`▁`) ок,
            // как и пустой токен (тогда строка — это только `<id>`).
            let (tok, id_str) = match raw.rsplit_once(' ') {
                Some((t, i)) => (t, i),
                None => ("", raw),
            };
            let id: usize = id_str.trim().parse().with_context(|| {
                format!("vocab line {}: «{}» — не удалось распарсить id", line_no + 1, raw)
            })?;
            entries.push((tok.to_string(), id));
        }

        if entries.is_empty() {
            anyhow::bail!("vocab пустой");
        }

        let max_id = entries.iter().map(|(_, i)| *i).max().unwrap_or(0);
        let mut tokens = vec![String::new(); max_id + 1];
        let mut seen = vec![false; max_id + 1];
        for (tok, id) in entries {
            if seen[id] {
                anyhow::bail!("vocab: дублирующийся id {}", id);
            }
            tokens[id] = tok;
            seen[id] = true;
        }
        if !seen.iter().all(|&s| s) {
            let gap = seen.iter().position(|&s| !s).unwrap_or(0);
            anyhow::bail!("vocab: пропущен id {} (нет такой строки)", gap);
        }

        let blank_id = tokens
            .iter()
            .position(|t| t.is_empty())
            .unwrap_or(tokens.len().saturating_sub(1));

        Ok(Self { tokens, blank_id })
    }

    /// Получить токен по индексу. `None` — если индекс за границами.
    pub fn get(&self, idx: usize) -> Option<&str> {
        self.tokens.get(idx).map(String::as_str)
    }
}

/// SentencePiece-детокенизация: склеиваем пиесы, заменяем `▁` (U+2581) на пробел,
/// убираем возможный ведущий пробел.
pub fn sp_detokenize(pieces: &[&str]) -> String {
    let joined: String = pieces.iter().copied().collect();
    let with_spaces = joined.replace(SP_SPACE, " ");
    with_spaces.trim_start().to_string()
}

/// Простая детокенизация для char-вокабов (NeMo / GigaAM v3 base): токены уже являются
/// одиночными символами или пробелом — склеиваем как есть.
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
        assert_eq!(char_detokenize(&["п", "р", "и", " ", "в", "е", "т"]), "при вет");
    }
}
