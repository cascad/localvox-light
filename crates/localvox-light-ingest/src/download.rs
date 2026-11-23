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

/// All partial downloads and conversions belong to this object, including on error paths.
pub struct DownloadedAudio {
    _directory: tempfile::TempDir,
    path: PathBuf,
}
impl std::ops::Deref for DownloadedAudio {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.path
    }
}
impl AsRef<Path> for DownloadedAudio {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

/// The queue owns retries. These apply to each tool invocation, including metadata requests.
pub const NETWORK_ARGS: &[&str] = &[
    "--socket-timeout",
    "10",
    "--retries",
    "0",
    "--fragment-retries",
    "0",
    "--extractor-retries",
    "0",
    "--abort-on-unavailable-fragments",
    "--no-playlist",
];

pub fn download_audio(
    yt_dlp: &str,
    url: &str,
    ffmpeg_location: Option<&str>,
    js_runtime: Option<&str>,
    verbose: bool,
) -> Result<DownloadedAudio> {
    download_with_command(
        Command::new(yt_dlp),
        url,
        ffmpeg_location,
        js_runtime,
        verbose,
        tempfile::Builder::new()
            .prefix("localvox-download-")
            .tempdir()?,
        std::time::Duration::from_secs(1800),
    )
}

fn download_with_command(
    mut command: Command,
    url: &str,
    ffmpeg_location: Option<&str>,
    js_runtime: Option<&str>,
    verbose: bool,
    directory: tempfile::TempDir,
    timeout: std::time::Duration,
) -> Result<DownloadedAudio> {
    use localvox_light_process::{Limits, Scope};
    let output = directory.path().join("audio.%(ext)s");
    let manifest = directory.path().join("downloaded-path.txt");
    command
        .args(NETWORK_ARGS)
        .args([
            "--encoding",
            "utf-8",
            "-x",
            "-f",
            "bestaudio",
            "--no-simulate",
        ])
        .arg("-o")
        .arg(output)
        .args(["--print-to-file", "after_move:filepath"])
        .arg(&manifest);
    if let Some(location) = ffmpeg_location {
        command.arg("--ffmpeg-location").arg(location);
    }
    if let Some(runtime) = js_runtime {
        command.arg("--js-runtime").arg(runtime);
    }
    command.arg("--").arg(url);
    let mut limits = Limits::new(timeout);
    if std::env::var(localvox_light_process::CONTAINED_ENV).as_deref() == Ok("1") {
        limits.scope = Scope::Inherit;
    }
    let result = localvox_light_process::run(command, None, limits, || false, |_, _| {})
        .context("yt-dlp: не удалось запустить загрузку")?;
    let stderr = String::from_utf8_lossy(&result.stderr);
    if verbose {
        eprint!("{stderr}");
    }
    if !result.termination.success() {
        let why = stderr
            .lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .rev()
            .take(3)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join(" / ");
        anyhow::bail!(
            "yt-dlp: {:?}{}{}",
            result.termination,
            if why.is_empty() { "" } else { ": " },
            why
        );
    }
    let path = PathBuf::from(
        std::fs::read_to_string(&manifest)
            .context("yt-dlp не подтвердил путь скачанного аудио")?
            .trim(),
    );
    let path = path.canonicalize().context("yt-dlp не создал аудиофайл")?;
    let root = directory.path().canonicalize()?;
    anyhow::ensure!(
        path.parent() == Some(root.as_path()) && path.is_file() && path.metadata()?.len() > 0,
        "yt-dlp вернул недопустимый путь аудио"
    );
    Ok(DownloadedAudio {
        _directory: directory,
        path,
    })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    #[ignore = "private yt-dlp executable fixture, invoked by download ownership tests"]
    fn downloader_fixture() {
        let args: Vec<String> = std::env::args().collect();
        for (name, value) in [
            ("--socket-timeout", "10"),
            ("--retries", "0"),
            ("--fragment-retries", "0"),
            ("--extractor-retries", "0"),
        ] {
            let at = args.iter().position(|a| a == name).unwrap();
            assert_eq!(args[at + 1], value);
        }
        assert!(args.iter().any(|a| a == "--abort-on-unavailable-fragments"));
        let output = PathBuf::from(&args[args.iter().position(|a| a == "-o").unwrap() + 1]);
        let manifest =
            PathBuf::from(&args[args.iter().position(|a| a == "--print-to-file").unwrap() + 2]);
        std::fs::write(output.with_extension("flac.part"), b"partial").unwrap();
        match std::env::var("LOCALVOX_DOWNLOAD_FIXTURE").unwrap().as_str() {
            "failed" => {
                eprintln!("DOWNLOAD FIXTURE UNAVAILABLE");
                std::process::exit(42);
            }
            "timeout" => {
                std::thread::sleep(Duration::from_secs(10));
            }
            _ => {}
        }
        let path = output.with_extension("flac");
        std::fs::write(&path, b"complete fake audio").unwrap();
        std::fs::write(manifest, path.to_str().unwrap()).unwrap();
    }

    fn command(mode: &str) -> Command {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command.args([
            "--ignored",
            "--exact",
            "download::tests::downloader_fixture",
            "--nocapture",
            "--",
        ]);
        command.env("LOCALVOX_DOWNLOAD_FIXTURE", mode);
        command
    }

    #[test]
    fn downloaded_audio_owns_the_complete_workspace_until_the_decoder_finishes() {
        let parent = tempfile::tempdir().unwrap();
        let directory = tempfile::tempdir_in(parent.path()).unwrap();
        let owned = directory.path().to_path_buf();
        let audio = download_with_command(
            command("success"),
            "https://example.invalid/audio",
            None,
            None,
            false,
            directory,
            Duration::from_secs(3),
        )
        .unwrap();
        assert_eq!(audio.extension().unwrap(), "flac");
        assert!(audio.exists() && owned.exists());
        drop(audio);
        assert!(!owned.exists());
    }

    #[test]
    fn failed_or_timed_out_download_preserves_the_reason_and_removes_partials() {
        for mode in ["failed", "timeout"] {
            let parent = tempfile::tempdir().unwrap();
            let directory = tempfile::tempdir_in(parent.path()).unwrap();
            let owned = directory.path().to_path_buf();
            let start = Instant::now();
            let error = download_with_command(
                command(mode),
                "https://example.invalid/audio",
                None,
                None,
                false,
                directory,
                if mode == "timeout" {
                    Duration::from_millis(150)
                } else {
                    Duration::from_secs(3)
                },
            )
            .err()
            .expect("the failed download must not produce audio");
            let why = format!("{error:#}");
            assert!(
                why.contains(if mode == "timeout" {
                    "TimedOut"
                } else {
                    "DOWNLOAD FIXTURE UNAVAILABLE"
                }),
                "{why}"
            );
            assert!(start.elapsed() < Duration::from_secs(5));
            assert!(
                !owned.exists(),
                "a failed download left its partial workspace"
            );
        }
    }
}
