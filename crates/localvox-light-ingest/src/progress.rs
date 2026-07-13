//! Progress indicators (indicatif) on top of `download.rs`: downloading, ffmpeg
//! conversion, plus the universal [`with_spinner`] for long-running work in a
//! background thread (loading an ASR model, for example). With `hide_ui = true`
//! everything works synchronously — handy for `--debug`, where the spinner overlaps
//! the tracing logs.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};

use crate::download;

fn draw_target(hidden: bool) -> ProgressDrawTarget {
    if hidden {
        ProgressDrawTarget::hidden()
    } else {
        ProgressDrawTarget::stderr()
    }
}

fn make_spinner(color: &str, message: String) -> ProgressBar {
    let tmpl = format!("{{spinner:.{color}.bold}} {{wide_msg}} ({{elapsed_precise}})");
    let pb = ProgressBar::new_spinner();
    pb.set_draw_target(draw_target(false));
    pb.set_style(
        ProgressStyle::with_template(&tmpl)
            .expect("spinner template")
            .tick_strings(&["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"]),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_message(message);
    pb
}

/// Run `work` in a background thread with a spinner in stderr.
/// With `hide_ui = true` the spinner is not drawn and `work` runs in the current thread.
///
/// `color` — the name of an indicatif color (`"cyan"`, `"yellow"`, `"magenta"`, …).
pub fn with_spinner<T, F>(
    message: impl Into<String>,
    color: &str,
    hide_ui: bool,
    work: F,
) -> Result<T>
where
    F: FnOnce() -> Result<T> + Send + 'static,
    T: Send + 'static,
{
    if hide_ui {
        return work();
    }
    let pb = make_spinner(color, message.into());
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let _ = tx.send(work());
    });
    let res = rx
        .recv()
        .context("the background thread finished without an answer (a panic?)")??;
    pb.finish_and_clear();
    Ok(res)
}

pub fn download_audio_with_progress(
    hide_ui: bool,
    yt_dlp: &str,
    url: &str,
    ffmpeg_location: Option<&str>,
    js_runtime: Option<&str>,
    verbose: bool,
) -> Result<PathBuf> {
    if hide_ui {
        return download::download_audio(yt_dlp, url, ffmpeg_location, js_runtime, verbose);
    }

    let yt_dlp = yt_dlp.to_string();
    let url_for_msg = url.to_string();
    let url_for_work = url.to_string();
    let ffmpeg_location = ffmpeg_location.map(str::to_string);
    let js_runtime = js_runtime.map(str::to_string);

    with_spinner(
        format!("Downloading audio: {url_for_msg}"),
        "yellow",
        false,
        move || {
            download::download_audio(
                &yt_dlp,
                &url_for_work,
                ffmpeg_location.as_deref(),
                js_runtime.as_deref(),
                false,
            )
        },
    )
}

pub fn convert_to_pcm_with_progress(
    hide_ui: bool,
    ffmpeg: &str,
    input: &Path,
    verbose: bool,
) -> Result<Vec<u8>> {
    if hide_ui {
        return download::convert_to_pcm_s16le(ffmpeg, input, verbose);
    }

    let ffmpeg = ffmpeg.to_string();
    let input = input.to_path_buf();

    with_spinner(
        "Converting to PCM (ffmpeg)…",
        "magenta",
        false,
        move || download::convert_to_pcm_s16le(&ffmpeg, &input, false),
    )
}
