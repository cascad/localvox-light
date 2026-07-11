//! `localvox-asr` — офлайн-транскрипция через ONNX-модели.
//!
//! Сейчас поддерживается один адаптер: **GigaAM v3 E2E CTC** (с пунктуацией
//! и нормализацией). Скачайте `v3_e2e_ctc.int8.onnx` + `v3_e2e_ctc_vocab.txt`
//! с `huggingface.co/istupakov/gigaam-v3-onnx` в каталог `--model-dir`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use localvox_light_core::asr::onnx::adapters::GigaamV3E2eCtc;
use localvox_light_core::asr::onnx::OnnxEngine;
use localvox_light_ingest::{
    load_settings_named,
    progress::{convert_to_pcm_with_progress, download_audio_with_progress, with_spinner},
    pcm_s16le_to_f32, resolve_ffmpeg, resolve_ffmpeg_location_for_ytdlp, resolve_js_runtime,
    resolve_output_paths, resolve_yt_dlp, verify_ffmpeg, verify_js_runtime_path_if_explicit,
    verify_yt_dlp, OutputSpec,
};

/// Поддерживаемые адаптеры.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum ModelKind {
    /// GigaAM v3 E2E CTC: текст с пунктуацией и нормализацией.
    /// Файл модели: `v3_e2e_ctc.onnx` или `v3_e2e_ctc.int8.onnx`. Словарь: `v3_e2e_ctc_vocab.txt`.
    #[value(name = "gigaam-v3-e2e-ctc")]
    GigaamV3E2eCtc,
}

enum MediaSource {
    Url(String),
    File(PathBuf),
}

#[derive(Parser)]
#[command(name = "localvox-asr")]
#[command(about = "Офлайн-ASR через ONNX-модели (GigaAM v3 E2E CTC). Ingest: yt-dlp/файл → ffmpeg → PCM → ONNX → текст.")]
struct AsrCli {
    /// URL видео (yt-dlp), можно повторять.
    #[arg(required = false, value_name = "URL")]
    urls: Vec<String>,

    /// Локальный медиафайл (mp4, mkv, wav, mp3, …). Можно повторять.
    #[arg(long, short = 'f', value_name = "FILE", action = clap::ArgAction::Append)]
    file: Vec<PathBuf>,

    /// Какой адаптер использовать.
    #[arg(long, default_value = "gigaam-v3-e2e-ctc", env = "LOCALVOX_ASR_MODEL")]
    model: ModelKind,

    /// Каталог с файлами модели (`*.onnx` + `vocab.txt`). Конкретные имена зависят от адаптера.
    #[arg(
        long = "model-dir",
        env = "LOCALVOX_ASR_MODEL_DIR",
        default_value = "models/gigaam-v3-e2e-ctc"
    )]
    model_dir: PathBuf,

    /// Явный путь к .onnx-файлу. Если не задан — выбирается по конвенции адаптера.
    #[arg(long = "model-file", env = "LOCALVOX_ASR_MODEL_FILE")]
    model_file: Option<PathBuf>,

    /// Файл результата (один источник).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Каталог результатов (несколько источников → `transcript_001.txt`, …).
    #[arg(long)]
    output_dir: Option<PathBuf>,

    /// Базовый каталог для вывода по умолчанию.
    #[arg(long, env = "LOCALVOX_ASR_OUTPUT_DIR")]
    asr_output_dir: Option<PathBuf>,

    #[arg(long = "yt-dlp", env = "LOCALVOX_LIGHT_YT_DLP")]
    yt_dlp: Option<PathBuf>,

    #[arg(long, env = "LOCALVOX_LIGHT_YT_FFMPEG")]
    ffmpeg: Option<PathBuf>,

    #[arg(long = "js-runtime", env = "LOCALVOX_LIGHT_YT_JS_RUNTIME")]
    js_runtime: Option<String>,

    #[arg(long = "js-runtime-path", env = "LOCALVOX_LIGHT_YT_JS_RUNTIME_PATH")]
    js_runtime_path: Option<String>,

    /// Показывать stderr yt-dlp / ffmpeg.
    #[arg(short, long)]
    verbose: bool,

    /// Логи tracing в stderr.
    #[arg(long)]
    debug: bool,

    /// Только загрузить ONNX-модель и напечатать имена/типы её входов и выходов,
    /// затем выйти. Полезно, чтобы сверить с константами в адаптере.
    #[arg(long = "inspect-model")]
    inspect_model: bool,
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
}

fn resolve_model_file(
    kind: ModelKind,
    model_dir: &Path,
    override_file: Option<&Path>,
) -> Result<PathBuf> {
    if let Some(p) = override_file {
        if !p.is_file() {
            anyhow::bail!("--model-file: файл не найден: {}", p.display());
        }
        return Ok(p.to_path_buf());
    }
    let candidates: Vec<&str> = match kind {
        ModelKind::GigaamV3E2eCtc => vec!["v3_e2e_ctc.int8.onnx", "v3_e2e_ctc.onnx"],
    };
    for name in &candidates {
        let p = model_dir.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    anyhow::bail!(
        "в каталоге {} не найден ни один из: {} — скачайте модель и положите туда (см. README)",
        model_dir.display(),
        candidates.join(", ")
    )
}

fn load_adapter_and_engine(
    kind: ModelKind,
    model_dir: &Path,
    model_file: &Path,
    hide_ui: bool,
) -> Result<OnnxEngine> {
    let model_dir = model_dir.to_path_buf();
    let model_file = model_file.to_path_buf();
    with_spinner(
        format!("Загрузка модели ONNX: {}", model_file.display()),
        "cyan",
        hide_ui,
        move || {
            let adapter = match kind {
                ModelKind::GigaamV3E2eCtc => GigaamV3E2eCtc::from_model_dir(&model_dir)
                    .context("сборка адаптера GigaAM v3 E2E CTC")?,
            };
            OnnxEngine::new(&model_file, adapter)
        },
    )
}

fn inspect_model(model_file: &Path) -> Result<()> {
    eprintln!("Загрузка ONNX: {}", model_file.display());
    let (inputs, outputs) = localvox_light_core::asr::onnx::inspect_model_io(model_file)?;
    println!("inputs:");
    for i in &inputs {
        println!("  - {}  (type={})", i.name, i.type_repr);
    }
    println!("outputs:");
    for o in &outputs {
        println!("  - {}  (type={})", o.name, o.type_repr);
    }
    Ok(())
}

fn main() -> Result<()> {
    portable_env_bootstrap();
    let cli = AsrCli::parse();

    if cli.inspect_model {
        let model_file = resolve_model_file(cli.model, &cli.model_dir, cli.model_file.as_deref())?;
        return inspect_model(&model_file);
    }

    if cli.urls.is_empty() && cli.file.is_empty() {
        anyhow::bail!(
            "укажите хотя бы один URL или ключ --file / -f с путём к локальному медиафайлу"
        );
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

    localvox_light_core::init_tracing(cli.debug, false);

    let model_file = resolve_model_file(cli.model, &cli.model_dir, cli.model_file.as_deref())?;

    let settings = load_settings_named(&["localvox-asr-settings.json", "settings.json"]);
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

    let engine = load_adapter_and_engine(cli.model, &cli.model_dir, &model_file, cli.debug)?;

    let output_base = cli.asr_output_dir.clone().or_else(|| {
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
        default_multi_dir: "asr-transcripts",
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
        eprintln!("  аудио {:.1} с", f32_pcm.len() as f64 / 16000.0);
        eprintln!("  распознавание (ONNX)…");
        let t0 = std::time::Instant::now();
        let text = engine
            .transcribe_pcm_16k_mono_f32(&f32_pcm)
            .context("ONNX inference")?;
        eprintln!("  готово за {:.1} с", t0.elapsed().as_secs_f64());

        if let Some(parent) = out_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out_file, format!("{text}\n"))
            .with_context(|| format!("запись {}", out_file.display()))?;
        eprintln!("  готово: {}", out_file.display());
    }

    Ok(())
}
