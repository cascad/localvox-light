//! Reading reference sets and the audio that goes with them.
//!
//! The manifest format is **NeMo jsonl**, the same one in which Golos, Russian LibriSpeech and
//! OpenSTT are distributed and in which GigaAM/T-one measure themselves:
//! `{"audio_filepath": "...", "text": "reference", "duration": 3.2}`.
//! The `audio`/`ref` fields are accepted as synonyms — so that one can assemble one's own set out
//! of one's own recordings by hand.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::Deserialize;

#[derive(Deserialize)]
struct RawLine {
    #[serde(alias = "audio")]
    audio_filepath: String,
    #[serde(alias = "ref", default)]
    text: String,
    #[serde(default)]
    duration: Option<f64>,
}

pub struct Utterance {
    pub audio: PathBuf,
    pub reference: String,
    pub duration_sec: Option<f64>,
}

/// Paths in the manifest are relative to the manifest's directory (that is how datasets ship them).
pub fn load_manifest(path: &Path) -> Result<Vec<Utterance>> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("reading the manifest {}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let raw: RawLine = serde_json::from_str(line)
            .with_context(|| format!("{}:{}: not NeMo jsonl", path.display(), i + 1))?;
        let audio = PathBuf::from(&raw.audio_filepath);
        let audio = if audio.is_absolute() {
            audio
        } else {
            base.join(audio)
        };
        out.push(Utterance {
            audio,
            reference: raw.text,
            duration_sec: raw.duration,
        });
    }
    if out.is_empty() {
        bail!("the manifest {} is empty", path.display());
    }
    Ok(out)
}

/// Audio → 16 kHz mono s16. A WAV in the right format is read directly (fast), everything else
/// (opus in Golos, mp3 in Common Voice, flac, 48 kHz, stereo) is converted with ffmpeg.
/// `-xerror`: a corrupted file is an error, not a silently truncated fragment.
pub fn load_audio_16k_mono(path: &Path) -> Result<Vec<i16>> {
    if path.extension().and_then(|e| e.to_str()) == Some("wav") {
        if let Ok(reader) = hound::WavReader::open(path) {
            let spec = reader.spec();
            if spec.sample_rate == 16_000
                && spec.channels == 1
                && spec.bits_per_sample == 16
                && spec.sample_format == hound::SampleFormat::Int
            {
                return reader
                    .into_samples::<i16>()
                    .collect::<std::result::Result<Vec<_>, _>>()
                    .with_context(|| format!("reading {}", path.display()));
            }
        }
    }
    decode_via_ffmpeg(path)
}

fn decode_via_ffmpeg(path: &Path) -> Result<Vec<i16>> {
    let ffmpeg = localvox_light_core::chunks::resolve_ffmpeg_for_decode();
    let out = std::process::Command::new(&ffmpeg)
        .args(["-hide_banner", "-loglevel", "error", "-xerror", "-i"])
        .arg(path)
        .args(["-f", "s16le", "-ac", "1", "-ar", "16000", "-"])
        .output()
        .with_context(|| {
            format!(
                "running {} for {} (ffmpeg is needed in PATH or in LOCALVOX_LIGHT_YT_FFMPEG)",
                ffmpeg.to_string_lossy(),
                path.display()
            )
        })?;
    if !out.status.success() {
        bail!(
            "ffmpeg did not decode {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(out
        .stdout
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect())
}

/// Writing a 16 kHz mono s16 WAV (assembling long-form sets).
pub fn write_wav_16k_mono(path: &Path, samples: &[i16]) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut w = hound::WavWriter::create(path, spec)
        .with_context(|| format!("creating {}", path.display()))?;
    for &s in samples {
        w.write_sample(s)?;
    }
    w.finalize()
        .with_context(|| format!("finalizing {}", path.display()))?;
    Ok(())
}
