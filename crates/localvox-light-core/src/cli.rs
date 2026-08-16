//! CLI (`clap`), `.env`, Vosk model validation, devices, tracing.

use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

use crate::audio;

/// Waits for the engine thread no longer than `max_wait`. If it does not make it —
/// `process::exit(0)`: unprocessed WAVs in the workspace with no line in `transcript.jsonl`
/// are picked up by `recover` on the next run.
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
                "localvox-light: the engine did not finish within {} s — exiting. WAVs with no line in the transcript will be processed on the next run.",
                max_wait.as_secs()
            );
            std::process::exit(0);
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => {}
    }
}

#[derive(Parser, Clone)]
#[command(
    name = "localvox-light",
    about = "Local audio transcription (no server)"
)]
pub struct Cli {
    /// Microphone: index, name substring, `micidx:N`, or a stable CPAL id `host:…` (see --list-devices).
    #[arg(long, env = "LOCALVOX_LIGHT_MIC")]
    pub mic: Option<String>,

    /// Enable system audio capture (loopback)
    #[arg(long)]
    pub loopback: bool,

    /// Disable loopback even if it is enabled in localvox-light-config.json
    #[arg(long)]
    pub no_loopback: bool,

    /// Loopback: Windows — name/index/default-output (WASAPI). macOS — id/`lbidx:N` of an **output** (speakers). Linux — a monitor input, id from --list-devices.
    #[arg(long, env = "LOCALVOX_LIGHT_LOOPBACK_DEVICE")]
    pub loopback_device: Option<String>,

    /// JSON with the fields mic, loopback, loopback_device (saved from the TUI, F2). Otherwise localvox-light-config.json is looked up in cwd.
    #[arg(long, env = "LOCALVOX_LIGHT_CONFIG")]
    pub config: Option<std::path::PathBuf>,

    /// Vosk model directory (as downloaded by scripts/fetch-models.* → models/vosk-model-ru-0.42)
    #[arg(
        long,
        default_value = "models/vosk-model-ru-0.42",
        env = "LOCALVOX_LIGHT_MODEL"
    )]
    pub model: String,

    /// Workspace directory: WAV, transcript.jsonl (override via LOCALVOX_LIGHT_AUDIO_DIR)
    #[arg(
        long,
        default_value = "localvox-audio",
        env = "LOCALVOX_LIGHT_AUDIO_DIR"
    )]
    pub audio_dir: String,

    /// Export directory for `e` in the TUI: a sorted `transcript_dump_*.jsonl`. An empty string disables export.
    #[arg(
        long,
        default_value = "./transcript-dumps",
        env = "LOCALVOX_LIGHT_TRANSCRIPT_DUMP_DIR"
    )]
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

    /// Check the installation and exit: models, the cook, the archive, the LLM.
    ///
    /// Exit code: 0 — everything works; 1 — something is missing but the product runs without
    /// it; 2 — it will not work. A machine can branch on that; a human reads the lines.
    #[arg(long)]
    pub doctor: bool,

    /// Update the bundled yt-dlp in place (to the nightly channel) and exit. YouTube breaks old
    /// versions every few weeks (HTTP 403 on download); this is the one-command fix `--doctor`
    /// points to. Uses yt-dlp's own in-place updater — it rewrites its own binary for this OS.
    #[arg(long)]
    pub update_yt_dlp: bool,

    /// Full-screen TUI (transcript + stage table)
    #[arg(long)]
    pub tui: bool,

    /// Do not open the TUI (logs to stderr only), even in an interactive terminal
    #[arg(long)]
    pub no_tui: bool,

    /// Verbose logs to stderr (tracing), as without the TUI
    #[arg(long)]
    pub debug: bool,

    /// Verbose stage lines in the TUI (Debug panel: segment / gate / asr / load …)
    #[arg(long)]
    pub verbose: bool,

    /// Parallel ASR worker threads (>1 helps when mic + loopback segments overlap)
    #[arg(long, default_value = "2", env = "LOCALVOX_LIGHT_ASR_WORKERS")]
    pub asr_workers: usize,

    /// Do not write the continuous session audio chunks (the primary artifact for the slow lane, F8)
    #[arg(long)]
    pub no_session_chunks: bool,

    /// Audio chunk duration, sec (file rotation; adjacent chunks join sample-exactly)
    #[arg(long, default_value = "300", env = "LOCALVOX_LIGHT_CHUNK_SEC")]
    pub chunk_sec: f64,

    /// Audio chunk retention, days; 0 — keep forever
    #[arg(
        long,
        default_value = "14",
        env = "LOCALVOX_LIGHT_RETENTION_AUDIO_DAYS"
    )]
    pub retention_audio_days: u32,

    /// Recompress closed chunks WAV → FLAC (needs ffmpeg in PATH or LOCALVOX_LIGHT_YT_FFMPEG)
    #[arg(long)]
    pub chunk_flac: bool,

    /// Legacy: write fast-lane segments to disk, as before WP-B1 (default is RAM)
    #[arg(long)]
    pub segments_to_disk: bool,

    /// Watchdog: warn if the microphone is silent for longer than N sec (0 — disable)
    #[arg(
        long,
        default_value = "15",
        env = "LOCALVOX_LIGHT_MIC_SILENCE_WARN_SEC"
    )]
    pub mic_silence_warn_sec: f64,

    /// Pre-roll ring: seconds of the past kept IN MEMORY (never on disk). Pressing "record"
    /// pulls them into the session — a conversation that began before the button is not lost.
    /// Five minutes of two tracks ≈ 19 MB.
    #[arg(long, default_value = "300", env = "LOCALVOX_LIGHT_PREROLL_SEC")]
    pub preroll_sec: f64,

    /// Auto-stop: silence on both tracks for longer than N sec closes the recording; 0 — never
    #[arg(long, default_value = "900", env = "LOCALVOX_LIGHT_AUTOSTOP_SEC")]
    pub autostop_sec: f64,

    /// Background mode: recording + voice + cooking + HTTP API, no terminal interface.
    ///
    /// This is the product as it normally runs. On Windows a tray icon is raised on top of it;
    /// elsewhere there is simply no icon, and that is not a lesser mode — the interface lives at
    /// the HTTP address either way. The old spelling `--tray` still works: it named the garnish,
    /// not the dish.
    #[arg(long, alias = "tray")]
    pub daemon: bool,

    /// Chdir into this directory before reading any config (set by autostart: its
    /// cwd = system32, while `.env`, `models/` and slots all depend on the directory)
    #[arg(long, hide = true)]
    pub cwd: Option<PathBuf>,
}

fn long_flag_in_argv(long: &str) -> bool {
    let eq = format!("{long}=");
    std::env::args().any(|a| a == long || a.starts_with(&eq))
}

fn env_truthy(name: &str) -> Option<bool> {
    std::env::var(name)
        .ok()
        .map(|v| matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"))
}

/// Boolean flags from `.env` / the environment, if the corresponding `--long` was not passed
/// in argv.
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
    if !long_flag_in_argv("--no-tui") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_NO_TUI") {
            cli.no_tui = t;
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
    if !long_flag_in_argv("--no-session-chunks") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_NO_SESSION_CHUNKS") {
            cli.no_session_chunks = t;
        }
    }
    if !long_flag_in_argv("--chunk-flac") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_CHUNK_FLAC") {
            cli.chunk_flac = t;
        }
    }
    if !long_flag_in_argv("--segments-to-disk") {
        if let Some(t) = env_truthy("LOCALVOX_LIGHT_SEGMENTS_DISK") {
            cli.segments_to_disk = t;
        }
    }
    // Both spellings, on the command line and in the environment: `--tray` is what autostart
    // entries and the desktop shell in the wild already say.
    if !long_flag_in_argv("--daemon") && !long_flag_in_argv("--tray") {
        if let Some(t) =
            env_truthy("LOCALVOX_LIGHT_DAEMON").or_else(|| env_truthy("LOCALVOX_LIGHT_TRAY"))
        {
            cli.daemon = t;
        }
    }
}

/// CLI + optional `localvox-light-config.json` / `--config` (like devices in client-reliable).
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

/// Third-party libraries that have nothing to say to a human.
///
/// **ONNX Runtime.** The `ort` crate creates the ORT environment at level VERBOSE and
/// offloads filtering onto `tracing` — that is, onto us. Do not filter it, and on EVERY
/// inference the log gets showered with internal bookkeeping: "Extended allocation by
/// 16777216 bytes", "Allocated memory at 000001D0…", "GraphTransformer … modified".
/// These are neither our events nor the user's: this is somebody else's allocator being
/// debugged. The useful stuff (model load errors, tensor shape mismatches) arrives at
/// warn+ and gets through.
///
/// Silence here is not concealment: `RUST_LOG=ort=info` brings it all back.
const QUIET_DEPS: [&str; 1] = ["ort=warn"];

/// Log filter: our own level plus silence for third-party libraries.
///
/// The silence is LAYERED ON TOP OF `RUST_LOG`, not substituted for it. This is not a
/// nitpick: `RUST_LOG=info` (which is set in `.env`) would override the default entirely —
/// and the whole point would be lost, the log flooded with ORT internals again. That is
/// exactly how this was caught.
///
/// Third-party debug output can be brought back by naming the library explicitly:
/// `RUST_LOG=info,ort=info`. Then we do not interfere — the human knows what they are asking for.
pub fn log_filter(default_level: &str) -> EnvFilter {
    let requested = std::env::var("RUST_LOG").unwrap_or_else(|_| default_level.to_string());
    let mut filter = EnvFilter::new(&requested);
    for quiet in quiet_directives(&requested) {
        if let Ok(d) = quiet.parse() {
            filter = filter.add_directive(d);
        }
    }
    filter
}

/// Which third-party libraries to silence for a given request. A separate function so that
/// the rule is proved by a test, rather than eyeballed in the log.
fn quiet_directives(requested: &str) -> Vec<&'static str> {
    QUIET_DEPS
        .iter()
        .filter(|q| {
            let target = q.split('=').next().unwrap_or_default();
            // The human named the library explicitly — we do not argue, they know what they want.
            !requested.contains(target)
        })
        .copied()
        .collect()
}

/// Color goes ONLY to a terminal.
///
/// The output of a child cook is captured by the daemon and poured into its own log
/// (WP-C17). Colorize unconditionally and the log gets raw escape sequences —
/// `\x1b[2m2026-07-13T…\x1b[0m` instead of a timestamp. A log you have to repair with your
/// eyes is a log people stop reading.
fn ansi_ok() -> bool {
    use std::io::IsTerminal;
    std::io::stderr().is_terminal()
}

/// Logs for the tools (`localvox-process`, `localvox-api`, `localvox-mcp`): one call —
/// and both the filter and the color are decided identically in every binary.
pub fn init_tracing_tool(default_level: &str) {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(log_filter(default_level))
        .with_target(false)
        .with_ansi(ansi_ok())
        .with_writer(std::io::stderr)
        .try_init();
}

pub fn init_tracing(debug: bool, tui: bool) {
    let filter = if tui && !debug {
        // Verbose stages go to the TUI panel with --verbose; to stderr without --debug only error+.
        log_filter("error")
    } else if debug {
        // Even under --debug, ORT stays silent: the human is debugging OUR code, not
        // somebody else's allocator. If they want it too — `RUST_LOG=debug,ort=debug`.
        log_filter("debug,localvox_light_core=debug,localvox_light_core::pipeline=debug")
    } else {
        log_filter("info")
    };
    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_ansi(ansi_ok()),
        )
        .try_init();
}

#[cfg(test)]
mod log_tests {
    use super::*;

    /// THE TRAP THIS WAS CAUGHT ON: `.env` has `RUST_LOG=info`, which overrode the
    /// default ENTIRELY — and ONNX Runtime internals ("Allocated memory at 000001D0…",
    /// "Extended allocation by 16777216 bytes") flooded the cook log.
    /// The silence of third-party libraries is layered ON TOP OF any request.
    #[test]
    fn a_plain_rust_log_does_not_bring_back_the_noise_of_other_libraries() {
        assert_eq!(quiet_directives("info"), ["ort=warn"]);
        assert_eq!(quiet_directives("debug,localvox_light_core=debug"), ["ort=warn"]);
    }

    /// But if the human ASKED for third-party debug output — they get it. Silence must not
    /// turn into an inability to look.
    #[test]
    fn asking_for_the_library_explicitly_gives_it_back() {
        assert!(quiet_directives("info,ort=info").is_empty());
        assert!(quiet_directives("ort=debug").is_empty());
    }
}

/// The path from the CLI / `.env`: trim and strip one pair of quotes `"…"` / `'…'`
/// (a common mistake in .env).
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

/// Validation of an unpacked Vosk model directory (`am/`, `conf/`, `graph/`).
pub fn validate_vosk_model_dir(p: &Path) -> Result<()> {
    if !p.exists() {
        anyhow::bail!(
            "Vosk model: directory not found: {}. By default models/vosk-model-ru-0.42 is expected after scripts/fetch-models.* (or pass --model).",
            p.display()
        );
    }
    if !p.is_dir() {
        anyhow::bail!(
            "Vosk model: a directory with the unpacked model is expected, not a file: {}",
            p.display()
        );
    }
    // The standard model archive: a root like vosk-model-ru-0.42/ with am/, conf/, graph/
    let am = p.join("am");
    if !am.is_dir() {
        anyhow::bail!(
            "Vosk model: there is no am/ directory in {}. Point at the root of the unpacked model (not the parent folder and not conf/graph inside it). It must contain am/, conf/, graph/.",
            p.display()
        );
    }
    if !am.join("final.mdl").is_file() {
        anyhow::bail!(
            "Vosk model: no am/final.mdl in {} — the model archive is incomplete or corrupted.",
            p.display()
        );
    }
    Ok(())
}

/// Before audio capture and the TUI: the model is needed for recording (not for
/// `--list-devices`).
pub fn validate_vosk_model(cli: &Cli) -> Result<()> {
    let path_str = normalized_model_path(cli);
    if path_str.is_empty() {
        anyhow::bail!(
            "LOCALVOX_LIGHT_MODEL is empty. Set the model directory or remove the variable (default: models/vosk-model-ru-0.42)."
        );
    }
    validate_vosk_model_dir(Path::new(&path_str))
}

pub fn print_devices() {
    println!("=== Input devices (microphones) ===");
    for (i, (dev, name)) in audio::collect_input_devices().iter().enumerate() {
        let id = audio::device_id_save_token(dev).unwrap_or_else(|| "—".into());
        println!("  [{i}] {name}  |  id: {id}");
    }
    println!("\n=== Loopback / system audio ===");
    #[cfg(windows)]
    {
        println!("(Windows: output names for WASAPI loopback)");
        for (i, name) in audio::list_output_device_names() {
            println!("  [{i}] {name}");
        }
    }
    #[cfg(not(windows))]
    {
        #[cfg(target_os = "macos")]
        println!("(macOS: **outputs** for CPAL loopback; copy the id into the config)");
        #[cfg(not(target_os = "macos"))]
        println!("(Linux and others: monitor inputs; copy the id into the config)");
        let lb = audio::list_loopback_capture_devices();
        #[cfg(target_os = "macos")]
        if lb.is_empty() {
            if let Some(hint) = audio::macos_loopback_empty_hint() {
                println!("  ! {hint}");
            }
        }
        for (i, (dev, name)) in lb.iter().enumerate() {
            let id = audio::device_id_save_token(dev).unwrap_or_else(|| format!("lbidx:{i}"));
            println!("  [{i}] {name}  |  id: {id}");
        }
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
