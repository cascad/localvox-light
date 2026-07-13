//! A glossary of IT terms (F1): a deterministic pass + hints for the LLM.
//!
//! The format is `assets/glossary/*.toml` (see `it-ru-base.toml`): the canonical form
//! + unambiguous variants (auto-replaced) + ambiguous variants (they coincide with
//! ordinary speech — we do not auto-replace them, only mention them in the prompt).
//! Replacements are logged — an audit trail of «what was replaced» (data principles).

use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Deserialize, Default)]
pub struct Glossary {
    #[serde(rename = "term", default)]
    pub terms: Vec<Term>,
}

#[derive(Deserialize)]
pub struct Term {
    pub canonical: String,
    #[serde(default)]
    pub variants: Vec<String>,
    #[serde(default)]
    pub ambiguous_variants: Vec<String>,
}

#[derive(Debug, PartialEq)]
pub struct Replacement {
    pub from: String,
    pub to: String,
    pub count: usize,
}

impl Glossary {
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("reading the glossary {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))
    }

    /// All `*.toml` files of the directory merged into one glossary; a missing directory
    /// yields an empty one.
    pub fn load_dir(dir: &Path) -> Result<Self> {
        let mut merged = Glossary::default();
        let Ok(entries) = fs::read_dir(dir) else {
            return Ok(merged);
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension().and_then(|x| x.to_str()) == Some("toml") {
                merged.terms.extend(Self::load(&p)?.terms);
            }
        }
        Ok(merged)
    }

    /// Deterministic replacement of unambiguous variants (case-insensitive, on word
    /// boundaries). Ambiguous variants are left alone.
    pub fn apply(&self, text: &str) -> (String, Vec<Replacement>) {
        let mut out = text.to_string();
        let mut log = Vec::new();
        for term in &self.terms {
            for variant in &term.variants {
                let (replaced, count) = replace_word_ci(&out, variant, &term.canonical);
                if count > 0 {
                    out = replaced;
                    log.push(Replacement {
                        from: variant.clone(),
                        to: term.canonical.clone(),
                        count,
                    });
                }
            }
        }
        (out, log)
    }

    /// The block for the prompt — ONLY the terms that actually occur in this text.
    ///
    /// The whole glossary used to go into the prompt, and that turned out to be fuel
    /// for hallucinations: on a session where the owner was simply humming (the
    /// transcript was «Т.»), qwen3.5:4b made up the summary of a meeting that never
    /// happened, assembling it verbatim from the list of terms — «диаграмма Ганта»,
    /// «capacity», «Jira», action items for Иван and Мария. For the model, a list in
    /// the context is indistinguishable from content. Filtering by occurrence removes
    /// both the fuel and the wasted tokens.
    pub fn prompt_block(&self, transcript: &str) -> String {
        let hay = transcript.to_lowercase();
        let mentioned = |t: &Term| {
            std::iter::once(&t.canonical)
                .chain(t.variants.iter())
                .chain(t.ambiguous_variants.iter())
                .any(|v| contains_word_ci(&hay, &v.to_lowercase()))
        };
        let terms: Vec<&Term> = self.terms.iter().filter(|t| mentioned(t)).collect();
        if terms.is_empty() {
            return String::new();
        }
        let mut s = String::from(
            "Справочник написаний для терминов, которые ВСТРЕТИЛИСЬ в расшифровке \
             ниже. Это словарь орфографии, а НЕ содержание разговора: не упоминай \
             термин, если его нет в расшифровке.\n",
        );
        for t in terms {
            s.push_str("- ");
            s.push_str(&t.canonical);
            if !t.ambiguous_variants.is_empty() {
                s.push_str(" (в записи может звучать как: ");
                s.push_str(&t.ambiguous_variants.join(", "));
                s.push_str(" — заменяй только если по контексту это термин)");
            }
            s.push('\n');
        }
        s
    }
}

/// Whether `needle` occurs in `hay` as a whole word (both strings are already
/// lowercased). A multi-word term («story points») is searched for as a substring with
/// word boundaries.
fn contains_word_ci(hay: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return false;
    }
    let h: Vec<char> = hay.chars().collect();
    let n: Vec<char> = needle.chars().collect();
    if n.len() > h.len() {
        return false;
    }
    let boundary = |c: char| !c.is_alphanumeric();
    for i in 0..=(h.len() - n.len()) {
        if h[i..i + n.len()] != n[..] {
            continue;
        }
        let left_ok = i == 0 || boundary(h[i - 1]);
        let right_ok = i + n.len() == h.len() || boundary(h[i + n.len()]);
        if left_ok && right_ok {
            return true;
        }
    }
    false
}

/// Case-insensitive replacement of `needle` → `to`, whole words only (boundaries are
/// non-alphanumeric characters). Returns (result, number of replacements).
fn replace_word_ci(haystack: &str, needle: &str, to: &str) -> (String, usize) {
    let lower_hay: Vec<char> = haystack.to_lowercase().chars().collect();
    let hay: Vec<char> = haystack.chars().collect();
    // to_lowercase can change the length in chars for exotic input — then we honestly
    // skip
    if lower_hay.len() != hay.len() {
        return (haystack.to_string(), 0);
    }
    let needle: Vec<char> = needle.to_lowercase().chars().collect();
    if needle.is_empty() || needle.len() > hay.len() {
        return (haystack.to_string(), 0);
    }

    let is_word = |c: char| c.is_alphanumeric();
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0usize;
    let mut count = 0usize;
    while i < hay.len() {
        let end = i + needle.len();
        let matches = end <= hay.len()
            && lower_hay[i..end] == needle[..]
            && (i == 0 || !is_word(hay[i - 1]))
            && (end == hay.len() || !is_word(hay[end]));
        if matches {
            out.push_str(to);
            i = end;
            count += 1;
        } else {
            out.push(hay[i]);
            i += 1;
        }
    }
    (out, count)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn glossary() -> Glossary {
        toml::from_str(
            r#"
[[term]]
canonical = "capacity"
variants = ["капаштет", "капасити"]
ambiguous_variants = ["как посетит"]

[[term]]
canonical = "Jira"
variants = ["джира"]
ambiguous_variants = ["жира"]
"#,
        )
        .unwrap()
    }

    #[test]
    fn replaces_unambiguous_variants_word_bounded_ci() {
        let g = glossary();
        let (out, log) = g.apply("Считаем Капаштет команды, заводим в джира задачу.");
        assert_eq!(out, "Считаем capacity команды, заводим в Jira задачу.");
        assert_eq!(log.len(), 2);
        assert_eq!(log[0].count, 1);
    }

    #[test]
    fn does_not_touch_ambiguous_or_substrings() {
        let g = glossary();
        let (out, log) = g.apply("Много жира в еде. Поджира не слово.");
        assert_eq!(out, "Много жира в еде. Поджира не слово.");
        assert!(log.is_empty());
    }

    #[test]
    fn multiword_variant_is_replaced() {
        let g: Glossary = toml::from_str(
            r#"
[[term]]
canonical = "drag-and-drop"
variants = ["дракон троп"]
"#,
        )
        .unwrap();
        let (out, _) = g.apply("Сделали дракон троп для колбасок.");
        assert_eq!(out, "Сделали drag-and-drop для колбасок.");
    }

    #[test]
    fn prompt_block_lists_canon_and_ambiguous() {
        // the term was heard in a distorted form — the canonical spelling and the hint
        // about the homonym are both needed in the prompt
        let block = glossary().prompt_block("а как посетит команды на спринт?");
        assert!(block.contains("capacity"));
        assert!(block.contains("как посетит"));
    }

    #[test]
    fn real_seed_glossary_parses() {
        // the live seed from the repository must always parse
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../assets/glossary/it-ru-base.toml");
        let g = Glossary::load(&path).unwrap();
        assert!(g.terms.len() > 20);
    }

    /// A glossary in the prompt is fuel for hallucinations: on an empty transcript the
    /// model assembled a «meeting summary» out of it (acceptance run 2026-07-12).
    /// ONLY the terms that really are in the transcript go into the prompt.
    #[test]
    fn prompt_block_lists_only_terms_present_in_the_transcript() {
        let g: Glossary = toml::from_str(
            r#"
            [[term]]
            canonical = "capacity"
            ambiguous_variants = ["как посетит"]

            [[term]]
            canonical = "диаграмма Ганта"
            variants = ["гант"]

            [[term]]
            canonical = "Jira"
            variants = ["джира"]
            "#,
        )
        .unwrap();

        // humming: not a single term was said → the block is empty
        assert_eq!(g.prompt_block("Т. э-э, ла-ла-ла."), "");

        // one term was said (in a distorted form) → only it is in the prompt
        let block = g.prompt_block("надо завести задачу в джира до пятницы");
        assert!(block.contains("Jira"), "{block}");
        assert!(
            !block.contains("Ганта"),
            "a spurious term in the prompt: {block}"
        );
        assert!(
            !block.contains("capacity"),
            "a spurious term in the prompt: {block}"
        );
    }

    #[test]
    fn term_inside_another_word_does_not_count() {
        let g: Glossary =
            toml::from_str("[[term]]\ncanonical = \"Jira\"\nvariants = [\"джира\"]\n").unwrap();
        // «джираф» is not «джира»
        assert_eq!(g.prompt_block("в зоопарке джираф"), "");
        assert!(g.prompt_block("создай тикет в Jira").contains("Jira"));
    }
}
