//! Digits → words (Russian, cardinal numerals).
//!
//! Why this is in the benchmark. GigaAM v3 is an «e2e» model: it places the
//! punctuation, the case AND THE DIGITS itself («9 472 824», «2013»). The Golos,
//! Russian LibriSpeech and Common Voice references are written out in WORDS
//! («девять четыреста семьдесят два», «две тысячи тринадцать»). Without expanding
//! the digits, WER measures not the quality of recognition but a mismatch of
//! formats: on the very first run four of the five worst recordings had a WER of
//! 100% while the numbers had been heard perfectly.
//!
//! That is exactly why the authors of GigaAM write in their WER table: «with
//! post-processing applied (removing punctuation and capitalization, replacing
//! numerals, etc.)».
//!
//! Gender: «тысяча» is feminine («две тысячи»), «миллион»/«миллиард» are masculine
//! («два миллиона»); the last group is read in the masculine («двадцать один»), the
//! way it is dictated in the reference sets (numbers, codes, years).

const UNITS_M: [&str; 20] = [
    "ноль",
    "один",
    "два",
    "три",
    "четыре",
    "пять",
    "шесть",
    "семь",
    "восемь",
    "девять",
    "десять",
    "одиннадцать",
    "двенадцать",
    "тринадцать",
    "четырнадцать",
    "пятнадцать",
    "шестнадцать",
    "семнадцать",
    "восемнадцать",
    "девятнадцать",
];
const UNITS_F: [&str; 3] = ["ноль", "одна", "две"];
const TENS: [&str; 10] = [
    "",
    "",
    "двадцать",
    "тридцать",
    "сорок",
    "пятьдесят",
    "шестьдесят",
    "семьдесят",
    "восемьдесят",
    "девяносто",
];
const HUNDREDS: [&str; 10] = [
    "",
    "сто",
    "двести",
    "триста",
    "четыреста",
    "пятьсот",
    "шестьсот",
    "семьсот",
    "восемьсот",
    "девятьсот",
];

/// Scale groups: (singular, 2–4, 5+), and whether the scale is feminine.
const SCALES: [(&str, &str, &str, bool); 4] = [
    ("", "", "", false),
    ("тысяча", "тысячи", "тысяч", true),
    ("миллион", "миллиона", "миллионов", false),
    ("миллиард", "миллиарда", "миллиардов", false),
];

fn group_words(n: u64, feminine: bool, out: &mut Vec<String>) {
    debug_assert!(n < 1000);
    let (h, rest) = ((n / 100) as usize, n % 100);
    if h > 0 {
        out.push(HUNDREDS[h].into());
    }
    if rest == 0 {
        return;
    }
    if rest < 20 {
        let i = rest as usize;
        if feminine && i < UNITS_F.len() && i > 0 {
            out.push(UNITS_F[i].into());
        } else {
            out.push(UNITS_M[i].into());
        }
        return;
    }
    out.push(TENS[(rest / 10) as usize].into());
    let u = (rest % 10) as usize;
    if u > 0 {
        if feminine && u < UNITS_F.len() {
            out.push(UNITS_F[u].into());
        } else {
            out.push(UNITS_M[u].into());
        }
    }
}

/// The form of the scale noun by the last digits («две тысячи», «пять тысяч»).
fn plural(n: u64, forms: (&'static str, &'static str, &'static str)) -> &'static str {
    let (one, few, many) = forms;
    match (n % 100, n % 10) {
        (11..=14, _) => many,
        (_, 1) => one,
        (_, 2..=4) => few,
        _ => many,
    }
}

/// An integer in words. `None` — if the number is too large (beyond a trillion):
/// such numbers do not occur in the references, and there is no need to lie about them.
pub fn spell(n: u64) -> Option<Vec<String>> {
    if n == 0 {
        return Some(vec!["ноль".into()]);
    }
    if n >= 1_000_000_000_000 {
        return None;
    }
    let mut groups: Vec<u64> = Vec::new();
    let mut rest = n;
    while rest > 0 {
        groups.push(rest % 1000);
        rest /= 1000;
    }
    let mut out: Vec<String> = Vec::new();
    for idx in (0..groups.len()).rev() {
        let g = groups[idx];
        if g == 0 {
            continue;
        }
        let (one, few, many, feminine) = SCALES[idx];
        group_words(g, feminine, &mut out);
        if idx > 0 {
            out.push(plural(g, (one, few, many)).into());
        }
    }
    Some(out)
}

/// A token made entirely of digits → words. Leading zeros are dropped («065» → 65):
/// that is exactly how they are dictated («шестьдесят пять»). A non-digit token and
/// digit sequences that are too long are returned as they are — silently corrupting
/// the text is worse than leaving the mismatch visible.
pub fn expand_token(tok: &str) -> Vec<String> {
    if tok.is_empty() || !tok.chars().all(|c| c.is_ascii_digit()) {
        return vec![tok.to_string()];
    }
    match tok.parse::<u64>().ok().and_then(spell) {
        Some(words) => words,
        None => vec![tok.to_string()],
    }
}

/// A numeral word → its value. The reverse side of `spell`: needed by the grounding
/// check — the model writes «больше ста пятидесяти», and that is exactly the same
/// number 150 that must be present in the record.
///
/// The forms live in the LEXICON (`lexicon/numerals-ru.toml`), not in the code: they
/// are data, and the user is entitled to extend them. They are listed explicitly
/// rather than derived by a stemmer: «пять» and «пятьдесят» begin the same way, and
/// any truncation of the stem glues 5 together with 50.
fn word_value(w: &str, lex: &crate::lexicon::Lexicon) -> Option<u64> {
    let w = w.trim_matches(|c: char| !c.is_alphabetic()).to_lowercase();
    lex.numeral_forms.get(&w).copied()
}

/// Numbers written out in WORDS: «сто пятьдесят задач» → 150.
///
/// We add up ONLY by the rules of Russian counting: inside a number the scales go in
/// descending order (hundreds → tens → units). Two numerals of the same scale in a row
/// are TWO numbers, not a sum: «четыре… пять» is 4 and 5, not 9. Acceptance run
/// 13.07.2026: naive addition produced an «invented number 9» that was not in the
/// record in any form, and sent honest minutes into quarantine.
///
/// «Один/одна/одно» are NOT counted as a number on their own: in Russian this is more
/// often an article («одна из задач», «ещё одну») than a quantity. As part of a number
/// they work as usual: «двадцать один», «одна тысяча».
pub fn parse_spelled(text: &str) -> Vec<u64> {
    parse_spelled_with(text, crate::lexicon::active())
}

/// The numbers of the RECORD — we parse them generously: «одна задача» also gives 1.
///
/// The asymmetry is deliberate. A superfluous number in the record is harmless: all it
/// can do is GROUND something in the answer. A superfluous number in the answer is a
/// false accusation and the quarantine of an honest text. So we read the record
/// generously and the answer strictly.
///
/// Without this, the English «we need one more server» in the record did not yield 1,
/// and an honest «1 more server» in the answer was declared an invention.
pub fn parse_spelled_in_source(text: &str) -> Vec<u64> {
    parse_inner(text, crate::lexicon::active(), false)
}

/// The scale of a numeral: only a strictly smaller one may attach to a larger one.
fn magnitude(v: u64) -> u8 {
    match v {
        1..=9 => 1,     // units
        10..=19 => 2,   // «одиннадцать» is by itself a ten plus a unit
        20..=90 => 3,   // tens
        100..=900 => 4, // hundreds
        _ => 5,         // thousands and up
    }
}

/// The same, but with an explicit lexicon (tests, custom dictionaries).
pub fn parse_spelled_with(text: &str, lex: &crate::lexicon::Lexicon) -> Vec<u64> {
    parse_inner(text, lex, true)
}

/// `skip_articles` — whether a lone «один»/«one» counts as an article rather than a
/// number.
///
/// There are NO WORDS OF ANY LANGUAGE here, and there must not be: articles and
/// connectors are a fact of language, and they live in the lexicon (`articles`,
/// `connectors`), just like the numerals themselves. There are many languages, but only
/// one parsing routine.
fn parse_inner(text: &str, lex: &crate::lexicon::Lexicon, skip_articles: bool) -> Vec<u64> {
    let mut out = Vec::new();
    let mut acc: Option<u64> = None; // the accumulated group (hundreds+tens+units)
    let mut last_mag: u8 = u8::MAX; // the scale of the last addend of the group
    let mut total: u64 = 0; // including «thousands»/«millions»

    let flush = |acc: &mut Option<u64>, total: &mut u64, last_mag: &mut u8, out: &mut Vec<u64>| {
        let v = total.saturating_add(acc.take().unwrap_or(0));
        if v > 0 {
            out.push(v);
        }
        *total = 0;
        *last_mag = u8::MAX;
    };

    for word in text.split(|c: char| !c.is_alphabetic()) {
        if word.is_empty() {
            continue;
        }
        let low = word.to_lowercase();
        let Some(v) = word_value(&low, lex) else {
            // A connector inside a number does not break the group.
            if !lex.numeral_connectors.contains(&low) {
                flush(&mut acc, &mut total, &mut last_mag, &mut out);
            }
            continue;
        };
        // «одна задача» is not a number. But «двадцать один» and «одна тысяча» are.
        if skip_articles && lex.numeral_articles.contains(&low) && acc.is_none() && total == 0 {
            flush(&mut acc, &mut total, &mut last_mag, &mut out);
            continue;
        }
        match v {
            // MULTIPLIERS of a scale, not addends. In Russian the hundreds are separate
            // words («двести»), so «сто» as a multiplier with a default group of 1 gives
            // exactly the previous result. In English «hundred» is precisely a
            // multiplier: «two hundred» is 200, not 2 and 100. Without this, any English
            // number ≥ 100 drove honest minutes into quarantine.
            100 | 1_000 | 1_000_000 | 1_000_000_000 => {
                let group = acc.take().unwrap_or(1);
                if v == 100 {
                    // the hundreds stay INSIDE the group: «two hundred fifty» → 250
                    acc = Some(group.saturating_mul(v));
                    last_mag = magnitude(v);
                } else {
                    total = total.saturating_add(group.saturating_mul(v));
                    last_mag = u8::MAX;
                }
            }
            _ => {
                let mag = magnitude(v);
                // The scale MUST DECREASE: «сто пятьдесят» — yes, «четыре пять» — no.
                if mag >= last_mag {
                    flush(&mut acc, &mut total, &mut last_mag, &mut out);
                }
                acc = Some(acc.unwrap_or(0) + v);
                last_mag = mag;
            }
        }
    }
    flush(&mut acc, &mut total, &mut last_mag, &mut out);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(n: u64) -> String {
        spell(n).unwrap().join(" ")
    }

    #[test]
    fn spelled_numbers_are_parsed_back() {
        assert_eq!(parse_spelled("в бэклоге сто пятьдесят задач"), vec![150]);
        assert_eq!(parse_spelled("больше ста пятидесяти"), vec![150]);
        assert_eq!(parse_spelled("две тысячи тринадцать"), vec![2013]);
        assert_eq!(parse_spelled("пять миллионов"), vec![5_000_000]);
        assert_eq!(parse_spelled("сорок два и восемь"), vec![42, 8]);
        assert!(parse_spelled("никаких чисел тут нет").is_empty());
    }

    /// «hundred» is a MULTIPLIER, not an addend. Before the fix «two hundred» gave
    /// [2, 100], and any English number ≥ 100 drove honest minutes into quarantine:
    /// «two hundred tickets» in the record, «200 tickets» in the minutes — «invented
    /// number 200».
    #[test]
    fn english_hundreds_multiply_instead_of_adding() {
        assert_eq!(parse_spelled("about two hundred open tickets"), vec![200]);
        assert_eq!(parse_spelled("one hundred and fifty"), vec![150]);
        assert_eq!(parse_spelled("a hundred and fifty"), vec![150]);
        assert_eq!(parse_spelled("two hundred fifty"), vec![250]);
        assert_eq!(parse_spelled("two hundred thousand"), vec![200_000]);
        assert_eq!(parse_spelled("two billion"), vec![2_000_000_000]);
        assert_eq!(parse_spelled("twenty five"), vec![25]);
        assert_eq!(parse_spelled("forty two and eight"), vec![42, 8]);
    }

    /// «One decision was taken» is not the number 1. English has an article too.
    #[test]
    fn a_leading_one_is_an_article_in_the_answer() {
        assert!(parse_spelled("one of the tasks is done").is_empty());
        assert!(parse_spelled("no one knows").is_empty());
        assert_eq!(parse_spelled("twenty one tasks"), vec![21]);
    }

    /// …but IN THE RECORD a lone one does count: otherwise an honest «1 more server» in
    /// the answer found no support in the spoken «one more server» and went into
    /// quarantine.
    #[test]
    fn in_the_record_a_lone_one_still_counts() {
        assert_eq!(parse_spelled_in_source("we need one more server"), vec![1]);
        assert_eq!(parse_spelled_in_source("одна задача осталась"), vec![1]);
    }

    /// The scales MUST DECREASE. Two numerals of the same scale in a row are TWO numbers,
    /// not a sum: acceptance run 13.07.2026 — naive addition of «четыре… пять» gave an
    /// «invented number 9» that was not in the record, and sent honest minutes into
    /// quarantine.
    #[test]
    fn adjacent_numerals_of_the_same_rank_are_two_numbers_not_a_sum() {
        assert_eq!(parse_spelled("четыре пять"), vec![4, 5]);
        assert_eq!(parse_spelled("два семь"), vec![2, 7]);
        // and a genuine compound number is still added up
        assert_eq!(parse_spelled("двадцать один"), vec![21]);
        assert_eq!(parse_spelled("сто двадцать три"), vec![123]);
    }

    /// «Один/одна» in Russian is more often an article than a number.
    #[test]
    fn the_word_one_alone_is_an_article_not_a_number() {
        assert!(parse_spelled("одна из задач").is_empty());
        assert!(parse_spelled("ещё одну попытку").is_empty());
        // but as part of a number it works as usual
        assert_eq!(parse_spelled("двадцать один процент"), vec![21]);
        assert_eq!(parse_spelled("одна тысяча"), vec![1000]);
    }

    /// Invertibility — for everything except a bare one: «один» as a standalone word we
    /// deliberately do not count as a number (in Russian it is an article).
    #[test]
    fn spell_and_parse_are_inverse_for_common_values() {
        for n in [7u64, 15, 42, 100, 150, 999, 2013, 5000] {
            let words = spell(n).unwrap().join(" ");
            assert_eq!(parse_spelled(&words), vec![n], "did not match on {n}: {words}");
        }
    }

    #[test]
    fn units_and_teens() {
        assert_eq!(s(0), "ноль");
        assert_eq!(s(1), "один");
        assert_eq!(s(11), "одиннадцать");
        assert_eq!(s(19), "девятнадцать");
    }

    #[test]
    fn tens_hundreds() {
        assert_eq!(s(30), "тридцать");
        assert_eq!(s(65), "шестьдесят пять");
        assert_eq!(s(472), "четыреста семьдесят два");
        assert_eq!(s(824), "восемьсот двадцать четыре");
        assert_eq!(s(900), "девятьсот");
    }

    #[test]
    fn thousands_are_feminine() {
        assert_eq!(s(1000), "одна тысяча");
        assert_eq!(s(2000), "две тысячи");
        assert_eq!(s(2013), "две тысячи тринадцать");
        assert_eq!(s(5000), "пять тысяч");
        assert_eq!(s(21_000), "двадцать одна тысяча");
        assert_eq!(s(111_000), "сто одиннадцать тысяч");
    }

    #[test]
    fn millions_are_masculine() {
        assert_eq!(s(1_000_000), "один миллион");
        assert_eq!(s(2_000_000), "два миллиона");
        assert_eq!(s(5_000_000), "пять миллионов");
    }

    #[test]
    fn leading_zeros_are_dropped_like_speech() {
        assert_eq!(expand_token("065"), vec!["шестьдесят", "пять"]);
        assert_eq!(expand_token("007"), vec!["семь"]);
    }

    #[test]
    fn non_digits_pass_through() {
        assert_eq!(expand_token("привет"), vec!["привет"]);
        assert_eq!(expand_token("15-й"), vec!["15-й"]);
    }

    #[test]
    fn absurdly_long_digit_run_is_left_alone() {
        let long = "1".repeat(30);
        assert_eq!(expand_token(&long), vec![long]);
    }

    #[test]
    fn golos_phone_number_matches_reference_wording() {
        // a real reference from Golos crowd; GigaAM's hypothesis: «9 472 824 065 30»
        let hyp: Vec<String> = "9 472 824 065 30"
            .split_whitespace()
            .flat_map(expand_token)
            .collect();
        assert_eq!(
            hyp.join(" "),
            "девять четыреста семьдесят два восемьсот двадцать четыре шестьдесят пять тридцать"
        );
    }
}
