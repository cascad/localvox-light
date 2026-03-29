//! Durable transcript writer: appends to transcript.jsonl with fsync.
//! Each line is a self-contained JSON object — safe against partial writes.

use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local, Utc};
use uuid::Uuid;

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TranscriptEntry {
    pub seg_id: String,
    pub source_id: u8,
    pub text: String,
    pub duration_sec: f64,
    pub timestamp: String,
}

pub struct TranscriptWriter {
    _path: PathBuf,
    file: File,
}

impl TranscriptWriter {
    pub fn open(session_dir: &Path) -> anyhow::Result<Self> {
        let path = session_dir.join("transcript.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        Ok(Self { _path: path, file })
    }

    /// Пересоздать transcript.jsonl пустым (дальнейшие append с начала файла).
    pub fn reopen_truncated(session_dir: &Path) -> anyhow::Result<Self> {
        let path = session_dir.join("transcript.jsonl");
        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .open(&path)?;
        Ok(Self { _path: path, file })
    }

    pub fn append(&mut self, entry: &TranscriptEntry) -> anyhow::Result<()> {
        let line = serde_json::to_string(entry)?;
        writeln!(self.file, "{}", line)?;
        self.file.flush()?;
        #[cfg(unix)]
        {
            use std::os::unix::io::AsRawFd;
            unsafe { libc::fsync(self.file.as_raw_fd()) };
        }
        Ok(())
    }

    /// Все записи из transcript.jsonl по порядку (для гидратации TUI = файлу на диске).
    pub fn read_all_entries(session_dir: &Path) -> Vec<TranscriptEntry> {
        let path = session_dir.join("transcript.jsonl");
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return Vec::new(),
        };
        let mut out = Vec::new();
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(&line) {
                out.push(entry);
            }
        }
        out
    }

    /// Returns seg_ids already in the transcript (for crash recovery).
    pub fn processed_seg_ids(session_dir: &Path) -> HashSet<String> {
        let path = session_dir.join("transcript.jsonl");
        let mut set = HashSet::new();
        let file = match File::open(&path) {
            Ok(f) => f,
            Err(_) => return set,
        };
        for line in BufReader::new(file).lines().map_while(Result::ok) {
            if let Ok(entry) = serde_json::from_str::<TranscriptEntry>(&line) {
                set.insert(entry.seg_id);
            }
        }
        set
    }
}

/// Ключ сортировки `src0_000042` → (0, 42); иначе в конец (как `session::wav_stem_sort_key`).
fn seg_sort_tuple(seg_id: &str) -> (u8, u32) {
    let Some(rest) = seg_id.strip_prefix("src") else {
        return (255, u32::MAX);
    };
    let Some((src_s, seq_s)) = rest.split_once('_') else {
        return (255, u32::MAX);
    };
    let src = src_s.parse().unwrap_or(255);
    let seq = seq_s.parse().unwrap_or(u32::MAX);
    (src, seq)
}

/// Читает `transcript.jsonl` сессии, сортирует по времени записи (RFC3339), затем по (src, seq),
/// пишет в `dump_root` файл `transcript_dump_YYYY-MM-DD_HHMMSS.jsonl` (не порядок параллельного ASR).
pub fn export_sorted_jsonl(session_dir: &Path, dump_root: &Path) -> anyhow::Result<(PathBuf, usize)> {
    if dump_root.as_os_str().is_empty() {
        anyhow::bail!("каталог дампа пустой (задайте LOCALVOX_LIGHT_TRANSCRIPT_DUMP_DIR)");
    }
    let mut entries = TranscriptWriter::read_all_entries(session_dir);
    if entries.is_empty() {
        anyhow::bail!("нет строк в transcript.jsonl");
    }
    entries.sort_by(|a, b| {
        let pa = DateTime::parse_from_rfc3339(a.timestamp.trim()).map(|d| d.with_timezone(&Utc));
        let pb = DateTime::parse_from_rfc3339(b.timestamp.trim()).map(|d| d.with_timezone(&Utc));
        match (pa, pb) {
            (Ok(da), Ok(db)) => match da.cmp(&db) {
                Ordering::Equal => seg_sort_tuple(&a.seg_id).cmp(&seg_sort_tuple(&b.seg_id)),
                o => o,
            },
            (Ok(_), Err(_)) => Ordering::Less,
            (Err(_), Ok(_)) => Ordering::Greater,
            (Err(_), Err(_)) => seg_sort_tuple(&a.seg_id).cmp(&seg_sort_tuple(&b.seg_id)),
        }
    });

    fs::create_dir_all(dump_root)?;
    let path = unique_dump_path(dump_root);
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)?;
    let n = entries.len();
    for e in &entries {
        writeln!(file, "{}", serde_json::to_string(e)?)?;
    }
    file.flush()?;
    #[cfg(unix)]
    {
        use std::os::unix::io::AsRawFd;
        unsafe { libc::fsync(file.as_raw_fd()) };
    }
    tracing::info!("[dump] export {n} строк → {}", path.display());
    Ok((path, n))
}

fn unique_dump_path(root: &Path) -> PathBuf {
    let ts = Local::now().format("%Y-%m-%d_%H%M%S");
    let base = format!("transcript_dump_{ts}");
    let mut path = root.join(format!("{base}.jsonl"));
    if path.exists() {
        let id = Uuid::now_v7();
        let short = id.to_string();
        let short = short.split('-').next().unwrap_or(&short);
        path = root.join(format!("{base}_{short}.jsonl"));
    }
    path
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn seg_sort_tuple_parses_stem() {
        assert_eq!(super::seg_sort_tuple("src0_000042"), (0, 42));
        assert_eq!(super::seg_sort_tuple("src1_000001"), (1, 1));
        assert_eq!(super::seg_sort_tuple("bad"), (255, u32::MAX));
    }

    #[test]
    fn read_all_skips_invalid_jsonl_lines() {
        let dir = tempdir().unwrap();
        let p = dir.path().join("transcript.jsonl");
        let mut f = File::create(&p).unwrap();
        writeln!(f, "{{\"seg_id\":\"src0_000001\",\"source_id\":0,\"text\":\"a\",\"duration_sec\":1.0,\"timestamp\":\"2026-01-01T00:00:00Z\"}}").unwrap();
        writeln!(f, "not json").unwrap();
        writeln!(f, "{{\"seg_id\":\"src0_000002\",\"source_id\":0,\"text\":\"b\",\"duration_sec\":1.0,\"timestamp\":\"2026-01-01T00:00:01Z\"}}").unwrap();
        drop(f);
        let v = TranscriptWriter::read_all_entries(dir.path());
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].seg_id, "src0_000001");
        assert_eq!(v[1].seg_id, "src0_000002");
    }

    #[test]
    fn append_and_processed_seg_ids_roundtrip() {
        let dir = tempdir().unwrap();
        let mut tw = TranscriptWriter::open(dir.path()).unwrap();
        let e = TranscriptEntry {
            seg_id: "src0_000099".into(),
            source_id: 0,
            text: "hello".into(),
            duration_sec: 1.5,
            timestamp: "2026-03-01T12:00:00+00:00".into(),
        };
        tw.append(&e).unwrap();
        drop(tw);
        let set = TranscriptWriter::processed_seg_ids(dir.path());
        assert!(set.contains("src0_000099"));
        let again = TranscriptWriter::read_all_entries(dir.path());
        assert_eq!(again.len(), 1);
        assert_eq!(again[0].text, "hello");
    }

    #[test]
    fn export_sorted_jsonl_orders_by_timestamp_then_seg() {
        let session = tempdir().unwrap();
        let dump = tempdir().unwrap();
        let p = session.path().join("transcript.jsonl");
        let mut f = File::create(&p).unwrap();
        // Позже по времени, но меньший seq — должен быть вторым после сортировки по времени
        writeln!(
            f,
            r#"{{"seg_id":"src0_000002","source_id":0,"text":"b","duration_sec":1.0,"timestamp":"2026-01-01T00:00:02+00:00"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"seg_id":"src0_000001","source_id":0,"text":"a","duration_sec":1.0,"timestamp":"2026-01-01T00:00:01+00:00"}}"#
        )
        .unwrap();
        drop(f);
        let (_path, n) = export_sorted_jsonl(session.path(), dump.path()).unwrap();
        assert_eq!(n, 2);
        // dump — один файл с непредсказуемым именем; читаем единственный jsonl
        let files: Vec<_> = std::fs::read_dir(dump.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect();
        assert_eq!(files.len(), 1);
        let lines: Vec<_> = BufReader::new(File::open(&files[0]).unwrap())
            .lines()
            .map_while(Result::ok)
            .collect();
        assert_eq!(lines.len(), 2);
        let first: TranscriptEntry = serde_json::from_str(&lines[0]).unwrap();
        let second: TranscriptEntry = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(first.seg_id, "src0_000001");
        assert_eq!(second.seg_id, "src0_000002");
    }

    #[test]
    fn export_sorted_jsonl_empty_bails() {
        let session = tempdir().unwrap();
        let dump = tempdir().unwrap();
        File::create(session.path().join("transcript.jsonl")).unwrap();
        let r = export_sorted_jsonl(session.path(), dump.path());
        assert!(r.is_err());
    }

    #[test]
    fn reopen_truncated_clears_transcript() {
        let dir = tempdir().unwrap();
        let mut tw = TranscriptWriter::open(dir.path()).unwrap();
        tw.append(&TranscriptEntry {
            seg_id: "src0_000001".into(),
            source_id: 0,
            text: "x".into(),
            duration_sec: 1.0,
            timestamp: "2026-01-01T00:00:00+00:00".into(),
        })
        .unwrap();
        drop(tw);
        let mut tw2 = TranscriptWriter::reopen_truncated(dir.path()).unwrap();
        tw2.append(&TranscriptEntry {
            seg_id: "src0_000002".into(),
            source_id: 0,
            text: "y".into(),
            duration_sec: 1.0,
            timestamp: "2026-01-02T00:00:00+00:00".into(),
        })
        .unwrap();
        drop(tw2);
        let v = TranscriptWriter::read_all_entries(dir.path());
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].seg_id, "src0_000002");
    }

    #[test]
    fn export_sorted_jsonl_same_timestamp_tiebreak_by_seg() {
        let session = tempdir().unwrap();
        let dump = tempdir().unwrap();
        let p = session.path().join("transcript.jsonl");
        let mut f = File::create(&p).unwrap();
        let ts = "2026-01-01T00:00:00+00:00";
        writeln!(
            f,
            r#"{{"seg_id":"src0_000002","source_id":0,"text":"b","duration_sec":1.0,"timestamp":"{ts}"}}"#
        )
        .unwrap();
        writeln!(
            f,
            r#"{{"seg_id":"src0_000001","source_id":0,"text":"a","duration_sec":1.0,"timestamp":"{ts}"}}"#
        )
        .unwrap();
        drop(f);
        let (_path, n) = export_sorted_jsonl(session.path(), dump.path()).unwrap();
        assert_eq!(n, 2);
        let files: Vec<_> = std::fs::read_dir(dump.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect();
        let lines: Vec<_> = BufReader::new(File::open(&files[0]).unwrap())
            .lines()
            .map_while(Result::ok)
            .collect();
        let a: TranscriptEntry = serde_json::from_str(&lines[0]).unwrap();
        let b: TranscriptEntry = serde_json::from_str(&lines[1]).unwrap();
        assert_eq!(a.seg_id, "src0_000001");
        assert_eq!(b.seg_id, "src0_000002");
    }
}
