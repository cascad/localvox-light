//! Export of a session's best transcript version (WP-A5): txt / md (Obsidian) / srt.
//!
//! Derived artifacts: written to the session root (`transcript.txt|md|srt`), overwriting
//! is safe — the source of truth stays in `transcripts/` (P2).

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::versions::{read_transcript_lines, TranscriptLine, VersionStore};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportFormat {
    Txt,
    Md,
    Srt,
}

impl std::str::FromStr for ExportFormat {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "txt" => Ok(Self::Txt),
            "md" => Ok(Self::Md),
            "srt" => Ok(Self::Srt),
            other => bail!("unknown export format: {other} (txt|md|srt)"),
        }
    }
}

impl ExportFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Txt => "txt",
            Self::Md => "md",
            Self::Srt => "srt",
        }
    }
}

/// Who said it. Diarization gives the real speaker; without it — the name of the SOURCE, which
/// depends on where the audio came from and on what the human called it (`chunks::source_label`).
fn speaker(l: &TranscriptLine, meta: &crate::chunks::SessionMeta) -> String {
    match l.speaker.as_deref() {
        Some(name) => name.to_string(),
        None => crate::chunks::source_label(meta, l.source_id),
    }
}

fn fmt_mmss(sec: f64) -> String {
    format!("{:02}:{:02}", (sec / 60.0) as u64, sec as u64 % 60)
}

fn fmt_srt(sec: f64) -> String {
    let ms = (sec * 1000.0).round() as u64;
    format!(
        "{:02}:{:02}:{:02},{:03}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}

/// Exports the session's best version; returns the path of the finished file.
pub fn export_session(session_dir: &Path, format: ExportFormat) -> Result<PathBuf> {
    let store = VersionStore::open(session_dir)?;
    let Some(best) = store.best() else {
        bail!(
            "no transcript versions in {} — cook it first (localvox-process)",
            session_dir.display()
        );
    };
    let src = store
        .resolve(best.id)
        .context("the best version file was not found")?;
    let lines = read_transcript_lines(&src)?;
    if lines.is_empty() {
        bail!("the best version is empty — there is nothing to export");
    }

    let session_name = session_dir
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    // The source names live in the session's meta: who «Я» actually is depends on where the audio
    // came from, and on what the human called it. A missing meta is not fatal here — the defaults
    // still apply; an export refusing to run over a naming detail would be the wrong trade.
    let meta: crate::chunks::SessionMeta = std::fs::read(session_dir.join("meta.json"))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let body = match format {
        ExportFormat::Txt => render_txt(&lines, &meta),
        ExportFormat::Md => render_md(&lines, &meta, &session_name, &best.label, &best.model),
        ExportFormat::Srt => render_srt(&lines, &meta),
    };
    let out = session_dir.join(format!("transcript.{}", format.extension()));
    fs::write(&out, body).with_context(|| format!("writing {}", out.display()))?;
    Ok(out)
}

fn render_txt(lines: &[TranscriptLine], meta: &crate::chunks::SessionMeta) -> String {
    let mut s = String::new();
    for l in lines {
        s.push_str(&format!(
            "[{}] ({}) {}\n",
            speaker(l, meta),
            fmt_mmss(l.start_sec),
            l.text
        ));
    }
    s
}

fn render_md(
    lines: &[TranscriptLine],
    meta: &crate::chunks::SessionMeta, session: &str, label: &str, model: &str) -> String {
    let mut s = format!("# Транскрипт: {session}\n\n> версия: {label} · модель: {model}\n\n");
    for l in lines {
        s.push_str(&format!(
            "**[{}]** `{}`\n{}\n\n",
            speaker(l, meta),
            fmt_mmss(l.start_sec),
            l.text
        ));
    }
    s
}

fn render_srt(lines: &[TranscriptLine], meta: &crate::chunks::SessionMeta) -> String {
    let mut s = String::new();
    for (i, l) in lines.iter().enumerate() {
        s.push_str(&format!(
            "{}\n{} --> {}\n[{}] {}\n\n",
            i + 1,
            fmt_srt(l.start_sec),
            fmt_srt(l.end_sec),
            speaker(l, meta),
            l.text
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::versions::{now_rfc3339, VersionEntry};
    use tempfile::tempdir;

    fn make_session(dir: &Path) {
        let store = VersionStore::open(dir).unwrap();
        let (id, path) = store.next_version("test").unwrap();
        let lines = [
            TranscriptLine {
                source_id: 0,
                start_sec: 61.5,
                end_sec: 65.0,
                text: "Привет".into(),
                speaker: None,
            },
            TranscriptLine {
                source_id: 1,
                start_sec: 3.2,
                end_sec: 10.0,
                text: "Ответ".into(),
                speaker: None,
            },
        ];
        let body: String = lines
            .iter()
            .map(|l| serde_json::to_string(l).unwrap() + "\n")
            .collect();
        fs::write(&path, body).unwrap();
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
    }

    #[test]
    fn txt_sorted_by_time_with_speakers() {
        let dir = tempdir().unwrap();
        make_session(dir.path());
        let out = export_session(dir.path(), ExportFormat::Txt).unwrap();
        let text = fs::read_to_string(out).unwrap();
        let first = text.lines().next().unwrap();
        assert!(first.starts_with("[Собеседники] (00:03)"), "{first}");
        assert!(text.contains("[Я] (01:01) Привет"));
    }

    #[test]
    fn srt_has_valid_timecodes_and_numbering() {
        let dir = tempdir().unwrap();
        make_session(dir.path());
        let out = export_session(dir.path(), ExportFormat::Srt).unwrap();
        let text = fs::read_to_string(out).unwrap();
        assert!(text.starts_with("1\n00:00:03,200 --> 00:00:10,000\n"));
        assert!(text.contains("2\n00:01:01,500 --> 00:01:05,000\n[Я] Привет"));
    }

    #[test]
    fn md_contains_header_and_provenance() {
        let dir = tempdir().unwrap();
        make_session(dir.path());
        let out = export_session(dir.path(), ExportFormat::Md).unwrap();
        let text = fs::read_to_string(out).unwrap();
        assert!(text.starts_with("# Транскрипт:"));
        assert!(text.contains("версия: test · модель: m"));
    }

    #[test]
    fn export_without_versions_fails_clearly() {
        let dir = tempdir().unwrap();
        let err = export_session(dir.path(), ExportFormat::Txt).unwrap_err();
        assert!(err.to_string().contains("no transcript versions"));
    }
}
