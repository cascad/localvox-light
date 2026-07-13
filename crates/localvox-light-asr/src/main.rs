//! `localvox-asr` — offline transcription via ONNX models.
//!
//! Right now a single adapter is supported: **GigaAM v3 E2E CTC** (with punctuation
//! and normalization). Download `v3_e2e_ctc.int8.onnx` + `v3_e2e_ctc_vocab.txt`
//! from `huggingface.co/istupakov/gigaam-v3-onnx` into the `--model-dir` directory.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use localvox_light_core::asr::onnx::adapters::GigaamV3E2eCtc;
use localvox_light_core::asr::onnx::OnnxEngine;
use localvox_light_ingest::{
    load_settings_named, pcm_s16le_to_f32,
    progress::{convert_to_pcm_with_progress, download_audio_with_progress, with_spinner},
    resolve_ffmpeg, resolve_ffmpeg_location_for_ytdlp, resolve_js_runtime, resolve_output_paths,
    resolve_yt_dlp, verify_ffmpeg, verify_js_runtime_path_if_explicit, verify_yt_dlp, OutputSpec,
};

/// Supported adapters.
#[derive(Clone, Copy, Debug, ValueEnum)]
enum ModelKind {
    /// GigaAM v3 E2E CTC: text with punctuation and normalization.
    /// Model file: `v3_e2e_ctc.onnx` or `v3_e2e_ctc.int8.onnx`. Vocabulary: `v3_e2e_ctc_vocab.txt`.
    #[value(name = "gigaam-v3-e2e-ctc")]
    GigaamV3E2eCtc,
}

enum MediaSource {
    Url(String),
    File(PathBuf),
}

#[derive(Parser)]
#[command(name = "localvox-asr")]
#[command(
    about = "Offline ASR via ONNX models (GigaAM v3 E2E CTC). Ingest: yt-dlp/file → ffmpeg → PCM → ONNX → text."
)]
struct AsrCli {
    /// Video URL (yt-dlp), may be repeated.
    #[arg(required = false, value_name = "URL")]
    urls: Vec<String>,

    /// Local media file (mp4, mkv, wav, mp3, …). May be repeated.
    #[arg(long, short = 'f', value_name = "FILE", action = clap::ArgAction::Append)]
    file: Vec<PathBuf>,

    /// Which adapter to use.
    #[arg(long, default_value = "gigaam-v3-e2e-ctc", env = "LOCALVOX_ASR_MODEL")]
    model: ModelKind,

    /// Directory with the model files (`*.onnx` + `vocab.txt`). The exact names depend on the adapter.
    #[arg(
        long = "model-dir",
        env = "LOCALVOX_ASR_MODEL_DIR",
        default_value = "models/gigaam-v3-e2e-ctc"
    )]
    model_dir: PathBuf,

    /// Explicit path to the .onnx file. If unset — chosen by the adapter's convention.
    #[arg(long = "model-file", env = "LOCALVOX_ASR_MODEL_FILE")]
    model_file: Option<PathBuf>,

    /// Result file (a single source).
    #[arg(short, long)]
    output: Option<PathBuf>,

    /// Results directory (several sources → `transcript_001.txt`, …).
    #[arg(long)]
    output_dir: Option<PathBuf>,

    /// Base directory for the default output.
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

    /// Show the stderr of yt-dlp / ffmpeg.
    #[arg(short, long)]
    verbose: bool,

    /// tracing logs to stderr.
    #[arg(long)]
    debug: bool,

    /// Only load the ONNX model and print the names/types of its inputs and outputs, then exit.
    /// Useful for cross-checking against the constants in the adapter.
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
            anyhow::bail!("--model-file: file not found: {}", p.display());
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
        "none of these were found in directory {}: {} — download the model and put it there (see README)",
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
        format!("Loading the ONNX model: {}", model_file.display()),
        "cyan",
        hide_ui,
        move || {
            let adapter = match kind {
                ModelKind::GigaamV3E2eCtc => GigaamV3E2eCtc::from_model_dir(&model_dir)
                    .context("building the GigaAM v3 E2E CTC adapter")?,
            };
            OnnxEngine::new(&model_file, adapter)
        },
    )
}

fn inspect_model(model_file: &Path) -> Result<()> {
    eprintln!("Loading ONNX: {}", model_file.display());
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
            "pass at least one URL, or the --file / -f option with a path to a local media file"
        );
    }

    for url in &cli.urls {
        url::Url::parse(url).with_context(|| format!("malformed URL: {url}"))?;
    }
    for path in &cli.file {
        let meta = std::fs::metadata(path)
            .with_context(|| format!("local file not found: {}", path.display()))?;
        if !meta.is_file() {
            anyhow::bail!(
                "--file expects a regular file, not a directory: {}",
                path.display()
            );
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
        verify_yt_dlp(&yt_dlp).context("checking dependencies")?;
        verify_js_runtime_path_if_explicit(js_runtime.as_deref())
            .context("checking dependencies")?;
    }
    verify_ffmpeg(&ffmpeg).context("checking dependencies")?;

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
                .with_context(|| format!("downloading: {url}"))?;
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
            MediaSource::File(path) => {
                convert_to_pcm_with_progress(cli.debug || cli.verbose, &ffmpeg, path, cli.verbose)
                    .with_context(|| format!("ffmpeg pcm: {}", path.display()))?
            }
        };

        let f32_pcm = pcm_s16le_to_f32(&pcm);
        eprintln!("  audio {:.1} s", f32_pcm.len() as f64 / 16000.0);
        eprintln!("  recognition (ONNX)…");
        let t0 = std::time::Instant::now();
        let text = engine
            .transcribe_pcm_16k_mono_f32(&f32_pcm)
            .context("ONNX inference")?;
        eprintln!("  done in {:.1} s", t0.elapsed().as_secs_f64());

        if let Some(parent) = out_file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(out_file, format!("{text}\n"))
            .with_context(|| format!("writing {}", out_file.display()))?;
        eprintln!("  done: {}", out_file.display());
    }

    Ok(())
}
