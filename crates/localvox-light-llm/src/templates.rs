//! Prompt templates are files (F1). The defaults are baked into the binary; user
//! templates are placed in the `templates/` directory (env `LOCALVOX_LLM_TEMPLATES_DIR`)
//! and override the baked-in ones by name. Placeholders: `{{transcript}}`, `{{glossary}}`.

use std::fs;
use std::path::Path;

use anyhow::{bail, Result};

const SUMMARY_RU: &str = include_str!("../templates/summary-ru.md");
const CLEANUP_RU: &str = include_str!("../templates/cleanup-ru.md");
const VIDEO_NOTES_RU: &str = include_str!("../templates/video-notes-ru.md");
/// A short recording (a thought out loud, a quick call): a 1–3 sentence digest instead
/// of a summary. A layout with an «Action items» section provokes the model to invent them.
const NOTE_RU: &str = include_str!("../templates/note-ru.md");

/// Standup: broken down BY SPEAKER, not by topic.
const STANDUP_RU: &str = include_str!("../templates/standup-ru.md");
/// One-on-one: here they talk about PEOPLE, and an invented detail is not a mistake
/// but machine-generated gossip. The rule about inventions is strictest here.
const ONE_ON_ONE_RU: &str = include_str!("../templates/one-on-one-ru.md");

const SUMMARY_EN: &str = include_str!("../templates/summary-en.md");
const CLEANUP_EN: &str = include_str!("../templates/cleanup-en.md");
const NOTE_EN: &str = include_str!("../templates/note-en.md");

/// A question to the archive. It has its own placeholders — `{{question}}` and
/// `{{fragments}}`: this is not the processing of a single recording, but an answer
/// BASED ON FOUND fragments of many.
const CHAT_RU: &str = include_str!("../templates/chat-ru.md");
const CHAT_EN: &str = include_str!("../templates/chat-en.md");

/// Template text by name: first the file `<dir>/<name>.md`, then the baked-in default.
pub fn load(name: &str, dir: Option<&Path>) -> Result<String> {
    if let Some(dir) = dir {
        let p = dir.join(format!("{name}.md"));
        if p.exists() {
            return Ok(fs::read_to_string(&p)?);
        }
    }
    match name {
        "summary-ru" => Ok(SUMMARY_RU.to_string()),
        "cleanup-ru" => Ok(CLEANUP_RU.to_string()),
        "video-notes-ru" => Ok(VIDEO_NOTES_RU.to_string()),
        "note-ru" => Ok(NOTE_RU.to_string()),
        "standup-ru" => Ok(STANDUP_RU.to_string()),
        "one-on-one-ru" => Ok(ONE_ON_ONE_RU.to_string()),
        "summary-en" => Ok(SUMMARY_EN.to_string()),
        "cleanup-en" => Ok(CLEANUP_EN.to_string()),
        "note-en" => Ok(NOTE_EN.to_string()),
        "chat-ru" => Ok(CHAT_RU.to_string()),
        "chat-en" => Ok(CHAT_EN.to_string()),
        other => bail!(
            "template «{other}» not found (neither a file nor a baked-in one); baked-in: \
             summary-ru, cleanup-ru, video-notes-ru, note-ru, summary-en, cleanup-en, note-en"
        ),
    }
}

/// A template for the LANGUAGE of the recording: `<base>-<lang>`, and if there is
/// no such one — English.
///
/// English as the fallback is not an imperial habit but a property of the template: it
/// tells the model to answer IN THE LANGUAGE OF THE TRANSCRIPT. So for a language we
/// have no template for, the answer will still come in the right language — the model
/// will just read the instructions in English. Silently slipping in the Russian template
/// would be worse: it tells the model to «write in Russian».
///
/// Returns the name (for the log) and the text.
pub fn for_lang(base: &str, lang: &str, dir: Option<&Path>) -> Result<(String, String)> {
    let name = format!("{base}-{lang}");
    if let Ok(text) = load(&name, dir) {
        return Ok((name, text));
    }
    let fallback = format!("{base}-en");
    let text = load(&fallback, dir).map_err(|e| {
        anyhow::anyhow!("no template «{name}», and no fallback «{fallback}» either: {e}")
    })?;
    tracing::info!(
        "there is no template «{name}» — taking «{fallback}» (it answers in the \
         language of the recording)"
    );
    Ok((fallback, text))
}

pub fn render(template: &str, transcript: &str, glossary_block: &str) -> String {
    template
        .replace("{{transcript}}", transcript)
        .replace("{{glossary}}", glossary_block)
}

/// Substitution of arbitrary placeholders: a question to the archive has its own.
///
/// We substitute IN ONE PASS, not one placeholder after another: the transcript text
/// may itself contain something that looks like `{{...}}` (a person said it, the ASR
/// wrote it down), and the second pass would substitute into it. This is not paranoia
/// but prompt injection through the microphone.
pub fn fill(template: &str, vars: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len() + 512);
    let mut rest = template;
    'outer: while let Some(at) = rest.find("{{") {
        out.push_str(&rest[..at]);
        let tail = &rest[at..];
        for (name, value) in vars {
            let token = format!("{{{{{name}}}}}");
            if let Some(after) = tail.strip_prefix(token.as_str()) {
                out.push_str(value);
                rest = after;
                continue 'outer;
            }
        }
        // An unknown placeholder is left as is: silently eating a piece of the template
        // is worse than showing it to a human.
        out.push_str("{{");
        rest = &tail[2..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_templates_have_placeholders() {
        // ALL the load-bearing templates, not just two: note-ru and video-notes-ru
        // ship to prod as well, and a template without {{transcript}} is a prompt
        // without a transcript.
        for name in [
            "summary-ru",
            "cleanup-ru",
            "note-ru",
            "video-notes-ru",
            "standup-ru",
            "one-on-one-ru",
            "summary-en",
            "cleanup-en",
            "note-en",
        ] {
            let t = load(name, None).unwrap();
            assert!(t.contains("{{transcript}}"), "{name} without transcript");
            assert!(t.contains("{{glossary}}"), "{name} without glossary");
        }
    }

    #[test]
    fn an_unknown_language_falls_back_to_english_not_to_russian() {
        // The Russian template tells the model to «write in Russian» — slipping it to a
        // German recording means silently answering in the wrong language. The English
        // one tells the model to answer IN THE LANGUAGE OF THE TRANSCRIPT, which is why
        // it is the fallback.
        let (name, text) = for_lang("summary", "de", None).unwrap();
        assert_eq!(name, "summary-en");
        assert!(text.contains("SAME LANGUAGE"));

        let (name, _) = for_lang("summary", "ru", None).unwrap();
        assert_eq!(name, "summary-ru");
    }

    #[test]
    fn a_user_template_wins_over_the_builtin_one_for_its_language() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("summary-de.md"), "deutsch {{transcript}}").unwrap();
        let (name, text) = for_lang("summary", "de", Some(dir.path())).unwrap();
        assert_eq!(name, "summary-de");
        assert!(text.starts_with("deutsch"));
    }

    #[test]
    fn file_overrides_builtin() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("summary-ru.md"), "custom {{transcript}}").unwrap();
        let t = load("summary-ru", Some(dir.path())).unwrap();
        assert!(t.starts_with("custom"));
    }

    #[test]
    fn render_substitutes_all() {
        let out = render("a {{glossary}} b {{transcript}}", "T", "G");
        assert_eq!(out, "a G b T");
    }

    /// Substitution runs in ONE pass. Otherwise the text substituted first would be
    /// scanned by the second one — and a person who said «curly brace fragments» out
    /// loud would get someone else's data into the prompt. Injection via the microphone.
    #[test]
    fn a_value_is_not_scanned_for_further_placeholders() {
        let out = fill(
            "В: {{question}}
Ф: {{fragments}}",
            &[("question", "что такое {{fragments}}?"), ("fragments", "текст")],
        );
        assert_eq!(out, "В: что такое {{fragments}}?
Ф: текст");
    }

    #[test]
    fn an_unknown_placeholder_survives_instead_of_being_eaten() {
        assert_eq!(fill("a {{nope}} b", &[("x", "1")]), "a {{nope}} b");
    }

    #[test]
    fn chat_templates_have_their_own_placeholders() {
        for name in ["chat-ru", "chat-en"] {
            let t = load(name, None).unwrap();
            assert!(t.contains("{{question}}"), "{name} without a question");
            assert!(t.contains("{{fragments}}"), "{name} without fragments");
        }
    }
}
