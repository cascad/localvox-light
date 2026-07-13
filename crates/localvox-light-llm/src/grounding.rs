//! Grounding check: is there anything in the model's answer that was not in the input.
//!
//! Why. Acceptance run 2026-07-12: on a session that was just humming, the LLM produced
//! a summary with action items for «Иван» and «Мария» and «150 tasks in the backlog».
//! The first patch was a threshold on the word count — do not call the model if there is
//! little speech. A bad patch: it protects against silence at the price of a short
//! thought and a five-minute call, and that is exactly what the product is for.
//!
//! It is not brevity that is dangerous, it is being UNGROUNDED. So we check not the
//! length of the input but the output: every name, number and foreign-script word in the
//! answer must lean on the transcript. The check is deterministic — it does not depend on
//! the model, the prompt, or the length of the recording.
//!
//! How it is built (every item is a lesson paid for in the WP-C16 review):
//!
//! * **We compare TRANSLITERATED keys, not raw characters.** Russian ASR writes in
//!   Cyrillic: «слак», «фигма», «гитхаб», «Антропику». The LLM legitimately canonicalizes
//!   them into Slack, Figma, GitHub, Anthropic — and that is a benefit, exactly what the
//!   model is there for. A character-wise Levenshtein between alphabets is always maximal,
//!   so the first version of the check counted NOT A SINGLE one of those repairs: on the
//!   real `transcript_uat.txt` it found 26 «inventions», all false. Through the
//!   transliteration «слак»→`slak` against `slack` — one edit.
//!
//! * **All the rules are RELATIVE TO THE SCRIPT OF THE RECORDING, not relative to the
//!   Latin script.** A token in a FOREIGN script is almost always a canonicalization of
//!   what was heard, so we check it, and we judge it generously (the budget is half the
//!   length). A capitalized token in the NATIVE script is almost always a name, and here
//!   generosity is lethal: on a 4000-word transcript, 9 of 13 invented names were
//!   «repaired» against random words («Мария» against «серия», «Антон» against «кантом»).
//!   For them the budget is one edit, and the first letter MUST match.
//!
//!   The formulation «Latin = foreign» rested solely on the recording being Russian. On an
//!   English recording it fell apart in both directions at once: an honest retelling got
//!   six accusations of inventing ordinary English words, while moving a meeting from
//!   «Tuesday» to «Thursday» passed as «similar» (generous budget, two edits).
//!
//! * **PEOPLE are tagged by an NER MODEL, not by a list of names** (if the model is
//!   present; see `core::ner`). A list is a dead end for two reasons: there are infinitely
//!   many names in a language, and it NEVER resolves homonymy, because it does not see the
//!   context. The model does see it: «Роман закроет задачу» → person 0.89, «дописать
//!   роман» → no person at all (measured). This is not «a model checking a model»: GLiNER
//!   is an encoder, it scores SPANS OF THE INPUT and is physically unable to invent a name.
//!
//!   The model judges ONLY PEOPLE, and that too is measured. A deadline and an amount are
//!   legitimately REWORDED («около двухсот тысяч» → «порядка 200 000», «next week» →
//!   «the following week»), and checking them by words declared an honest retelling an
//!   invention. Their content is a NUMBER, and numbers are deterministic. A name cannot be
//!   reworded: it was either said or invented.
//!
//!   No model — people are not checked, and we SAY SO. Silently pretending that the names
//!   have been checked is not allowed.
//!
//! * **Numbers are not repaired** and are checked in both directions: «150» in the answer
//!   must be in the recording either as digits or as words («сто пятьдесят»), and «сто
//!   пятьдесят» in the answer is expanded back into 150 and checked the same way.
//!
//! The dictionaries (abbreviations, numerals, colleagues' names) are not code but data:
//! they live in [`localvox_light_core::lexicon`], are baked in by default, are **extended**
//! by user TOML files from `LOCALVOX_LEXICON_DIR` and can be **replaced** wholesale
//! (`replace = ["names"]`). By now this is a HINT on top of the model, not a load-bearing
//! structure.
//!
//! Morphology and string proximity are libraries, not home-grown: `rust-stemmers`
//! (Snowball ru), `strsim`, `deunicode`.
//!
//! What the check does NOT catch and does not pretend to catch: invented statements
//! without names and numbers («the team decided to postpone the release») and deadlines
//! spelled out in words («by Friday»). That requires a judge model, not a heuristic.

use std::collections::BTreeSet;

use localvox_light_core::lexicon::{self, Lexicon};
use localvox_light_core::num2words;

/// What in the answer does not lean on the input.
#[derive(Debug, Default, PartialEq)]
pub struct Ungrounded {
    /// Numbers that are in the recording neither as digits nor as words. The hardest
    /// proof of an invention: deadlines and quantities do not get «repaired».
    pub numbers: Vec<String>,
    /// Names/titles that do not resemble anything in the recording.
    pub names: Vec<String>,
}

impl Ungrounded {
    pub fn is_empty(&self) -> bool {
        self.numbers.is_empty() && self.names.is_empty()
    }

    /// How much is unconfirmed in total — so that out of two answers from the model we
    /// can pick the one with fewer inventions (losing both is not allowed: they contain
    /// a correct part).
    pub fn count(&self) -> usize {
        self.numbers.len() + self.names.len()
    }

    /// Human-readable — for the log and for the follow-up request to the model.
    pub fn describe(&self) -> String {
        let mut parts = Vec::new();
        if !self.numbers.is_empty() {
            parts.push(format!("числа: {}", self.numbers.join(", ")));
        }
        if !self.names.is_empty() {
            parts.push(format!("имена/названия: {}", self.names.join(", ")));
        }
        parts.join("; ")
    }
}

/// Significant words: the things that are worth checking at all. An ordinary Russian word
/// («обсудили») tells us nothing — it will always match; a name, a number and a
/// foreign-script word do tell us something.
/// Strips the markdown structure at the start of a line: heading hashes, list bullets,
/// quotes and NUMBERING («### 3.6. Изменения…»).
///
/// Without this, a section number ends up among the «invented numbers» (on a real summary
/// from the repository that is exactly what happened: 6, 7, 9, 11–14 are subsection
/// numbers), and the first word of a heading ends up among the «names». What must be
/// checked is the text, not the markup.
fn strip_structure(line: &str) -> &str {
    let mut s = line.trim_start();
    s = s.trim_start_matches(['#', '>', '*', '-', '•', ' ']);
    // «3.6.» / «12)» at the start is numbering, not a fact
    let digits_end = s
        .find(|c: char| !c.is_ascii_digit() && c != '.' && c != ')')
        .unwrap_or(s.len());
    let head = &s[..digits_end];
    if !head.is_empty() && head.chars().any(|c| c.is_ascii_digit()) && head.ends_with(['.', ')']) {
        s = s[digits_end..].trim_start();
    }
    s
}

/// The script OF THE RECORDING. All the rules below are formulated relative to it, not
/// relative to the Latin script.
///
/// «A token in a foreign script is suspicious» and «a foreign script needs a generous
/// budget» — both rules hold exactly as long as the foreign script is Latin, that is, as
/// long as the recording is Cyrillic. For an ENGLISH recording the Latin script is native,
/// and checking every Latin word is not allowed: any honest retelling («rollout» →
/// «deployment») instantly becomes an «invention». Measured on an honest English summary:
/// six false accusations in a row («deployment», «documentation», «schedule»,
/// «unstable»…), on an equivalent Russian one — zero.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Script {
    Cyrillic,
    Latin,
}

/// Which script the recording is written in. A tie and an empty input mean Cyrillic: it is
/// the default language, and on a degenerate input the behaviour MUST stay as it was.
fn script_of(text: &str) -> Script {
    let (mut cyr, mut lat) = (0usize, 0usize);
    for c in text.chars().flat_map(char::to_lowercase) {
        if ('а'..='я').contains(&c) || c == 'ё' {
            cyr += 1;
        } else if c.is_ascii_alphabetic() {
            lat += 1;
        }
    }
    if lat > cyr {
        Script::Latin
    } else {
        Script::Cyrillic
    }
}

/// The word is written in a script FOREIGN to the recording. Such a word is almost always
/// a canonicalization of what was heard («слак» → `Slack`), and that is why it is both
/// checked and judged more generously. A word in the native script is just a word.
fn is_foreign(word: &str, record: Script) -> bool {
    match record {
        Script::Cyrillic => word.chars().any(|c| c.is_ascii_alphabetic()),
        Script::Latin => word
            .chars()
            .flat_map(char::to_lowercase)
            .any(|c| ('а'..='я').contains(&c) || c == 'ё'),
    }
}

fn checkable(text: &str, lex: &Lexicon, record: Script) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for line in text.lines().map(strip_structure) {
        // A capitalized Cyrillic word at the START of a sentence is usually an ordinary
        // word, not a name: «Компания работает в минус», «Решений не распознано».
        // Treating them as names is not allowed — we would reject almost any honest text
        // (the very first candidate for an «invention» was «Компания»).
        //
        // But a name from the dictionary is checked REGARDLESS of position: otherwise an
        // invention hides in a list item («* Иван → обновить…»), where a capital letter
        // means nothing. That is exactly the blind spot the review exploited.
        let mut sentence_start = true;
        for tok in line.split_inclusive(|c: char| !c.is_alphanumeric()) {
            let w: String = tok.chars().filter(|c| c.is_alphanumeric()).collect();
            let sep: String = tok.chars().filter(|c| !c.is_alphanumeric()).collect();
            let at_start = sentence_start;
            if !w.is_empty() {
                sentence_start = false;
            }
            // A phrase boundary is not only punctuation but also markdown structure:
            // a table cell («| Вероятная нормализация |»), the arrow of an action item,
            // a dash. There a capital letter is also a beginning, not a proper name:
            // on a real summary from the repository it was precisely the table cells
            // that produced «invented names» like «Вероятная», «Внешняя», «Панель».
            if sep.contains(['.', '!', '?', ':', ';', '|', '—', '–', '→', '\t']) {
                sentence_start = true;
            }
            let low = w.to_lowercase();
            // Numbers are significant ALWAYS, including single-digit ones: «5 задач» must
            // lean on a «5» or a «пять» in the recording (len<2 used to skip them).
            if !w.is_empty() && w.chars().all(|c| c.is_ascii_digit()) {
                out.insert(low);
                continue;
            }
            if w.chars().count() < 2 {
                continue;
            }
            // An abbreviation carries no facts (UI, UX, HR, UAT) — checking it means
            // breeding false alarms: there is no name, deadline or quantity in it.
            if lex.is_abbreviation(&low) {
                continue;
            }
            let first = w.chars().next().unwrap_or(' ');
            if is_foreign(&w, record)
                || lex.is_person_name(&low)
                || (first.is_uppercase() && !at_start)
            {
                out.insert(low);
            }
        }
    }
    out
}

/// All the words of the input (not only the significant ones) — we look for grounding for
/// names and terms among them.
fn all_words(text: &str) -> Vec<String> {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.chars().count() >= 2)
        .map(str::to_lowercase)
        .collect()
}

/// The comparison key: lowercase + transliteration into ASCII (`deunicode`).
/// Cyrillic and Latin land in one space — otherwise «слак» and `Slack` are a distance of
/// 5 out of 5, and a transliteration repair is never counted.
fn key(w: &str) -> String {
    deunicode::deunicode(&w.to_lowercase())
}

/// The stem of a word — a real Snowball stemmer for Russian (`rust-stemmers`), not «the
/// first N characters»: truncation glued «Компан» to «Компаньоном» and drove «Решений»
/// apart from «Решения». Latin is stemmed with the English Snowball.
fn stem(w: &str) -> String {
    use rust_stemmers::{Algorithm, Stemmer};
    let low = w.to_lowercase();
    let cyrillic = low.chars().any(|c| ('а'..='я').contains(&c) || c == 'ё');
    let algo = if cyrillic {
        Algorithm::Russian
    } else {
        Algorithm::English
    };
    let stemmed = Stemmer::create(algo).stem(&low).into_owned();
    // The key is computed AFTER stemming: the stem must also be in the common alphabet.
    key(&stemmed)
}

/// Whether a word of the answer leans on something in the recording.
///
/// The similarity budget depends on whether the script of the word is NATIVE, and this is
/// fundamental:
/// * **foreign script** (Latin in a Russian recording) — almost always a canonicalization
///   of what was heard («слак» → `Slack`, «Антропику» → `Anthropic`). Generous: half the
///   length;
/// * **native script** (a name, a title) — generosity here is lethal. On a 4000-word
///   transcript a budget of 2 edits «repaired» «Мария» against «серия» and «Антон»
///   against «кантом»: in a large corpus a neighbour within two edits will be found for
///   almost any name. Strict: one edit, and the first letter MUST match.
///
/// Handing out the generous budget to ALL Latin is not allowed. In an English recording
/// the Latin script is native, and generosity blinds the check in the opposite direction:
/// «Thursday» (8 letters, budget 4) would be grounded against «Tuesday» — two edits. The
/// model would move the meeting to another day, and we would certify it.
/// Text without markdown markup — for feeding into the NER.
///
/// The markup THROWS the model off, and this is measured (13.07.2026, corpus):
///
/// ```text
///   «## Главное\nДостоевский заклеймил Обломова»  → people: [Обломова]     ← the name is GONE
///   «Достоевский заклеймил Обломова»              → people: [Достоевский, Обломова]
/// ```
///
/// An invented name right after a heading was not tagged AT ALL — and passed as a fact.
/// That is the worst kind of breakage: not an error but a silent miss. The model is
/// trained on prose, not on markdown; feeding it hashes is our fault, not its own.
fn plain(text: &str) -> String {
    text.lines()
        // A heading is DROPPED entirely, not cleaned of its hashes. Removing the «##» is
        // not enough: what remains is «Главное Достоевский заклеймил…», and the model
        // reads it as one whole — the name disappears. A heading is structure, not speech.
        .filter(|l| !l.trim_start().starts_with('#'))
        .map(strip_structure)
        .filter(|l| !l.trim().is_empty())
        .collect::<Vec<_>>()
        .join(". ")
}

/// The digits OF THE ANSWER. We strip the markup with the same hand as everywhere else:
/// «### 3.6.» is a section number, not a fact about the recording (on a real summary the
/// subsection numbers ended up among the «invented numbers» — 6, 7, 9, 11–14).
fn answer_digits(text: &str) -> BTreeSet<u64> {
    let mut out = BTreeSet::new();
    for line in text.lines().map(strip_structure) {
        for group in digit_groups(line) {
            if let Ok(n) = group.parse::<u64>() {
                out.insert(n);
            }
        }
    }
    out
}

/// The numbers of a line, GLUING digit groups separated by a space: «200 000» is one
/// number, not 200 and 0.
///
/// This is not pedantry: GigaAM v3 writes numbers exactly like that («1 000 000»,
/// «9 472 824»), and the LLM carries the format over into the summary. Without the gluing,
/// an honest «200 000 рублей» against a spoken «двухсот тысяч» produced «invented numbers
/// 0 and 200» — and sent a correct summary into quarantine.
///
/// We glue only by the digit-group rule: the next group is EXACTLY three digits.
/// Otherwise «15 задач и 20 задач» would be glued into nonsense.
fn digit_groups(line: &str) -> Vec<String> {
    let toks: Vec<&str> = line
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .collect();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < toks.len() {
        if !toks[i].chars().all(|c| c.is_ascii_digit()) {
            i += 1;
            continue;
        }
        let mut num = toks[i].to_string();
        let mut j = i + 1;
        while j < toks.len()
            && toks[j].len() == 3
            && toks[j].chars().all(|c| c.is_ascii_digit())
            // the separator between groups is only a space, not a comma/period:
            // «15, 20» is two numbers
            && separator_is_space(line, toks[j - 1], toks[j])
        {
            num.push_str(toks[j]);
            j += 1;
        }
        out.push(num);
        i = j;
    }
    out
}

/// Exactly a space (or a non-breaking space) stands between two tokens.
fn separator_is_space(line: &str, left: &str, right: &str) -> bool {
    let Some(l) = line.find(left) else {
        return false;
    };
    let after = l + left.len();
    let Some(r) = line[after..].find(right).map(|x| x + after) else {
        return false;
    };
    let sep = &line[after..r];
    !sep.is_empty()
        && sep
            .chars()
            .all(|c| c == ' ' || c == '\u{a0}' || c == '\u{202f}')
}

/// Entities → words fit for checking. An entity can be multi-word («около двухсот тысяч
/// рублей»): every one of its words must lean on something. Abbreviations are filtered
/// out — they carry no facts.
fn entity_words(entities: &[String], lex: &Lexicon) -> Vec<String> {
    let mut out: Vec<String> = entities
        .iter()
        .flat_map(|e| {
            e.split(|c: char| !c.is_alphanumeric())
                .filter(|w| w.chars().count() >= 2)
                .map(str::to_lowercase)
                .collect::<Vec<_>>()
        })
        .filter(|w| !lex.is_abbreviation(w))
        .collect();
    out.sort();
    out.dedup();
    out
}

/// One word is the same as another in a different grammatical case: a common root plus a
/// short tail. Works on transliterated keys.
///
/// A minimum of 4 letters for the shorter one — otherwise a two-letter name would be
/// grounded by any word that starts with it.
fn case_ending(a: &str, b: &str) -> bool {
    const MAX_TAIL: usize = 3;
    const MIN_STEM: usize = 4;
    let (short, long) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    short.chars().count() >= MIN_STEM
        && long.starts_with(short)
        && long.chars().count() - short.chars().count() <= MAX_TAIL
}

fn resembles_something(word: &str, source_words: &[String], record: Script) -> bool {
    let k = key(word);
    let foreign = is_foreign(word, record);
    let budget = if foreign {
        // Transliteration is not phonetic («гитхаб» → `gitkhab` against `github`), so a
        // canonicalization needs slack: half the length. A false alarm here is expensive
        // (an honest summary goes into the draft), while a false «grounding» of a
        // foreign-script term is cheap: terms do not invent facts.
        ((k.chars().count() as f64 * 0.5).round() as usize).max(1)
    } else {
        // a name/title in the language of the recording: one edit is a grammatical case,
        // not «similar»
        1
    };
    if budget == 0 {
        let s = stem(word);
        return source_words.iter().any(|src| stem(src) == s);
    }
    source_words.iter().any(|src| {
        let sk = key(src);
        // A CASE ENDING. One word is a prefix of the other, and the tail is short:
        // «Иван» / «Иваном», «Антон» / «Антоном». The stemmer MUST NOT be trusted here,
        // and this is measured: Snowball gives «иван» → «ива», but «иваном» → «иван» —
        // the STEMS ARE DIFFERENT, and a name that was actually spoken was declared an
        // invention. A budget of one edit does not save us either: «ivan» → «ivanom» is
        // two.
        //
        // Short words do not fall under this rule (a minimum of 4 letters): otherwise
        // «Ян» would latch onto «января».
        if case_ending(&k, &sk) {
            return true;
        }
        if sk.chars().count().abs_diff(k.chars().count()) > budget {
            return false;
        }
        // The first letter is the most stable feature UNDER DECLENSION: it survives any
        // grammatical case. Without this check «Роман» latches onto «команд», and
        // «Максим» onto «каким».
        //
        // It does NOT survive transliteration, and that is measured: `deunicode` writes
        // «капасити» as `kapasiti`, while English spells the same sound with a `c` —
        // `capacity`. The distance is 3 against a budget of 4; the ONLY thing that
        // rejected the repair was the first letter. The same collision hits «кэш»/`cache`
        // and «хаб»/`hub`: к→k/c and х→kh/h are everyday sounds of this vocabulary.
        // An honest summary that canonicalizes a mangled anglicism must not be called a
        // liar for it — so for a word in a FOREIGN script the first letter is not a
        // requirement, and what holds the check are the halved budget and the equal
        // length.
        if !foreign && sk.chars().next() != k.chars().next() {
            return false;
        }
        strsim::levenshtein(&k, &sk) <= budget
    })
}

/// All the numbers of the input: as digits AND as words. The spelled-out form is expanded
/// («сто пятьдесят» → 150), otherwise a «150» in the summary would look like an invention
/// even though the number was actually spoken.
fn source_numbers(text: &str) -> BTreeSet<u64> {
    let mut out: BTreeSet<u64> = text
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<u64>().ok())
        .collect();
    // The recording is read GENEROUSLY (see num2words): a spurious number in the
    // recording is harmless — it can only ground the answer; a spurious one in the answer
    // is a false accusation.
    out.extend(num2words::parse_spelled_in_source(text));
    out
}

/// Does the answer say anything at all ABOUT THIS PARTICULAR recording.
///
/// The sign that there was nothing to talk about is not the wording but a **zero overlap
/// of content**. A real analysis MUST reuse the words of the recording: names, terms, key
/// nouns. A refusal does not overlap with it in a single meaningful word — «в записи не
/// распознано ни одной законченной фразы» and «разбиваю шесть яиц» have no words in
/// common at all.
///
/// This is more reliable than looking for the phrase «не распознано» in the answer: the
/// model rewords it (acceptance run 12.07.2026 — the template prescribed one thing, the
/// model returned another, the exact comparison did not fire, and the refusal landed in
/// the archive as a «summary»). The overlap, on the other hand, does not depend on the
/// wording.
///
/// Function words are filtered out by length: a significant word is 4 letters or more.
///
/// `template` is the text of our own template (and the prescribed refusal line lives in
/// it). Its words are SUBTRACTED from the comparison, and this is fundamental: they cannot
/// be proof that the model is talking about the recording — we handed them to it
/// ourselves. Without this, the refusal «Содержательной **речи** в **записи** не
/// **распознано**» overlapped with the speech «Проверяю **распознавание речи**» and went
/// into the archive as a summary (caught by the review, reproduced).
pub fn says_something_about(speech: &str, answer: &str, template: &str) -> bool {
    let content = |text: &str| -> BTreeSet<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| w.chars().count() >= 4)
            .map(stem)
            .collect()
    };
    let ours = content(template);
    let src: BTreeSet<String> = content(speech).difference(&ours).cloned().collect();
    if src.is_empty() {
        return false; // the recording has nothing of its own — there is nothing to compare to
    }
    content(answer).intersection(&src).next().is_some()
}

/// What in `answer` does not lean on `source`, using the process's active lexicon
/// (the built-in dictionaries + the user directory `LOCALVOX_LEXICON_DIR`).
///
/// `source` is all the text the model saw: the speech AND the template AND the glossary.
/// It did not invent the words from our own template («Решения», «Поручения»).
pub fn check(source: &str, answer: &str) -> Ungrounded {
    check_parts(lexicon::active(), source, source, answer)
}

/// The same with an explicit lexicon — for tests and for foreign dictionaries.
pub fn check_with(lex: &Lexicon, source: &str, answer: &str) -> Ungrounded {
    check_parts(lex, source, source, answer)
}

/// A check with SEPARATE sources for words and for numbers.
///
/// Grounding words in the template is allowed — we gave them to the model ourselves
/// («Решения», «Поручения», the canonical form of a term from the glossary). NUMBERS are
/// not: our own prompt contains «1–3 предложения», «2–4 предложения», and they grounded
/// the invented «3 задачи» in the answer (caught by the review). A number is a fact about
/// the recording, and it MUST lean on the speech, not on the instruction.
pub fn check_parts(
    lex: &Lexicon,
    words_source: &str,
    numbers_source: &str,
    answer: &str,
) -> Ungrounded {
    check_all(lex, words_source, numbers_source, answer, None)
}

/// The source of entities is **not a generative model**.
///
/// An implementation MUST only TAG spans of the input (GLiNER: an encoder, it has no
/// decoder, it has nothing to produce a new token with). Handing an LLM in here would mean
/// returning into the checking loop exactly what the check protects against.
///
/// Why this instead of a list of names. A list does not see the context and therefore
/// NEVER resolves homonymy: «Роман закроет задачу» and «дописать роман» are the same word
/// to it. Measured on the live model: 0.89 against 0.03 — the context decides. And besides,
/// there are infinitely many names in a language, and a list is doomed by construction.
pub trait Entities: Send + Sync {
    /// PEOPLE in the answer — strictly.
    fn people(&self, text: &str) -> Vec<String>;

    /// PEOPLE in the recording — generously (a lower threshold).
    ///
    /// The asymmetry is deliberate and exactly the same as with numbers. A spurious person
    /// in the RECORDING is harmless: it can only GROUND something in the answer. A
    /// spurious one in the ANSWER is a false accusation and the quarantining of an honest
    /// summary.
    fn people_in_source(&self, text: &str) -> Vec<String> {
        self.people(text)
    }
}

/// A check with NER: names, organizations, dates and amounts are taken from the tagging
/// model, not from a dictionary.
pub fn check_with_entities(
    lex: &Lexicon,
    words_source: &str,
    numbers_source: &str,
    answer: &str,
    ner: &dyn Entities,
) -> Ungrounded {
    check_all(lex, words_source, numbers_source, answer, Some(ner))
}

fn check_all(
    lex: &Lexicon,
    words_source: &str,
    numbers_source: &str,
    answer: &str,
    ner: Option<&dyn Entities>,
) -> Ungrounded {
    let source = words_source;
    // The script of the recording is taken from the SPEECH, not from the whole source:
    // the source includes the template, and the template may be in another language (an
    // English template for a German recording), in which case the script would be
    // determined by our own instruction.
    let record = script_of(numbers_source);
    let src_checkable = checkable(source, lex, record);
    // The stems of the source are taken from ALL the words, not only the significant ones:
    // «Ивану» in the speech grounds «Иван» in the summary, even though it might not have
    // made it into `checkable`.
    let src_words = all_words(source);
    let src_keys: BTreeSet<String> = src_words.iter().map(|w| key(w)).collect();
    let src_stems: BTreeSet<String> = src_words.iter().map(|w| stem(w)).collect();
    let src_numbers = source_numbers(numbers_source);

    let mut out = Ungrounded::default();
    // Numbers written out IN WORDS in the answer are the same facts: «больше ста
    // пятидесяти» is 150, and it MUST be in the recording. Such numbers used not to be
    // checked at all — a whole class of invention slipped through.
    //
    // NUMBERS STAY DETERMINISTIC EVEN WITH NER. Numerals and dates are a closed class with
    // a finite grammar; here the rules beat any model on recall and give one hundred
    // percent explainability. This is not a fallback path but the right tool — unlike lists
    // of NAMES, which are a dead end.
    // Numbers are collected REGARDLESS of whether the model is there or not. The model does
    // not judge them and must not: a closed class, a finite grammar — the rules are more
    // precise and more explainable. If the digits came from the model's tagging, then a
    // model that tagged nothing would silently switch off the hardest part of the check.
    let mut answer_numbers: BTreeSet<u64> = num2words::parse_spelled(answer).into_iter().collect();
    answer_numbers.extend(answer_digits(answer));

    // What to check as a NAME/TITLE.
    //
    // If the model is there — we ask it: it tags spans of the input and sees the context,
    // so it resolves homonymy and does not depend on any list. If there is no model — the
    // old heuristics work (a capital letter not at the start of a phrase, a foreign script,
    // the user's dictionary). The dictionary stays a HINT in both cases: a colleague's name
    // that the model missed will still be checked.
    // PEOPLE are the only class the model judges, and this is measured, not chosen out of
    // taste.
    //
    // A name CANNOT BE REWORDED: «Иван» was either spoken or invented. A deadline and an
    // amount, on the other hand, are legitimately reworded — «около двухсот тысяч» →
    // «порядка 200 000», «next week» → «the following week». By checking them BY WORDS we
    // declared an honest retelling an invention (measured: «порядка», «following»,
    // «suite»). Their content is a NUMBER, and a number is checked deterministically,
    // above.
    //
    // A person is checked against the PEOPLE OF THE RECORDING, not against its words. The
    // word «роман» is in the recording — but there it is a book. A word-based check will
    // miss such an invention: the string was found, after all. A people-based check will
    // not.
    let people: BTreeSet<String> = match ner {
        Some(ner) => {
            // The markup is stripped BEFORE the model: it is trained on prose, and heading
            // hashes throw it off so badly that the name after «## Главное» disappears.
            let src_plain = plain(numbers_source);
            let ans_plain = plain(answer);

            let src_people = entity_words(&ner.people_in_source(&src_plain), lex);
            let src_people_keys: BTreeSet<String> = src_people.iter().map(|w| key(w)).collect();
            let ans_people = entity_words(&ner.people(&ans_plain), lex);

            // The words OF OUR OWN TEMPLATE are not an invention: we gave them to the
            // model ourselves.
            //
            // Recordings without a single name exist («я отработал на заводе три года…»),
            // and the summary has to call the speaker something. It calls them «спикер» —
            // and that is a ROLE, not a person. The NER honestly tags it as a name, and an
            // honest summary went into quarantine (acceptance run 13.07.2026, the live
            // archive). A role stops being an invention exactly when it is WE who give the
            // word for it: the template tells the model to call an unnamed speaker
            // «спикером».
            //
            // ONLY a role can be grounded by the template. A name will never appear in the
            // template — there is not a single person in it.
            // We subtract the words of the SPEECH, not of the whole source: `src_words` IS
            // the whole source (the template + the speech), and subtracting it from itself
            // would give emptiness.
            let speech_words: BTreeSet<String> = all_words(numbers_source).into_iter().collect();
            let template_words: BTreeSet<String> = all_words(words_source)
                .into_iter()
                .filter(|w| !speech_words.contains(w))
                .map(|w| key(&w))
                .collect();

            for w in &ans_people {
                if w.chars().all(|c| c.is_ascii_digit()) {
                    continue; // numbers are judged by the rules, not by the model
                }
                if template_words.contains(&key(w))
                    || src_people_keys.contains(&key(w))
                    || resembles_something(w, &src_people, record)
                {
                    continue;
                }
                out.names.push(w.clone());
            }
            ans_people.into_iter().collect()
        }
        None => BTreeSet::new(),
    };

    // Everything else is the old heuristics: a foreign script, a capital letter not at the
    // start of a phrase, the user's dictionary. They stay a HINT: a colleague's name that
    // the model did not tag will still be checked.
    for w in checkable(answer, lex, record) {
        if people.contains(&w) {
            continue; // already judged strictly, against the people of the recording
        }
        // Digits are NOT touched here: `answer_digits` has already collected them, and
        // collected them CORRECTLY — gluing the digit groups («200 000» is one number).
        // A word-by-word parse sees «200» and «000» and yields «invented 200 and 0» on an
        // honest summary.
        if w.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if src_checkable.contains(&w)
            || src_keys.contains(&key(&w))
            || src_stems.contains(&stem(&w))
        {
            continue;
        }
        if !resembles_something(&w, &src_words, record) {
            out.names.push(w);
        }
    }
    out.names.sort();
    out.names.dedup();
    for n in answer_numbers {
        if !src_numbers.contains(&n) {
            out.numbers.push(n.to_string());
        }
    }
    out
}

#[cfg(test)]
mod ner_tests {
    use super::*;
    use localvox_light_core::lexicon::Lexicon;

    /// A stub tagger: returns what the real GLiNER returned on these very phrases
    /// (measured 13.07.2026, model_fp16). The test is about THE LOGIC OF THE COMPARISON,
    /// not about the model: the model is verified by a probe, the logic — here.
    struct Stub(Vec<(&'static str, Vec<&'static str>)>);
    impl Entities for Stub {
        fn people(&self, text: &str) -> Vec<String> {
            self.0
                .iter()
                .find(|(t, _)| *t == text)
                .map(|(_, e)| e.iter().map(|s| s.to_string()).collect())
                .unwrap_or_default()
        }
    }

    fn empty_lex() -> Lexicon {
        // IMPORTANT: the name dictionary is EMPTY. All the work is done by the model: if
        // the test passes, then lists of names are no longer needed.
        let mut lex = Lexicon::builtin();
        lex.names.clear();
        lex
    }

    #[test]
    fn an_invented_person_is_caught_without_any_name_list() {
        // «Marcus» stands at the start of a phrase, where a capital letter means nothing,
        // and he is in no dictionary. He used to be caught only by the list of names —
        // now the model catches him, because it sees that this is a PERSON.
        let speech = "We agreed to ship the feature next week and to tell the team about it.";
        let answer = "Marcus will ship the feature next week.";
        let ner = Stub(vec![(speech, vec![]), (answer, vec!["Marcus"])]);

        let u = check_with_entities(&empty_lex(), speech, speech, answer, &ner);
        assert!(
            u.names.iter().any(|n| n == "marcus"),
            "an invented person got through: {u:?}"
        );
    }

    #[test]
    fn homonymy_is_resolved_by_context_not_by_a_dictionary() {
        // One and the same word. The only difference is the context — and the model sees
        // it (measured): «Роман закроет задачу» → person 0.89; «дописать роман» → no
        // person at all. A list NEVER solves this.
        //
        // The key to the test: the model also tags THE RECORDING. In the recording «роман»
        // is a book, that is, there is NO PERSON there. That is why the comparison is
        // ENTITY AGAINST ENTITIES, not word against words: the word IS in the recording,
        // and a word-based comparison would have missed the invention.
        let speech = "Обсудили планы: надо дописать роман к концу года.";

        // (a) роман as a book — there is no person either in the recording or in the answer
        let answer_book = "Решили дописать роман к концу года.";
        let ner = Stub(vec![(speech, vec![]), (answer_book, vec![])]);
        let u = check_with_entities(&empty_lex(), speech, speech, answer_book, &ner);
        assert!(u.is_empty(), "an honest retelling was accused: {u:?}");

        // (b) Роман as a PERSON, who was not in the recording
        let answer_person = "Роман закроет задачу.";
        let ner = Stub(vec![(speech, vec![]), (answer_person, vec!["Роман"])]);
        let u = check_with_entities(&empty_lex(), speech, speech, answer_person, &ner);
        assert!(
            u.names.iter().any(|n| n == "роман"),
            "an invented person got through disguised as a book: {u:?}"
        );
    }

    #[test]
    fn a_paraphrase_is_not_an_invention() {
        // A retelling («rollout» → «deployment») is not an entity, the model does not tag
        // it — and the check stays silent. This is the very barrage of false accusations
        // that the «Latin ⇒ check it» heuristic produced.
        let speech = "So we discussed the release timeline and agreed to postpone the rollout.";
        let answer = "The team decided to delay deployment until the following week.";
        let ner = Stub(vec![(speech, vec![]), (answer, vec![])]);

        let u = check_with_entities(&empty_lex(), speech, speech, answer, &ner);
        assert!(
            u.is_empty(),
            "an honest retelling was declared an invention: {u:?}"
        );
    }

    #[test]
    fn a_person_actually_spoken_is_grounded_in_any_case_form() {
        // «Иваном» in the recording grounds «Иван» in the summary — the grammatical case
        // is repaired by the stemmer, as before. The model merely SAID that this is a
        // person.
        let speech = "Мы с Иваном обсудили бюджет.";
        let answer = "Иван отвечает за бюджет.";
        let ner = Stub(vec![(speech, vec!["Иваном"]), (answer, vec!["Иван"])]);

        let u = check_with_entities(&empty_lex(), speech, speech, answer, &ner);
        assert!(
            u.is_empty(),
            "a name that was actually spoken was declared an invention: {u:?}"
        );
    }

    #[test]
    fn numbers_stay_deterministic_even_with_a_model() {
        // The model does not judge numbers. A closed class, a finite grammar — the rules
        // are more precise and more explainable here, and handing them to the model is not
        // allowed.
        let speech = "В бэклоге около двухсот задач.";
        let answer = "В бэклоге 200 задач, срок — 15 числа.";
        // the model tagged NOTHING
        let ner = Stub(vec![(speech, vec![]), (answer, vec![])]);

        let u = check_with_entities(&empty_lex(), speech, speech, answer, &ner);
        assert!(
            !u.numbers.iter().any(|n| n == "200"),
            "«двести» in the recording MUST ground «200» in the answer: {u:?}"
        );
        assert!(
            u.numbers.iter().any(|n| n == "15"),
            "an invented deadline slipped through, even though numbers are always \
             checked: {u:?}"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// That very fabricated summary against that very transcript (the humming).
    /// It is caught by the names (the dictionary, regardless of position) and by the
    /// numbers. «API» is an abbreviation, it carries no facts and is not checked: you
    /// cannot invent a person, a deadline or a quantity with it.
    #[test]
    fn the_fabricated_meeting_is_caught() {
        let source = "[0s] Т.\n[15s] .\n[47s] э-э, ла-ла-ла.";
        let answer = "## Поручения\n* Иван → обновить компонент → до пятницы.\n\
                      * Мария → перенести логику в API → до среды.\n\
                      Задач в бэклоге больше 150.";
        let u = check(source, answer);
        assert!(!u.is_empty(), "a fabricated summary passed the check");
        assert!(u.numbers.contains(&"150".into()), "{u:?}");
        assert!(u.names.contains(&"иван".into()), "{u:?}");
        assert!(u.names.contains(&"мария".into()), "{u:?}");
    }

    /// The dictionary is configurable: a colleague's name from one's own file works the
    /// same way as a built-in one. That is exactly why the lexicon was moved into data.
    #[test]
    fn a_custom_name_from_the_user_lexicon_is_checked_too() {
        let mut lex = Lexicon::builtin();
        lex.names.insert("кассандра".into());
        let u = check_with(&lex, "обсудили сроки", "Задачу берёт Кассандра.");
        assert!(u.names.contains(&"кассандра".into()), "{u:?}");
        // without one's own dictionary this word is not considered a name
        let u = check_with(
            &Lexicon::builtin(),
            "обсудили сроки",
            "Задачу берёт кассандра.",
        );
        assert!(u.is_empty(), "{u:?}");
    }

    /// A name mentioned INSIDE THE TEXT (and not as the first word of a bullet) is caught
    /// as a name.
    #[test]
    fn invented_person_in_a_sentence_is_caught() {
        let source = "[0s] обсудили сроки выкладки";
        let u = check(source, "Решено, что Иван обновит компонент.");
        assert!(u.names.contains(&"иван".into()), "{u:?}");
    }

    /// The blind spot is CLOSED: a name from the dictionary is checked regardless of
    /// position, so an invention no longer hides as the first word of a list item.
    #[test]
    fn bullet_initial_invented_name_is_no_longer_a_blind_spot() {
        let source = "[0s] обсудили сроки выкладки";
        let u = check(source, "## Поручения\n* Иван → обновить компонент.");
        assert!(u.names.contains(&"иван".into()), "{u:?}");
    }

    /// A capitalized ordinary word at the start of a sentence is NOT a name. A check that
    /// rejects honest text is worse than no check at all: the very first candidate for an
    /// «invention» in the first version was «Компания».
    #[test]
    fn a_capitalized_common_word_at_sentence_start_is_not_a_name() {
        let source = "расходы XI от И превышают выручку";
        let u = check(source, "Компания XAI работает в минус.");
        assert!(u.is_empty(), "{u:?}");
    }

    /// Repairing an ASR distortion is not an invention but a benefit: that is exactly what
    /// the LLM is for. Russian ASR writes in Cyrillic, the model canonicalizes into Latin —
    /// between the alphabets a character-wise Levenshtein is always maximal, which is why
    /// we compare transliterated keys. On the real transcript_uat.txt the first version
    /// found 26 «inventions», and every one of them was such a repair.
    #[test]
    fn an_honest_english_summary_is_not_accused_of_inventing_english_words() {
        // Measured on a real run before the fix: an honest English summary got six
        // accusations in a row — «deployment», «documentation», «schedule», «reviewed»,
        // «unstable», «until». All of them are ordinary words of a retelling. The reason:
        // the rule «Latin ⇒ check it» is only true for a CYRILLIC recording, where Latin
        // is the foreign script. In an English recording the Latin script is native, and
        // every single word fell under the rule.
        let speech = "So we discussed the release timeline and agreed to postpone the rollout \
                      to next week because the tests are still failing. Also we need to update \
                      the onboarding docs before the demo.";
        let answer = "## Key points\n\
                      - The team reviewed the release schedule and decided to delay deployment \
                      until the following week, since the test suite remains unstable.\n\
                      - Documentation for onboarding must be refreshed prior to the demo.\n";
        let u = check(speech, answer);
        assert!(
            u.is_empty(),
            "an honest retelling of an English recording was declared an invention: {:?}",
            u.names
        );
    }

    #[test]
    fn in_an_english_record_a_moved_deadline_is_still_caught() {
        // The opposite danger: if the native Latin script is handed the GENEROUS budget
        // (it is meant for transliteration), the check goes blind. «Thursday» (8 letters,
        // budget 4) would be grounded against «Tuesday» — two edits, the same first
        // letter. The model would move the meeting to another day, and we would certify it.
        let speech = "Let us meet on Tuesday and go through the remaining items together.";
        let u = check(speech, "The team will meet on Thursday.");
        assert!(
            u.names.iter().any(|n| n == "thursday"),
            "an invented day of the week passed the check: {:?}",
            u.names
        );
    }

    /// The HONEST BOUNDARY of the check WITHOUT the NER model.
    ///
    /// Numbers are always caught — those are deterministic rules, and they do not depend on
    /// any model. An invented person at the START of a phrase, however, is not caught
    /// without the model: a capital letter means nothing there, and we no longer have a
    /// list of English names — and rightly so. There are infinitely many names in a
    /// language, a list is doomed, and it never resolves homonymy («Роман»/роман).
    ///
    /// This hole is closed by the NER model (see `ner_tests`), and only by it. While the
    /// model is absent — we SAY SO, instead of pretending that the names have been checked.
    #[test]
    fn without_a_model_numbers_are_still_caught_but_a_sentence_initial_name_is_not() {
        let speech = "We agreed to ship the feature next week and to tell the team about it.";
        let u = check(speech, "Marcus will ship 15 features on Friday.");

        assert!(
            u.numbers.iter().any(|n| n == "15"),
            "numbers MUST always be caught: {:?}",
            u.numbers
        );
        assert!(
            !u.names.iter().any(|n| n == "marcus"),
            "the test is lying: without a model and without a dictionary there is nothing \
             to catch «Marcus» at the start of a phrase with"
        );
    }

    #[test]
    fn cross_alphabet_repairs_are_grounded() {
        let cases = [
            (
                "расходы XI от И превышают выручку",
                "Компания XAI в минусе.",
            ),
            ("прикрутим слак и фигму", "Решили подключить Slack и Figma."),
            ("задачи держим в гитхабе", "Задачи в GitHub."),
            (
                "база на постгресе, кэш в редисе",
                "База на Postgres, кэш в Redis.",
            ),
            ("расходы Антропику растут", "Расходы Anthropic растут."),
            // The transliteration of the recording and the English spelling start with
            // DIFFERENT letters: к → `k`, but English writes the same sound as `c`;
            // х → `kh`, but English writes `h`. The first letter survives declension —
            // it does not survive transliteration.
            //
            // The budget still binds, and deliberately: «кэш» (`kesh`) is 4 edits away
            // from `cache` out of 5 letters and stays UNGROUNDED. Stretching the budget
            // past half the length is where any Latin term starts finding a neighbour.
            (
                "капасити посчитали по головам",
                "Capacity посчитали по головам.",
            ),
            ("выкатим на хаб", "Выкатим на hub."),
        ];
        for (source, answer) in cases {
            let u = check(source, answer);
            assert!(
                u.is_empty(),
                "a repair was taken for an invention: {source} → {u:?}"
            );
        }
    }

    /// The flip side: a generous similarity budget is lethal for names. On a 4000-word
    /// transcript the first version «repaired» «Мария» against «серия», «Антон» against
    /// «кантом», «Максим» against «каким» — 9 of 13 invented names got through. For
    /// Cyrillic the budget is one edit + a matching first letter.
    #[test]
    fn invented_names_do_not_latch_onto_random_words() {
        let source = "серия задач, кантом закрыли, каким путём пойдём, поинт команды, \
                      обсудили тему, его предложение, кате отдали";
        let answer = "Задачу берёт Мария. Антон и Максим помогут. Артём и Полина \
                      проверят. Роман и Тимур закроют.";
        let u = check(source, answer);
        for name in [
            "мария",
            "антон",
            "максим",
            "артём",
            "полина",
            "роман",
            "тимур",
        ] {
            assert!(
                u.names.contains(&name.to_string()),
                "«{name}» latched onto a random word: {u:?}"
            );
        }
    }

    /// A word that is BOTH a name AND a common noun is a trap when it is in the name
    /// dictionary: it is checked EVERYWHERE, while it legitimately appears in an honest
    /// retelling. The boundary runs along the frequency in ordinary speech, not along
    /// «can this be a name at all»: «надежда» in a business text is almost always hope,
    /// «Роман» is almost always a person.
    #[test]
    fn a_word_that_is_both_a_name_and_a_common_noun_does_not_accuse_an_honest_retelling() {
        let speech = "Ребята, есть надежда, что до конца недели успеем, но веры в это мало.";
        let u = check(speech, "Есть надежда закрыть работу к концу недели.");
        assert!(
            u.is_empty(),
            "an honest retelling was accused: {:?}",
            u.names
        );
    }

    #[test]
    fn a_common_first_name_is_still_caught_when_invented() {
        let speech = "Задачу закроем на неделе, там немного осталось.";
        let u = check(speech, "Роман закроет задачу на неделе.");
        assert!(
            u.names.iter().any(|n| n == "роман"),
            "an invented person got through: {:?}",
            u.names
        );
    }

    /// A name that WAS ACTUALLY spoken (even if in another grammatical case) is grounded.
    #[test]
    fn a_name_actually_spoken_is_grounded_in_any_case_form() {
        let source = "поручим это Ивану, а Марии отдадим вторую часть";
        let u = check(source, "Иван берёт первую часть, Мария — вторую.");
        assert!(u.is_empty(), "real names were taken for inventions: {u:?}");
    }

    /// Cleaning up a line: repairing is allowed, adding is not. This same check stands in
    /// `refine_session` line by line, because the refined version becomes best, that is,
    /// THE transcript itself: whatever the model adds there is «grounded» by definition
    /// from then on.
    #[test]
    fn refine_may_repair_a_line_but_not_add_to_it() {
        // The line has already been through the deterministic glossary (as it is in prod:
        // refine compares the model's answer against the line AFTER the glossary). Terms
        // that transliteration cannot pull off («джиру» → `dzhiru`, while `Jira` starts
        // with a j) are repaired precisely by the glossary — that is what it is for.
        let original = "ну это самое надо Jira завести и созвон в четверг";
        // cleaning up what was recognized is legitimate
        assert!(check(original, "Надо завести Jira и созвон в четверг.").is_empty());
        // so is canonicalizing what transliteration does pull off
        assert!(check("прикрутим слак", "Подключим Slack.").is_empty());
        // but an added name and time are not
        let u = check(original, "Иван заведёт Jira, созвон в четверг в 15:00.");
        assert!(u.names.contains(&"иван".into()), "{u:?}");
        assert!(u.numbers.contains(&"15".into()), "{u:?}");
    }

    /// The same for an ENGLISH line. Before the rules were fixed (they silently relied on
    /// «the recording is always Russian») the line-by-line comparison rejected ANY cleaned
    /// up English line: the preposition «on» and the synonym «notify» were declared
    /// additions, the line was rolled back to the raw ASR — and the whole refine feature
    /// was silently switched off for non-Russian recordings.
    #[test]
    fn refine_of_an_english_line_is_not_rolled_back_wholesale() {
        let original = "um so we we should uh ship it friday and i will ping the team";
        let refined = "So we should ship it on Friday, and I will notify the team.";
        let u = check(original, refined);
        assert!(
            u.is_empty(),
            "an honest cleanup of an English line was declared an addition: {u:?}"
        );

        // an added TIME does not pass: numbers are deterministic and require no model
        let u = check(original, "Tom will ship it on Friday at 15:00.");
        assert!(u.numbers.iter().any(|n| n == "15"), "{u:?}");
        // an added «Tom» at the start of a phrase, on the other hand, is impossible to
        // catch without the NER model — and that is stated honestly in
        // `without_a_model_numbers_are_still_caught…`
    }

    /// Markup is not facts: section numbers and table cells must end up neither among the
    /// «numbers» nor among the «names». On a real summary from the repository the first
    /// version found «invented» 6, 7, 9, 11–14 and «Вероятная», «Внешняя» exactly this way.
    #[test]
    fn markdown_structure_is_not_content() {
        let source = "обсудили планирование и сроки";
        let answer = "### 3.6. Изменения в планировании\n\
                      | Как звучит | Вероятная нормализация | Комментарий |\n\
                      |---|---|---|\n\
                      | планер | планировщик | Внутренний инструмент. |";
        let u = check(source, answer);
        assert!(u.is_empty(), "the markup was taken for content: {u:?}");
    }

    #[test]
    fn numbers_are_checked_in_both_directions() {
        // in words in the recording, in digits in the summary — it is the same number
        let source = "в бэклоге сто пятьдесят задач";
        assert!(check(source, "Задач в бэклоге: 150.").is_empty());
        // and the other way round: in digits in the recording, in words in the summary
        assert!(check("в бэклоге 150 задач", "Задач больше ста пятидесяти.").is_empty());
        // neither form of the recording saves an invented number
        assert_eq!(
            check(source, "Задач в бэклоге: 200.").numbers,
            vec!["200".to_string()]
        );
        assert_eq!(
            check(source, "Задач около двухсот.").numbers,
            vec!["200".to_string()]
        );
    }

    /// Numbers from OUR OWN template («1–3 предложения») do not ground facts about the
    /// recording. Words do ground them (the model did not invent them), numbers do not:
    /// quoting a figure from the instruction and passing it off as a fact is not allowed.
    #[test]
    fn numbers_from_the_template_do_not_ground_facts() {
        let lex = Lexicon::builtin();
        let template = "Сделай выжимку в 1–3 предложения. Разделов 7.";
        let speech = "обсудили сроки и договорились";
        let answer = "Осталось 3 задачи.";
        // the template's words are in the base — but the number 3 must lean on the SPEECH
        let u = check_parts(&lex, &format!("{template}\n{speech}"), speech, answer);
        assert_eq!(u.numbers, vec!["3".to_string()], "{u:?}");
        // and if the number was actually spoken — it is grounded
        let u = check_parts(&lex, template, "осталось три задачи", answer);
        assert!(u.numbers.is_empty(), "{u:?}");
    }

    /// Single-digit numbers are facts too: «5 задач» must lean on the recording.
    /// The first version discarded them as «too short».
    #[test]
    fn single_digit_numbers_are_checked_too() {
        let u = check("обсудили задачи", "Осталось 5 задач.");
        assert_eq!(u.numbers, vec!["5".to_string()], "{u:?}");
        assert!(check("осталось пять задач", "Осталось 5 задач.").is_empty());
    }

    #[test]
    fn declension_of_prompt_words_is_not_invention() {
        let source = "## Решения\n## Поручения\nрасшифровка: ла-ла-ла";
        assert!(check(source, "Решений не распознано. Поручений нет.").is_empty());
    }

    /// A short thought is a legitimate input, not «too few words». A summary made from it
    /// MUST pass the check, provided nothing has been made up on top of it.
    #[test]
    fn short_voice_note_is_fully_grounded() {
        let source = "[3s] Отметь мысль: надо переписать варку на пул воркеров.";
        let u = check(source, "Мысль: переписать варку на пул воркеров.");
        assert!(u.is_empty(), "{u:?}");
    }

    #[test]
    fn short_note_with_invented_deadline_is_caught() {
        let source = "[3s] Отметь мысль: надо переписать варку на пул воркеров.";
        let u = check(
            source,
            "Мысль: переписать варку на пул воркеров до 15 числа.",
        );
        assert_eq!(u.numbers, vec!["15".to_string()]);
    }

    #[test]
    fn short_word_needs_exact_stem_not_fuzzy_match() {
        // «Ян» must not be «repaired» against «январь»: too short to count that as the
        // correction of a distortion
        let u = check("обсудили январь и февраль", "Отчёт сделает Ян.");
        assert!(!u.names.is_empty(), "a short name slipped through: {u:?}");
    }
}
