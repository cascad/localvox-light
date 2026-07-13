//! A question to the archive (F4, the final stage): "what did we decide about the
//! migration?".
//!
//! **This is not a chat with a model but an answer BASED ON THE FOUND FRAGMENTS.**
//! The difference is fundamental. A model asked "what did we decide about the
//! migration" will always answer: it knows what happens at meetings like that, and it
//! will compose a plausible answer out of thin air. The person will take it for fact
//! and build a decision on it — and that is the worst thing our product can do.
//! Therefore:
//!
//! 1. first a SEARCH over the archive (hybrid: words + meaning) — and if it found
//!    nothing, the model is NOT CALLED AT ALL. "В записях этого нет" ("it is not in
//!    the recordings") is a full-fledged answer, and the code must produce it, not the
//!    prompt: a prompt is a request, not a mechanism;
//! 2. the fragments are numbered, and the model MUST cite the numbers. A statement
//!    without a citation is a guess;
//! 3. the answer is checked by the same machinery as the summary ([`grounding`]):
//!    names and numbers that were not in the fragments are named out loud.
//!
//! **Why fragments and not search snippets.** A snippet is a highlighted line of 200
//! characters; you cannot answer a question from it, you can only guess. That is why
//! every hit is expanded into a REAL piece of the transcript — the neighbouring lines
//! around the found one, with the speaker and the timecode.
//!
//! **WHAT THE CHECK DOES NOT CATCH — and this must be known.** It is deterministic and
//! holds on to what "cannot be fixed up": numbers, names, Latin script. A measurement
//! on the live archive (13.07.2026): asked about a recipe, the model answered
//! correctly from the recordings — and added «хабанеро» (habanero), which was NOT in
//! the recordings. The word is a common noun, Russian, lowercase — it is neither a
//! number nor a name, and the check let it through.
//!
//! It MUST NOT be tightened to "any word that is not in the source": the model
//! legitimately paraphrases («обсуждали», «описывается»), and such a check would shout
//! at every honest answer. A warning that always shouts stops being read — we have
//! already paid for that lesson (false alarms on roles in NER).
//!
//! That is why the mechanism here is different, and it lives in the UI: under the
//! answer there are ALWAYS the sources it cited — with the session and the timecode.
//! An answer that has nothing to open and listen to must not inspire trust, and the
//! product says so directly.

use anyhow::{Context, Result};
use serde::Serialize;

use localvox_light_core::versions::{read_transcript_lines, VersionStore};
use localvox_light_llm::{grounding, templates, LlmClient};

use crate::archive::Archive;

/// How many search records we take into work. More is not better: the model drowns in
/// context, while a question is usually about one or two meetings.
const HITS: usize = 8;

/// How many lines around the found one we take into a fragment. The found line is a
/// 15-second window; the neighbours on both sides give ~45 seconds of conversation.
const NEIGHBOURS: usize = 1;

/// The fragment ceiling in characters. The model's context is not made of rubber, and
/// a fragment cut off mid-word is worse than a missing one: it looks like a fact.
const BUDGET_CHARS: usize = 12_000;

/// A piece of the recording the model can cite. The number is what it writes in square
/// brackets; by the session and the timecode a person will open and listen to it.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Fragment {
    pub n: usize,
    pub session: String,
    /// `transcript` | `summary` | `processed` — where the piece was taken from.
    pub kind: String,
    pub start_sec: Option<f64>,
    pub text: String,
}

#[derive(Debug, Serialize)]
pub struct Answer {
    pub text: String,
    /// The fragments the model CITED. Not all of the found ones — only those it used:
    /// the rest have nothing to do with the answer.
    pub sources: Vec<Fragment>,
    /// Names and numbers from the answer that were not in the fragments. Empty — the
    /// answer stands entirely on the recordings.
    pub ungrounded: Vec<String>,
    /// The answer is confirmed by the recordings: there are citations and there are no
    /// inventions. A lie here would be worse than having no check at all — the person
    /// would stop double-checking.
    pub verified: bool,
}

/// Ask the archive.
pub fn ask(archive: &Archive, question: &str, llm: &LlmClient) -> Result<Answer> {
    let question = question.trim();
    anyhow::ensure!(!question.is_empty(), "пустой вопрос");

    let hits = archive.search_mode(question, HITS, "hybrid")?;
    let fragments = expand(archive, &hits);

    // Nothing was found — we do NOT call the model AT ALL. The prompt "do not invent"
    // is a request; a refusal in the code is a mechanism.
    if fragments.is_empty() {
        return Ok(Answer {
            text: NOTHING_FOUND.into(),
            sources: Vec::new(),
            ungrounded: Vec::new(),
            verified: true,
        });
    }

    let lang = localvox_light_core::lang::detect(question).unwrap_or_else(|| "ru".into());
    let dir = std::env::var_os("LOCALVOX_LLM_TEMPLATES_DIR").map(std::path::PathBuf::from);
    let (_, template) = templates::for_lang("chat", &lang, dir.as_deref())?;
    let prompt = templates::fill(
        &template,
        &[("question", question), ("fragments", &render(&fragments))],
    );

    let mut text = llm
        .chat(&[localvox_light_llm::user(prompt.clone())])
        .context("вопрос к архиву")?
        .trim()
        .to_string();

    // A REFUSAL IS NOT AN ANSWER WHEN THE WORDS OF THE QUESTION ARE RIGHT THERE IN THE
    // FRAGMENTS.
    //
    // Measured on the live archive (13.07.2026): the question «что было про торт?», the
    // fragments literally talk about a cake — and the model answered «В записях этого нет».
    // On the very same prompt, the next call answered correctly. It was tossing a coin, and
    // rule 4 of the template («a refusal is a full-fledged answer») made the refusal too
    // attractive an escape hatch — exactly the same trap as in the LLM cleanup re-ask.
    //
    // The temperature is now 0, but a mechanism is more reliable than a prompt: we KNOW that
    // the lexical search found the words of the question inside these fragments. So a refusal
    // is the model's mistake, not an empty archive, and we do not take its word for it.
    if is_refusal(&text) && !lexically_matched(&fragments, &hits) {
        // The search matched only by meaning — a refusal is plausible, leave it.
    } else if is_refusal(&text) {
        tracing::warn!("the model refused although the fragments contain the words of the question — asking again");
        let insist = format!(
            "{prompt}

ВНИМАНИЕ: фрагменты выше НАЙДЕНЫ ПО ЭТОМУ ЖЕ ВОПРОСУ и содержат его              слова. Ответ в них ЕСТЬ. Изложи то, что в них сказано, со ссылками на номера.              Отказ здесь — ошибка."
        );
        let second = llm
            .chat(&[localvox_light_llm::user(insist)])
            .context("вопрос к архиву (повтор)")?
            .trim()
            .to_string();
        if !is_refusal(&second) {
            text = second;
        }
    }

    let cited = cited(&text, fragments.len());
    let mut sources: Vec<Fragment> = fragments
        .iter()
        .filter(|f| cited.contains(&f.n))
        .cloned()
        .collect();

    // If the model still insists on refusing, we show WHAT WAS FOUND anyway. A refusal with
    // no sources is useless: the human cannot check it and cannot even see that the search
    // did its part. Let him look at the fragments and judge for himself — he can, the model
    // could not.
    if sources.is_empty() && is_refusal(&text) && lexically_matched(&fragments, &hits) {
        text = format!(
            "{text}

(Но поиск нашёл эти места по словам вашего вопроса — посмотрите сами.)"
        );
        sources = fragments.clone();
    }

    // We check the answer AGAINST THE VERY SAME fragments that were given to it. The
    // numbers are always checked, the names — if NER is available; the rules are the
    // same as for the summary.
    let source_text = render(&fragments);
    let ungrounded = grounding::check(&source_text, &text);

    Ok(Answer {
        // An answer without a single citation is not an answer from the recordings but
        // an essay inspired by them. Except for an honest "nothing was found": there is
        // nothing to cite there.
        verified: ungrounded.is_empty() && (!sources.is_empty() || is_refusal(&text)),
        text,
        sources,
        ungrounded: ungrounded
            .numbers
            .iter()
            .chain(&ungrounded.names)
            .cloned()
            .collect(),
    })
}

const NOTHING_FOUND: &str = "В записях этого нет: поиск не нашёл ни одного подходящего фрагмента.";

/// Did the LEXICAL search find the words of the question inside these fragments.
///
/// If it did, a refusal by the model is its own mistake and not an empty archive: the words
/// are right there, in front of it. If the fragments arrived only through the semantic
/// channel («по смыслу»), a refusal is plausible — there may indeed be nothing there.
fn lexically_matched(fragments: &[Fragment], hits: &[crate::archive::Hit]) -> bool {
    hits.iter().any(|h| {
        !h.matched.is_empty() && fragments.iter().any(|f| f.session == h.session)
    })
}

/// The model honestly said "not found". Then there must be no citations either.
fn is_refusal(text: &str) -> bool {
    let t = text.to_lowercase();
    ["в записях", "не нашёл", "нет ответа", "not in the record", "no answer"]
        .iter()
        .any(|m| t.contains(m))
        && t.len() < 300
}

/// Search hits → pieces of the real transcript.
///
/// The transcript of each session is read ONCE, even if there are several hits from
/// it: otherwise a dozen hits from one meeting means a dozen reads of one file.
fn expand(archive: &Archive, hits: &[crate::archive::Hit]) -> Vec<Fragment> {
    let mut out: Vec<Fragment> = Vec::new();
    let mut budget = BUDGET_CHARS;
    let mut lines_cache: std::collections::HashMap<String, Vec<Line>> = Default::default();

    for h in hits {
        let text = match h.start_sec {
            // Transcript: we expand it into a piece of the conversation around the
            // found line.
            Some(at) => {
                let lines = lines_cache
                    .entry(h.session.clone())
                    .or_insert_with(|| transcript_lines(archive, &h.session));
                context_at(lines, at, NEIGHBOURS)
            }
            // The summary and the cleaned-up text: they have no timecodes — we take
            // them as they are.
            None => h.snippet.clone(),
        };
        let text = text.trim().to_string();
        if text.is_empty() {
            continue;
        }
        // A piece already taken is not taken a second time: two hits from the same
        // minute of the conversation give one and the same context.
        if out.iter().any(|f| f.session == h.session && f.text == text) {
            continue;
        }
        if text.chars().count() > budget {
            break;
        }
        budget -= text.chars().count();
        out.push(Fragment {
            n: out.len() + 1,
            session: h.session.clone(),
            kind: h.kind.clone(),
            start_sec: h.start_sec,
            text,
        });
    }
    out
}

struct Line {
    start_sec: f64,
    who: String,
    text: String,
}

fn transcript_lines(archive: &Archive, session: &str) -> Vec<Line> {
    let Ok(dir) = archive.session_dir(session) else {
        return Vec::new();
    };
    let Ok(store) = VersionStore::open(&dir) else {
        return Vec::new();
    };
    let Some(best) = store.best() else {
        return Vec::new();
    };
    let path = dir.join("transcripts").join(&best.file);
    read_transcript_lines(&path)
        .unwrap_or_default()
        .into_iter()
        .map(|l| Line {
            start_sec: l.start_sec,
            who: match l.speaker {
                Some(name) => name,
                None if l.source_id == 0 => "Я".into(),
                None => "Собеседники".into(),
            },
            text: l.text,
        })
        .collect()
}

/// A piece of the conversation around the moment `at`: the found line and its
/// neighbours.
fn context_at(lines: &[Line], at: f64, neighbours: usize) -> String {
    let Some(i) = lines
        .iter()
        .position(|l| (l.start_sec - at).abs() < 0.5)
        .or_else(|| lines.iter().rposition(|l| l.start_sec <= at))
    else {
        return String::new();
    };
    let from = i.saturating_sub(neighbours);
    let to = (i + neighbours + 1).min(lines.len());
    lines[from..to]
        .iter()
        .map(|l| format!("{}: {}", l.who, l.text))
        .collect::<Vec<_>>()
        .join("\n")
}

/// The fragments exactly as the model sees them. The same text goes to the grounding
/// check as well — the answer must be verified against precisely what the model was
/// shown.
fn render(fragments: &[Fragment]) -> String {
    fragments
        .iter()
        .map(|f| {
            let when = match f.start_sec {
                Some(s) => format!(", {:02}:{:02}", (s as u64) / 60, (s as u64) % 60),
                None => String::new(),
            };
            format!("[{}] {}{}\n{}", f.n, f.session, when, f.text)
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

/// The fragment numbers the answer cited: `[2]`, `[1, 3]`, `[4][5]`.
///
/// A number outside the list is not a citation but an invention (the model cited a
/// fragment it was never given), and it does not make it into the sources.
fn cited(answer: &str, total: usize) -> std::collections::BTreeSet<usize> {
    let mut out = std::collections::BTreeSet::new();
    let mut rest = answer;
    while let Some(open) = rest.find('[') {
        let tail = &rest[open + 1..];
        let Some(close) = tail.find(']') else { break };
        for part in tail[..close].split(&[',', ' '][..]) {
            if let Ok(n) = part.trim().parse::<usize>() {
                if n >= 1 && n <= total {
                    out.insert(n);
                }
            }
        }
        rest = &tail[close + 1..];
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(start_sec: f64, who: &str, text: &str) -> Line {
        Line {
            start_sec,
            who: who.into(),
            text: text.into(),
        }
    }

    /// Showing the found line is not enough: you cannot answer a question from a
    /// 200-character line, you can only guess. We take the conversation AROUND it.
    #[test]
    fn a_hit_is_expanded_into_the_conversation_around_it() {
        let lines = vec![
            line(0.0, "Я", "начнём"),
            line(15.0, "Иван", "миграцию беру я"),
            line(30.0, "Я", "хорошо, до пятницы"),
            line(45.0, "Иван", "успею"),
        ];
        let ctx = context_at(&lines, 15.0, 1);
        assert!(ctx.contains("начнём") && ctx.contains("миграцию") && ctx.contains("пятницы"));
        assert!(!ctx.contains("успею"), "took too much");
    }

    /// The hit's timecode may not match a line byte-for-byte (a summary, another
    /// version) — we take the nearest line before it, not emptiness.
    #[test]
    fn a_timecode_between_lines_still_finds_the_conversation() {
        let lines = vec![line(0.0, "Я", "раз"), line(30.0, "Я", "два")];
        assert!(context_at(&lines, 22.0, 0).contains("раз"));
        assert!(context_at(&[], 5.0, 1).is_empty());
    }

    /// Citations are what holds the answer to the recordings. We parse every form, and
    /// a number we did NOT GIVE the model is not counted as a citation: that is an
    /// invention.
    #[test]
    fn citations_are_parsed_and_invented_ones_are_rejected() {
        let got = cited("Иван взял миграцию [2], срок — пятница [1, 3]. Ещё [9] и [abc].", 3);
        assert_eq!(got.into_iter().collect::<Vec<_>>(), vec![1, 2, 3]);
        assert!(cited("без единой ссылки", 3).is_empty());
    }

    /// "В записях этого нет" ("it is not in the recordings") is a full-fledged answer,
    /// not a defeat: there is nothing to cite there, and citations MUST NOT be demanded
    /// of it.
    #[test]
    fn a_refusal_is_a_valid_answer_without_citations() {
        assert!(is_refusal("В записях этого нет."));
        assert!(is_refusal("That is not in the recordings."));
        assert!(
            !is_refusal("Иван взял миграцию [2]"),
            "an ordinary answer was taken for a refusal"
        );
    }

    /// The fragments the model sees are exactly the ones its answer is later checked
    /// against. Should they diverge, the check would catch inventions where there are
    /// none.
    #[test]
    fn the_model_sees_numbered_fragments_with_session_and_time() {
        let f = vec![Fragment {
            n: 1,
            session: "20260713_011211".into(),
            kind: "transcript".into(),
            start_sec: Some(125.0),
            text: "Иван: миграцию беру я".into(),
        }];
        let out = render(&f);
        assert!(out.starts_with("[1] 20260713_011211, 02:05"));
        assert!(out.contains("миграцию беру я"));
    }
}
