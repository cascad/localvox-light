//! Dictionary evidence, not a sentence parser. Never predict an analysis for an unknown word.
use rs_morphy4::{analysis::DictionaryAnalyzer, dictionary::DictionaryData, Parse, Tag};
use std::sync::OnceLock;

fn dictionary() -> Option<&'static DictionaryAnalyzer> {
    static DICTIONARY: OnceLock<Result<DictionaryAnalyzer, String>> = OnceLock::new();
    DICTIONARY
        .get_or_init(|| {
            DictionaryAnalyzer::from_data(DictionaryData {
                prediction_suffixes: None,
                ..DictionaryData::bundled()
            })
            .map_err(|error| {
                tracing::error!(%error, "Russian correction dictionary unavailable; word changes refused");
                error.to_string()
            })
        })
        .as_ref()
        .ok()
}

fn russian(word: &str) -> bool {
    !word.is_empty() && word.chars().all(|c| ('а'..='я').contains(&c) || c == 'ё')
}

/// Other alphabets retain the existing admission rules; this dictionary proves nothing for them.
pub(super) fn admits(source: &str, candidate: &str, subject: Option<&str>) -> bool {
    if source == candidate || !russian(source) || !russian(candidate) {
        return true;
    }
    let Some(dictionary) = dictionary() else {
        return false;
    };
    let (Ok(before), Ok(after)) = (
        dictionary.parse_with_yo(source),
        dictionary.parse_with_yo(candidate),
    ) else {
        return false;
    };
    if after.is_empty() {
        return false;
    }
    if before.is_empty() {
        return true;
    }
    let Some(subject) = subject.filter(|word| russian(word)) else {
        return false;
    };
    let Ok(subject) = dictionary.parse_with_yo(subject) else {
        return false;
    };
    past_agreement(&before, &after, &subject)
}

fn agrees(verb: &Tag, subject: &Tag) -> bool {
    verb.number().is_some()
        && verb.number() == subject.number()
        && (verb.number() == Some("plur")
            || (verb.gender().is_some() && verb.gender() == subject.gender()))
}

fn stable_features(tag: &Tag) -> impl Iterator<Item = &str> {
    tag.grammemes
        .iter()
        .map(String::as_str)
        .filter(|g| !matches!(*g, "masc" | "femn" | "neut" | "sing" | "plur"))
}

fn past_agreement(before: &[Parse], after: &[Parse], subject: &[Parse]) -> bool {
    let Some(first) = before.first() else {
        return false;
    };
    // An alternative object/interjection/etc. reading is ambiguity, even if its rank is lower.
    if subject.is_empty()
        || !subject
            .iter()
            .all(|p| matches!(p.tag.pos.as_str(), "NOUN" | "NPRO") && p.tag.case() == Some("nomn"))
        || !before.iter().chain(after).all(|p| {
            p.tag.pos == "VERB"
                && p.tag.tense() == Some("past")
                && p.tag.contains("indc")
                && p.normal_form == first.normal_form
                && stable_features(&p.tag).eq(stable_features(&first.tag))
        })
    {
        return false;
    }
    !after.is_empty()
        && subject.iter().all(|s| {
            before.iter().all(|v| !agrees(&v.tag, &s.tag))
                && after.iter().all(|v| agrees(&v.tag, &s.tag))
        })
}
