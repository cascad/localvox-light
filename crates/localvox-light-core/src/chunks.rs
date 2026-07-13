//! Continuous recording of session audio chunks — the primary durable artifact (F8, WP-A1).
//!
//! Storage is decoupled from the semantics of speech: PCM is written as a stream (frequent
//! fsync — a crash costs seconds), the file rotates on a fixed duration.
//! Chunks of one source join sample-exactly — a file boundary may fall in the middle of a
//! word, and that is not a loss: the windows for ASR (≤180 s, cut on VAD silence) are sliced
//! at read time, on top of the joined stream (WP-A3).
//!
//! Layout: `<work_dir>/sessions/<YYYYMMDD_HHMMSS>/audio/src{N}_chunk{SEQ}.wav`,
//! with `meta.json` next to `audio/`. A closed chunk is optionally recompressed to FLAC
//! (external ffmpeg, on a background thread); `meta.json` stores the name without an
//! extension: the reader looks for `.flac`, then `.wav`.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

const SAMPLE_RATE: u32 = 16_000;
const BYTES_PER_SAMPLE: u64 = 2;
const WAV_HEADER_LEN: u64 = 44;
const FSYNC_INTERVAL: Duration = Duration::from_secs(5);

// ─────────────────────────── parameters ───────────────────────────

pub struct ChunkParams {
    /// `<session>/audio`
    pub audio_dir: PathBuf,
    /// Path to the session's `meta.json`.
    pub meta_path: PathBuf,
    /// Chunk duration: the file rotates once it is reached (neighbours join sample-exactly).
    pub chunk_sec: f64,
    /// Recompress closed chunks into FLAC (needs ffmpeg).
    pub flac: bool,
    /// The ffmpeg command/path for FLAC (usually "ffmpeg" from PATH).
    pub ffmpeg: PathBuf,
}

// ─────────────────────────── meta.json ───────────────────────────

#[derive(Serialize, Deserialize, Default)]
pub struct SessionMeta {
    pub started_at: String,
    pub sample_rate: u32,
    pub chunks: Vec<ChunkEntry>,
    /// Calls/meetings detected during the session (F3, detect.rs).
    #[serde(default)]
    pub meetings: Vec<MeetingMark>,
    /// When the session was closed and why ("вручную", "тишина", "новые сутки").
    /// Empty — it closed by itself (the engine exited). Needed so that it is visible
    /// afterwards that the recording was cut short by a human, not by a failure.
    #[serde(default)]
    pub stopped_at: Option<String>,
    #[serde(default)]
    pub stopped_reason: Option<String>,
    /// The recording language chosen by the HUMAN (ISO-639-1). Empty — "auto". It goes into
    /// the cook recipes and the LLM artifacts, so changing the language — with no extra code —
    /// devalues everything derived and puts the session up for a re-cook
    /// (see `lang.rs`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang: Option<String>,
    /// The language detected FROM THE RECOGNIZED TEXT. A guess, not a choice: the template
    /// selection and the LLM's answer language rest on it, but the ASR model selection does not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lang_detected: Option<String>,
    /// The title given by the HUMAN ("Планёрка", "1:1 с Иваном").
    ///
    /// Auto-detection sees a call inside an application, but it will never see a face-to-face
    /// meeting at the table: for the system that is the same background noise as the whole day.
    /// There is no reliable automatic sign of such a meeting — that is why the human has a
    /// button, and their word outweighs any guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// This is a MEETING, not background: marked so by the human (button/voice) or by the call
    /// detector. Meeting-minutes summaries are selected and the meeting list is built by this flag.
    #[serde(default)]
    pub meeting: bool,
}

#[derive(Serialize, Deserialize)]
pub struct MeetingMark {
    /// The applications that were holding the microphone (zoom.exe, chrome.exe …).
    pub apps: Vec<String>,
    /// The foreground window title at the start — raw material for the meeting auto-title.
    pub window_title: String,
    pub started_at: String,
    pub ended_at: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ChunkEntry {
    /// File name without an extension (`src0_chunk0001`); the reader looks for `.flac` | `.wav`.
    pub file: String,
    pub source_id: u8,
    /// The start of the file's first sample on the source's audio timeline (sec from the start
    /// of the session).
    pub start_offset_sec: f64,
    pub duration_sec: f64,
    /// Mean signal level (RMS, 0..32767). Quiet speech ≈ 200–400,
    /// normal ≈ 1000–3000, silence ≈ 0. Diagnosing "why did it recognize badly"
    /// without re-listening: it is visible straight from the meta.
    #[serde(default)]
    pub rms: u16,
    /// Fraction of time with speech/sound (0..1) over 100 ms windows. A conversation with
    /// pauses ≈ 0.1–0.3, a continuous lecture ≈ 0.5+, silence ≈ 0.
    #[serde(default)]
    pub voice_ratio: f32,
}

#[cfg(test)]
mod meta_tests {
    use super::*;

    #[test]
    fn saving_meta_from_memory_does_not_wipe_a_language_set_from_outside() {
        // The engine holds meta in memory (with lang: None there) and rewrites the file
        // in full on every chunk rotation. Without a merge the human's choice vanished
        // silently: they saw "language: en, we will redo everything", the derived data was
        // already wiped, and the session was re-cooked back into Russian.
        let dir = tempfile::tempdir().unwrap();
        let meta_path = dir.path().join("meta.json");

        let in_memory = SessionMeta {
            started_at: "t".into(),
            sample_rate: SAMPLE_RATE,
            ..Default::default()
        };
        save_meta(&meta_path, &in_memory);

        // the language was set FROM OUTSIDE while the session is being written
        crate::lang::set(dir.path(), Some("en")).unwrap();

        // ...and the engine appended another chunk
        save_meta(&meta_path, &in_memory);

        assert_eq!(
            crate::lang::asr(dir.path()),
            "en",
            "writing meta from memory wiped the language chosen by the human"
        );
    }
}

/// Atomic write of meta.json (tmp + rename). Losing the meta is not fatal:
/// chunks are recoverable by scanning the directory; meta is a speed-up and exact offsets.
/// The public meta write (meeting detection writes from its own thread).
pub fn save_meta_public(meta_path: &Path, meta: &SessionMeta) {
    save_meta(meta_path, meta);
}

fn save_meta(meta_path: &Path, meta: &SessionMeta) {
    let mut value = match serde_json::to_value(meta) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("meta.json was not serialized: {e}");
            return;
        }
    };
    // The language may have been set FROM OUTSIDE (the web UI, `--lang`) while the session is
    // being written — and we hold a copy of meta in memory where it is absent, and we write the
    // file in full. Without this the human's choice vanished silently on the next chunk
    // rotation: they saw "language: en, we will redo everything", the derived files were
    // already wiped, and the session was re-cooked back into Russian. Fields that are not ours
    // we do not touch.
    if let (Some(obj), Ok(on_disk)) = (
        value.as_object_mut(),
        fs::read(meta_path).and_then(|b| {
            serde_json::from_slice::<serde_json::Value>(&b).map_err(std::io::Error::other)
        }),
    ) {
        for field in ["lang", "lang_detected"] {
            if !obj.contains_key(field) {
                if let Some(v) = on_disk.get(field) {
                    obj.insert(field.to_string(), v.clone());
                }
            }
        }
    }

    let tmp = meta_path.with_extension("json.tmp");
    let write = fs::write(&tmp, serde_json::to_vec_pretty(&value).unwrap_or_default())
        .and_then(|()| fs::rename(&tmp, meta_path));
    if let Err(e) = write {
        tracing::warn!("meta.json was not written: {e}");
    }
}

// ─────────────────────── streaming WAV writer ───────────────────────

struct StreamingWav {
    part_path: PathBuf,
    file: File,
    data_bytes: u64,
    last_sync: Instant,
}

impl StreamingWav {
    fn create(part_path: PathBuf) -> std::io::Result<Self> {
        let mut file = File::create(&part_path)?;
        file.write_all(&wav_header(0))?;
        Ok(Self {
            part_path,
            file,
            data_bytes: 0,
            last_sync: Instant::now(),
        })
    }

    fn write(&mut self, samples: &[i16]) -> std::io::Result<()> {
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all(&buf)?;
        self.data_bytes += buf.len() as u64;
        if self.last_sync.elapsed() >= FSYNC_INTERVAL {
            let _ = self.file.sync_data();
            self.last_sync = Instant::now();
        }
        Ok(())
    }

    fn duration_sec(&self) -> f64 {
        self.data_bytes as f64 / (BYTES_PER_SAMPLE as f64 * SAMPLE_RATE as f64)
    }

    /// Patch the header + `.part` → `.wav`. An empty file is deleted and None is returned.
    fn finalize(mut self) -> Option<PathBuf> {
        if self.data_bytes == 0 {
            drop(self.file);
            let _ = fs::remove_file(&self.part_path);
            return None;
        }
        let patch = (|| -> std::io::Result<()> {
            self.file.seek(SeekFrom::Start(0))?;
            self.file.write_all(&wav_header(self.data_bytes))?;
            self.file.sync_all()
        })();
        if let Err(e) = patch {
            tracing::error!("chunk finalize {}: {e}", self.part_path.display());
        }
        drop(self.file);
        let final_path = self.part_path.with_extension("wav");
        match fs::rename(&self.part_path, &final_path) {
            Ok(()) => Some(final_path),
            Err(e) => {
                tracing::error!("chunk rename: {e}");
                None
            }
        }
    }
}

fn wav_header(data_bytes: u64) -> [u8; WAV_HEADER_LEN as usize] {
    let data = data_bytes.min(u32::MAX as u64) as u32;
    let riff = data.saturating_add(36);
    let byte_rate = SAMPLE_RATE * BYTES_PER_SAMPLE as u32;
    let mut h = [0u8; 44];
    h[..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&riff.to_le_bytes());
    h[8..12].copy_from_slice(b"WAVE");
    h[12..16].copy_from_slice(b"fmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    h[22..24].copy_from_slice(&1u16.to_le_bytes()); // mono
    h[24..28].copy_from_slice(&SAMPLE_RATE.to_le_bytes());
    h[28..32].copy_from_slice(&byte_rate.to_le_bytes());
    h[32..34].copy_from_slice(&(BYTES_PER_SAMPLE as u16).to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

// ─────────────────────── recorder for one source ───────────────────────

pub struct ChunkRecorder {
    source_id: u8,
    seq: u32,
    wav: Option<StreamingWav>,
    /// The source's audio position from the start of the session in SAMPLES — integral, so
    /// that the offsets in meta do not drift from f64 accumulation.
    timeline_samples: u64,
    /// The timeline position of the current file's first sample.
    chunk_start_samples: u64,
    params: Arc<ChunkParams>,
    meta: Arc<Mutex<SessionMeta>>,
    /// Metrics of the current chunk (computed on the fly, without a second pass over the
    /// audio): sum of squares → RMS, and the fraction of 100 ms windows with sound → voice_ratio.
    level: LevelStats,
}

/// Streaming level counter: RMS of the whole chunk + the fraction of "sounding" windows.
#[derive(Default)]
struct LevelStats {
    sum_sq: f64,
    n: u64,
    /// Accumulator of the current 100 ms window.
    win_sum_sq: f64,
    win_n: u32,
    loud_windows: u32,
    total_windows: u32,
}

impl LevelStats {
    /// 100 ms at 16 kHz.
    const WIN: u32 = 1600;
    /// RMS threshold for a "sounding" window: silence/the noise floor is below, speech is above.
    const LOUD_RMS: f64 = 300.0;

    fn feed(&mut self, samples: &[i16]) {
        for &s in samples {
            let v = f64::from(s);
            self.sum_sq += v * v;
            self.n += 1;
            self.win_sum_sq += v * v;
            self.win_n += 1;
            if self.win_n >= Self::WIN {
                let rms = (self.win_sum_sq / f64::from(self.win_n)).sqrt();
                if rms > Self::LOUD_RMS {
                    self.loud_windows += 1;
                }
                self.total_windows += 1;
                self.win_sum_sq = 0.0;
                self.win_n = 0;
            }
        }
    }

    /// (RMS, the fraction of sounding windows).
    fn finish(&self) -> (u16, f32) {
        let rms = if self.n > 0 {
            (self.sum_sq / self.n as f64)
                .sqrt()
                .min(f64::from(u16::MAX)) as u16
        } else {
            0
        };
        let ratio = if self.total_windows > 0 {
            self.loud_windows as f32 / self.total_windows as f32
        } else {
            0.0
        };
        (rms, ratio)
    }
}

impl ChunkRecorder {
    pub fn new(source_id: u8, params: Arc<ChunkParams>, meta: Arc<Mutex<SessionMeta>>) -> Self {
        Self {
            source_id,
            seq: 0,
            wav: None,
            timeline_samples: 0,
            chunk_start_samples: 0,
            params,
            meta,
            level: LevelStats::default(),
        }
    }

    pub fn feed(&mut self, samples: &[i16]) {
        if samples.is_empty() {
            return;
        }
        if self.wav.is_none() {
            self.open_next();
        }
        if let Some(ref mut w) = self.wav {
            match w.write(samples) {
                // We count the level ONLY over samples that actually landed in the file.
                // Otherwise the diagnostics lie exactly where they are needed: a chunk that
                // failed to open (disk full, antivirus holding the file) would drag its level
                // into the next one, and silence would report itself as loud speech.
                Ok(()) => self.level.feed(samples),
                Err(e) => tracing::error!("chunk write src{}: {e}", self.source_id),
            }
        }
        self.timeline_samples += samples.len() as u64;

        let chunk_samples = (self.params.chunk_sec * SAMPLE_RATE as f64) as u64;
        if self.timeline_samples - self.chunk_start_samples >= chunk_samples {
            self.close_current();
            self.open_next();
        }
    }

    /// Pause/stop: close the current chunk (the data is kept — it is the primary artifact).
    pub fn finalize_current(&mut self) {
        self.close_current();
    }

    fn close_current(&mut self) {
        // The level accumulator lives exactly as long as the chunk: we take it
        // UNCONDITIONALLY, before every early return. If the chunk did not finalize
        // (empty file, a rename that fell through), its metrics must not leak into the
        // next one — they would describe audio that is not in it.
        let (rms, voice_ratio) = std::mem::take(&mut self.level).finish();
        let Some(w) = self.wav.take() else { return };
        let duration = w.duration_sec();
        let start = self.chunk_start_samples as f64 / SAMPLE_RATE as f64;
        if let Some(path) = w.finalize() {
            let stem = path
                .file_stem()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
            {
                let mut meta = self.meta.lock().unwrap();
                meta.chunks.push(ChunkEntry {
                    file: stem,
                    source_id: self.source_id,
                    start_offset_sec: start,
                    duration_sec: duration,
                    rms,
                    voice_ratio,
                });
                save_meta(&self.params.meta_path, &meta);
            }
            tracing::info!(
                "chunk src{} closed: {} ({duration:.1} s, RMS {rms}, speech {:.0} %)",
                self.source_id,
                path.display(),
                voice_ratio * 100.0
            );
            if self.params.flac {
                spawn_flac_convert(self.params.ffmpeg.clone(), path);
            }
        }
    }

    fn open_next(&mut self) {
        self.seq += 1;
        let name = format!("src{}_chunk{:04}.part", self.source_id, self.seq);
        let path = self.params.audio_dir.join(name);
        match StreamingWav::create(path) {
            Ok(w) => {
                self.chunk_start_samples = self.timeline_samples;
                self.wav = Some(w);
            }
            Err(e) => tracing::error!("chunk creation src{}: {e}", self.source_id),
        }
    }
}

// ─────────────────────── session / recovery / retention ───────────────────────

/// `<work_dir>/sessions/<YYYYMMDD_HHMMSS>[_<label>]/audio` + an initial meta.json.
/// The label (for example, the file name on ingest) is cleaned down to [a-z0-9-].
pub fn create_session_dir(
    work_dir: &Path,
    label: Option<&str>,
) -> std::io::Result<(PathBuf, PathBuf)> {
    let stamp = chrono::Local::now().format("%Y%m%d_%H%M%S");
    let base = match label.map(slug) {
        Some(s) if !s.is_empty() => format!("{stamp}_{s}"),
        _ => stamp.to_string(),
    };
    // Uniqueness within one second: the ambient cut and the rotation on a call
    // (WP-C6) create sessions faster than the one-second tick of the name — without a
    // suffix the second one would overwrite the first.
    let sessions = work_dir.join("sessions");
    let mut name = base.clone();
    let mut n = 2;
    while sessions.join(&name).exists() {
        name = format!("{base}-{n}");
        n += 1;
    }
    let session = sessions.join(name);
    let audio = session.join("audio");
    fs::create_dir_all(&audio)?;
    let meta_path = session.join("meta.json");
    let meta = SessionMeta {
        started_at: chrono::Local::now().to_rfc3339(),
        sample_rate: SAMPLE_RATE,
        chunks: Vec::new(),
        // We PIN the global language choice here, at the moment of recording.
        //
        // Otherwise `LOCALVOX_LANG` would act retroactively: the language goes into the recipe,
        // and a human who set the variable for the sake of one English call would declare the
        // ENTIRE Russian archive "cooked wrong" — and it would be re-cooked with the English
        // model. The variable MUST affect only NEW recordings.
        lang: crate::lang::global(),
        ..Default::default()
    };
    save_meta(&meta_path, &meta);
    Ok((audio, meta_path))
}

/// ffmpeg for decoding/recompression: env → next to the exe (portable distribution; under
/// autostart cwd = system32 and the .env will not be found) → the bare name from PATH.
pub fn resolve_ffmpeg_for_decode() -> std::ffi::OsString {
    if let Some(p) = std::env::var_os("LOCALVOX_LIGHT_YT_FFMPEG") {
        return p;
    }
    if let Some(near) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("ffmpeg.exe")))
        .filter(|p| p.exists())
    {
        return near.into_os_string();
    }
    "ffmpeg".into()
}

/// The session label in the directory name: [a-z0-9-], no longer than 40 characters.
///
/// Cyrillic is TRANSLITERATED, not thrown away. Otherwise "Планёрка" would turn into an
/// empty string, and the meeting the human named would sit in a directory holding nothing
/// but a date — that is, indistinguishable from the rest of the day. The human sees the
/// directory name: in the file explorer, in the backup, in the path to the file.
fn slug(label: &str) -> String {
    let s: String = label
        .to_lowercase()
        .chars()
        .flat_map(|c| translit(c).chars().collect::<Vec<_>>())
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    // Collapse runs of hyphens: "1:1 с Иваном" would otherwise give "1-1--s-ivanom".
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if c == '-' && out.ends_with('-') {
            continue;
        }
        out.push(c);
    }
    out.trim_matches('-').chars().take(40).collect()
}

fn translit(c: char) -> String {
    const RU: [(char, &str); 33] = [
        ('а', "a"), ('б', "b"), ('в', "v"), ('г', "g"), ('д', "d"), ('е', "e"), ('ё', "e"),
        ('ж', "zh"), ('з', "z"), ('и', "i"), ('й', "y"), ('к', "k"), ('л', "l"), ('м', "m"),
        ('н', "n"), ('о', "o"), ('п', "p"), ('р', "r"), ('с', "s"), ('т', "t"), ('у', "u"),
        ('ф', "f"), ('х', "h"), ('ц', "c"), ('ч', "ch"), ('ш', "sh"), ('щ', "sch"), ('ъ', ""),
        ('ы', "y"), ('ь', ""), ('э', "e"), ('ю', "yu"), ('я', "ya"),
    ];
    RU.iter()
        .find(|(ru, _)| *ru == c)
        .map(|(_, lat)| (*lat).to_string())
        .unwrap_or_else(|| c.to_string())
}

/// Unfinished `.part` chunks after a crash: we patch the header against the facts and rename
/// to `.wav` — the data is kept (unlike fast-lane segments, where a `.part` is garbage).
pub fn recover_orphan_chunks(work_dir: &Path) -> usize {
    let sessions = work_dir.join("sessions");
    let mut recovered = 0usize;
    // The recovery order is deterministic (by session and chunk number): with several
    // orphaned chunks of one source their offsets in meta depend on the registration order,
    // while find_files returns the files in FS order.
    let mut parts = find_files(&sessions, "part");
    parts.sort_by_key(|p| {
        let seq = p
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(parse_src_seq)
            .map(|(_, s)| s);
        (p.parent().map(PathBuf::from), seq)
    });
    for part in parts {
        match recover_part(&part) {
            Ok(true) => {
                tracing::info!("chunk recovered after a crash: {}", part.display());
                register_recovered_in_meta(&part.with_extension("wav"));
                recovered += 1;
            }
            Ok(false) => {}
            Err(e) => tracing::warn!("recovery {}: {e}", part.display()),
        }
    }
    // The window between rename(.part→.wav) and the meta write: a crash / a busy meta.json
    // would leave the chunk forever invisible to the player and to the cook. We double-check:
    // any .wav in audio/ that is not listed in meta.chunks gets registered after the fact
    // (register_recovered_in_meta deduplicates by name; we do not touch .flac — its duration
    // cannot be derived from the file size).
    for wav in find_files(&sessions, "wav") {
        if wav.parent().and_then(|p| p.file_name()) == Some(std::ffi::OsStr::new("audio")) {
            register_recovered_in_meta(&wav);
        }
    }
    recovered
}

/// `src{N}_chunk{SEQ}` → (N, SEQ).
fn parse_src_seq(stem: &str) -> Option<(u8, u32)> {
    let rest = stem.strip_prefix("src")?;
    let (src, seq) = rest.split_once("_chunk")?;
    Some((src.parse().ok()?, seq.parse().ok()?))
}

/// Appends a recovered chunk to the session's `meta.json` (WP-A1, the last little thing):
/// the offset is the end of the last chunk of the same source (a sample-exact join), the
/// duration comes from the actual file size. Without a meta entry the chunk is invisible to
/// the player (clips are looked up through meta.chunks) and gives the cook no exact
/// timecodes.
fn register_recovered_in_meta(wav: &Path) {
    let Some(stem) = wav.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let Some((source_id, seq)) = parse_src_seq(stem) else {
        return;
    };
    let Some(session_dir) = wav.parent().and_then(|audio| audio.parent()) else {
        return;
    };
    let meta_path = session_dir.join("meta.json");
    let Ok(text) = fs::read_to_string(&meta_path) else {
        return; // sessions without a meta (the old format) are left alone
    };
    let Ok(mut meta) = serde_json::from_str::<SessionMeta>(&text) else {
        tracing::warn!("recovery: meta.json is corrupted — chunk {stem} was not registered");
        return;
    };
    if meta.chunks.iter().any(|c| c.file == stem) {
        return; // already written
    }
    let Ok(len) = fs::metadata(wav).map(|m| m.len()) else {
        return;
    };
    let samples = len.saturating_sub(WAV_HEADER_LEN) / BYTES_PER_SAMPLE;
    let duration_sec = samples as f64 / f64::from(SAMPLE_RATE);
    // The join: the start = the end of the PRECEDING chunks of this source (by number, not
    // over all of them!). An orphaned chunk need not be the last one: if chunk 4 failed to be
    // renamed while 5 closed normally, "the end of the maximum" would put the 4th AFTER the
    // 5th — and every clip and timecode after it would slide.
    let start_offset_sec = meta
        .chunks
        .iter()
        .filter(|c| c.source_id == source_id)
        .filter(|c| match parse_src_seq(&c.file) {
            Some((_, cs)) => cs < seq,
            None => true, // an off-schema name — counted conservatively
        })
        .map(|c| c.start_offset_sec + c.duration_sec)
        .fold(0.0f64, f64::max);
    // The metrics of the recovered chunk are computed from the file (the recorder is gone).
    let (rms, voice_ratio) = wav_level_stats(wav);
    meta.chunks.push(ChunkEntry {
        file: stem.to_string(),
        source_id,
        start_offset_sec,
        duration_sec,
        rms,
        voice_ratio,
    });
    save_meta(&meta_path, &meta);
    tracing::info!(
        "recovery: chunk {stem} appended to meta ({duration_sec:.1} s from {start_offset_sec:.1} s)"
    );
}

/// The signal level of an already written WAV (for recovery — the recorder is gone).
fn wav_level_stats(wav: &Path) -> (u16, f32) {
    let Ok(bytes) = fs::read(wav) else {
        return (0, 0.0);
    };
    if bytes.len() <= WAV_HEADER_LEN as usize {
        return (0, 0.0);
    }
    let mut stats = LevelStats::default();
    let samples: Vec<i16> = bytes[WAV_HEADER_LEN as usize..]
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    stats.feed(&samples);
    stats.finish()
}

fn recover_part(part: &Path) -> std::io::Result<bool> {
    let len = fs::metadata(part)?.len();
    if len <= WAV_HEADER_LEN {
        fs::remove_file(part)?;
        return Ok(false);
    }
    let data_bytes = len - WAV_HEADER_LEN;
    let mut f = OpenOptions::new().read(true).write(true).open(part)?;
    let mut magic = [0u8; 4];
    f.read_exact(&mut magic)?;
    if &magic != b"RIFF" {
        return Ok(false); // not our file
    }
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&wav_header(data_bytes))?;
    f.sync_all()?;
    drop(f);
    fs::rename(part, part.with_extension("wav"))?;
    Ok(true)
}

/// Deletes audio chunks older than `days` (by mtime). `days == 0` — retention is off.
/// Transcripts and meta.json are NEVER touched (the data principles P2/P3).
pub fn sweep_audio_retention(work_dir: &Path, days: u32) -> usize {
    if days == 0 {
        return 0;
    }
    let cutoff = std::time::SystemTime::now() - Duration::from_secs(u64::from(days) * 24 * 3600);
    let sessions = work_dir.join("sessions");
    let mut removed = 0usize;
    for ext in ["wav", "flac"] {
        for f in find_files(&sessions, ext) {
            // only inside audio/ directories
            if f.parent().and_then(|p| p.file_name()) != Some(std::ffi::OsStr::new("audio")) {
                continue;
            }
            let old = fs::metadata(&f)
                .and_then(|m| m.modified())
                .map(|t| t < cutoff)
                .unwrap_or(false);
            if old && fs::remove_file(&f).is_ok() {
                removed += 1;
            }
        }
    }
    if removed > 0 {
        tracing::info!("retention: removed {removed} audio chunks older than {days} d.");
    }
    removed
}

/// Recursive search for files with the extension `ext` (the session tree is shallow).
fn find_files(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(find_files(&p, ext));
        } else if p.extension().and_then(|x| x.to_str()) == Some(ext) {
            out.push(p);
        }
    }
    out
}

fn spawn_flac_convert(ffmpeg: PathBuf, wav: PathBuf) {
    std::thread::Builder::new()
        .name("chunk-flac".into())
        .spawn(move || {
            let flac = wav.with_extension("flac");
            let status = std::process::Command::new(&ffmpeg)
                .args(["-y", "-v", "quiet", "-i"])
                .arg(&wav)
                .arg(&flac)
                .status();
            match status {
                Ok(s) if s.success() && flac.exists() => {
                    let _ = fs::remove_file(&wav);
                }
                Ok(s) => tracing::warn!("ffmpeg flac: status {s}, keeping the WAV"),
                Err(e) => tracing::warn!("ffmpeg did not start ({e}), keeping the WAV"),
            }
        })
        .ok();
}

// ─────────────────────────── tests ───────────────────────────

#[cfg(test)]
mod tests {

    /// A meeting the human NAMED must be visible in the directory name too:
    /// the human looks at it in the file explorer and in the backup. Cyrillic used to be
    /// simply thrown away, and "Планёрка" produced an empty label — that is, a directory
    /// with nothing but a date, indistinguishable from the rest of the day.
    #[test]
    fn a_russian_meeting_name_survives_in_the_folder_name() {
        assert_eq!(slug("Планёрка"), "planerka");
        assert_eq!(slug("1:1 с Иваном"), "1-1-s-ivanom");
        assert_eq!(slug("Daily standup"), "daily-standup");
        // A title made of punctuation alone — nothing to transliterate, and that is not an
        // error: the session simply stays with its date.
        assert_eq!(slug("!!!"), "");
        assert!(slug("оченьдлинноеназваниевстречикотороеточнонепоместитсявсорокзнаков").len() <= 40);
    }

    use super::*;
    use tempfile::tempdir;

    #[test]
    fn streaming_wav_writes_valid_header() {
        let dir = tempdir().unwrap();
        let part = dir.path().join("a.part");
        let mut w = StreamingWav::create(part.clone()).unwrap();
        let samples = vec![7i16; 16000]; // 1 second
        w.write(&samples).unwrap();
        assert!((w.duration_sec() - 1.0).abs() < 1e-9);
        let path = w.finalize().unwrap();
        assert_eq!(path.extension().unwrap(), "wav");
        let bytes = fs::read(&path).unwrap();
        assert_eq!(&bytes[..4], b"RIFF");
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data as usize, samples.len() * 2);
        assert_eq!(bytes.len() as u64, WAV_HEADER_LEN + data as u64);
    }

    #[test]
    fn empty_wav_is_removed_on_finalize() {
        let dir = tempdir().unwrap();
        let part = dir.path().join("empty.part");
        let w = StreamingWav::create(part.clone()).unwrap();
        assert!(w.finalize().is_none());
        assert!(!part.exists() && !part.with_extension("wav").exists());
    }

    #[test]
    fn recorder_rotates_by_duration_with_sample_exact_continuity() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("audio");
        fs::create_dir_all(&audio).unwrap();
        let meta_path = dir.path().join("meta.json");
        let params = Arc::new(ChunkParams {
            audio_dir: audio.clone(),
            meta_path: meta_path.clone(),
            chunk_sec: 1.0,
            flac: false,
            ffmpeg: PathBuf::from("ffmpeg"),
        });
        let meta = Arc::new(Mutex::new(SessionMeta::default()));
        let mut rec = ChunkRecorder::new(0, params, meta.clone());
        // 3.5 seconds in 20 ms portions → chunks of ~1 s
        for _ in 0..175 {
            rec.feed(&vec![0i16; 320]);
        }
        rec.finalize_current();
        let m = meta.lock().unwrap();
        assert!(
            m.chunks.len() >= 3,
            "expected ≥3 chunks, got {}",
            m.chunks.len()
        );
        // sample-exact join: the start of the next one == the end of the previous one
        // (internally these are whole samples; in seconds the tolerance is only the ulp of
        // the division)
        for pair in m.chunks.windows(2) {
            let end = pair[0].start_offset_sec + pair[0].duration_sec;
            assert!((pair[1].start_offset_sec - end).abs() < 1e-9);
        }
        assert!(meta_path.exists());
        // the directory contents match the manifest
        for c in &m.chunks {
            assert!(audio.join(format!("{}.wav", c.file)).exists());
        }
    }

    #[test]
    fn recorder_pause_closes_and_resumes_new_chunk() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("audio");
        fs::create_dir_all(&audio).unwrap();
        let params = Arc::new(ChunkParams {
            audio_dir: audio.clone(),
            meta_path: dir.path().join("meta.json"),
            chunk_sec: 100.0,
            flac: false,
            ffmpeg: PathBuf::from("ffmpeg"),
        });
        let meta = Arc::new(Mutex::new(SessionMeta::default()));
        let mut rec = ChunkRecorder::new(0, params, meta.clone());
        rec.feed(&vec![0i16; 16000]);
        rec.finalize_current(); // pause
        rec.feed(&vec![0i16; 8000]); // resume
        rec.finalize_current();
        let m = meta.lock().unwrap();
        assert_eq!(m.chunks.len(), 2);
        assert_eq!(m.chunks[0].duration_sec, 1.0);
        assert_eq!(m.chunks[1].duration_sec, 0.5);
        // resuming continues the audio timeline with no gaps
        let end0 = m.chunks[0].start_offset_sec + m.chunks[0].duration_sec;
        assert!((m.chunks[1].start_offset_sec - end0).abs() < 1e-9);
    }

    #[test]
    fn meta_carries_level_metrics_per_chunk() {
        // Diagnosing "why did it recognize badly": the loudness and the speech fraction must
        // land in the meta per chunk separately, without a second pass over the audio.
        let dir = tempdir().unwrap();
        let audio = dir.path().join("audio");
        fs::create_dir_all(&audio).unwrap();
        let params = Arc::new(ChunkParams {
            audio_dir: audio,
            meta_path: dir.path().join("meta.json"),
            chunk_sec: 100.0,
            flac: false,
            ffmpeg: PathBuf::from("ffmpeg"),
        });
        let meta = Arc::new(Mutex::new(SessionMeta::default()));
        let mut rec = ChunkRecorder::new(0, params, meta.clone());

        // chunk 1: a second of loud signal + a second of silence → half voice
        rec.feed(&vec![2000i16; SAMPLE_RATE as usize]);
        rec.feed(&vec![0i16; SAMPLE_RATE as usize]);
        rec.finalize_current();
        // chunk 2: pure silence — the counters MUST reset between chunks
        rec.feed(&vec![0i16; SAMPLE_RATE as usize]);
        rec.finalize_current();

        let m = meta.lock().unwrap();
        assert_eq!(m.chunks.len(), 2);
        // the RMS of half of a full 2000 signal ≈ 1414
        assert!(
            (1350..=1500).contains(&m.chunks[0].rms),
            "rms of the loud chunk: {}",
            m.chunks[0].rms
        );
        assert!(
            (m.chunks[0].voice_ratio - 0.5).abs() < 0.02,
            "voice_ratio: {}",
            m.chunks[0].voice_ratio
        );
        assert_eq!(m.chunks[1].rms, 0);
        assert_eq!(m.chunks[1].voice_ratio, 0.0);
    }

    #[test]
    fn failed_chunk_metrics_do_not_leak_into_the_next_one() {
        // The audio/ directory is unavailable (disk/antivirus/a race with the sweeper) → the
        // chunk did not open. Its loud samples MUST NOT surface in the next chunk:
        // otherwise silence reports itself as loud speech — exactly the inversion of the
        // field's meaning.
        let dir = tempdir().unwrap();
        let audio = dir.path().join("audio");
        let params = Arc::new(ChunkParams {
            audio_dir: audio.clone(),
            meta_path: dir.path().join("meta.json"),
            chunk_sec: 100.0,
            flac: false,
            ffmpeg: PathBuf::from("ffmpeg"),
        });
        let meta = Arc::new(Mutex::new(SessionMeta::default()));
        let mut rec = ChunkRecorder::new(0, params, meta.clone());

        rec.feed(&vec![3000i16; SAMPLE_RATE as usize]); // the directory is not there yet — no file created
        rec.finalize_current();
        fs::create_dir_all(&audio).unwrap(); // the directory is back
        rec.feed(&vec![0i16; SAMPLE_RATE as usize]); // pure silence
        rec.finalize_current();

        let m = meta.lock().unwrap();
        assert_eq!(m.chunks.len(), 1, "only the second chunk was written");
        assert_eq!(
            m.chunks[0].rms, 0,
            "the loudness of the dead chunk leaked into the silence"
        );
        assert_eq!(m.chunks[0].voice_ratio, 0.0);
    }

    #[test]
    fn recover_orphan_patches_and_renames() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("sessions/20260711_120000/audio");
        fs::create_dir_all(&audio).unwrap();
        let part = audio.join("src0_chunk0001.part");
        // simulate a crash: a header with zero length + data
        let mut w = StreamingWav::create(part.clone()).unwrap();
        w.write(&vec![3i16; 32000]).unwrap();
        std::mem::forget(w); // no finalize — as on a kill

        let n = recover_orphan_chunks(dir.path());
        assert_eq!(n, 1);
        let wav = audio.join("src0_chunk0001.wav");
        assert!(wav.exists());
        let bytes = fs::read(&wav).unwrap();
        let data = u32::from_le_bytes(bytes[40..44].try_into().unwrap());
        assert_eq!(data as u64, 32000 * BYTES_PER_SAMPLE);
    }

    #[test]
    fn recovered_chunk_is_registered_in_meta_after_previous_ones() {
        let dir = tempdir().unwrap();
        let session = dir.path().join("sessions/20260712_rec");
        let audio = session.join("audio");
        fs::create_dir_all(&audio).unwrap();
        // the meta already knows the first chunk (2 s) — the recovered one must land after it
        let meta = SessionMeta {
            started_at: "t".into(),
            sample_rate: SAMPLE_RATE,
            chunks: vec![ChunkEntry {
                file: "src0_chunk0001".into(),
                source_id: 0,
                start_offset_sec: 0.0,
                duration_sec: 2.0,
                rms: 0,
                voice_ratio: 0.0,
            }],
            ..Default::default()
        };
        save_meta(&session.join("meta.json"), &meta);
        // the crashed second chunk: 1 s of audio (16000 samples)
        let part = audio.join("src0_chunk0002.part");
        let mut w = StreamingWav::create(part).unwrap();
        w.write(&vec![1i16; 16_000]).unwrap();
        std::mem::forget(w);

        assert_eq!(recover_orphan_chunks(dir.path()), 1);
        let text = fs::read_to_string(session.join("meta.json")).unwrap();
        let m: SessionMeta = serde_json::from_str(&text).unwrap();
        assert_eq!(m.chunks.len(), 2, "the recovered chunk was not appended to the meta");
        let rec = m
            .chunks
            .iter()
            .find(|c| c.file == "src0_chunk0002")
            .unwrap();
        assert_eq!(rec.source_id, 0);
        assert!(
            (rec.start_offset_sec - 2.0).abs() < 1e-9,
            "the join is off by a shift: {rec:?}",
            rec = rec.start_offset_sec
        );
        assert!(
            (rec.duration_sec - 1.0).abs() < 1e-9,
            "duration from the file: {}",
            rec.duration_sec
        );
        // a repeated call does not duplicate the entry
        recover_orphan_chunks(dir.path());
        let m2: SessionMeta =
            serde_json::from_str(&fs::read_to_string(session.join("meta.json")).unwrap()).unwrap();
        assert_eq!(m2.chunks.len(), 2);
    }

    #[test]
    fn recovered_mid_timeline_chunk_lands_before_later_ones() {
        // Chunk 2 failed to be renamed (the antivirus was holding it), chunk 3 closed
        // normally. The recovered 2nd one MUST land BETWEEN 1 and 3, not at the end —
        // otherwise every clip/timecode after it slides.
        let dir = tempdir().unwrap();
        let session = dir.path().join("sessions/20260712_mid");
        let audio = session.join("audio");
        fs::create_dir_all(&audio).unwrap();
        let meta = SessionMeta {
            started_at: "t".into(),
            sample_rate: SAMPLE_RATE,
            chunks: vec![
                ChunkEntry {
                    file: "src0_chunk0001".into(),
                    source_id: 0,
                    start_offset_sec: 0.0,
                    duration_sec: 2.0,
                    rms: 0,
                    voice_ratio: 0.0,
                },
                // chunk 3 knows its true place (the engine counts the timeline in samples)
                ChunkEntry {
                    file: "src0_chunk0003".into(),
                    source_id: 0,
                    start_offset_sec: 3.0,
                    duration_sec: 2.0,
                    rms: 0,
                    voice_ratio: 0.0,
                },
            ],
            ..Default::default()
        };
        save_meta(&session.join("meta.json"), &meta);
        let part = audio.join("src0_chunk0002.part");
        let mut w = StreamingWav::create(part).unwrap();
        w.write(&vec![1i16; 16_000]).unwrap(); // 1 s
        std::mem::forget(w);

        recover_orphan_chunks(dir.path());
        let m: SessionMeta =
            serde_json::from_str(&fs::read_to_string(session.join("meta.json")).unwrap()).unwrap();
        let rec = m
            .chunks
            .iter()
            .find(|c| c.file == "src0_chunk0002")
            .unwrap();
        assert!(
            (rec.start_offset_sec - 2.0).abs() < 1e-9,
            "a mid-timeline chunk landed at the end of the timeline: {}",
            rec.start_offset_sec
        );
    }

    #[test]
    fn retention_zero_days_keeps_everything() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("sessions/s/audio");
        fs::create_dir_all(&audio).unwrap();
        fs::write(audio.join("src0_chunk0001.wav"), b"x").unwrap();
        assert_eq!(sweep_audio_retention(dir.path(), 0), 0);
        assert!(audio.join("src0_chunk0001.wav").exists());
    }

    #[test]
    fn retention_keeps_fresh_files() {
        let dir = tempdir().unwrap();
        let audio = dir.path().join("sessions/s/audio");
        fs::create_dir_all(&audio).unwrap();
        fs::write(audio.join("src0_chunk0001.wav"), b"x").unwrap();
        // the file was just created — a 14-day retention will not touch it
        assert_eq!(sweep_audio_retention(dir.path(), 14), 0);
        assert!(audio.join("src0_chunk0001.wav").exists());
    }
}
