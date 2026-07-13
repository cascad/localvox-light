//! The "invention" (faithfulness) bench for LLM summaries.
//!
//! The reason. Acceptance 2026-07-12: on a session where the owner was humming (the transcript
//! read "Т."), qwen3.5:4b produced a full-blown meeting summary — with topics, decisions,
//! numbers and action items for Ivan and Maria. Not one of those words was in the recording:
//! the model assembled the document out of our own glossary, which had got into the prompt.
//!
//! Hence two checks, both on the product's real prompt (the same template, the same glossary,
//! the same client), not on a toy one:
//!
//!   TRAP      — a transcript with no content (humming, fragments). The right answer is a
//!               refusal. Any summary here = a failure, no matter how beautiful the wording.
//!   INVENTION — a real transcript. We count the "checkable" tokens of the summary (numbers,
//!               capitalized names, Latin script) and see how many of them are ABSENT from the
//!               transcript. That is exactly what was invented: a name that was never spoken,
//!               or a deadline nobody named.
//!
//! The metric is crude (homonymy, inflection), so we look not at the absolute value but at a
//! comparison of models on the same input — that is enough to pick an engine.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::{Context, Result};
use localvox_light_llm::{glossary::Glossary, templates, LlmClient, LlmProfile};

/// The trap transcript: exactly what was in that ill-fated session.
const TRAP: &str = "[0s] [Я] Т.\n[15s] [Я] .\n[47s] [Я] э-э, ла-ла-ла.\n";

pub struct Case {
    pub name: String,
    pub transcript: String,
    /// A trap: there is no content, the model MUST refuse.
    pub is_trap: bool,
}

pub struct Verdict {
    pub case: String,
    pub is_trap: bool,
    /// It refused to make things up (for the trap — the only right outcome).
    pub refused: bool,
    pub invented: Vec<String>,
    pub checkable: usize,
    pub chars: usize,
    pub wall_sec: f64,
}

impl Verdict {
    /// The share of invented tokens among the checkable ones, %.
    pub fn invented_pct(&self) -> f64 {
        if self.checkable == 0 {
            return 0.0;
        }
        100.0 * self.invented.len() as f64 / self.checkable as f64
    }

    /// A FAILURE on the trap: the model brought facts out of nowhere. Exactly that, and not the
    /// shape of the answer: "## Решения\n(нет материала)" is honest, if ugly;
    /// "## Поручения\nИван → до пятницы" is a catastrophe.
    pub fn fabricated(&self) -> bool {
        self.is_trap && !self.invented.is_empty()
    }

    /// A FAILURE on a real recording: it refused to process something that does contain speech.
    /// The same kind of defect as invention: a secretary who says nothing about a conversation
    /// that did happen is useless (this is how granite4.1:8b behaves — acceptance 12.07.2026).
    pub fn over_refused(&self) -> bool {
        !self.is_trap && self.refused
    }

    pub fn invented_numbers(&self) -> Vec<String> {
        invented_numbers(&self.invented)
    }
}

/// Tokens whose presence in the transcript can be checked: numbers, capitalized words (names,
/// titles) and Latin script. Ordinary Russian words are not taken — their coincidence says
/// nothing.
fn checkable_tokens(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    // The hyphen is a word boundary: "React-компонент" is "React" + "компонент", otherwise the
    // name will not match the transcript, where it stands on its own.
    for w in text.split(|c: char| !c.is_alphanumeric()) {
        if w.chars().count() < 2 {
            continue;
        }
        let first = w.chars().next().unwrap_or(' ');
        let is_number = w.chars().all(|c| c.is_ascii_digit());
        let is_latin = w.chars().any(|c| c.is_ascii_alphabetic());
        if is_number || is_latin || first.is_uppercase() {
            out.insert(w.to_lowercase());
        }
    }
    out
}

/// The word's stem — the first 5 letters. A crude stemmer, but it solves exactly the problem the
/// bench lied to us over twice: "Решений" from the answer and "Решения" from our own template are
/// one and the same word, not an invented name. Without accounting for morphology the metric
/// brands the model for inflection.
fn stem(w: &str) -> String {
    w.chars().take(5).collect()
}

/// Tokens of the answer that are absent from the input (up to the word's stem).
fn invented_against(source: &BTreeSet<String>, produced: &BTreeSet<String>) -> Vec<String> {
    let stems: BTreeSet<String> = source.iter().map(|w| stem(w)).collect();
    produced
        .iter()
        .filter(|w| !source.contains(*w) && !stems.contains(&stem(w)))
        .cloned()
        .collect()
}

/// Numbers that were not in the input. The hardest evidence of invention: a name the model could
/// have REPAIRED (the transcript's "XI от И" → "XAI" is a benefit, not a lie), whereas a deadline
/// "до пятницы" and "150 задач" have nowhere to come from.
fn invented_numbers(invented: &[String]) -> Vec<String> {
    invented
        .iter()
        .filter(|w| w.chars().all(|c| c.is_ascii_digit()))
        .cloned()
        .collect()
}

/// A refusal to make things up. We also count as a refusal a "summary" in which UNDER EVERY
/// heading it says "не распознано": granite4.1 answers exactly like that — the shape is ugly, but
/// no content was invented, and punishing it for the shape would be unfair. So we look not at the
/// markup but at whether anything is left after stripping headings, list markers and refusal
/// phrases.
fn is_refusal(answer: &str) -> bool {
    // Models word "empty" however they please — from "(нет материала)" to "Нет ключевых выводов,
    // так как в материале нет законченных фраз".
    const NO_CONTENT: [&str; 12] = [
        "не распознано",
        "не распознана",
        "не распознаны",
        "содержательной речи",
        "не удалось распознать",
        "нет материала",
        "не определено",
        "не зафиксирован",
        "нет ключевых",
        "нет чисел",
        "нет поручений",
        "нет решений",
    ];
    let mut leftover = String::new();
    for line in answer.lines() {
        let l = line
            .trim()
            .trim_start_matches(['#', '*', '-', '•', ' '])
            .trim();
        if l.is_empty() {
            continue;
        }
        let low = l.to_lowercase();
        // a refusal line and a heading line from our own template carry no content
        if NO_CONTENT.iter().any(|n| low.contains(n)) {
            continue;
        }
        if line.trim_start().starts_with('#') {
            continue;
        }
        leftover.push_str(l);
    }
    // only letters from headings/refusals are left — which means nothing was made up
    leftover.chars().filter(|c| c.is_alphanumeric()).count() < 20
}

pub struct BenchParams<'a> {
    pub base_url: &'a str,
    pub glossary_dir: &'a Path,
    pub templates_dir: Option<&'a Path>,
    pub template: &'a str,
    pub timeout_sec: u64,
    /// Where to dump the raw model answers: numbers without the text are deceptive — going
    /// through the answers by eye is mandatory.
    pub dump_dir: Option<&'a Path>,
}

/// Run one model over a set of transcripts. The prompt is assembled exactly as in production
/// (`pipeline::process_session`), but WITHOUT the "too little speech" threshold: the threshold is
/// our defence, and here we check what the model itself does once the defence is removed.
pub fn run_model(model: &str, cases: &[Case], p: &BenchParams) -> Result<Vec<Verdict>> {
    let client = LlmClient::new(LlmProfile {
        base_url: p.base_url.to_string(),
        model: model.to_string(),
        api_key: None,
        timeout_sec: p.timeout_sec,
        ..Default::default()
    });
    let glossary = Glossary::load_dir(p.glossary_dir)?;
    let template = templates::load(p.template, p.templates_dir)
        .with_context(|| format!("template {}", p.template))?;

    let mut out = Vec::new();
    for c in cases {
        let (text, _) = glossary.apply(&c.transcript);
        let block = glossary.prompt_block(&text);
        let prompt = templates::render(&template, &text, &block);

        let t0 = std::time::Instant::now();
        let answer = client
            .chat(&[localvox_light_llm::user(prompt.clone())])
            .with_context(|| format!("{model} on case {}", c.name))?;
        let wall_sec = t0.elapsed().as_secs_f64();

        if let Some(dir) = p.dump_dir {
            std::fs::create_dir_all(dir).ok();
            let safe = |s: &str| s.replace([':', '/', '\\', ' '], "_");
            let path = dir.join(format!("{}__{}.md", safe(model), safe(&c.name)));
            std::fs::write(&path, &answer).ok();
        }

        // The comparison base is the WHOLE prompt, not just the transcript: the words from our
        // own template ("Решения", "Поручения", the section names) were not invented by the
        // model, we handed them to it. Otherwise the bench penalizes headings and any conclusions
        // drown in the noise.
        let source = checkable_tokens(&prompt);
        let produced = checkable_tokens(&answer);
        let invented = invented_against(&source, &produced);

        out.push(Verdict {
            case: c.name.clone(),
            is_trap: c.is_trap,
            refused: is_refusal(&answer),
            checkable: produced.len(),
            invented,
            chars: answer.chars().count(),
            wall_sec,
        });
    }
    Ok(out)
}

/// The trap plus the transcripts of the given sessions (the best version).
pub fn build_cases(work_dir: &Path, sessions: &[String]) -> Result<Vec<Case>> {
    let mut cases = vec![Case {
        name: "trap: humming".into(),
        transcript: TRAP.into(),
        is_trap: true,
    }];
    for name in sessions {
        let dir = work_dir.join("sessions").join(name);
        let text = localvox_light_llm::pipeline::render_transcript(&dir)
            .with_context(|| format!("transcript of session {name}"))?;
        cases.push(Case {
            name: name.clone(),
            transcript: text,
            is_trap: false,
        });
    }
    Ok(cases)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkable_takes_names_numbers_latin_not_plain_russian() {
        let t = checkable_tokens("Иван обновит React-компонент до 15 числа, обсудили сроки");
        assert!(t.contains("иван"));
        assert!(
            t.contains("react"),
            "the hyphen must be a word boundary: {t:?}"
        );
        assert!(t.contains("15"));
        assert!(
            !t.contains("обсудили"),
            "an ordinary word is not checked: {t:?}"
        );
    }

    /// granite4.1 answers "Решений не распознано." — the word is taken from a HEADING of our own
    /// template, just in a different case. A metric that brands the model for inflection is
    /// useless: it lied to me twice at the acceptance.
    #[test]
    fn declension_of_our_own_template_words_is_not_invention() {
        let prompt = checkable_tokens("## Решения\n## Поручения\n## Числа и факты");
        let answer = checkable_tokens("Решений не распознано. Поручений нет.");
        assert!(
            invented_against(&prompt, &answer).is_empty(),
            "an inflected heading was taken for an invention: {:?}",
            invented_against(&prompt, &answer)
        );
    }

    #[test]
    fn a_real_name_is_still_invention() {
        let prompt = checkable_tokens("## Поручения\nРасшифровка: ла-ла-ла.");
        let answer = checkable_tokens("Иван обновит компонент до 15 числа.");
        let inv = invented_against(&prompt, &answer);
        assert!(inv.contains(&"иван".to_string()), "{inv:?}");
        assert!(inv.contains(&"15".to_string()), "{inv:?}");
    }

    #[test]
    fn invented_numbers_are_separated_from_repaired_names() {
        // "XAI" instead of the transcript's "XI от И" is a repair, not a lie;
        // "150" and "пятница" have nowhere to come from.
        let invented = vec!["xai".to_string(), "150".to_string(), "иван".to_string()];
        assert_eq!(invented_numbers(&invented), vec!["150".to_string()]);
    }

    #[test]
    fn the_real_hallucination_is_caught() {
        // a fragment of the real invented summary (acceptance 12.07.2026)
        // against the real transcript of that session — the humming
        let source = checkable_tokens(TRAP);
        let fake = checkable_tokens(
            "## Поручения\n* **Иван** → обновить React-компонент → до пятницы.\n\
             * **Мария** → перенести логику в API → до среды.\n\
             Задач в бэклоге больше 150.",
        );
        let invented = invented_against(&source, &fake);
        assert!(invented.len() >= 4, "the invention was not caught: {invented:?}");
        assert!(invented.contains(&"иван".to_string()));
        assert!(invented_numbers(&invented).contains(&"150".to_string()));
    }

    #[test]
    fn refusal_is_recognized_and_protocol_is_not() {
        assert!(is_refusal("Содержательной речи в записи не распознано."));
        assert!(is_refusal(""));
        // granite4.1 refuses "in the shape of a summary" — that is still a refusal:
        // no content was invented in any section
        assert!(is_refusal(
            "## Основные темы\nНе распознано содержательной речи.\n\
             ## Решения\nНе распознано содержательной речи.\n\
             ## Поручения\nНе распознано содержательной речи."
        ));
        assert!(!is_refusal(
            "## Основные темы\n* Обсуждение планов на неделю и сроков релиза.\n\
             ## Решения\n* Перенести выкладку на среду."
        ));
        // a "refusal" with a real summary stitched onto it is not a refusal
        assert!(!is_refusal(
            "Содержательной речи мало, но вот протокол:\n\
             ## Решения\n* Иван обновит компонент до пятницы, Мария перенесёт логику."
        ));
    }
}
