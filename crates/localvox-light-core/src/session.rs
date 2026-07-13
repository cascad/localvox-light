//! The workspace directory (`LOCALVOX_LIGHT_AUDIO_DIR`): WAV segments and `transcript.jsonl`
//! live in its root. No separate `session_*` subdirectories are created — every run continues
//! in the same directory. Resetting the transcript: [x] in the TUI, or editing/deleting the
//! files by hand.

use std::fs;
use std::path::{Path, PathBuf};

use crate::transcript::TranscriptWriter;

/// Exclusive lock on the workspace directory: one writing process per work_dir.
/// `Ok(Some(file))` — keep it alive until the end of the run; `Ok(None)` — the directory
/// is taken by another instance. Without this guard a second instance would "recover"
/// the first one's live `.part` chunks (std opens files with FILE_SHARE_DELETE —
/// a rename succeeds even under an open writer).
pub fn try_instance_lock(workspace: &Path) -> std::io::Result<Option<fs::File>> {
    fs::create_dir_all(workspace)?;
    let f = fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(workspace.join(".localvox.lock"))?;
    match f.try_lock() {
        Ok(()) => Ok(Some(f)),
        Err(std::fs::TryLockError::WouldBlock) => Ok(None),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

/// Sweeping empty session directories (WP-A1): a run with no sound leaves
/// `sessions/<ts>/audio` without a single file and a meta without chunks — garbage.
/// A directory with ANY valuable content (audio, transcripts, chunks or meetings in
/// meta) is left alone; known housekeeping files (`.cook.lock`, `meta.json.tmp`) do not
/// count as value. `grace` — do not touch directories younger than this (a parallel
/// ingest may have just created the session and not yet written its first chunk).
pub fn sweep_empty_session_dirs(workspace: &Path, grace: std::time::Duration) -> usize {
    let root = workspace.join("sessions");
    let Ok(entries) = fs::read_dir(&root) else {
        return 0;
    };
    let mut removed = 0usize;
    for e in entries.flatten() {
        let dir = e.path();
        if !dir.is_dir() {
            continue;
        }
        // a freshly created directory (ingest by a neighbouring process) — do not touch;
        // time errors are treated as "fresh" (the safe side)
        let fresh = fs::metadata(&dir)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|e| e < grace)
            .unwrap_or(true);
        if fresh && !grace.is_zero() {
            continue;
        }
        let audio_empty = fs::read_dir(dir.join("audio"))
            .map(|mut rd| rd.next().is_none())
            .unwrap_or(true);
        if !audio_empty {
            continue;
        }
        // anything besides an empty audio/, meta.json and housekeeping files is valuable
        let has_other = fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten().any(|c| {
                    let name = c.file_name();
                    name != "audio"
                        && name != "meta.json"
                        && name != ".cook.lock"
                        && name != "meta.json.tmp"
                })
            })
            .unwrap_or(true);
        if has_other {
            continue;
        }
        // a meta with chunks OR meetings is not an empty session (the audio may have gone
        // to retention; a call may have been detected without a single sample)
        let meta_has_content = fs::read_to_string(dir.join("meta.json"))
            .ok()
            .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok())
            .map(|v| {
                ["chunks", "meetings"].iter().any(|k| {
                    v.get(*k)
                        .and_then(|c| c.as_array())
                        .is_some_and(|a| !a.is_empty())
                })
            })
            .unwrap_or(false);
        if meta_has_content {
            continue;
        }
        if fs::remove_dir_all(&dir).is_ok() {
            tracing::info!("removed empty session directory: {}", dir.display());
            removed += 1;
        }
    }
    removed
}

/// Unfinished `.part` files left by a crashed process — deleted at startup (a live pipeline
/// will create new ones).
pub fn remove_orphan_part_files(workspace: &Path) {
    let Ok(entries) = fs::read_dir(workspace) else {
        return;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.extension().and_then(|x| x.to_str()) != Some("part") {
            continue;
        }
        match fs::remove_file(&p) {
            Ok(()) => tracing::info!("Removed orphan .part: {}", p.display()),
            Err(err) => tracing::warn!("Could not remove {}: {err}", p.display()),
        }
    }
}

/// (number of WAVs with no line in the jsonl, the sum of their MB, MB of all files in the
/// directory).
pub fn workspace_queue_stats(workspace: &Path) -> (usize, f64, f64) {
    let processed = TranscriptWriter::processed_seg_ids(workspace);
    let Ok(entries) = fs::read_dir(workspace) else {
        return (0, 0.0, 0.0);
    };

    let mut unprocessed_wavs = 0usize;
    let mut unprocessed_bytes = 0u64;
    let mut total_bytes = 0u64;

    for e in entries.flatten() {
        let path = e.path();
        let Ok(meta) = path.metadata() else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        let len = meta.len();
        total_bytes += len;

        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !name.ends_with(".wav") {
            continue;
        }
        let seg_id = name.strip_suffix(".wav").unwrap_or(&name);
        if processed.contains(seg_id) {
            continue;
        }
        unprocessed_wavs += 1;
        unprocessed_bytes += len;
    }

    let unprocessed_mb = unprocessed_bytes as f64 / (1024.0 * 1024.0);
    let workspace_total_mb = total_bytes as f64 / (1024.0 * 1024.0);
    (unprocessed_wavs, unprocessed_mb, workspace_total_mb)
}

/// The highest segment number `src{n}_NNNNNN` among `.wav` and `.part` files in the root of
/// the workspace directory.
pub fn max_segment_seq_on_disk(workspace: &Path, source_id: u8) -> u32 {
    let prefix = format!("src{source_id}_");
    let mut max_seq = 0u32;
    let Ok(entries) = fs::read_dir(workspace) else {
        return 0;
    };
    for e in entries.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if !name.starts_with(&prefix) {
            continue;
        }
        let stem = name
            .strip_suffix(".wav")
            .or_else(|| name.strip_suffix(".part"))
            .unwrap_or(&name);
        if let Some((_, seq)) = wav_stem_sort_key(stem) {
            max_seq = max_seq.max(seq);
        }
    }
    max_seq
}

/// All WAVs with no line in transcript.jsonl, sorted by (src, seq).
pub fn recover_unprocessed(workspace: &Path) -> Vec<(PathBuf, u8)> {
    let processed = TranscriptWriter::processed_seg_ids(workspace);
    let mut pending: Vec<(PathBuf, u8)> = Vec::new();

    let entries = match fs::read_dir(workspace) {
        Ok(e) => e,
        Err(_) => return pending,
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        if !name.ends_with(".wav") {
            continue;
        }
        let seg_id = name.strip_suffix(".wav").unwrap_or(&name);
        if processed.contains(seg_id) {
            continue;
        }
        let source_id = parse_source_id(&name).unwrap_or(0);
        pending.push((path, source_id));
    }

    pending.sort_by(|(pa, _), (pb, _)| {
        let sa = pa
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(wav_stem_sort_key)
            .unwrap_or_else(fallback_sort_key);
        let sb = pb
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(wav_stem_sort_key)
            .unwrap_or_else(fallback_sort_key);
        sa.cmp(&sb)
    });
    pending
}

fn parse_source_id(filename: &str) -> Option<u8> {
    if filename.starts_with("src0_") {
        Some(0)
    } else if filename.starts_with("src1_") {
        Some(1)
    } else {
        None
    }
}

/// Sort key for `src0_000042` → (0, 42). Off-schema names go to the end of the queue.
pub fn wav_stem_sort_key(stem: &str) -> Option<(u8, u32)> {
    let rest = stem.strip_prefix("src")?;
    let (src_s, seq_s) = rest.split_once('_')?;
    let src: u8 = src_s.parse().ok()?;
    let seq: u32 = seq_s.parse().ok()?;
    Some((src, seq))
}

fn fallback_sort_key() -> (u8, u32) {
    (255, u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn wav_stem_sort_key_examples() {
        assert_eq!(wav_stem_sort_key("src0_000001"), Some((0, 1)));
        assert_eq!(wav_stem_sort_key("src1_000010"), Some((1, 10)));
        assert_eq!(wav_stem_sort_key("nope"), None);
    }

    #[test]
    fn sweep_removes_only_truly_empty_sessions() {
        let dir = tempdir().unwrap();
        let sessions = dir.path().join("sessions");
        // empty: audio/ with no files, meta with no chunks (+housekeeping files are no obstacle)
        let empty = sessions.join("20260712_empty");
        std::fs::create_dir_all(empty.join("audio")).unwrap();
        std::fs::write(
            empty.join("meta.json"),
            r#"{"started_at":"t","sample_rate":16000,"chunks":[]}"#,
        )
        .unwrap();
        std::fs::write(empty.join(".cook.lock"), b"").unwrap();
        std::fs::write(empty.join("meta.json.tmp"), b"").unwrap();
        // with audio — do not touch
        let with_audio = sessions.join("20260712_audio");
        std::fs::create_dir_all(with_audio.join("audio")).unwrap();
        std::fs::write(with_audio.join("audio/src0_chunk0001.wav"), b"x").unwrap();
        // no audio, but with transcripts — do not touch
        let with_tr = sessions.join("20260712_tr");
        std::fs::create_dir_all(with_tr.join("audio")).unwrap();
        std::fs::create_dir_all(with_tr.join("transcripts")).unwrap();
        // no audio, but meta remembers chunks (retention ate them) — do not touch
        let retained = sessions.join("20260712_ret");
        std::fs::create_dir_all(retained.join("audio")).unwrap();
        std::fs::write(retained.join("meta.json"),
            r#"{"started_at":"t","sample_rate":16000,"chunks":[{"file":"src0_chunk0001","source_id":0,"start_offset_sec":0.0,"duration_sec":1.0}]}"#).unwrap();
        // no audio and no chunks, but a meeting in meta (a call with a dead microphone) —
        // the call metadata is valuable, do not touch
        let with_meeting = sessions.join("20260712_meet");
        std::fs::create_dir_all(with_meeting.join("audio")).unwrap();
        std::fs::write(with_meeting.join("meta.json"),
            r#"{"started_at":"t","sample_rate":16000,"chunks":[],"meetings":[{"apps":["zoom.exe"],"window_title":"Weekly","started_at":"t","ended_at":null}]}"#).unwrap();

        assert_eq!(
            sweep_empty_session_dirs(dir.path(), std::time::Duration::ZERO),
            1
        );
        assert!(!empty.exists(), "the empty one must be removed");
        assert!(
            with_audio.exists() && with_tr.exists() && retained.exists() && with_meeting.exists()
        );
    }

    #[test]
    fn sweep_grace_protects_fresh_dirs() {
        let dir = tempdir().unwrap();
        let fresh = dir.path().join("sessions/20260712_fresh");
        std::fs::create_dir_all(fresh.join("audio")).unwrap();
        // freshly created (mtime = now) under grace — not removed
        assert_eq!(
            sweep_empty_session_dirs(dir.path(), std::time::Duration::from_secs(600)),
            0
        );
        assert!(fresh.exists());
    }

    #[test]
    fn workspace_queue_stats_counts_unprocessed() {
        let dir = tempdir().unwrap();
        let w = dir.path();
        std::fs::write(w.join("src0_000001.wav"), [0u8; 100]).unwrap();
        std::fs::write(w.join("transcript.jsonl"), "").unwrap();
        let (n, umb, smb) = workspace_queue_stats(w);
        assert_eq!(n, 1);
        assert!((umb - 100.0 / (1024.0 * 1024.0)).abs() < 1e-6);
        assert!((smb - 100.0 / (1024.0 * 1024.0)).abs() < 1e-6);
        let line = r#"{"seg_id":"src0_000001","source_id":0,"text":"x","duration_sec":1.0,"timestamp":"2026-01-01T00:00:00+00:00"}"#;
        std::fs::write(w.join("transcript.jsonl"), format!("{line}\n")).unwrap();
        let (n2, _, _) = workspace_queue_stats(w);
        assert_eq!(n2, 0);
    }

    #[test]
    fn recover_unprocessed_sorted_order() {
        let dir = tempdir().unwrap();
        let w = dir.path();
        std::fs::write(w.join("src0_000003.wav"), b"x").unwrap();
        std::fs::write(w.join("src0_000001.wav"), b"x").unwrap();
        std::fs::write(w.join("src1_000001.wav"), b"x").unwrap();
        std::fs::write(w.join("transcript.jsonl"), "").unwrap();
        let pending = recover_unprocessed(w);
        assert_eq!(pending.len(), 3);
        assert_eq!(pending[0].0.file_name().unwrap(), "src0_000001.wav");
        assert_eq!(pending[1].0.file_name().unwrap(), "src0_000003.wav");
        assert_eq!(pending[2].0.file_name().unwrap(), "src1_000001.wav");
    }

    #[test]
    fn max_segment_seq_includes_wav_and_part() {
        let dir = tempdir().unwrap();
        let w = dir.path();
        std::fs::write(w.join("src0_000007.wav"), b"a").unwrap();
        std::fs::write(w.join("src0_000002.part"), b"b").unwrap();
        assert_eq!(super::max_segment_seq_on_disk(w, 0), 7);
        assert_eq!(super::max_segment_seq_on_disk(w, 1), 0);
    }

    #[test]
    fn remove_orphan_part_files_deletes_part_only() {
        let dir = tempdir().unwrap();
        let w = dir.path();
        std::fs::write(w.join("keep.wav"), b"x").unwrap();
        std::fs::write(w.join("orphan.part"), b"x").unwrap();
        remove_orphan_part_files(w);
        assert!(w.join("keep.wav").exists());
        assert!(!w.join("orphan.part").exists());
    }
}
