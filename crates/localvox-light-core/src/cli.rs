//! CLI (`clap`), `.env`, проверка модели Vosk, устройства, tracing.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use crate::audio;

/// Ждёт поток движка не дольше `max_wait`. Если не успел — `process::exit(0)`:
/// необработанные WAV в рабочей папке без строки в `transcript.jsonl` подхватит `recover` при следующем запуске.
pub fn join_engine_thread(handle: thread::JoinHandle<()>, max_wait: Duration) {
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
pub struct Cli {
    /// Microphone device (name, index, or "default"). Перекрывает значение из конфига устройств.
    #[arg(long, env = "LOCALVOX_LIGHT_MIC")]
    pub mic: Option<String>,

    /// Enable system audio capture (loopback)
    #[arg(long)]
    pub loopback: bool,

    /// Отключить loopback даже если включён в localvox-light-config.json
    #[arg(long)]
    pub no_loopback: bool,

    /// Loopback device (name, index, or "default-output")
    #[arg(long, env = "LOCALVOX_LIGHT_LOOPBACK_DEVICE")]
    pub loopback_device: Option<String>,

    /// JSON с полями mic, loopback, loopback_device (сохраняется из TUI F2). Иначе ищется localvox-light-config.json в cwd.
    #[arg(long, env = "LOCALVOX_LIGHT_CONFIG")]
    pub config: Option<std::path::PathBuf>,

    /// Каталог модели Vosk (как качает scripts/setup-vosk.* → models/vosk-model-ru-0.42)
    #[arg(
        long,
        default_value = "models/vosk-model-ru-0.42",
        env = "LOCALVOX_LIGHT_MODEL"
    )]
    pub model: String,

    /// Рабочий каталог: WAV, transcript.jsonl (переопределение через LOCALVOX_LIGHT_AUDIO_DIR)
    #[arg(long, default_value = "localvox-audio", env = "LOCALVOX_LIGHT_AUDIO_DIR")]
    pub audio_dir: String,

    /// Каталог экспорта по `e` в TUI: отсортированный `transcript_dump_*.jsonl`. Пустая строка — экспорт недоступен.
    #[arg(long, default_value = "./transcript-dumps", env = "LOCALVOX_LIGHT_TRANSCRIPT_DUMP_DIR")]
    pub transcript_dump_dir: PathBuf,

    /// Max segment duration (seconds)
    #[arg(long, default_value = "10", env = "LOCALVOX_LIGHT_MAX_CHUNK_SEC")]
    pub max_chunk_sec: f64,

    /// Min segment duration before VAD can split (seconds)
    #[arg(long, default_value = "1.5", env = "LOCALVOX_LIGHT_MIN_CHUNK_SEC")]
    pub min_chunk_sec: f64,

    /// VAD silence duration to trigger segment split (seconds)
    #[arg(long, default_value = "0.8", env = "LOCALVOX_LIGHT_VAD_SILENCE_SEC")]
    pub vad_silence_sec: f64,

    /// Speech-ratio threshold for noise gate (0 = disabled)
    #[arg(long, default_value = "0.15", env = "LOCALVOX_LIGHT_NOISE_GATE")]
    pub noise_gate: f32,

    /// List audio devices and exit
    #[arg(long)]
    pub list_devices: bool,

    /// Full-screen TUI (транскрипт + таблица этапов)
    #[arg(long)]
    pub tui: bool,

    /// Подробные логи в stderr (tracing), как без TUI
    #[arg(long)]
    pub debug: bool,

    /// Подробные строки этапов в TUI (панель Debug: segment / gate / asr / load …)
    #[arg(long)]
    pub verbose: bool,

    /// Parallel ASR worker threads (>1 helps when mic + loopback segments overlap)
    #[arg(long, default_value = "2", env = "LOCALVOX_LIGHT_ASR_WORKERS")]
    pub asr_workers: usize,
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
pub fn merge_env_bools(cli: &mut Cli) {
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
pub fn resolve_audio_from_cli_and_file(cli: &Cli) -> crate::light_config::LightDeviceConfig {
    let path = crate::light_config::explicit_config_path(&cli.config)
        .or_else(crate::light_config::cwd_config_path);
    let file_cfg = path
        .as_ref()
        .and_then(|p| crate::light_config::LightDeviceConfig::load(p).ok());

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

    crate::light_config::LightDeviceConfig {
        mic,
        loopback,
        loopback_device,
    }
}

pub fn init_tracing(debug: bool, tui: bool) {
    let filter = if tui && !debug {
        // Подробные этапы — в панели TUI с --verbose; в stderr без --debug только error+.
        EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("error"))
    } else if debug {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new("debug,localvox_light_core=debug,localvox_light_core::pipeline=debug")
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

/// Путь из CLI / `.env`: trim и снятие одной пары кавычек `"…"` / `'…'` (частая ошибка в .env).
pub fn normalized_model_path(cli: &Cli) -> String {
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
pub fn validate_vosk_model(cli: &Cli) -> Result<()> {
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

pub fn print_devices() {
    println!("=== Input devices (microphones) ===");
    for (i, (_dev, name)) in audio::collect_input_devices().iter().enumerate() {
        println!("  [{i}] {name}");
    }
    println!("\n=== Output devices (for loopback) ===");
    for (i, name) in audio::list_output_device_names() {
        println!("  [{i}] {name}");
    }
}

#[cfg(test)]
mod cli_helpers_tests {
    use super::*;
    use clap::Parser;
    use std::fs;
    use tempfile::tempdir;

    fn default_cli() -> Cli {
        Cli::try_parse_from(["localvox-light"]).expect("cli")
    }

    fn cli_with_model(model: &str) -> Cli {
        Cli::try_parse_from(["localvox-light", "--model", model]).expect("cli")
    }

    #[test]
    fn normalized_model_path_strips_quotes_and_trim() {
        let c = cli_with_model("  \"models/foo\"  ");
        assert_eq!(normalized_model_path(&c), "models/foo");
        let c2 = cli_with_model("'bar/baz'");
        assert_eq!(normalized_model_path(&c2), "bar/baz");
    }

    fn touch_model_layout(root: &std::path::Path) {
        fs::create_dir_all(root.join("am")).unwrap();
        fs::write(root.join("am").join("final.mdl"), b"x").unwrap();
    }

    #[test]
    fn validate_vosk_model_accepts_am_final_mdl() {
        let dir = tempdir().unwrap();
        touch_model_layout(dir.path());
        let mut c = default_cli();
        c.model = dir.path().to_string_lossy().to_string();
        validate_vosk_model(&c).unwrap();
    }

    #[test]
    fn validate_vosk_model_rejects_empty_model_path() {
        let mut c = default_cli();
        c.model = String::new();
        assert!(validate_vosk_model(&c).is_err());
    }

    #[test]
    fn validate_vosk_model_rejects_missing_dir() {
        let dir = tempdir().unwrap();
        let ghost = dir.path().join("no-such-model-dir-42");
        let mut c = default_cli();
        c.model = ghost.to_string_lossy().to_string();
        assert!(validate_vosk_model(&c).is_err());
    }

    #[test]
    fn validate_vosk_model_rejects_file_instead_of_dir() {
        let dir = tempdir().unwrap();
        let f = dir.path().join("notadir");
        fs::write(&f, b"x").unwrap();
        let mut c = default_cli();
        c.model = f.to_string_lossy().to_string();
        assert!(validate_vosk_model(&c).is_err());
    }

    #[test]
    fn validate_vosk_model_rejects_without_am() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path()).unwrap();
        let mut c = default_cli();
        c.model = dir.path().to_string_lossy().to_string();
        assert!(validate_vosk_model(&c).is_err());
    }

    #[test]
    fn validate_vosk_model_rejects_am_without_final_mdl() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("am")).unwrap();
        let mut c = default_cli();
        c.model = dir.path().to_string_lossy().to_string();
        assert!(validate_vosk_model(&c).is_err());
    }
}
