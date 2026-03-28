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
