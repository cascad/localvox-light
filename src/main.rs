//! localvox-light: standalone local transcription.
//! Audio capture → VAD segmentation → disk → Vosk ASR → transcript.jsonl
//! No server, no network. Durable: survives crashes via WAV-on-disk queue.

mod asr;
mod audio;
mod events;
mod keys;
mod light_config;
mod pipeline;
mod session;
mod transcript;
mod tui;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use cpal::traits::DeviceTrait;
use crossbeam_channel::Sender;
use hound::WavReader;
use tracing::{debug, info};
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use std::sync::mpsc;

use asr::{speech_ratio, trim_to_speech, AsrEngine};
use events::{StructuredLog, UiMsg};
use pipeline::{PipelineConfig, SegmentReady};
use transcript::{TranscriptEntry, TranscriptWriter};

/// Ждёт поток движка не дольше `max_wait`. Если не успел — `process::exit(0)`:
/// необработанные WAV в рабочей папке без строки в `transcript.jsonl` подхватит `recover` при следующем запуске.
fn join_engine_thread(handle: thread::JoinHandle<()>, max_wait: Duration) {
    let (done_tx, done_rx) = mpsc::sync_channel(0);
    thread::spawn(move || {
        let _ = handle.join();
        let _ = done_tx.send(());
    });
    match done_rx.recv_timeout(max_wait) {
        Ok(()) => {}
        Err(mpsc::RecvTimeoutError::Timeout) => {
            eprintln!(
                "localvox-light: движок не завершился за {} с — выход. WAV без строки в transcript доработаются при следующем запуске.",
                max_wait.as_secs()
            );
            std::process::exit(0);
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {}
    }
}

#[derive(Parser, Clone)]
#[command(name = "localvox-light", about = "Local audio transcription (no server)")]
struct Cli {
    /// Microphone device (name, index, or "default"). Перекрывает значение из конфига устройств.
    #[arg(long, env = "LOCALVOX_LIGHT_MIC")]
    mic: Option<String>,

    /// Enable system audio capture (loopback)
    #[arg(long)]
    loopback: bool,

    /// Отключить loopback даже если включён в localvox-light-config.json
    #[arg(long)]
    no_loopback: bool,

    /// Loopback device (name, index, or "default-output")
    #[arg(long, env = "LOCALVOX_LIGHT_LOOPBACK_DEVICE")]
    loopback_device: Option<String>,

    /// JSON с полями mic, loopback, loopback_device (сохраняется из TUI F2). Иначе ищется localvox-light-config.json в cwd.
    #[arg(long, env = "LOCALVOX_LIGHT_CONFIG")]
    config: Option<std::path::PathBuf>,

    /// Каталог модели Vosk (как качает scripts/setup-vosk.* → models/vosk-model-ru-0.42)
    #[arg(
        long,
        default_value = "models/vosk-model-ru-0.42",
        env = "LOCALVOX_LIGHT_MODEL"
    )]
    model: String,

    /// Рабочий каталог: WAV, transcript.jsonl (переопределение через LOCALVOX_LIGHT_AUDIO_DIR)
    #[arg(long, default_value = "localvox-audio", env = "LOCALVOX_LIGHT_AUDIO_DIR")]
    audio_dir: String,

    /// Каталог экспорта по `e` в TUI: отсортированный `transcript_dump_*.jsonl`. Пустая строка — экспорт недоступен.
    #[arg(long, default_value = "./transcript-dumps", env = "LOCALVOX_LIGHT_TRANSCRIPT_DUMP_DIR")]
    transcript_dump_dir: PathBuf,

    /// Max segment duration (seconds)
    #[arg(long, default_value = "10", env = "LOCALVOX_LIGHT_MAX_CHUNK_SEC")]
    max_chunk_sec: f64,

    /// Min segment duration before VAD can split (seconds)
    #[arg(long, default_value = "1.5", env = "LOCALVOX_LIGHT_MIN_CHUNK_SEC")]
    min_chunk_sec: f64,

    /// VAD silence duration to trigger segment split (seconds)
    #[arg(long, default_value = "0.8", env = "LOCALVOX_LIGHT_VAD_SILENCE_SEC")]
    vad_silence_sec: f64,

    /// Speech-ratio threshold for noise gate (0 = disabled)
    #[arg(long, default_value = "0.15", env = "LOCALVOX_LIGHT_NOISE_GATE")]
    noise_gate: f32,

    /// List audio devices and exit
    #[arg(long)]
    list_devices: bool,

    /// Full-screen TUI (транскрипт + таблица этапов)
    #[arg(long)]
    tui: bool,

    /// Подробные логи в stderr (tracing), как без TUI
    #[arg(long)]
    debug: bool,

    /// Подробные строки этапов в TUI (панель Debug: segment / gate / asr / load …)
    #[arg(long)]
    verbose: bool,

    /// Parallel ASR worker threads (>1 helps when mic + loopback segments overlap)
    #[arg(long, default_value = "2", env = "LOCALVOX_LIGHT_ASR_WORKERS")]
    asr_workers: usize,
}

fn long_flag_in_argv(long: &str) -> bool {
    let eq = format!("{long}=");
    std::env::args().any(|a| a == long || a.starts_with(&eq))
}

fn env_truthy(name: &str) -> Option<bool> {
    std::env::var(name).ok().map(|v| {
        matches!(
            v.to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// Булевы флаги из `.env` / окружения, если соответствующий `--long` не передан в argv.
fn merge_env_bools(cli: &mut Cli) {
    if !long_flag_in_argv("--loopback") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_LOOPBACK") {
            cli.loopback = t;
        }
    }
    if !long_flag_in_argv("--no-loopback") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_NO_LOOPBACK") {
            cli.no_loopback = t;
        }
    }
    if !long_flag_in_argv("--tui") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_TUI") {
            cli.tui = t;
        }
    }
    if !long_flag_in_argv("--debug") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_DEBUG") {
            cli.debug = t;
        }
    }
    if !long_flag_in_argv("--verbose") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_VERBOSE") {
            cli.verbose = t;
        }
    }
    if !long_flag_in_argv("--list-devices") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_LIST_DEVICES") {
            cli.list_devices = t;
        }
    }
}

/// CLI + optional `localvox-light-config.json` / `--config` (как устройства в client-reliable).
fn resolve_audio_from_cli_and_file(cli: &Cli) -> light_config::LightDeviceConfig {
    let path = light_config::explicit_config_path(&cli.config).or_else(light_config::cwd_config_path);
    let file_cfg = path
        .as_ref()
        .and_then(|p| light_config::LightDeviceConfig::load(p).ok());

    let mut mic = file_cfg
        .as_ref()
        .map(|c| c.mic.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default".into());
    let mut loopback = file_cfg.as_ref().map(|c| c.loopback).unwrap_or(false);
    let mut loopback_device = file_cfg
        .as_ref()
        .map(|c| c.loopback_device.clone())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "default-output".into());

    if let Some(ref m) = cli.mic {
        mic = m.clone();
    }
    if cli.loopback {
        loopback = true;
    }
    if cli.no_loopback {
        loopback = false;
    }
    if let Some(ref d) = cli.loopback_device {
        loopback_device = d.clone();
    }

    light_config::LightDeviceConfig {
        mic,
        loopback,
        loopback_device,
    }
}

fn init_tracing(debug: bool, tui: bool) {
    let filter = if tui && !debug {
        // Подробные этапы — в панели TUI с --verbose; в stderr без --debug только error+.
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("error"))
    } else if debug {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("debug,localvox_light=debug,localvox_light::pipeline=debug")
        })
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(true),
        )
        .try_init();
}

fn format_entry_time_local(rfc: &str) -> String {
    chrono::DateTime::parse_from_rfc3339(rfc.trim())
        .map(|dt| dt.with_timezone(&chrono::Local).format("%H:%M:%S").to_string())
        .unwrap_or_else(|_| chrono::Local::now().format("%H:%M:%S").to_string())
}

fn ui_log(tx: Option<&Sender<UiMsg>>, log: StructuredLog) {
    if let Some(t) = tx {
        let _ = t.send(UiMsg::Log(log));
    }
}

/// Путь из CLI / `.env`: trim и снятие одной пары кавычек `"…"` / `'…'` (частая ошибка в .env).
fn normalized_model_path(cli: &Cli) -> String {
    let s = cli.model.trim();
    let b = s.as_bytes();
    let unquoted = if s.len() >= 2 {
        let first = b[0];
        let last = b[s.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            &s[1..s.len() - 1]
        } else {
            s
        }
    } else {
        s
    };
    unquoted.trim().to_string()
}

/// До захвата аудио и TUI: модель нужна для записи (не для `--list-devices`).
fn validate_vosk_model(cli: &Cli) -> Result<()> {
    let path_str = normalized_model_path(cli);
    if path_str.is_empty() {
        anyhow::bail!(
            "LOCALVOX_LIGHT_MODEL пустой. Задайте каталог модели или удалите переменную (дефолт: models/vosk-model-ru-0.42)."
        );
    }
    let p = Path::new(&path_str);
    if !p.exists() {
        anyhow::bail!(
            "Модель Vosk: каталог не найден: {}. По умолчанию ожидается models/vosk-model-ru-0.42 после scripts/setup-vosk.* (или укажите --model).",
            p.display()
        );
    }
    if !p.is_dir() {
        anyhow::bail!(
            "Модель Vosk: ожидается каталог с распакованной моделью, не файл: {}",
            p.display()
        );
    }
    // Стандартный архив с alphacephei.com: корень вида vosk-model-ru-0.42/ с am/, conf/, graph/
    let am = p.join("am");
    if !am.is_dir() {
        anyhow::bail!(
            "Модель Vosk: в {} нет каталога am/. Укажите корень распакованной модели (не родительскую папку и не conf/graph внутри). Внутри должны быть am/, conf/, graph/.",
            p.display()
        );
    }
    if !am.join("final.mdl").is_file() {
        anyhow::bail!(
            "Модель Vosk: нет am/final.mdl в {} — архив модели неполный или повреждён.",
            p.display()
        );
    }
    Ok(())
}

fn main() -> Result<()> {
    let _ = dotenvy::dotenv().ok();
    let mut cli = Cli::parse();
    merge_env_bools(&mut cli);
    if cli.list_devices {
        let _ = tracing_subscriber::fmt::try_init();
        print_devices();
        return Ok(());
    }

    validate_vosk_model(&cli)?;

    init_tracing(cli.debug, cli.tui);

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            eprintln!("\nStopping...");
            r.store(false, Ordering::SeqCst);
        })?;
    }

    let audio_devices = resolve_audio_from_cli_and_file(&cli);

    if cli.tui {
        let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<UiMsg>();
        let (reset_tx, reset_rx) = crossbeam_channel::unbounded::<()>();
        let record_pcm = Arc::new(AtomicBool::new(true));
        let tui_verbose = cli.verbose;
        let cli_engine = cli.clone();
        let dev_engine = audio_devices.clone();
        let r_engine = running.clone();
        let r_tui = running.clone();
        let record_engine = Arc::clone(&record_pcm);
        let record_tui = Arc::clone(&record_pcm);
        let engine_handle = thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                if let Err(e) = run_engine(
                    cli_engine,
                    dev_engine,
                    Some(ui_tx),
                    reset_rx,
                    r_engine,
                    record_engine,
                ) {
                    tracing::error!("Engine stopped: {e:#}");
                }
            })?;
        let cfg_path = light_config::save_path_for_write();
        tui::run(
            &ui_rx,
            reset_tx,
            r_tui,
            record_tui,
            "localvox-light".into(),
            audio_devices,
            cfg_path,
            tui_verbose,
        )?;
        join_engine_thread(engine_handle, Duration::from_secs(2));
        info!("Session finished.");
        return Ok(());
    }

    let (_noop_reset_tx, noop_reset_rx) = crossbeam_channel::unbounded::<()>();
    run_engine(
        cli,
        audio_devices,
        None,
        noop_reset_rx,
        running,
        Arc::new(AtomicBool::new(true)),
    )?;
    Ok(())
}

fn run_engine(
    cli: Cli,
    audio_devices: light_config::LightDeviceConfig,
    ui_tx: Option<Sender<UiMsg>>,
    reset_rx: crossbeam_channel::Receiver<()>,
    running: Arc<AtomicBool>,
    record_pcm: Arc<AtomicBool>,
) -> Result<()> {
    let work_dir = PathBuf::from(&cli.audio_dir);
    std::fs::create_dir_all(&work_dir)?;
    info!("Workspace: {}", work_dir.display());

    session::remove_orphan_part_files(&work_dir);

    if let Some(ref t) = ui_tx {
        let _ = t.send(UiMsg::WorkspacePaths {
            workspace_dir: work_dir.clone(),
            dump_dir: cli.transcript_dump_dir.clone(),
        });
        let _ = t.send(UiMsg::Status(format!("Данные: {}", work_dir.display())));
    }

    if let Some(ref t) = ui_tx {
        let sd_stats = work_dir.clone();
        let tx_stats = t.clone();
        let run_stats = running.clone();
        thread::Builder::new()
            .name("queue-stats".into())
            .spawn(move || {
                while run_stats.load(Ordering::Relaxed) {
                    let (n, umb, smb) = session::workspace_queue_stats(&sd_stats);
                    let _ = tx_stats.send(UiMsg::QueuePending {
                        unprocessed_wavs: n,
                        unprocessed_mb: umb,
                        workspace_total_mb: smb,
                    });
                    thread::sleep(Duration::from_secs(1));
                }
            })
            .expect("spawn queue-stats");
    }

    if let Some(ref t) = ui_tx {
        let entries = TranscriptWriter::read_all_entries(&work_dir);
        let n = entries.len();
        let rows: Vec<_> = entries
            .into_iter()
            .map(|e| {
                let time = format_entry_time_local(&e.timestamp);
                (time, e.source_id, e.text)
            })
            .collect();
        let _ = t.send(UiMsg::TranscriptHistory(rows));
        let _ = t.send(UiMsg::Log(StructuredLog {
            stage: "hydrate".into(),
            source_id: 0,
            chunk_sec: n as f64,
            proc_sec: 0.0,
            detail: if n > 0 {
                format!("TUI ← transcript.jsonl ({n} строк)")
            } else {
                "TUI ← transcript.jsonl (пусто)".into()
            },
            verbose_only: true,
        }));
    }

    let model_path_buf = PathBuf::from(normalized_model_path(&cli));
    let noise_gate = cli.noise_gate;

    let (pcm_tx, pcm_rx) = crossbeam_channel::bounded::<audio::PcmChunk>(1024);
    let (seg_tx, seg_rx) = crossbeam_channel::unbounded::<SegmentReady>();
    let (engine_tx, engine_rx) = crossbeam_channel::bounded::<Result<asr::vosk::VoskEngine, String>>(1);

    // Recovery: все необработанные WAV с диска → в канал ДО пайплайна (отсортированные по src/seq).
    let recovery = session::recover_unprocessed(&work_dir);
    if !recovery.is_empty() {
        let n = recovery.len();
        debug!("Recovery: {n} unprocessed WAV(s) → channel");
        if let Some(ref t) = ui_tx {
            let _ = t.send(UiMsg::Log(StructuredLog {
                stage: "recover".into(),
                source_id: 0,
                chunk_sec: n as f64,
                proc_sec: 0.0,
                detail: format!("{n} WAV queued for re-processing"),
                verbose_only: true,
            }));
        }
        for (path, source_id) in recovery {
            let duration_sec = wav_duration_sec(&path).unwrap_or(0.0);
            let _ = seg_tx.send(SegmentReady {
                path,
                source_id,
                duration_sec,
            });
        }
    }

    let pipeline_cfg = PipelineConfig {
        max_chunk_sec: cli.max_chunk_sec,
        min_chunk_sec: cli.min_chunk_sec,
        vad_silence_sec: cli.vad_silence_sec,
        work_dir: work_dir.clone(),
        initial_seg_seq: [
            session::max_segment_seq_on_disk(&work_dir, 0),
            session::max_segment_seq_on_disk(&work_dir, 1),
        ],
    };
    let pipeline_running = running.clone();
    let pipeline_record_pcm = Arc::clone(&record_pcm);
    let log_tx = ui_tx.clone();
    let pipeline_seg_tx = seg_tx.clone();
    drop(seg_tx);
    let pipeline_handle = thread::Builder::new()
        .name("pipeline".into())
        .spawn(move || {
            pipeline::run(
                pipeline_cfg,
                pcm_rx,
                pipeline_seg_tx,
                pipeline_running,
                pipeline_record_pcm,
                log_tx,
            )
        })?;

    let ui_load = ui_tx.clone();
    thread::Builder::new()
        .name("vosk-load".into())
        .spawn(move || {
            if let Some(ref t) = ui_load {
                let _ = t.send(UiMsg::Status("Loading Vosk model…".into()));
                let _ = t.send(UiMsg::Log(StructuredLog {
                    stage: "load".into(),
                    source_id: 0,
                    chunk_sec: 0.0,
                    proc_sec: 0.0,
                    detail: model_path_buf.display().to_string(),
                    verbose_only: true,
                }));
            }
            let t0 = Instant::now();
            let r = asr::vosk::VoskEngine::new(&model_path_buf).map_err(|e| e.to_string());
            let proc = t0.elapsed().as_secs_f64();
            if let Some(ref t) = ui_load {
                match &r {
                    Ok(_) => {
                        let _ = t.send(UiMsg::Log(StructuredLog {
                            stage: "load".into(),
                            source_id: 0,
                            chunk_sec: 0.0,
                            proc_sec: proc,
                            detail: "Vosk ready".into(),
                            verbose_only: true,
                        }));
                        let _ = t.send(UiMsg::Status("Recording — Vosk ready".into()));
                    }
                    Err(e) => {
                        let _ = t.send(UiMsg::Log(StructuredLog {
                            stage: "load".into(),
                            source_id: 0,
                            chunk_sec: 0.0,
                            proc_sec: proc,
                            detail: format!("FAILED: {e}"),
                            verbose_only: false,
                        }));
                    }
                }
            }
            let _ = engine_tx.send(r);
        })?;

    let asr_work_dir = work_dir.clone();
    let asr_running = running.clone();
    let ui_asr = ui_tx.clone();
    let num_workers = cli.asr_workers;
    let asr_handle = thread::Builder::new()
        .name("asr-pool".into())
        .spawn(move || {
            asr_worker_pool(
                engine_rx,
                noise_gate,
                seg_rx,
                asr_work_dir,
                asr_running,
                reset_rx,
                ui_asr,
                num_workers,
            );
        })?;

    let mic_device = audio::resolve_mic(&audio_devices.mic).map_err(|e| {
        let msg = format!("Микрофон: {e:#}");
        if let Some(ref t) = ui_tx {
            let _ = t.send(UiMsg::EngineFatal {
                message: msg.clone(),
            });
        }
        eprintln!("localvox-light: {msg}");
        running.store(false, Ordering::SeqCst);
        e
    })?;
    let mic_name = mic_device
        .description()
        .map(|d| d.name().to_string())
        .unwrap_or_else(|_| "unknown".into());
    info!("Mic: {mic_name}");

    let mic_tx = pcm_tx.clone();
    let mic_running = running.clone();
    let mic_ui = ui_tx.clone();
    let mic_handle = thread::Builder::new()
        .name("mic-capture".into())
        .spawn(move || {
            if let Err(e) = audio::mic_capture(mic_device, 0, mic_tx, mic_running, mic_ui) {
                tracing::error!("Mic capture error: {e}");
            }
        })?;

    let loopback_handle = if audio_devices.loopback {
        let lb_query = audio_devices.loopback_device.clone();
        let lb_tx = pcm_tx.clone();
        let lb_running = running.clone();
        let lb_ui = ui_tx.clone();
        Some(
            thread::Builder::new()
                .name("loopback-capture".into())
                .spawn(move || {
                    if let Err(e) = audio::loopback_capture(&lb_query, lb_tx, lb_running, lb_ui) {
                        tracing::error!("Loopback capture error: {e}");
                    }
                })?,
        )
    } else {
        None
    };

    drop(pcm_tx);
    info!("Recording (WAV → disk); Vosk loads in parallel. Ctrl+C to stop.");
    if let Some(ref t) = ui_tx {
        let _ = t.send(UiMsg::Status(
            "Recording — WAV на диск, модель грузится…".into(),
        ));
    }

    // Сначала pipeline: дренаж pcm до отпускания всех Sender в mic/loopback (иначе bounded send в колбэке cpal зависает).
    pipeline_handle.join().ok();
    mic_handle.join().ok();
    if let Some(h) = loopback_handle {
        h.join().ok();
    }
    asr_handle.join().ok();

    info!("Workspace saved: {}", work_dir.display());
    Ok(())
}

/// Координатор пула: ждёт модель, открывает TranscriptWriter, запускает N воркеров.
/// Recovery-файлы уже в канале (отправлены в run_engine до старта пайплайна).
#[allow(clippy::too_many_arguments)]
fn asr_worker_pool(
    engine_rx: crossbeam_channel::Receiver<Result<asr::vosk::VoskEngine, String>>,
    noise_gate: f32,
    seg_rx: crossbeam_channel::Receiver<SegmentReady>,
    work_dir: PathBuf,
    running: Arc<AtomicBool>,
    reset_rx: crossbeam_channel::Receiver<()>,
    ui_tx: Option<Sender<UiMsg>>,
    num_workers: usize,
) {
    let engine = match engine_rx.recv() {
        Ok(Ok(e)) => e,
        Ok(Err(e)) => {
            tracing::error!("Vosk model failed to load: {e}");
            let msg = format!("Модель Vosk: {e}");
            if let Some(ref t) = ui_tx {
                let _ = t.send(UiMsg::EngineFatal {
                    message: msg.clone(),
                });
            }
            eprintln!("localvox-light: {msg}");
            ui_log(
                ui_tx.as_ref(),
                StructuredLog {
                    stage: "load".into(),
                    source_id: 0,
                    chunk_sec: 0.0,
                    proc_sec: 0.0,
                    detail: format!("abort: {e}"),
                    verbose_only: false,
                },
            );
            running.store(false, Ordering::SeqCst);
            return;
        }
        Err(_) => {
            tracing::error!("Vosk load channel closed");
            let msg = "Загрузка модели: канал закрыт до ответа".to_string();
            if let Some(ref t) = ui_tx {
                let _ = t.send(UiMsg::EngineFatal {
                    message: msg.clone(),
                });
            }
            eprintln!("localvox-light: {msg}");
            running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let tw = match TranscriptWriter::open(&work_dir) {
        Ok(t) => t,
        Err(e) => {
            tracing::error!("Failed to open transcript: {e}");
            let msg = format!("transcript.jsonl: {e:#}");
            if let Some(ref t) = ui_tx {
                let _ = t.send(UiMsg::EngineFatal {
                    message: msg.clone(),
                });
            }
            eprintln!("localvox-light: {msg}");
            running.store(false, Ordering::SeqCst);
            return;
        }
    };

    let engine = Arc::new(engine);
    let tw = Arc::new(Mutex::new(tw));
    let num = num_workers.max(1);
    debug!("Starting {num} ASR worker thread(s)");

    let mut handles = Vec::with_capacity(num);
    let mut reset_rx_slot = Some(reset_rx);

    for i in 0..num {
        let engine = Arc::clone(&engine);
        let seg_rx = seg_rx.clone();
        let tw = Arc::clone(&tw);
        let running = Arc::clone(&running);
        let ui_tx = ui_tx.clone();
        let work_dir = work_dir.clone();
        let worker_reset_rx = if i == 0 { reset_rx_slot.take() } else { None };

        let h = thread::Builder::new()
            .name(format!("asr-{i}"))
            .spawn(move || {
                asr_thread_loop(
                    i,
                    engine,
                    seg_rx,
                    tw,
                    running,
                    ui_tx,
                    work_dir,
                    noise_gate,
                    worker_reset_rx,
                );
            })
            .expect("spawn asr worker");
        handles.push(h);
    }

    for h in handles {
        h.join().ok();
    }
    debug!("ASR worker pool stopped ({num} threads)");
}

#[allow(clippy::too_many_arguments)]
fn asr_thread_loop(
    id: usize,
    engine: Arc<asr::vosk::VoskEngine>,
    seg_rx: crossbeam_channel::Receiver<SegmentReady>,
    tw: Arc<Mutex<TranscriptWriter>>,
    running: Arc<AtomicBool>,
    ui_tx: Option<Sender<UiMsg>>,
    work_dir: PathBuf,
    noise_gate: f32,
    reset_rx: Option<crossbeam_channel::Receiver<()>>,
) {
    loop {
        if let Some(ref rx) = reset_rx {
            let mut do_reset = false;
            while rx.try_recv().is_ok() {
                do_reset = true;
            }
            if do_reset {
                match TranscriptWriter::reopen_truncated(&work_dir) {
                    Ok(n) => {
                        *tw.lock().unwrap() = n;
                        debug!("transcript.jsonl cleared (новый лист)");
                    }
                    Err(e) => tracing::error!("Failed to truncate transcript: {e}"),
                }
                if let Some(ref t) = ui_tx {
                    let _ = t.send(UiMsg::ClearTranscript);
                    let _ = t.send(UiMsg::Status(
                        "Запись — транскрипт обнулён (x), продолжаем".into(),
                    ));
                }
            }
        }

        match seg_rx.recv_timeout(Duration::from_millis(200)) {
            Ok(seg) => {
                if !seg.path.exists() {
                    continue;
                }
                // Остановка: не стартуем новый тяжёлый ASR — WAV на диске, подхватит recover.
                if !running.load(Ordering::Relaxed) {
                    continue;
                }
                if let Some(entry) = process_segment(
                    &*engine,
                    &seg.path,
                    seg.source_id,
                    noise_gate,
                    ui_tx.as_ref(),
                    Some(seg.duration_sec),
                    &running,
                ) {
                    if let Some(ref t) = ui_tx {
                        if t.send(UiMsg::Transcript {
                            source_id: entry.source_id,
                            text: entry.text.clone(),
                            time: None,
                        })
                        .is_err()
                        {
                            tracing::warn!("TUI channel closed; строка только в transcript.jsonl");
                        }
                    }
                    match tw.lock().unwrap().append(&entry) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&seg.path);
                        }
                        Err(e) => tracing::error!("Failed to write transcript: {e}"),
                    }
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                if !running.load(Ordering::Relaxed) && seg_rx.is_empty() {
                    break;
                }
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
        }
    }
    debug!("asr-{id} stopped");
}

fn wav_duration_sec(path: &std::path::Path) -> Option<f64> {
    let r = WavReader::open(path).ok()?;
    let spec = r.spec();
    let n = r.len();
    if spec.sample_rate == 0 {
        return None;
    }
    Some(n as f64 / spec.sample_rate as f64)
}

fn process_segment(
    engine: &impl AsrEngine,
    path: &std::path::Path,
    source_id: u8,
    noise_gate: f32,
    ui_tx: Option<&Sender<UiMsg>>,
    pipeline_chunk_sec: Option<f64>,
    running: &AtomicBool,
) -> Option<TranscriptEntry> {
    let samples = match wav_to_f32(path) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("Failed to read {}: {e}", path.display());
            return None;
        }
    };
    if samples.is_empty() {
        let _ = std::fs::remove_file(path);
        return None;
    }

    let duration_sec = samples.len() as f64 / 16000.0;

    let samples = if noise_gate > 0.0 {
        let t_gate = Instant::now();
        let ratio = speech_ratio(&samples);
        if ratio == 0.0 {
            debug!(
                "src{}: {:.1}s -> (noise, 0% speech, skipped)",
                source_id, duration_sec
            );
            let _ = std::fs::remove_file(path);
            return None;
        }
        if ratio < noise_gate {
            match trim_to_speech(&samples, 300) {
                Some(trimmed) => {
                    let proc = t_gate.elapsed().as_secs_f64();
                    let new_dur = trimmed.len() as f64 / 16000.0;
                    debug!(
                        "src{}: {:.1}s -> trimmed to {:.1}s (speech {:.0}%)",
                        source_id, duration_sec, new_dur, ratio * 100.0
                    );
                    ui_log(
                        ui_tx,
                        StructuredLog {
                            stage: "gate".into(),
                            source_id,
                            chunk_sec: duration_sec,
                            proc_sec: proc,
                            detail: format!(
                                "trim → {:.1}s speech {:.0}%",
                                new_dur,
                                ratio * 100.0
                            ),
                            verbose_only: true,
                        },
                    );
                    trimmed
                }
                None => {
                    debug!(
                        "src{}: {:.1}s -> gate trim → empty (silence) [{:.3}s]",
                        source_id,
                        duration_sec,
                        t_gate.elapsed().as_secs_f64()
                    );
                    let _ = std::fs::remove_file(path);
                    return None;
                }
            }
        } else {
            let proc = t_gate.elapsed().as_secs_f64();
            ui_log(
                ui_tx,
                StructuredLog {
                    stage: "gate".into(),
                    source_id,
                    chunk_sec: duration_sec,
                    proc_sec: proc,
                    detail: format!("pass speech {:.0}%", ratio * 100.0),
                    verbose_only: true,
                },
            );
            samples
        }
    } else {
        samples
    };

    let final_dur = samples.len() as f64 / 16000.0;
    if !running.load(Ordering::Relaxed) {
        return None;
    }
    let t0 = Instant::now();
    let text = match engine.transcribe(&samples) {
        Ok(t) => t,
        Err(e) => {
            let proc = t0.elapsed().as_secs_f64();
            tracing::warn!("ASR error for {}: {e}", path.display());
            ui_log(
                ui_tx,
                StructuredLog {
                    stage: "asr".into(),
                    source_id,
                    chunk_sec: final_dur,
                    proc_sec: proc,
                    detail: format!("ERROR: {e}"),
                    verbose_only: false,
                },
            );
            return None;
        }
    };
    let elapsed = t0.elapsed().as_secs_f64();

    let pipe_hint = pipeline_chunk_sec
        .map(|s| format!(" pipeline_seg={s:.2}s"))
        .unwrap_or_default();

    if text.is_empty() {
        debug!(
            "src{}: {:.1}s -> (no speech) [{:.3}s]{}",
            source_id,
            final_dur,
            elapsed,
            pipe_hint
        );
        let _ = std::fs::remove_file(path);
        return None;
    }

    let display_text = if text.chars().count() > 80 {
        let truncated: String = text.chars().take(77).collect();
        format!("{truncated}...")
    } else {
        text.clone()
    };
    debug!(
        "src{}: {:.1}s -> {} [{:.1}s]",
        source_id, final_dur, display_text, elapsed
    );
    ui_log(
        ui_tx,
        StructuredLog {
            stage: "asr".into(),
            source_id,
            chunk_sec: final_dur,
            proc_sec: elapsed,
            detail: format!("{display_text}{pipe_hint}"),
            verbose_only: true,
        },
    );

    let seg_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_string();
    Some(TranscriptEntry {
        seg_id,
        source_id,
        text,
        duration_sec: final_dur,
        timestamp: chrono::Utc::now().to_rfc3339(),
    })
}

fn wav_to_f32(path: &std::path::Path) -> Result<Vec<f32>> {
    let reader = WavReader::open(path)?;
    let spec = reader.spec();
    if spec.channels != 1 || spec.bits_per_sample != 16 {
        anyhow::bail!(
            "expected 16-bit mono WAV, got {}ch {}bit",
            spec.channels,
            spec.bits_per_sample
        );
    }
    let samples: Vec<f32> = reader
        .into_samples::<i16>()
        .filter_map(Result::ok)
        .map(|s| s as f32 / 32768.0)
        .collect();
    Ok(samples)
}

fn print_devices() {
    println!("=== Input devices (microphones) ===");
    for (i, (_dev, name)) in audio::collect_input_devices().iter().enumerate() {
        println!("  [{i}] {name}");
    }
    println!("\n=== Output devices (for loopback) ===");
    for (i, name) in audio::list_output_device_names() {
        println!("  [{i}] {name}");
    }
}
