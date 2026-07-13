//! LLM routing of notes into sections (F5): «where do I file this?».
//!
//! Takes the text of a note/fragment and the list of available sections (name + hint),
//! and asks the model to suggest 1–3 fitting sections with a short reason. The crate
//! knows nothing about slots/integrations (P5) — only names and hints come in; the
//! «only existing things are suggested» validation lives here too.

use std::collections::HashSet;

use anyhow::Result;

use crate::{user, LlmClient};

/// The beginning of a note is enough to decide «where to file it»; we do not blow up
/// the prompt with a long text and do not load the LLM with a gigantic input (counting
/// chars — the UTF-8 boundary stays intact).
const MAX_TEXT_CHARS: usize = 4000;

/// A candidate section: name (as in the slot registry) + a free-form hint «what goes here».
pub struct RouteSlot {
    pub name: String,
    pub hint: String,
}

/// A routing suggestion: the section name (matches a real slot) + the reason.
#[derive(Debug, PartialEq)]
pub struct RouteSuggestion {
    pub slot: String,
    pub reason: String,
}

/// Suggest which sections the text should be filed into. Returns only existing names
/// (the model's hallucinations are dropped), the best fit first.
pub fn suggest_routing(
    text: &str,
    slots: &[RouteSlot],
    client: &LlmClient,
) -> Result<Vec<RouteSuggestion>> {
    if slots.is_empty() || text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut prompt = String::from("Есть разделы для заметок:\n");
    for s in slots {
        if s.hint.trim().is_empty() {
            prompt.push_str(&format!("- {}\n", s.name));
        } else {
            prompt.push_str(&format!("- {} — {}\n", s.name, s.hint.trim()));
        }
    }
    prompt.push_str("\nТекст:\n");
    let capped: String = text.trim().chars().take(MAX_TEXT_CHARS).collect();
    prompt.push_str(&capped);
    prompt.push_str(
        "\n\nВыбери 1–3 наиболее подходящих раздела ИЗ СПИСКА выше (используй \
         точные имена). Ответ — по одной строке на раздел в формате:\n\
         имя | краткая причина\n\
         Только существующие имена, самый подходящий первым, без прочего текста.",
    );

    let answer = client.chat(&[user(prompt)])?;
    Ok(parse_routing(&answer, slots))
}

/// Parsing the answer. Tolerant to the format (an LLM rarely keeps to a strict
/// `name | reason`): numbering/bullets/quotes/markdown emphasis are stripped from every
/// line, then the line is matched against a real slot name AS A PREFIX (the boundary is
/// the end of the line or a non-alphanumeric); the longest name wins. This covers
/// «- **идеи** — …», «1. идеи: …», «идеи | …» and a bare «идеи»; hallucinations
/// (non-existent names) and any other text are dropped; deduplicated by slot.
fn parse_routing(answer: &str, slots: &[RouteSlot]) -> Vec<RouteSuggestion> {
    let mut out: Vec<RouteSuggestion> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for raw in answer.lines() {
        let cleaned = raw
            .trim()
            .trim_start_matches(|c: char| {
                c.is_ascii_digit()
                    || matches!(
                        c,
                        '.' | ')' | '-' | '*' | '•' | '·' | '|' | '#' | '>' | ' ' | '\t'
                    )
            })
            .replace(['*', '_', '`'], "");
        let line = cleaned.trim();
        if line.is_empty() {
            continue;
        }
        let lower = line.to_lowercase();
        // a prefix name with a boundary; non-empty names; the longest name is the truer one
        let matched = slots
            .iter()
            .filter(|s| {
                let n = s.name.trim().to_lowercase();
                !n.is_empty()
                    && lower
                        .strip_prefix(&n)
                        .is_some_and(|r| r.chars().next().is_none_or(|c| !c.is_alphanumeric()))
            })
            .max_by_key(|s| s.name.trim().chars().count());
        let Some(slot) = matched else {
            continue;
        };
        if seen.insert(slot.name.clone()) {
            // the reason is the rest of the line after the name, without leading separators
            let reason: String = line
                .chars()
                .skip(slot.name.trim().chars().count())
                .collect();
            let reason = reason
                .trim_start_matches(['|', ':', '—', '–', '-', '.', ' ', '\t'])
                .trim()
                .to_string();
            out.push(RouteSuggestion {
                slot: slot.name.clone(),
                reason,
            });
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slots() -> Vec<RouteSlot> {
        vec![
            RouteSlot {
                name: "идеи".into(),
                hint: "продуктовые мысли".into(),
            },
            RouteSlot {
                name: "задачи".into(),
                hint: "todo".into(),
            },
        ]
    }

    #[test]
    fn parses_and_validates_names() {
        let ans = "Вот варианты:\n- идеи | это продуктовая мысль\nзадачи | есть действие\nнесуществующий | мимо";
        let got = parse_routing(ans, &slots());
        assert_eq!(got.len(), 2);
        assert_eq!(
            got[0],
            RouteSuggestion {
                slot: "идеи".into(),
                reason: "это продуктовая мысль".into()
            }
        );
        assert_eq!(got[1].slot, "задачи");
    }

    #[test]
    fn tolerates_natural_llm_formats() {
        // numbering + markdown bold + a dash (no '|') — a frequent format from models
        let ans = "1. **идеи** — продуктовая мысль\n2. `задачи`: есть действие";
        let got = parse_routing(ans, &slots());
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].slot, "идеи");
        assert_eq!(got[0].reason, "продуктовая мысль");
        assert_eq!(got[1].slot, "задачи");
        assert_eq!(got[1].reason, "есть действие");
    }

    #[test]
    fn bare_name_without_reason_matches() {
        let got = parse_routing("идеи", &slots());
        assert_eq!(
            got,
            vec![RouteSuggestion {
                slot: "идеи".into(),
                reason: String::new()
            }]
        );
    }

    #[test]
    fn dedups_and_ignores_garbage_and_hallucinations() {
        let ans = "идеи | раз\nИДЕИ | два (дубль)\nпросто болтовня\nнесуществующий | мимо";
        let got = parse_routing(ans, &slots());
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].slot, "идеи");
    }

    #[test]
    fn prefix_match_respects_word_boundary() {
        // «задачник» must not match the slot «задачи» (no boundary after the name)
        let ans = "задачник разное";
        assert!(parse_routing(ans, &slots()).is_empty());
    }
}
