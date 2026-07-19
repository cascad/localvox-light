//! The provenance header of a derived document — one line at the top of every summary and every
//! readable text saying what produced it: template, model, glossary replacements, when.
//!
//! Derived data is disposable BECAUSE it is reproducible, and it is only reproducible if you can
//! tell what produced it. Without this line two summaries of the same recording are
//! indistinguishable, and neither can be trusted over the other.
//!
//! Written here and read here. The format is not up for redesign: files carrying it are already
//! lying in the archive, and a reader that only understood a new format would report the old ones
//! as having no provenance at all — which is a lie about a file that has it.

use serde::{Deserialize, Serialize};

/// What marks the line as ours rather than an author's own HTML comment.
const MARK: &str = "localvox:";
const MODEL: &str = "модель ";
const GLOSSARY: &str = "глоссарий:";
const DOUBTS: &str = "стоит перепроверить:";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Provenance {
    /// The prompt template — `summary-ru`, `cleanup-lines`. Shown as written: it names a file in
    /// the repository, and translating it would send a person looking for something that is not
    /// there.
    pub template: String,
    pub model: String,
    /// How many glossary replacements were applied. Zero is an answer, not a missing value.
    pub glossary: usize,
    /// RFC3339, exactly as the writer stamped it — with its offset.
    pub at: String,
    /// What the grounding check doubted. The app shows it separately (with the "it is fine"
    /// button); it lives here because the header is what a person sees when they open the file
    /// outside the app.
    pub doubts: Option<String>,
}

/// The header line plus the blank line after it — the whole prefix of the file.
pub fn render(
    template: &str,
    model: &str,
    replacements: usize,
    at: &str,
    doubts: Option<&str>,
) -> String {
    let doubts = doubts
        .map(|d| format!(" | {DOUBTS} {d}"))
        .unwrap_or_default();
    format!("<!-- {MARK} {template} | {MODEL}{model} | {GLOSSARY} {replacements} замен | {at}{doubts} -->\n\n")
}

/// Take the header off a document, if it has one.
///
/// Returns the body UNCHANGED when there is no header: a file written before this line existed,
/// or one a person edited by hand, is still a document. Losing its first paragraph to an
/// over-eager parser would be far worse than showing no provenance.
pub fn split(text: &str) -> (Option<Provenance>, &str) {
    let (first, rest) = text.split_once('\n').unwrap_or((text, ""));
    match parse(first) {
        Some(p) => (Some(p), rest.trim_start_matches('\n')),
        None => (None, text),
    }
}

/// Parse one header line. Fields are found BY LABEL, not by position: a header written by an
/// older build may be missing one, and that must cost only that field.
pub fn parse(line: &str) -> Option<Provenance> {
    let body = line.trim().strip_prefix("<!--")?.strip_suffix("-->")?.trim();
    let body = body.strip_prefix(MARK)?.trim();

    // The doubts come last and are free text — they may well contain a `|` themselves, so they
    // are cut off before anything is split on it.
    let (head, doubts) = match body.split_once(DOUBTS) {
        Some((h, d)) => (h.trim_end().trim_end_matches('|').trim_end(), Some(d.trim())),
        None => (body, None),
    };

    let mut out = Provenance {
        doubts: doubts.filter(|d| !d.is_empty()).map(str::to_owned),
        ..Default::default()
    };

    for (i, part) in head.split('|').map(str::trim).enumerate() {
        if let Some(m) = part.strip_prefix(MODEL) {
            out.model = m.trim().to_owned();
        } else if let Some(g) = part.strip_prefix(GLOSSARY) {
            // "0 замен" — the count, whatever word follows it.
            out.glossary = g
                .trim()
                .split_whitespace()
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0);
        } else if i == 0 {
            out.template = part.to_owned();
        } else if out.at.is_empty() && looks_like_timestamp(part) {
            out.at = part.to_owned();
        }
    }

    // A header with nothing readable in it is not a header. Reporting an empty Provenance would
    // put an empty plaque on the screen and call it a fact.
    (!out.template.is_empty() || !out.model.is_empty()).then_some(out)
}

/// `2026-07-18T23:21:11…` — enough to tell the stamp from a field we do not know.
fn looks_like_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() >= 10 && b[..4].iter().all(u8::is_ascii_digit) && b[4] == b'-'
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What is lying in the owner's archive right now. If this ever fails, existing summaries
    /// have lost their provenance.
    const REAL: &str =
        "<!-- localvox: summary-ru | модель qwen3.5:9b | глоссарий: 0 замен | 2026-07-18T23:21:11.191898300+03:00 -->";

    #[test]
    fn reads_a_header_from_the_archive() {
        let p = parse(REAL).expect("the header on disk did not parse");
        assert_eq!(p.template, "summary-ru");
        assert_eq!(p.model, "qwen3.5:9b");
        assert_eq!(p.glossary, 0);
        assert_eq!(p.at, "2026-07-18T23:21:11.191898300+03:00");
        assert_eq!(p.doubts, None);
    }

    #[test]
    fn what_we_write_is_what_we_read() {
        let text = render("cleanup-lines", "granite4.1:8b", 12, "2026-07-19T10:00:00+03:00", None);
        let doc = text + "# Заголовок\n";
        let (p, body) = split(&doc);
        let p = p.expect("our own header did not parse");
        assert_eq!(p.template, "cleanup-lines");
        assert_eq!(p.model, "granite4.1:8b");
        assert_eq!(p.glossary, 12);
        assert_eq!(body, "# Заголовок\n", "the body lost text to the parser");
    }

    /// The doubt text is free-form and has every right to contain a pipe. Splitting on `|` first
    /// would chop it in half and hide the tail.
    #[test]
    fn doubts_survive_a_pipe_inside_them() {
        let line = render(
            "summary-ru",
            "qwen3.5:9b",
            0,
            "2026-07-19T10:00:00+03:00",
            Some("имена/названия: сергей | пётр"),
        );
        let p = parse(line.lines().next().unwrap()).unwrap();
        assert_eq!(p.doubts.as_deref(), Some("имена/названия: сергей | пётр"));
        assert_eq!(p.model, "qwen3.5:9b", "the doubts ate the fields before them");
    }

    /// A document without a header is still a document, and its first line is still its text.
    #[test]
    fn a_document_without_a_header_keeps_every_line() {
        let doc = "# Сводка\n\nПервый абзац.\n";
        let (p, body) = split(doc);
        assert!(p.is_none());
        assert_eq!(body, doc);
    }

    /// Someone else's comment is not our header. Swallowing it would delete a line the author
    /// wrote.
    #[test]
    fn a_foreign_comment_is_not_ours() {
        assert!(parse("<!-- заметка автора -->").is_none());
        let doc = "<!-- заметка автора -->\n\nтекст\n";
        assert_eq!(split(doc).1, doc);
    }

    /// An older build may not have written every field. Missing one costs that field only.
    #[test]
    fn a_partial_header_gives_up_only_the_missing_field() {
        let p = parse("<!-- localvox: summary-ru | модель qwen3.5:9b -->").unwrap();
        assert_eq!(p.template, "summary-ru");
        assert_eq!(p.model, "qwen3.5:9b");
        assert_eq!(p.at, "", "a timestamp was invented out of nothing");
    }

    #[test]
    fn an_empty_header_is_not_a_fact() {
        assert!(parse("<!-- localvox: -->").is_none());
    }
}
