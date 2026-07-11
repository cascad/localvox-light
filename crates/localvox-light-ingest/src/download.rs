//! yt-dlp + ffmpeg → PCM s16le 16 kHz mono (как в youtube-transcribe).

use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn pcm_s16le_to_f32(pcm: &[u8]) -> Vec<f32> {
    pcm
        .chunks_exact(2)
        .map(|c| {
            let v = i16::from_le_bytes([c[0], c[1]]);
            f32::from(v) / 32768.0
        })
        .collect()
}

pub fn download_audio(
    yt_dlp: &str,
    url: &str,
    ffmpeg_location: Option<&str>,
    js_runtime: Option<&str>,
    verbose: bool,
) -> Result<PathBuf> {
    let base = std::env::temp_dir().join(format!(
        "localvox_yt_{}_{}",
        std::process::id(),
        fast_simple_hash(url)
    ));
    let out_template = base.with_extension("%(ext)s");
    let mut args = vec![
        "-x",
        "-f",
        "bestaudio",
        "-o",
        out_template.to_str().unwrap(),
    ];
    if let Some(loc) = ffmpeg_location {
        args.push("--ffmpeg-location");
        args.push(loc);
    }
    if let Some(rt) = js_runtime {
        args.push("--js-runtime");
        args.push(rt);
    }
    args.push(url);
    let mut cmd = Command::new(yt_dlp);
    cmd.args(&args);
    if !verbose {
        cmd.stdout(std::process::Stdio::null());
        cmd.stderr(std::process::Stdio::null());
    }
    let status = cmd.status().context("yt-dlp (установите: https://github.com/yt-dlp/yt-dlp)")?;
    if !status.success() {
        anyhow::bail!("yt-dlp завершился с ошибкой (без --verbose stderr скрыт)");
    }
    for ext in ["webm", "m4a", "opus", "ogg", "mp3"] {
        let p = base.with_extension(ext);
        if p.exists() {
            return Ok(p);
        }
    }
    anyhow::bail!("yt-dlp не создал ожидаемый файл рядом с {:?}", base)
}

pub fn convert_to_pcm_s16le(ffmpeg: &str, input: &Path, verbose: bool) -> Result<Vec<u8>> {
    let mut cmd = Command::new(ffmpeg);
    cmd.args([
        "-i",
        input.to_str().unwrap(),
        "-ar",
        "16000",
        "-ac",
        "1",
        "-f",
        "s16le",
        "-",
    ]);
    cmd.stdout(std::process::Stdio::piped());
    if !verbose {
        cmd.stderr(std::process::Stdio::null());
    }
    let mut child = cmd.spawn().context("ffmpeg (нужен в PATH или рядом с exe)")?;
    let mut stdout = child.stdout.take().context("ffmpeg stdout")?;
    let mut pcm = Vec::new();
    stdout.read_to_end(&mut pcm)?;
    let st = child.wait()?;
    if !st.success() {
        anyhow::bail!("ffmpeg завершился с ошибкой");
    }
    Ok(pcm)
}

fn fast_simple_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
