//! Скачивание аудио YouTube (yt-dlp + ffmpeg) и офлайн-транскрипция через Vosk.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use indicatif::{ProgressBar, ProgressDrawTarget, ProgressStyle};
use localvox_light_core::asr::vosk::VoskEngine;
use localvox_light_core::{init_tracing, normalized_model_path, validate_vosk_model_dir, Cli};
use localvox_light_ingest::{
    load_settings_named, pcm_s16le_to_f32,
    progress::{convert_to_pcm_with_progress, download_audio_with_progress, with_spinner},
    resolve_ffmpeg, resolve_ffmpeg_location_for_ytdlp, resolve_js_runtime, resolve_output_paths,
    resolve_yt_dlp, verify_ffmpeg, verify_js_runtime_path_if_explicit, verify_yt_dlp, OutputSpec,
};

enum MediaSource {
    Url(String),
    File(PathBuf),
}

#[derive(Parser)]
#[command(name = "localvox-youtube")]
#[command(about = "YouTube (yt-dlp) или локальный файл → ffmpeg → транскрипт (Vosk). Нужен ffmpeg; yt-dlp только для URL.")]
struct YoutubeCli {
    /// URL видео (ноль или больше; вместе с --file нужен хотя бы один URL или один файл)
    #[arg(required = false, value_name = "URL")]
    urls: Vec<String>,

    /// Локальный медиафайл (mp4, mkv, wav, mp3, … — всё, что читает ffmpeg). Можно повторять.
    #[arg(long, short = 'f', value_name = "FILE", action = clap::ArgAction::Append)]
    file: Vec<PathBuf>,

    /// Каталог модели Vosk (как у localvox-light)
    #[arg(
        long,
        default_value = "models/vosk-model-ru-0.42",
        env = "LOCALVOX_LIGHT_MODEL"
    )]
    model: String,

    /// Файл результата при одном источнике (по умолчанию transcript.txt в каталоге из --youtube-output-dir / LOCALVOX_LIGHT_YOUTUBE_OUTPUT_DIR / settings)
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Каталог для нескольких источников (файлы transcript_001.txt, …)
    #[arg(long)]
    output_dir: Option<PathBuf>,

    /// Базовый каталог для вывода по умолчанию (один источник → …/transcript.txt; несколько → этот каталог вместо youtube-transcripts/)
    #[arg(long, env = "LOCALVOX_LIGHT_YOUTUBE_OUTPUT_DIR")]
    youtube_output_dir: Option<PathBuf>,

    #[arg(long = "yt-dlp", env = "LOCALVOX_LIGHT_YT_DLP")]
    yt_dlp: Option<PathBuf>,

    #[arg(long, env = "LOCALVOX_LIGHT_YT_FFMPEG")]
    ffmpeg: Option<PathBuf>,

    /// Среда для yt-dlp (`node`, `deno` или `node:C:/path/node.exe`)
    #[arg(long = "js-runtime", env = "LOCALVOX_LIGHT_YT_JS_RUNTIME")]
    js_runtime: Option<String>,

    #[arg(long = "js-runtime-path", env = "LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH")]
    js_runtime_path: Option<String>,

    /// Показывать stderr yt-dlp / ffmpeg
    #[arg(short, long)]
    verbose: bool,

    /// Логи tracing в stderr
    #[arg(long)]
    debug: bool,
}

fn portable_env_bootstrap() {
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    else {
        let _ = dotenvy::dotenv().ok();
        return;
    };
    let dotenv_path = exe_dir.join(".env");
    if dotenv_path.is_file() {
        let _ = dotenvy::from_path_override(&dotenv_path);
    } else {
        let _ = dotenvy::dotenv().ok();
    }
    let vosk_lib = exe_dir.join("vosk-lib");
    if vosk_lib.is_dir() {
        prepend_native_lib_search_path(&vosk_lib);
    }
}

#[cfg(windows)]
fn prepend_native_lib_search_path(dir: &Path) {
    let dir = dir.to_string_lossy();
    match std::env::var("PATH") {
        Ok(cur) => std::env::set_var("PATH", format!("{dir};{cur}")),
        Err(_) => std::env::set_var("PATH", dir.as_ref()),
    }
}

#[cfg(target_os = "macos")]
fn prepend_native_lib_search_path(dir: &Path) {
    let dir = dir.to_string_lossy();
    match std::env::var("DYLD_LIBRARY_PATH") {
        Ok(cur) => std::env::set_var("DYLD_LIBRARY_PATH", format!("{dir}:{cur}")),
        Err(_) => std::env::set_var("DYLD_LIBRARY_PATH", dir.as_ref()),
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn prepend_native_lib_search_path(dir: &Path) {
    let dir = dir.to_string_lossy();
    match std::env::var("LD_LIBRARY_PATH") {
        Ok(cur) => std::env::set_var("LD_LIBRARY_PATH", format!("{dir}:{cur}")),
        Err(_) => std::env::set_var("LD_LIBRARY_PATH", dir.as_ref()),
    }
}

fn main() -> Result<()> {
    portable_env_bootstrap();
    let cli = YoutubeCli::parse();

    if cli.urls.is_empty() && cli.file.is_empty() {
        anyhow::bail!("укажите хотя бы один URL или ключ --file / -f с путём к локальному медиафайлу");
    }

    for url in &cli.urls {
        url::Url::parse(url).with_context(|| format!("некорректный URL: {url}"))?;
    }

    for path in &cli.file {
        let meta = std::fs::metadata(path)
            .with_context(|| format!("локальный файл не найден: {}", path.display()))?;
        if !meta.is_file() {
            anyhow::bail!("--file ожидает обычный файл, не каталог: {}", path.display());
        }
    }

    init_tracing(cli.debug, false);

    let settings = load_settings_named(&["localvox-youtube-settings.json", "settings.json"]);
    let yt_dlp = resolve_yt_dlp(&settings, cli.yt_dlp.as_ref());
    let ffmpeg = resolve_ffmpeg(&settings, cli.ffmpeg.as_ref());
    let ffmpeg_location = resolve_ffmpeg_location_for_ytdlp(&ffmpeg);
    let js_runtime = resolve_js_runtime(
        &settings,
        cli.js_runtime.as_deref(),
        cli.js_runtime_path.as_deref(),
    );

    if !cli.urls.is_empty() {
        verify_yt_dlp(&yt_dlp).context("проверка зависимостей")?;
        verify_js_runtime_path_if_explicit(js_runtime.as_deref()).context("проверка зависимостей")?;
    }
    verify_ffmpeg(&ffmpeg).context("проверка зависимостей")?;

    let model_cli = Cli::try_parse_from(["localvox-light", "--model", cli.model.as_str()])
        .map_err(|e| anyhow::anyhow!("внутренняя ошибка clap: {e}"))?;
    let model_path = PathBuf::from(normalized_model_path(&model_cli));
    validate_vosk_model_dir(&model_path)?;

    let engine = load_vosk_engine_with_spinner(&model_path, cli.debug).context("загрузка Vosk")?;

    let output_base = cli
        .youtube_output_dir
        .clone()
        .or_else(|| {
            settings
                .output_dir
                .as_ref()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty())
                .map(PathBuf::from)
        });
    let out_paths = resolve_output_paths(OutputSpec {
        count: cli.urls.len() + cli.file.len(),
        output: cli.output.as_deref(),
        output_dir: cli.output_dir.as_deref(),
        output_base: output_base.as_deref(),
        default_multi_dir: "youtube-transcripts",
        single_filename: "transcript.txt",
        multi_prefix: "transcript",
    })?;

    let sources: Vec<MediaSource> = cli
        .urls
        .iter()
        .cloned()
        .map(MediaSource::Url)
        .chain(cli.file.iter().cloned().map(MediaSource::File))
        .collect();

    for (source, out_file) in sources.into_iter().zip(out_paths.iter()) {
        match &source {
            MediaSource::Url(url) => eprintln!("→ {url}"),
            MediaSource::File(p) => eprintln!("→ {}", p.display()),
        }

        let pcm = match &source {
            MediaSource::Url(url) => {
                let temp = download_audio_with_progress(
                    cli.debug || cli.verbose,
                    &yt_dlp,
                    url,
                    ffmpeg_location.as_deref(),
                    js_runtime.as_deref(),
                    cli.verbose,
                )
                .with_context(|| format!("скачивание: {url}"))?;
                let pcm = convert_to_pcm_with_progress(
                    cli.debug || cli.verbose,
                    &ffmpeg,
                    &temp,
                    cli.verbose,
                )
                .context("ffmpeg pcm")?;
                let _ = std::fs::remove_file(&temp);
                pcm
            }
            MediaSource::File(path) => convert_to_pcm_with_progress(
                cli.debug || cli.verbose,
                &ffmpeg,
                path,
                cli.verbose,
            )
            .with_context(|| format!("ffmpeg pcm: {}", path.display()))?,
        };

        let f32_pcm = pcm_s16le_to_f32(&pcm);
        eprintln!(
            "  аудио {:.1} с",
            f32_pcm.len() as f64 / 16000.0
        );
        let text = transcribe_with_progress(&engine, &f32_pcm, cli.debug).context("Vosk")?;
        if let Some(parent) = out_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out_file, format!("{text}\n")).with_context(|| {
            format!(
                "запись {}",
                out_file.display()
            )
        })?;
        eprintln!("  готово: {}", out_file.display());
    }

    Ok(())
}

fn load_vosk_engine_with_spinner(model_path: &Path, hide_ui: bool) -> Result<VoskEngine> {
    let model_path = model_path.to_path_buf();
    with_spinner("Загрузка модели Vosk…", "cyan", hide_ui, move || {
        VoskEngine::new(&model_path).context("загрузка Vosk")
    })
}

/// Полоса по доле обработанных сэмплов (точный процент до 100%).
fn transcribe_with_progress(
    engine: &VoskEngine,
    samples: &[f32],
    hide_ui: bool,
) -> Result<String> {
    if hide_ui {
        return engine.transcribe_pcm_16k_mono_f32(samples);
    }

    const MIN_SAMPLES: usize = 16000 * 5;
    if samples.len() < MIN_SAMPLES {
        return engine.transcribe_pcm_16k_mono_f32(samples);
    }

    let pb = ProgressBar::new(100);
    pb.set_draw_target(ProgressDrawTarget::stderr());
    pb.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {pos:>3}%  {wide_msg}",
        )
        .expect("bar template"),
    );
    pb.set_message("распознавание Vosk");

    let total = samples.len().max(1) as u128;
    let out = engine.transcribe_pcm_16k_mono_f32_with_progress(samples, |done, _| {
        let pct = ((done as u128 * 100) / total).min(100) as u64;
        pb.set_position(pct);
    });

    pb.finish_and_clear();
    out
}
