//! WER/CER and text normalization before comparison.
//!
//! The normalization repeats the practice accepted in evaluating Russian ASR models
//! (GigaAM, Whisper and Vosk measure themselves after "post-processing: removing
//! punctuation and capitalization…"): without it WER measures the model's punctuation,
//! not its recognition. The rules:
//!   * lower case, `ё` → `е` (references are inconsistent about this);
//!   * punctuation → space (a hyphen INSIDE a word is kept: «из-за» is one word);
//!   * collapsing of spaces.
//!
//!   * numbers written as digits are expanded into words (`num2words`): GigaAM v3 writes
//!     «2013» and «9 472 824», while the references have «две тысячи тринадцать» and «девять
//!     четыреста семьдесят два». Without this WER would measure the output format, not the
//!     recognition (verified: 4 of the 5 worst records were exactly of that kind).
//!
//! What the normalization does NOT fix: Latin script in the hypothesis against Cyrillic in the
//! reference («YouTube» ↔ «ютьюб») and ordinals («15-й» ↔ «пятнадцатый»). Transliteration by eye
//! would add errors of its own, so such records are simply counted and shown on a separate line —
//! it is visible how much of the WER falls on them.

use localvox_light_core::num2words;

/// Bringing text to a comparable form. Returns words.
pub fn normalize(s: &str) -> Vec<String> {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        let ch = ch.to_lowercase().next().unwrap_or(ch);
        let ch = match ch {
            'ё' => 'е',
            c => c,
        };
        if ch.is_alphanumeric() || ch == '-' || ch == '\'' {
            out.push(ch);
        } else {
            out.push(' ');
        }
    }
    out.split_whitespace()
        // a hyphen/apostrophe at the edge of a word is punctuation, not part of the word
        .map(|w| w.trim_matches(|c| c == '-' || c == '\''))
        .filter(|w| !w.is_empty())
        .flat_map(num2words::expand_token)
        .collect()
}

/// Latin script — a potential mismatch of the alphabets of the reference and the hypothesis.
pub fn has_latin(s: &str) -> bool {
    s.chars().any(|c| c.is_ascii_alphabetic())
}

/// An ordinal numeral written in digits («15-й») — the normalization will not expand it.
pub fn has_ordinal_digits(s: &str) -> bool {
    let b: Vec<char> = s.chars().collect();
    b.windows(2).any(|w| w[0].is_ascii_digit() && w[1] == '-')
}

/// Levenshtein distance over elements (words or characters).
/// Memory O(min(n,m)) — references can be long (long-form).
fn edit_distance<T: PartialEq>(a: &[T], b: &[T]) -> usize {
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let (a, b) = if a.len() < b.len() { (b, a) } else { (a, b) };
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0usize; b.len() + 1];
    for (i, ai) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, bj) in b.iter().enumerate() {
            let sub = prev[j] + usize::from(ai != bj);
            cur[j + 1] = sub.min(prev[j + 1] + 1).min(cur[j] + 1);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

/// The errors of one (reference, hypothesis) pair. Aggregated over the corpus by SUM, not by an
/// average over files: corpus WER = Σ edits / Σ reference words — that is how the whole world
/// measures it, an average over files inflates the contribution of short utterances.
#[derive(Default, Clone, Copy)]
pub struct Errors {
    pub word_edits: usize,
    pub ref_words: usize,
    pub char_edits: usize,
    pub ref_chars: usize,
}

impl Errors {
    pub fn compare(reference: &str, hypothesis: &str) -> Self {
        let rw = normalize(reference);
        let hw = normalize(hypothesis);
        let rc: Vec<char> = rw.join(" ").chars().collect();
        let hc: Vec<char> = hw.join(" ").chars().collect();
        Self {
            word_edits: edit_distance(&rw, &hw),
            ref_words: rw.len(),
            char_edits: edit_distance(&rc, &hc),
            ref_chars: rc.len(),
        }
    }

    pub fn add(&mut self, o: Errors) {
        self.word_edits += o.word_edits;
        self.ref_words += o.ref_words;
        self.char_edits += o.char_edits;
        self.ref_chars += o.ref_chars;
    }

    /// WER in per cent. May be > 100 % (hallucinations = insertions).
    pub fn wer(&self) -> f64 {
        if self.ref_words == 0 {
            return if self.word_edits == 0 { 0.0 } else { 100.0 };
        }
        100.0 * self.word_edits as f64 / self.ref_words as f64
    }

    pub fn cer(&self) -> f64 {
        if self.ref_chars == 0 {
            return if self.char_edits == 0 { 0.0 } else { 100.0 };
        }
        100.0 * self.char_edits as f64 / self.ref_chars as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalization_kills_case_punctuation_and_yo() {
        assert_eq!(
            normalize("Привет, мир! Ёлка — из-за угла."),
            vec!["привет", "мир", "елка", "из-за", "угла"]
        );
    }

    #[test]
    fn dash_as_punctuation_is_not_a_word() {
        // a separating dash must not turn into a "word" and spoil the WER
        assert_eq!(normalize("да - нет"), vec!["да", "нет"]);
    }

    #[test]
    fn identical_text_is_zero_error() {
        let e = Errors::compare("один два три", "Один, ДВА... три!");
        assert_eq!(e.word_edits, 0);
        assert_eq!(e.wer(), 0.0);
        assert_eq!(e.cer(), 0.0);
    }

    #[test]
    fn wer_counts_substitution_insertion_deletion() {
        // the reference has 4 words; the hypothesis: 1 substitution + 1 deletion
        let e = Errors::compare("а б в г", "а х в");
        assert_eq!(e.ref_words, 4);
        assert_eq!(e.word_edits, 2);
        assert_eq!(e.wer(), 50.0);
    }

    #[test]
    fn hallucination_can_exceed_100_percent() {
        // exactly our case: silence → the model poured out text
        let e = Errors::compare("да", "да да да да да");
        assert!(e.wer() > 100.0, "wer={}", e.wer());
    }

    #[test]
    fn empty_hypothesis_is_total_loss() {
        let e = Errors::compare("раз два три", "");
        assert_eq!(e.wer(), 100.0);
    }

    #[test]
    fn corpus_wer_is_sum_of_edits_over_sum_of_words() {
        // a short phrase with 1 error + a long one with none ⇒ corpus WER is small,
        // even though the average over files would be 50 %
        let mut agg = Errors::default();
        agg.add(Errors::compare("а б", "а х"));
        agg.add(Errors::compare(
            "раз два три четыре пять шесть семь восемь",
            "раз два три четыре пять шесть семь восемь",
        ));
        assert_eq!(agg.ref_words, 10);
        assert_eq!(agg.word_edits, 1);
        assert_eq!(agg.wer(), 10.0);
    }

    #[test]
    fn digits_are_expanded_so_format_is_not_counted_as_error() {
        // the GigaAM hypothesis is in digits, the Golos reference is in words — recognized RIGHT
        let e = Errors::compare("две тысячи тринадцать", "2013");
        assert_eq!(e.word_edits, 0, "the number expansion did not work");
    }

    #[test]
    fn alphabet_and_ordinal_mismatches_are_flagged() {
        assert!(has_latin("YouTube Riddle Stream"));
        assert!(!has_latin("ютьюб риддл стрим"));
        assert!(has_ordinal_digits("15-й сезон"));
        assert!(!has_ordinal_digits("пятнадцатый сезон"));
    }
}
