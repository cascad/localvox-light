//! Индикаторы прогресса (indicatif) поверх `download.rs`: скачивание, ffmpeg-конвертация
//! плюс универсальный [`with_spinner`] для долгой работы в фоновом потоке (например,
//! загрузка ASR-модели). При `hide_ui = true` всё работает синхронно — удобно для
//! `--debug`, где спиннер перекрывает логи tracing.

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
            .tick_strings(&[
                "⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏",
            ]),
    );
    pb.enable_steady_tick(Duration::from_millis(80));
    pb.set_message(message);
    pb
}

/// Запустить `work` в фоновом потоке со спиннером в stderr.
/// При `hide_ui = true` спиннер не рисуется и `work` выполняется в текущем потоке.
///
/// `color` — имя цвета indicatif (например, `"cyan"`, `"yellow"`, `"magenta"`).
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
        .context("фоновый поток завершился без ответа (panic?)")??;
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
        format!("Скачивание аудио: {url_for_msg}"),
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
        "Конвертация в PCM (ffmpeg)…",
        "magenta",
        false,
        move || download::convert_to_pcm_s16le(&ffmpeg, &input, false),
    )
}
