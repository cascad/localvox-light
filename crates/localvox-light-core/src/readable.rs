//! The readable text — as DATA, not as a page.
//!
//! What was said is a fact and does not change: it lives in the transcript version, which is
//! immutable. Who said it is a JUDGEMENT and changes — diarization names a voice, a human renames
//! a source, a downloaded video turns out not to be the owner speaking. Mixing the two into one
//! markdown file is what made renaming a speaker require re-cooking the document through an LLM:
//! the name had been baked into the text at cook time.
//!
//! So this artifact stores neither the speech nor the name. It stores ONLY what the cleanup did:
//!
//! ```json
//! {"version_id": 2, "edits": {"17": "причёсанная формулировка"}, …}
//! ```
//!
//! A delta against a pinned transcript version. Everything else — the words, the timecodes, the
//! speaker — is joined in at render time from the sources that own it. Rename a source and the
//! readable text follows on the next read, because there is nothing in the file to be stale.
//!
//! It also answers a question the old file could not: WHICH lines the model touched. An unedited
//! line is simply absent from the delta.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::chunks::{source_label, SessionMeta};
use crate::provenance::Provenance;
use crate::versions::{read_transcript_lines, VersionStore};

pub const FILE: &str = "processed.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Readable {
    /// The transcript version this was built from. Pinned, not "the best one": versions are
    /// immutable, but WHICH one is best can be switched by a human, and then the line numbers
    /// below would point at other people's words.
    pub version_id: u32,
    pub provenance: Provenance,
    /// Line index (0-based, into the version) → the cleaned wording.
    ///
    /// Sparse ON PURPOSE. A line the model left alone, or whose cleaning the grounding check
    /// refused, is not here at all — and its absence is the record that nothing happened to it.
    /// Copying the unchanged speech in here would duplicate the one thing in this system that is
    /// not allowed to have two copies.
    #[serde(default)]
    pub edits: BTreeMap<usize, String>,
    /// Lines sent to the model that never came back — it swallowed the batch. Not the same as
    /// "needed no cleaning", and the difference is the honest measure of how much work happened.
    #[serde(default)]
    pub omitted: usize,
    /// Lines the model answered for and the grounding check refused (it added or dropped
    /// something). The original stands in the document; this counts how often that happened.
    #[serde(default)]
    pub rejected: usize,
}

/// One line of the readable text, ready for any client to draw however it likes.
#[derive(Debug, Clone, Serialize)]
pub struct ReadableLine {
    /// Resolved at READ time: diarization's name, else the human's name for the source, else what
    /// the source honestly is. Never stored.
    pub who: String,
    pub source_id: u8,
    pub start_sec: f64,
    pub end_sec: f64,
    /// What the reader reads — the cleaned wording if there is one, the transcript otherwise.
    pub text: String,
    /// The recognizer's own words, present only when the cleanup changed this line. This is how a
    /// person checks the model instead of trusting it.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub original: Option<String>,
}

impl ReadableLine {
    pub fn edited(&self) -> bool {
        self.original.is_some()
    }
}

pub fn path(session_dir: &Path) -> PathBuf {
    session_dir.join(FILE)
}

pub fn exists(session_dir: &Path) -> bool {
    path(session_dir).is_file()
}

pub fn save(session_dir: &Path, r: &Readable) -> Result<PathBuf> {
    let p = path(session_dir);
    let body = serde_json::to_vec_pretty(r)?;
    std::fs::write(&p, body).with_context(|| format!("writing {}", p.display()))?;
    Ok(p)
}

pub fn load(session_dir: &Path) -> Result<Readable> {
    let p = path(session_dir);
    let body = std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?;
    serde_json::from_slice(&body).with_context(|| format!("parsing {}", p.display()))
}

/// The readable text, joined: the delta over the version it names, wearing today's speaker names.
///
/// Reads the version the artifact PINS, not the best one. If a human switched the working version
/// the two disagree — and answering from the wrong one would silently show words the cleanup never
/// saw, under line numbers that mean something else.
pub fn lines(session_dir: &Path) -> Result<Vec<ReadableLine>> {
    let r = load(session_dir)?;
    let store = VersionStore::open(session_dir)?;
    let src = store
        .resolve(r.version_id)
        .with_context(|| format!("версия v{} не найдена", r.version_id))?;
    let transcript = read_transcript_lines(&src)?;
    let meta: SessionMeta = std::fs::read(session_dir.join("meta.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();

    Ok(transcript
        .into_iter()
        .enumerate()
        .map(|(i, l)| {
            let cleaned = r.edits.get(&i);
            ReadableLine {
                who: match l.speaker {
                    Some(name) => name,
                    None => source_label(&meta, l.source_id),
                },
                source_id: l.source_id,
                start_sec: l.start_sec,
                end_sec: l.end_sec,
                text: cleaned.cloned().unwrap_or_else(|| l.text.clone()),
                original: cleaned.map(|_| l.text),
            }
        })
        .collect())
}

/// One rendering among several — markdown, for export and for the clipboard. The markup is OURS:
/// the model supplies wordings, never structure.
pub fn render_markdown(lines: &[ReadableLine]) -> String {
    let mut out = String::new();
    for l in lines {
        let mins = (l.start_sec / 60.0) as u64;
        let secs = l.start_sec as u64 % 60;
        out.push_str(&format!("[{}] ({mins:02}:{secs:02}) {}\n", l.who, l.text));
    }
    out
}

/// The speech alone, for the search index: labels and timecodes are not what anyone searches for,
/// and «(00:15)» would match the query «15».
pub fn render_speech(lines: &[ReadableLine]) -> String {
    let mut out = String::new();
    for l in lines {
        out.push_str(&l.text);
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::{now_rfc3339, TranscriptLine, VersionEntry, VersionStore};

    /// Writes a transcript version and returns its id.
    fn version(dir: &Path, lines: &[TranscriptLine]) -> u32 {
        let store = VersionStore::open(dir).unwrap();
        let (id, path) = store.next_version("test").unwrap();
        let body: String = lines
            .iter()
            .map(|l| serde_json::to_string(l).unwrap() + "\n")
            .collect();
        std::fs::write(&path, body).unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "test".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: "m".into(),
                params: serde_json::json!({}),
                created_at: now_rfc3339(),
                parents: vec![],
            })
            .unwrap();
        id
    }

    /// A session with two transcript lines, plus the readable delta over them.
    fn fixture(dir: &Path, edits: BTreeMap<usize, String>) {
        let id = version(
            dir,
            &[
                TranscriptLine {
                    source_id: 0,
                    start_sec: 0.0,
                    end_sec: 3.0,
                    text: "ну эээ я говорю что надо".into(),
                    speaker: None,
                },
                TranscriptLine {
                    source_id: 1,
                    start_sec: 3.0,
                    end_sec: 6.0,
                    text: "согласен".into(),
                    speaker: None,
                },
            ],
        );
        save(
            dir,
            &Readable {
                version_id: id,
                edits,
                ..Default::default()
            },
        )
        .unwrap();
    }

    #[test]
    fn an_edited_line_carries_its_original() {
        let dir = tempfile::tempdir().unwrap();
        let mut edits = BTreeMap::new();
        edits.insert(0, "Я говорю, что надо.".to_string());
        fixture(dir.path(), edits);

        let ls = lines(dir.path()).unwrap();
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0].text, "Я говорю, что надо.");
        assert_eq!(ls[0].original.as_deref(), Some("ну эээ я говорю что надо"));
        assert!(ls[0].edited());

        // Untouched: the transcript's own words, and no `original` — there is nothing to compare.
        assert_eq!(ls[1].text, "согласен");
        assert!(!ls[1].edited(), "an untouched line was reported as cleaned");
    }

    /// THE POINT OF THE WHOLE ARTIFACT. Renaming a source must reach the readable text without
    /// the file being touched at all — no LLM, no re-cook.
    #[test]
    fn renaming_a_source_reaches_the_text_without_rewriting_it() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path(), BTreeMap::new());

        assert_eq!(lines(dir.path()).unwrap()[0].who, "Я");
        let before = std::fs::read(path(dir.path())).unwrap();

        let mut meta = SessionMeta::default();
        meta.source_names.insert("0".into(), "Арсен Маркарян".into());
        std::fs::write(
            dir.path().join("meta.json"),
            serde_json::to_vec(&meta).unwrap(),
        )
        .unwrap();

        assert_eq!(lines(dir.path()).unwrap()[0].who, "Арсен Маркарян");
        assert_eq!(
            std::fs::read(path(dir.path())).unwrap(),
            before,
            "the readable artifact was rewritten — the name is supposed to live outside it"
        );
    }

    /// Diarization's answer outranks the source label: it knows WHO, not merely where the sound
    /// came from.
    #[test]
    fn a_named_voice_wins_over_the_source() {
        let dir = tempfile::tempdir().unwrap();
        let id = version(
            dir.path(),
            &[TranscriptLine {
                source_id: 1,
                start_sec: 0.0,
                end_sec: 1.0,
                text: "да".into(),
                speaker: Some("Иван".into()),
            }],
        );
        save(
            dir.path(),
            &Readable {
                version_id: id,
                ..Default::default()
            },
        )
        .unwrap();

        assert_eq!(lines(dir.path()).unwrap()[0].who, "Иван");
    }

    /// The delta names its version. If that version is gone, we say so instead of quietly
    /// rendering the delta over whatever transcript happens to be lying around — the line numbers
    /// would point at other people's words.
    #[test]
    fn a_missing_version_is_an_error_not_a_guess() {
        let dir = tempfile::tempdir().unwrap();
        save(
            dir.path(),
            &Readable {
                version_id: 42,
                ..Default::default()
            },
        )
        .unwrap();
        let err = lines(dir.path()).unwrap_err().to_string();
        assert!(err.contains("42"), "the error does not name the version: {err}");
    }

    #[test]
    fn markdown_is_a_rendering_of_the_join() {
        let dir = tempfile::tempdir().unwrap();
        let mut edits = BTreeMap::new();
        edits.insert(0, "Я говорю, что надо.".to_string());
        fixture(dir.path(), edits);
        let md = render_markdown(&lines(dir.path()).unwrap());
        assert_eq!(md, "[Я] (00:00) Я говорю, что надо.\n[Собеседники] (00:03) согласен\n");
    }
}
