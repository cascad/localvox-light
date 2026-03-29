//! Рабочий каталог (`LOCALVOX_LIGHT_AUDIO_DIR`): в корне лежат WAV-сегменты и `transcript.jsonl`.
//! Отдельные подкаталоги `session_*` не создаются — при каждом запуске продолжается тот же каталог.
//! Сброс транскрипта: [x] в TUI или ручная правка/удаление файлов.

use std::fs;
use std::path::{Path, PathBuf};

use crate::transcript::TranscriptWriter;

/// Незавершённые `.part` после падения процесса — удаляем при старте (живой пайплайн создаст новые).
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

/// (число WAV без строки в jsonl, сумма их МБ, МБ всех файлов в каталоге).
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

/// Максимальный номер сегмента `src{n}_NNNNNN` среди `.wav` и `.part` в корне рабочего каталога.
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

/// Все WAV без строки в transcript.jsonl, отсортированные по (src, seq).
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

/// Ключ сортировки для `src0_000042` → (0, 42). Нестандартные имена — в конец очереди.
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
