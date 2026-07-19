//! yt-dlp + ffmpeg → PCM s16le 16 kHz mono (as in youtube-transcribe).

use anyhow::{Context, Result};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn pcm_s16le_to_f32(pcm: &[u8]) -> Vec<f32> {
    pcm.chunks_exact(2)
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
        // Same reason as in `source_title`: on Windows yt-dlp writes a redirected stream in the
        // ANSI code page, so without this its error text reaches our log as mojibake — and an
        // unreadable error is barely better than the discarded one it replaced.
        "--encoding",
        "utf-8",
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
    // STDERR IS CAPTURED, NOT DISCARDED.
    //
    // It used to be thrown away, and the failure surfaced as «yt-dlp exited with an error (without
    // --verbose its stderr is hidden)» — a message that tells the operator to go and reproduce by
    // hand what the daemon had already been told. Measured 18.07.2026: an ingest failed three
    // times with that text, and the real reason was sitting in the discarded stderr —
    // «Requested format is not available», YouTube's SABR experiment against a four-month-old
    // yt-dlp. Diagnosing it took a manual re-run of the exact command.
    //
    // An error is a value with context. The child's own words ARE the context, and the only
    // moment they exist is right here.
    let out = if verbose {
        cmd.status().map(|s| (s, String::new()))
    } else {
        cmd.stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .output()
            .map(|o| (o.status, String::from_utf8_lossy(&o.stderr).into_owned()))
    };
    let (status, stderr) =
        out.context("yt-dlp (install it: https://github.com/yt-dlp/yt-dlp)")?;
    if !status.success() {
        // The tail, not the whole log: yt-dlp narrates every step, and the reason is what it says
        // last. The whole thing would bury the answer in the progress it printed getting there.
        let why: Vec<&str> = stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .rev()
            .take(3)
            .collect();
        let why = why.into_iter().rev().collect::<Vec<_>>().join(" / ");
        if why.is_empty() {
            anyhow::bail!("yt-dlp завершился с ошибкой и ничего не сказал");
        }
        anyhow::bail!("yt-dlp: {why}");
    }
    for ext in ["webm", "m4a", "opus", "ogg", "mp3"] {
        let p = base.with_extension(ext);
        if p.exists() {
            return Ok(p);
        }
    }
    anyhow::bail!("yt-dlp did not create the expected file next to {:?}", base)
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
    let mut child = cmd
        .spawn()
        .context("ffmpeg (needed in PATH or next to the exe)")?;
    let mut stdout = child.stdout.take().context("ffmpeg stdout")?;
    let mut pcm = Vec::new();
    stdout.read_to_end(&mut pcm)?;
    let st = child.wait()?;
    if !st.success() {
        anyhow::bail!("ffmpeg exited with an error");
    }
    Ok(pcm)
}

fn fast_simple_hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}
