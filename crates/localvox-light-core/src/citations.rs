//! Which lines of the recording a claim came from.
//!
//! A summary is the one artifact a person cannot check by reading it: the readable text stands
//! beside the recording line by line, but a summary is prose about half an hour of speech, and
//! «worth double-checking» over the whole document tells them only that something, somewhere,
//! may be wrong. Every check we have is document-level, so every doubt is too.
//!
//! So each claim carries the line numbers it was drawn from, written by the cook as `[[12,15]]`
//! at the end of the line. The numbers are the model's answer to «where did you get this», and
//! that answer is USELESS UNTIL VERIFIED — a model asked to cite will happily produce plausible
//! numbers. So we parse them ourselves, check that the lines exist, and check that the claim and
//! its sources actually share content. What survives becomes a play button next to the claim;
//! what does not gets marked, not deleted.
//!
//! The markers stay in the file. They are provenance, not decoration: a person opening
//! `summary.md` in a text editor should see where a claim came from, and the app strips them for
//! display and for the clipboard.

use serde::Serialize;

/// `[[12,15]]` — ours, never the model's prose. Double brackets because a single one is already
/// taken by markdown links and by the transcript's own `[Я]` labels.
const OPEN: &str = "[[";
const CLOSE: &str = "]]";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Heading,
    Bullet,
    Para,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Block {
    pub kind: Kind,
    /// What a person reads — the citation markers already removed.
    pub text: String,
    /// Line indices into the transcript version, in the order the model gave them. Empty means
    /// the claim cites nothing, which for a bullet is itself worth showing.
    pub lines: Vec<usize>,
}

/// Split a summary into blocks, taking the citation markers out of the text.
///
/// The parse is deliberately dumb: headings, bullets, everything else is a paragraph. It has to
/// survive whatever markdown the model produced, and a parser that demanded a shape would throw
/// away documents for having an extra blank line.
pub fn parse(markdown: &str) -> Vec<Block> {
    let mut out = Vec::new();
    for raw in markdown.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        let (text, lines) = split_marker(line);
        let text = text.trim();
        if text.is_empty() && lines.is_empty() {
            continue;
        }
        let (kind, body) = if let Some(rest) = text.strip_prefix("## ") {
            (Kind::Heading, rest)
        } else if let Some(rest) = text.strip_prefix("# ") {
            (Kind::Heading, rest)
        } else if let Some(rest) = text
            .strip_prefix("* ")
            .or_else(|| text.strip_prefix("- "))
            .or_else(|| text.strip_prefix("• "))
        {
            (Kind::Bullet, rest)
        } else {
            (Kind::Para, text)
        };
        out.push(Block {
            kind,
            text: body.trim().to_string(),
            lines,
        });
    }
    out
}

/// Cut `[[12, 15]]` off the end of a line and return the numbers.
///
/// Anywhere in the line, not only at the end: models put it before the final full stop about as
/// often as after it. Everything that is not a number inside the brackets is ignored rather than
/// making the whole marker invalid — `[[12, 15 и 16]]` should still yield three sources.
fn split_marker(line: &str) -> (String, Vec<usize>) {
    let Some(open) = line.rfind(OPEN) else {
        return (line.to_string(), Vec::new());
    };
    let after = &line[open + OPEN.len()..];
    let Some(close) = after.find(CLOSE) else {
        return (line.to_string(), Vec::new());
    };
    let inside = &after[..close];
    let lines: Vec<usize> = inside
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<usize>().ok())
        .collect();
    if lines.is_empty() {
        // Not a citation — someone's brackets. Leave the line alone rather than eating it.
        return (line.to_string(), Vec::new());
    }
    // The seam: cutting «500 [[3]] евро» out of the middle leaves two spaces where the marker
    // was. One space, or none if the marker was hugging punctuation.
    let head = line[..open].trim_end();
    let tail = after[close + CLOSE.len()..].trim_start();
    let mut text = String::with_capacity(line.len());
    text.push_str(head);
    if !head.is_empty() && !tail.is_empty() && !tail.starts_with(|c: char| c.is_ascii_punctuation())
    {
        text.push(' ');
    }
    text.push_str(tail);
    (text, lines)
}

/// Render blocks back to markdown, with the markers restored. This is what goes into the file.
pub fn render(blocks: &[Block]) -> String {
    let mut out = String::new();
    for b in blocks {
        let cite = if b.lines.is_empty() {
            String::new()
        } else {
            format!(
                " {OPEN}{}{CLOSE}",
                b.lines
                    .iter()
                    .map(|n| n.to_string())
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        match b.kind {
            Kind::Heading => out.push_str(&format!("\n## {}\n\n", b.text)),
            Kind::Bullet => out.push_str(&format!("* {}{cite}\n", b.text)),
            Kind::Para => out.push_str(&format!("{}{cite}\n\n", b.text)),
        }
    }
    out.trim_start().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_citation_is_taken_out_of_the_text() {
        let b = parse("* Из 17 000 евро собрали лишь 500. [[12,15]]");
        assert_eq!(b.len(), 1);
        assert_eq!(b[0].kind, Kind::Bullet);
        assert_eq!(b[0].text, "Из 17 000 евро собрали лишь 500.");
        assert_eq!(b[0].lines, vec![12, 15]);
    }

    /// Models put the marker before the final stop about as often as after it.
    #[test]
    fn a_marker_in_the_middle_is_still_a_marker() {
        let b = parse("* Собрали лишь 500 [[3]] евро.");
        assert_eq!(b[0].text, "Собрали лишь 500 евро.");
        assert_eq!(b[0].lines, vec![3]);
    }

    /// Sloppy contents are salvaged rather than dropped: three sources are three sources.
    #[test]
    fn junk_inside_the_brackets_does_not_lose_the_numbers() {
        let b = parse("* Что-то. [[12, 15 и 16]]");
        assert_eq!(b[0].lines, vec![12, 15, 16]);
    }

    /// Brackets that are not a citation belong to the text. Eating them would silently edit
    /// somebody's words.
    #[test]
    fn brackets_without_numbers_are_left_alone() {
        let line = "* Он сказал [[неразборчиво]] и ушёл.";
        let b = parse(line);
        assert_eq!(b[0].text, "Он сказал [[неразборчиво]] и ушёл.");
        assert!(b[0].lines.is_empty());
    }

    #[test]
    fn headings_bullets_and_paragraphs_are_told_apart() {
        let b = parse("## О чём запись\n\nДва предложения. [[0]]\n\n## Главное\n* Тезис. [[4]]");
        assert_eq!(b.len(), 4);
        assert_eq!(b[0].kind, Kind::Heading);
        assert_eq!(b[0].text, "О чём запись");
        assert_eq!(b[1].kind, Kind::Para);
        assert_eq!(b[1].lines, vec![0]);
        assert_eq!(b[3].kind, Kind::Bullet);
        assert_eq!(b[3].lines, vec![4]);
    }

    /// What is written to the file must parse back to what was written — otherwise the citations
    /// survive one cook and vanish on the next read.
    #[test]
    fn render_and_parse_are_inverse() {
        let src = "## О чём запись\n\nПересказ. [[0,1]]\n\n## Главное\n* Первый тезис. [[3]]\n* Второй. [[7,9]]";
        let blocks = parse(src);
        let back = parse(&render(&blocks));
        assert_eq!(blocks, back);
    }
}
