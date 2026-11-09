//! One logical chunk lease across WAV/FLAC, with bounded, supervised codecs.
use super::*;
use anyhow::{ensure, Context, Result};
use localvox_light_process::{Limits, Scope};
use sha2::{Digest, Sha256};
use std::sync::atomic::{AtomicBool, Ordering};

const MAX_PCM_BYTES: u64 = u32::MAX as u64 - 36;
const CODEC_TIMEOUT: Duration = Duration::from_secs(120);

fn key(stem: &str) -> Result<String> {
    ensure!(
        !stem.is_empty()
            && Path::new(stem).components().count() == 1
            && matches!(
                Path::new(stem).components().next(),
                Some(std::path::Component::Normal(_))
            ),
        "invalid audio chunk name"
    );
    let canonical = super::parse_src_seq(stem).map(|(id, seq)| format!("src{id}_chunk{seq}"));
    Ok(format!(
        "{:x}",
        Sha256::digest(canonical.as_deref().unwrap_or(stem).as_bytes())
    ))
}

fn lease_file(audio: &Path, stem: &str) -> Result<File> {
    let directory = audio
        .parent()
        .context("audio session")?
        .join(".audio-locks");
    fs::create_dir_all(&directory)?;
    Ok(OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(directory.join(key(stem)?))?)
}

fn acquire(
    audio: &Path,
    stem: &str,
    exclusive: bool,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
) -> Result<File> {
    let file = lease_file(audio, stem)?;
    loop {
        let result = if exclusive {
            file.try_lock()
        } else {
            file.try_lock_shared()
        };
        match result {
            Ok(()) => return Ok(file),
            Err(fs::TryLockError::Error(error)) => return Err(error.into()),
            Err(fs::TryLockError::WouldBlock) => {}
        }
        ensure!(!cancelled(), "audio operation cancelled");
        ensure!(Instant::now() < deadline, "audio chunk lease timed out");
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub fn with_read<T>(audio: &Path, stem: &str, read: impl FnOnce(&Path) -> Result<T>) -> Result<T> {
    with_read_cancelled(audio, stem, || false, read)
}
pub fn with_read_cancelled<T>(
    audio: &Path,
    stem: &str,
    cancelled: impl Fn() -> bool,
    read: impl FnOnce(&Path) -> Result<T>,
) -> Result<T> {
    let _lease = acquire(
        audio,
        stem,
        false,
        Instant::now() + CODEC_TIMEOUT,
        &cancelled,
    )?;
    // Select only after locking: conversion may have removed WAV while we waited.
    let wav = audio.join(format!("{stem}.wav"));
    let path = if wav.is_file() {
        wav
    } else {
        audio.join(format!("{stem}.flac"))
    };
    ensure!(path.is_file(), "audio chunk missing");
    read(&path)
}

/// STREAMINFO is mandatory first metadata (RFC 9639 §8.2). Counts remain untrusted
/// until decoded bytes have been checked; never allocate from this header.
pub fn samples(path: &Path) -> Result<u64> {
    if path.extension().and_then(|s| s.to_str()) == Some("wav") {
        return capture::wav_samples(path);
    }
    let mut file = File::open(path)?;
    let mut header = [0; 42];
    file.read_exact(&mut header)?;
    ensure!(
        &header[..4] == b"fLaC" && header[4] & 0x7f == 0 && header[5..8] == [0, 0, 34],
        "invalid FLAC STREAMINFO"
    );
    let packed = u64::from_be_bytes(header[18..26].try_into()?);
    ensure!(
        packed >> 44 == 16000 && (packed >> 41) & 7 == 0 && (packed >> 36) & 31 == 15,
        "unexpected FLAC PCM format"
    );
    let n = packed & 0xF_FFFF_FFFF;
    ensure!(
        n > 0 && n <= MAX_PCM_BYTES / 2,
        "unknown or excessive FLAC sample count"
    );
    Ok(n)
}

fn limits(deadline: Instant) -> Result<Limits> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .context("codec timed out")?;
    ensure!(!remaining.is_zero(), "codec timed out");
    let mut limits = Limits::new(remaining);
    limits.output_bytes = 64 * 1024;
    if std::env::var(localvox_light_process::CONTAINED_ENV).as_deref() == Ok("1") {
        limits.scope = Scope::Inherit;
    }
    Ok(limits)
}

fn tool(ffmpeg: &Path, args: &[std::ffi::OsString]) -> std::process::Command {
    let mut command = std::process::Command::new(ffmpeg);
    command.args(args);
    command
}
type MakeTool<'a> = dyn Fn(&Path, &[std::ffi::OsString]) -> std::process::Command + 'a;

fn run_tool(
    ffmpeg: &Path,
    args: Vec<std::ffi::OsString>,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
    make: &MakeTool<'_>,
) -> Result<()> {
    ensure!(!cancelled(), "codec cancelled");
    let output = localvox_light_process::run(
        make(ffmpeg, &args),
        None,
        limits(deadline)?,
        cancelled,
        |_, _| {},
    )?;
    ensure!(
        output.termination.success(),
        "codec failed: {:?}: {}",
        output.termination,
        String::from_utf8_lossy(&output.stderr)
    );
    Ok(())
}

fn decode_file(
    ffmpeg: &Path,
    path: &Path,
    pcm: &Path,
    n: u64,
    deadline: Instant,
    cancelled: &impl Fn() -> bool,
    make: &MakeTool<'_>,
) -> Result<()> {
    let cap = n
        .checked_add(1)
        .and_then(|n| n.checked_mul(2))
        .context("PCM limit overflow")?;
    let args = [
        "-hide_banner",
        "-loglevel",
        "error",
        "-nostdin",
        "-y",
        "-xerror",
        "-i",
    ]
    .map(Into::into)
    .to_vec();
    let mut args: Vec<std::ffi::OsString> = args;
    args.push(path.into());
    args.extend(
        [
            "-map",
            "0:a:0",
            "-f",
            "s16le",
            "-c:a",
            "pcm_s16le",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-fs",
        ]
        .map(Into::into),
    );
    args.push(cap.to_string().into());
    args.push(pcm.into());
    run_tool(ffmpeg, args, deadline, cancelled, make)?;
    ensure!(
        fs::metadata(pcm)?.len() == n * 2,
        "decoded PCM sample count differs"
    );
    Ok(())
}

pub fn stream(
    path: &Path,
    ffmpeg: &Path,
    cancelled: impl Fn() -> bool,
    mut consume: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    let n = samples(path)?;
    let temporary = if path.extension().and_then(|s| s.to_str()) == Some("flac") {
        let temp = tempfile::tempdir_in(path.parent().context("audio directory")?)?;
        decode_file(
            ffmpeg,
            path,
            &temp.path().join("decoded.pcm"),
            n,
            Instant::now() + CODEC_TIMEOUT,
            &cancelled,
            &tool,
        )?;
        Some(temp)
    } else {
        None
    };
    let mut file = File::open(
        temporary
            .as_ref()
            .map(|t| t.path().join("decoded.pcm"))
            .unwrap_or_else(|| path.into()),
    )?;
    if temporary.is_none() {
        file.seek(SeekFrom::Start(44))?;
    }
    let mut remaining = n * 2;
    let mut buf = [0u8; 64 * 1024];
    while remaining > 0 {
        ensure!(!cancelled(), "audio read cancelled");
        let count = remaining.min(buf.len() as u64) as usize;
        file.read_exact(&mut buf[..count])?;
        consume(&buf[..count])?;
        remaining -= count as u64;
    }
    Ok(())
}

pub fn range(path: &Path, from: u64, to: u64, ffmpeg: &Path) -> Result<Vec<u8>> {
    let n = samples(path)?;
    let a = from.min(n);
    let b = to.min(n);
    if b <= a {
        return Ok(vec![]);
    }
    let bytes = (b - a).checked_mul(2).context("PCM range overflow")?;
    if path.extension().and_then(|s| s.to_str()) == Some("wav") {
        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(44 + a * 2))?;
        let mut pcm = vec![0; usize::try_from(bytes)?];
        file.read_exact(&mut pcm)?;
        return Ok(pcm);
    }
    let mut command = std::process::Command::new(ffmpeg);
    command
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-nostdin",
            "-xerror",
            "-ss",
        ])
        .arg(format!("{:.9}", a as f64 / 16000.0))
        .arg("-i")
        .arg(path)
        .arg("-t")
        .arg(format!("{:.9}", (b - a) as f64 / 16000.0))
        .args([
            "-f",
            "s16le",
            "-c:a",
            "pcm_s16le",
            "-ac",
            "1",
            "-ar",
            "16000",
            "-",
        ]);
    let mut limits = limits(Instant::now() + Duration::from_secs(30))?;
    limits.background = false;
    limits.output_bytes = bytes.saturating_add(64 * 1024);
    let output = localvox_light_process::run(command, None, limits, || false, |_, _| {})?;
    ensure!(
        output.termination.success() && output.stdout.len() as u64 == bytes,
        "audio range decode failed or incomplete"
    );
    Ok(output.stdout)
}

fn digest(path: &Path, skip: u64, cancelled: &impl Fn() -> bool) -> Result<String> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(skip))?;
    let mut hash = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        ensure!(!cancelled(), "audio hashing cancelled");
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hash.update(&buf[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

pub fn convert(
    ffmpeg: &Path,
    wav: &Path,
    cancelled: impl Fn() -> bool,
    timeout: Duration,
) -> Result<()> {
    convert_with(ffmpeg, wav, &cancelled, timeout, &tool)
}
fn convert_with(
    ffmpeg: &Path,
    wav: &Path,
    cancelled: &impl Fn() -> bool,
    timeout: Duration,
    make: &MakeTool<'_>,
) -> Result<()> {
    let deadline = Instant::now() + timeout;
    let stop = || cancelled() || Instant::now() >= deadline;
    let cancelled = &stop;
    let audio = wav.parent().context("chunk directory")?;
    let stem = wav
        .file_stem()
        .and_then(|s| s.to_str())
        .context("chunk stem")?;
    let _lease = acquire(audio, stem, true, deadline, cancelled)?;
    let n = capture::wav_samples(wav)?;
    let original = digest(wav, 44, cancelled)?;
    let temp = tempfile::tempdir_in(audio)?;
    let encoded = temp.path().join("encoded.flac");
    let flac = wav.with_extension("flac");
    let candidate = if flac.is_file() {
        flac.clone()
    } else {
        let mut args: Vec<std::ffi::OsString> =
            ["-hide_banner", "-loglevel", "error", "-nostdin", "-y", "-i"]
                .map(Into::into)
                .to_vec();
        args.push(wav.into());
        args.extend(["-map", "0:a:0", "-c:a", "flac", "-f", "flac", "-fs"].map(Into::into));
        args.push((n * 4 + 1024 * 1024).to_string().into());
        args.push(encoded.clone().into());
        run_tool(ffmpeg, args, deadline, cancelled, make)?;
        encoded.clone()
    };
    ensure!(
        samples(&candidate)? == n,
        "FLAC header sample count differs"
    );
    let decoded = temp.path().join("decoded.pcm");
    decode_file(ffmpeg, &candidate, &decoded, n, deadline, cancelled, make)?;
    ensure!(
        digest(&decoded, 0, cancelled)? == original,
        "FLAC changed PCM samples"
    );
    OpenOptions::new()
        .read(true)
        .write(true)
        .open(&candidate)?
        .sync_all()
        .context("sync converted FLAC")?;
    if candidate != flac {
        ensure!(!flac.exists(), "FLAC appeared outside conversion owner");
        fs::rename(&candidate, &flac)?;
    }
    #[cfg(unix)]
    File::open(audio)?.sync_all()?;
    let generation = fs::read(audio.parent().unwrap().join("meta.json"))
        .ok()
        .and_then(|b| serde_json::from_slice::<SessionMeta>(&b).ok())
        .and_then(|m| m.capture_generation);
    let receipt = serde_json::json!({"version":1,"generation":generation,"samples":n,"pcm_sha256":original,"flac_sha256":digest(&flac,0,cancelled)?});
    let receipts = audio.parent().unwrap().join("audio-codecs");
    fs::create_dir_all(&receipts)?;
    crate::artifacts::atomic_write(
        &receipts.join(format!("{}.json", key(stem)?)),
        &serde_json::to_vec_pretty(&receipt)?,
    )?;
    ensure!(!cancelled(), "conversion cancelled before WAV removal");
    ensure!(
        capture::wav_samples(wav)? == n && digest(wav, 44, cancelled)? == original,
        "WAV changed during conversion"
    );
    fs::remove_file(wav)?;
    #[cfg(unix)]
    File::open(audio)?.sync_all()?;
    Ok(())
}

/// PCM identity from a conversion we verified before dropping WAV; legacy FLACs
/// still undergo full decode when read, but do not acquire invented provenance.
pub fn verify_conversion(audio: &Path, stem: &str, n: u64, generation: &str) -> Result<()> {
    let path = audio
        .parent()
        .unwrap()
        .join("audio-codecs")
        .join(format!("{}.json", key(stem)?));
    let receipt: serde_json::Value = serde_json::from_slice(&fs::read(path)?)?;
    let flac = audio.join(format!("{stem}.flac"));
    ensure!(
        receipt["version"] == 1
            && receipt["generation"] == generation
            && receipt["samples"] == n
            && receipt["flac_sha256"] == digest(&flac, 0, &|| false)?,
        "unconfirmed FLAC conversion"
    );
    Ok(())
}

/// At most two owned tasks (one running, one waiting). Busy conversion preserves WAV.
#[derive(Default)]
pub struct Conversions {
    tasks: Mutex<Vec<std::thread::JoinHandle<()>>>,
    cancelled: Arc<AtomicBool>,
    active: Arc<AtomicBool>,
}
impl Conversions {
    pub fn spawn(&self, ffmpeg: PathBuf, wav: PathBuf, owner: Option<Arc<Mutex<capture::Writer>>>) {
        let mut tasks = self.tasks.lock().unwrap_or_else(|e| e.into_inner());
        let mut pending = Vec::new();
        for task in tasks.drain(..) {
            if task.is_finished() {
                let _ = task.join();
            } else {
                pending.push(task)
            }
        }
        *tasks = pending;
        if tasks.len() >= 2 {
            tracing::warn!("FLAC resource busy; keeping WAV");
            return;
        }
        let cancelled = self.cancelled.clone();
        let active = self.active.clone();
        match std::thread::Builder::new()
            .name("chunk-flac".into())
            .spawn(move || {
                let _owner = owner;
                let deadline = Instant::now() + CODEC_TIMEOUT;
                while active
                    .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                    .is_err()
                {
                    if cancelled.load(Ordering::Relaxed) || Instant::now() >= deadline {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                struct Release(Arc<AtomicBool>);
                impl Drop for Release {
                    fn drop(&mut self) {
                        self.0.store(false, Ordering::Release)
                    }
                }
                let _active = Release(active);
                let remaining = deadline.saturating_duration_since(Instant::now());
                if let Err(error) = convert(
                    &ffmpeg,
                    &wav,
                    || cancelled.load(Ordering::Relaxed),
                    remaining,
                ) {
                    capture::report(
                        wav.parent().unwrap().parent().unwrap(),
                        "FLAC не подтверждён; WAV сохранён",
                        &error,
                    );
                }
            }) {
            Ok(task) => tasks.push(task),
            Err(error) => tracing::error!(%error,"FLAC thread not started; keeping WAV"),
        }
    }
}
impl Drop for Conversions {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        for task in self
            .tasks
            .get_mut()
            .unwrap_or_else(|e| e.into_inner())
            .drain(..)
        {
            let _ = task.join();
        }
    }
}

#[cfg(test)]
#[path = "codec_tests.rs"]
mod tests;
