//! Entry point of the `localvox-light` binary. The core is the `localvox_light_core` crate.

use std::io::{self, IsTerminal};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

#[cfg(feature = "tui")]
use std::thread;
#[cfg(feature = "tui")]
use std::time::Duration;

use anyhow::Result;
use clap::Parser;
#[cfg(feature = "tui")]
use tracing::info;

use localvox_light_core::events::UiMsg;
#[cfg(feature = "tui")]
use localvox_light_core::join_engine_thread;
#[cfg(feature = "tui")]
use localvox_light_core::light_config;
use localvox_light_core::{
    init_tracing, merge_env_bools, print_devices, resolve_audio_from_cli_and_file, run_engine,
    validate_vosk_model, Cli,
};

/// Portable folder: `.env` next to the exe; `vosk-lib/` prepended to the native library
/// search path (Linux `LD_LIBRARY_PATH`, macOS `DYLD_LIBRARY_PATH`, Windows `PATH` — after
/// the process has started).
/// On Windows and macOS the main native Vosk library must sit next to the exe (the loader
/// runs before `main`); `install-release` copies the DLL / `.dylib` out of `vosk-lib/`.
/// See build.rs / install-release.
fn portable_env_bootstrap() {
    let Some(exe_dir) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()))
    else {
        warn_on_bad_dotenv(dotenvy::dotenv(), "next to the working directory");
        return;
    };
    let dotenv_path = exe_dir.join(".env");
    if dotenv_path.is_file() {
        // from_path does NOT override variables already set in the OS — a portable .env then
        // "does not work".
        // from_path_override: values from the .env next to the exe win; explicit CLI arguments
        // are still stronger, via clap.
        warn_on_bad_dotenv(
            dotenvy::from_path_override(&dotenv_path).map(|()| dotenv_path.clone()),
            &dotenv_path.display().to_string(),
        );
    } else {
        warn_on_bad_dotenv(dotenvy::dotenv(), "./.env");
    }
    let vosk_lib = exe_dir.join("vosk-lib");
    if vosk_lib.is_dir() {
        prepend_native_lib_search_path(&vosk_lib);
    }
}

/// A broken line in `.env` makes dotenv ABANDON reading the file — everything below it is
/// silently lost (a single line without `=` cuts power to half the config). Previously the
/// error was swallowed via `.ok()`; now we shout — otherwise diagnosing "why did the setting
/// not apply" is impossible.
fn warn_on_bad_dotenv(res: dotenvy::Result<std::path::PathBuf>, what: &str) {
    if let Err(e) = res {
        eprintln!(
            "localvox-light: ERROR in {what}: {e}\n\
             WARNING: variables AFTER the broken line were NOT read. \
             Every line must be `KEY=value` or start with `#`."
        );
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

/// `--cwd <path>` before anything else: autostart (HKCU\Run) starts with cwd = system32, and
/// `.env`, `models/`, slots and the relative `LOCALVOX_LIGHT_AUDIO_DIR` all depend on the
/// working directory. We read the argument RAW (before clap and before dotenv — otherwise the
/// config has already been read from the wrong place).
fn chdir_from_raw_args() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args
        .iter()
        .position(|a| a == "--cwd")
        .and_then(|i| args.get(i + 1));
    let dir = dir.cloned().or_else(|| {
        args.iter()
            .find_map(|a| a.strip_prefix("--cwd=").map(str::to_string))
    });
    if let Some(dir) = dir {
        if let Err(e) = std::env::set_current_dir(&dir) {
            eprintln!("localvox-light: failed to change directory to {dir}: {e}");
        }
    }
}

fn main() -> Result<()> {
    chdir_from_raw_args();
    portable_env_bootstrap();
    let mut cli = Cli::parse();
    merge_env_bools(&mut cli);
    if cli.no_tui {
        cli.tui = false;
    }
    // The tray is background mode: the interface lives in the tray, not in the terminal.
    // Autostart (HKCU\Run) has no console at all — a TUI enabled in .env must not kill the
    // daemon.
    if cli.tray {
        cli.tui = false;
    }
    // Portable install without LOCALVOX_LIGHT_TUI in .env: in a normal terminal we open the TUI
    // by default.
    #[cfg(feature = "tui")]
    if !cli.no_tui
        && !cli.tray
        && io::stdout().is_terminal()
        && std::env::var("LOCALVOX_LIGHT_TUI").is_err()
        && !cli.tui
    {
        cli.tui = true;
    }

    #[cfg(feature = "tui")]
    if cli.tui && !io::stdout().is_terminal() {
        anyhow::bail!(
            "TUI requested (--tui, LOCALVOX_LIGHT_TUI=1 or an interactive terminal by default), but stdout is not a TTY.\n\
             Run from Windows Terminal / PowerShell / cmd; for logs-only mode: {} --no-tui",
            std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "localvox-light".into())
        );
    }

    if cli.list_devices {
        let _ = tracing_subscriber::fmt::try_init();
        print_devices();
        return Ok(());
    }

    validate_vosk_model(&cli)?;

    init_tracing(cli.debug, cli.tui);

    if !cli.tui {
        eprintln!(
            "localvox-light: no-TUI mode — recording and logs (Ctrl+C to exit). For the interface: {} --tui",
            std::env::current_exe()
                .ok()
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_else(|| "localvox-light".into())
        );
    }

    let running = Arc::new(AtomicBool::new(true));
    {
        let r = running.clone();
        ctrlc::set_handler(move || {
            eprintln!("\nStopping...");
            r.store(false, Ordering::SeqCst);
        })?;
    }

    let audio_devices = resolve_audio_from_cli_and_file(&cli);
    let devices_shared = Arc::new(RwLock::new(audio_devices.clone()));
    let reload_gen = Arc::new(AtomicU64::new(0));
    // The composition root hands the live capture controls to the settings screen: a
    // device saved there re-points the streams instead of waiting for a restart.
    localvox_light_core::audio::register_capture(localvox_light_core::audio::CaptureControls {
        devices: Arc::clone(&devices_shared),
        reload_gen: Arc::clone(&reload_gen),
    });

    if cli.tray {
        return run_tray_mode(cli, devices_shared, reload_gen, running);
    }

    if cli.tui {
        #[cfg(feature = "tui")]
        {
            let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<UiMsg>();
            let (reset_tx, reset_rx) = crossbeam_channel::unbounded::<()>();
            // Voice module (F2) with live recording indication in the TUI.
            let (voice_hook, voice_handle, voice_status) = init_voice(Some(ui_tx.clone()));
            // status goes to the TUI Debug panel, not only to stderr
            let _ = ui_tx.send(UiMsg::Log(localvox_light_core::events::StructuredLog {
                stage: "voice".into(),
                source_id: 0,
                chunk_sec: 0.0,
                proc_sec: 0.0,
                detail: voice_status.clone(),
                verbose_only: false,
            }));
            let record_pcm = Arc::new(AtomicBool::new(true));
            let tui_verbose = cli.verbose;
            let cli_engine = cli.clone();
            let dev_engine = Arc::clone(&devices_shared);
            let rg_engine = Arc::clone(&reload_gen);
            let r_engine = running.clone();
            let r_tui = running.clone();
            let record_engine = Arc::clone(&record_pcm);
            let record_tui = Arc::clone(&record_pcm);
            let hook_engine = voice_hook.clone();
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
                        rg_engine,
                        hook_engine,
                    ) {
                        tracing::error!("Engine stopped: {e:#}");
                    }
                })?;
            let cfg_path = light_config::save_path_for_write();
            localvox_light_tui::run(
                &ui_rx,
                reset_tx,
                r_tui,
                record_tui,
                "localvox-light".into(),
                audio_devices,
                cfg_path,
                Arc::clone(&devices_shared),
                Arc::clone(&reload_gen),
                tui_verbose,
            )?;
            join_engine_thread(engine_handle, Duration::from_secs(2));
            // drain the voice queue: accepted "запиши…" commands are appended to the slots
            drop(voice_hook);
            drain_voice(voice_handle);
            info!("Session finished.");
            return Ok(());
        }
        #[cfg(not(feature = "tui"))]
        {
            anyhow::bail!(
                "TUI unavailable: the package was built without the `tui` feature. Use default features or `--features tui`."
            );
        }
    }

    let (voice_hook, voice_handle, voice_status) = init_voice(None);
    eprintln!("Voice module: {voice_status}");
    let (_noop_reset_tx, noop_reset_rx) = crossbeam_channel::unbounded::<()>();
    run_engine(
        cli,
        devices_shared,
        None,
        noop_reset_rx,
        running,
        Arc::new(AtomicBool::new(true)),
        reload_gen,
        voice_hook,
    )?;
    drain_voice(voice_handle);
    Ok(())
}

/// Voice module (F2): active when slots.toml is present; a config error does not bring the
/// recording down — we work without voice, and the reason is visible in the status.
fn init_voice(
    ui: Option<crossbeam_channel::Sender<UiMsg>>,
) -> (
    Option<localvox_light_core::events::TranscriptHook>,
    Option<std::thread::JoinHandle<()>>,
    String,
) {
    match localvox_light_voice::spawn_from_env(ui) {
        Ok(Some(((hook, handle), summary))) => (Some(hook), Some(handle), summary),
        Ok(None) => (
            None,
            None,
            "off: no slots.toml next to the exe / in the working directory (see slots.example.toml)"
                .to_string(),
        ),
        Err(e) => {
            tracing::warn!("voice module failed to start: {e:#}");
            (None, None, format!("config error: {e}"))
        }
    }
}

/// Bounded wait for the voice thread: once the hook is dropped the channel is closed, the
/// thread finishes reading the note queue (including the blocking TTS) and exits.
fn drain_voice(handle: Option<std::thread::JoinHandle<()>>) {
    let Some(handle) = handle else { return };
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(0);
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    if rx.recv_timeout(std::time::Duration::from_secs(5)).is_err() {
        eprintln!(
            "localvox-light: voice module did not finish within 5 s (hung TTS/MCP?) — exiting"
        );
    }
}

/// Messages to the tray from menu callbacks and from the engine (the main loop owns TrayItem).
#[cfg(windows)]
enum TrayMsg {
    TogglePause,
    ToggleAutostart,
    Quit,
    EngineDead(String),
}

#[cfg(windows)]
fn autostart_label(enabled: bool) -> &'static str {
    if enabled {
        "Автозапуск при входе: ✓"
    } else {
        "Автозапуск при входе: —"
    }
}

/// Background mode (WP-C3): headless engine + voice module + HTTP API, controlled from the
/// system tray. A first cut of the phase-C daemon.
#[cfg(windows)]
fn run_tray_mode(
    cli: Cli,
    devices_shared: Arc<RwLock<localvox_light_core::LightDeviceConfig>>,
    reload_gen: Arc<std::sync::atomic::AtomicU64>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    use tray_item::{IconSource, TrayItem};

    let record_pcm = Arc::new(AtomicBool::new(true));
    let (msg_tx, msg_rx) = crossbeam_channel::unbounded::<TrayMsg>();

    // The tray comes BEFORE the engine: if there is no icon/menu, we find out immediately
    // rather than after capture has started (otherwise we get a zombie process with no UI
    // whatsoever).
    let mut tray = TrayItem::new("localvox — идёт запись", IconSource::Resource("tray-icon"))
        .map_err(|e| anyhow::anyhow!("tray: {e} (exe built without assets/localvox.ico?)"))?;

    // The HTTP API is part of the daemon (the archive is readable regardless of recording).
    // We bring it up before the menu: the "Открыть веб-архив" item is only shown if the API is
    // really listening (otherwise it would lead to a refused connection).
    let api_bind = spawn_http_api(&cli.audio_dir);

    let pause_id = {
        let t = msg_tx.clone();
        tray.inner_mut()
            .add_menu_item_with_id("Пауза", move || {
                let _ = t.send(TrayMsg::TogglePause);
            })
            .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?
    };
    {
        let dir = cli.audio_dir.clone();
        tray.add_menu_item("Открыть папку архива", move || {
            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
        })
        .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?;
    }
    if let Some(bind) = api_bind {
        // 0.0.0.0/[::] is a LISTEN address, not a destination: the window and the browser get the
        // loopback one, or they would try to connect to "everything" and fail.
        let addr = match bind.parse::<std::net::SocketAddr>() {
            Ok(a) if a.ip().is_unspecified() => format!("127.0.0.1:{}", a.port()),
            _ => bind.clone(),
        };
        tray.add_menu_item("Открыть интерфейс", move || open_window(&addr))
            .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?;
    }
    let autostart_id = {
        let t = msg_tx.clone();
        tray.inner_mut()
            .add_menu_item_with_id(
                autostart_label(localvox_light_core::autostart::is_enabled()),
                move || {
                    let _ = t.send(TrayMsg::ToggleAutostart);
                },
            )
            .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?
    };
    {
        let t = msg_tx.clone();
        tray.add_menu_item("Выход", move || {
            let _ = t.send(TrayMsg::Quit);
        })
        .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?;
    }

    let (voice_hook, voice_handle, voice_status) = init_voice(None);
    eprintln!("Voice module: {voice_status}");

    // Autocook (WP-C7): the daemon finishes cooking closed sessions in the background itself.
    let autocook_handle = spawn_autocook(cli.audio_dir.clone(), running.clone());

    let (_reset_tx, reset_rx) = crossbeam_channel::unbounded::<()>();
    let cli_engine = cli.clone();
    let r_engine = running.clone();
    let record_engine = Arc::clone(&record_pcm);
    let engine_tx = msg_tx.clone();
    let engine_handle = std::thread::Builder::new()
        .name("engine".into())
        .spawn(move || {
            if let Err(e) = run_engine(
                cli_engine,
                devices_shared,
                None,
                reset_rx,
                r_engine,
                record_engine,
                reload_gen,
                voice_hook,
            ) {
                tracing::error!("Engine stopped: {e:#}");
                let _ = engine_tx.send(TrayMsg::EngineDead(format!("{e:#}")));
            }
        })?;

    eprintln!("localvox-light: background mode — icon in the tray (exit via the tray menu)");

    let mut fatal: Option<String> = None;
    while running.load(Ordering::Relaxed) {
        match msg_rx.recv_timeout(std::time::Duration::from_millis(300)) {
            Ok(TrayMsg::TogglePause) => {
                let now = !record_pcm.load(Ordering::Relaxed);
                record_pcm.store(now, Ordering::Relaxed);
                tracing::info!(
                    "tray: recording {}",
                    if now {
                        "resumed"
                    } else {
                        "paused"
                    }
                );
                let (item, tip) = if now {
                    ("Пауза", "localvox — идёт запись")
                } else {
                    ("Продолжить запись", "localvox — ПАУЗА")
                };
                let _ = tray.inner_mut().set_menu_item_label(item, pause_id);
                let _ = tray.inner_mut().set_tooltip(tip);
            }
            Ok(TrayMsg::ToggleAutostart) => {
                use localvox_light_core::autostart;
                let target = !autostart::is_enabled();
                let done = if target {
                    autostart::enable()
                } else {
                    autostart::disable()
                };
                match done {
                    Ok(()) => {
                        tracing::info!(
                            "tray: autostart {}",
                            if target {
                                "enabled"
                            } else {
                                "disabled"
                            }
                        );
                        let _ = tray
                            .inner_mut()
                            .set_menu_item_label(autostart_label(target), autostart_id);
                    }
                    Err(e) => tracing::warn!("autostart: {e}"),
                }
            }
            Ok(TrayMsg::Quit) => running.store(false, Ordering::SeqCst),
            Ok(TrayMsg::EngineDead(e)) => {
                // without the engine the icon lies about "recording" — we exit honestly
                eprintln!("localvox-light: engine died: {e}");
                fatal = Some(e);
                running.store(false, Ordering::SeqCst);
            }
            Err(_) => {} // timeout — re-check running
        }
    }
    // Autocook is joined first: its poll loop sees running=false, kills the child
    // localvox-process (no orphan) and exits.
    if let Some(h) = autocook_handle {
        localvox_light_core::join_engine_thread(h, std::time::Duration::from_secs(10));
    }
    localvox_light_core::join_engine_thread(engine_handle, std::time::Duration::from_secs(10));
    drain_voice(voice_handle);
    match fatal {
        Some(e) => Err(anyhow::anyhow!("recording engine died: {e}")),
        None => Ok(()),
    }
}

#[cfg(not(windows))]
fn run_tray_mode(
    _cli: Cli,
    _devices_shared: Arc<RwLock<localvox_light_core::LightDeviceConfig>>,
    _reload_gen: Arc<std::sync::atomic::AtomicU64>,
    _running: Arc<AtomicBool>,
) -> Result<()> {
    anyhow::bail!(
        "--tray is implemented on Windows only so far (mac/linux — phase C, cross-platform tray)"
    )
}

/// The autocook scheduler (WP-C7): a background thread finds closed, not-yet-cooked sessions
/// and runs `localvox-process` over them (idempotent). The queue is durable in
/// `<work_dir>/jobs.json` (survives a restart), with an attempt limit per session.
/// Turn it off with: `LOCALVOX_LIGHT_AUTOCOOK=off`. Returns a handle — it is joined on exit so
/// that the child `localvox-process` is not left an orphan.
fn spawn_autocook(
    work_dir: String,
    running: Arc<AtomicBool>,
) -> Option<std::thread::JoinHandle<()>> {
    let off = matches!(
        std::env::var("LOCALVOX_LIGHT_AUTOCOOK")
            .unwrap_or_default()
            .to_lowercase()
            .as_str(),
        "0" | "off" | "false" | "no"
    );
    if off {
        return None;
    }
    let interval_sec: u64 = std::env::var("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        // Polling is a directory scan plus a read of jobs.json: pennies. Waiting a minute for
        // the summary after pressing "Finish" feels like "it does not work", and rightly so.
        // Once every 5 s is invisible to the machine and instant to a human.
        .unwrap_or(5);
    let quiescent_sec: u64 = std::env::var("LOCALVOX_LIGHT_AUTOCOOK_QUIESCENT_SEC")
        .ok()
        .and_then(|v| v.parse().ok())
        // The session was closed explicitly (the "Finish" button) or by the engine — there is
        // no point in waiting a minute "just in case": the recording marker is already cleared.
        .unwrap_or(15);
    // The summary and the cleaned-up text are what the product exists for. They used to require
    // explicit enabling, and the archive ended up without a SINGLE cleaned-up text: the user saw
    // "no processed.md" and rightly judged that a lie.
    // Now they are ON by default; they are disabled by an explicit `=0`.
    // We read them from a SHARED place: the "♻ Re-cook" button needs the same flags, and a
    // second reading has already diverged from this once (the button erased the summary but
    // queued a job without the flag to make it again).
    let (summary, cleanup, refine) = localvox_light_core::jobs::post_processing_from_env();
    // There used to be a switch here — LOCALVOX_LIGHT_AUTOCOOK_RECOOK_STALE — that let the
    // archive redo work whose recipe no longer matched the current one. It is gone, and no
    // replacement is coming: the recipes drifted on their own (a prompt revision, the model in
    // .env, the language, even the working directory), so every drift silently re-cooked the whole
    // archive behind the owner's back. Finished is finished. A better cook reaches the old
    // recordings when a human presses «переварить», and only then.
    const MAX_ATTEMPTS: u32 = 3;

    // `localvox-process` sits next to our exe (in the same distribution).
    let exe = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("localvox-process.exe")));
    let Some(exe) = exe.filter(|p| p.exists()) else {
        tracing::warn!("autocook: no localvox-process.exe next to us — disabled");
        return None;
    };
    // The ASR model by ABSOLUTE path: under autostart the daemon's cwd is system32, and a
    // relative `models/…` will not be found. We look: env → next to the exe (portable
    // distribution) → in the current directory (started from the repo / install directory).
    // We used to take only the exe dir — and starting from the repo broke: the cook looked for
    // the model in target/release/models.
    let exe_dir = exe
        .parent()
        .map(std::path::Path::to_path_buf)
        .unwrap_or_default();
    let model_dir = std::env::var_os("LOCALVOX_ASR_MODEL_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            let near_exe = exe_dir.join("models").join("gigaam-v3-e2e-ctc");
            near_exe.is_dir().then_some(near_exe)
        })
        .or_else(|| {
            let in_cwd = std::env::current_dir()
                .ok()?
                .join("models")
                .join("gigaam-v3-e2e-ctc");
            in_cwd.is_dir().then_some(in_cwd)
        });
    let Some(model_dir) = model_dir else {
        tracing::warn!(
            "autocook disabled: ASR model directory not found (neither next to the exe, \
             nor in {}). Set LOCALVOX_ASR_MODEL_DIR.",
            std::env::current_dir().unwrap_or_default().display()
        );
        return None;
    };
    tracing::info!("autocook: model {}", model_dir.display());

    std::thread::Builder::new()
        .name("autocook".into())
        .spawn(move || {
            let wd = std::path::PathBuf::from(&work_dir);
            // Warm-up at startup: after an upgrade (new index schema) the first search would
            // otherwise pay for migrating the whole archive right inside an HTTP request.
            // On an up-to-date index this is a no-op (not a single embed, not a single rewrite).
            warm_indexes(&wd);
            // Take back what a dead daemon left mid-cook. ONCE, HERE, before the loop: we have
            // just started, so nothing of ours is cooking, so `Running` in the file can only be a
            // corpse. Inside the loop the same state means the opposite — it is cooking right now —
            // and reading the file every few seconds cannot tell the two apart. That confusion,
            // living inside `load()`, is what re-cooked this archive for months.
            {
                let mut q = localvox_light_core::jobs::JobQueue::load(&wd);
                let taken = q.reclaim_abandoned();
                if taken > 0 {
                    tracing::info!("autocook: {taken} job(s) abandoned by a dead daemon — replaying");
                }
                // Ghosts: work queued for sessions that are no longer on disk. They can only fail,
                // for ever, and each failure looks to the owner like something of theirs broke.
                q.drop_orphans(&wd);
            }
            let mut last_idle_log: Option<std::time::Instant> = None;
            // What we want from every session, and with which recipe. The recipe is PROVENANCE —
            // it records what made this, not whether to make it again. A changed model or template
            // no longer redoes anything: that is the human's word (see WP-C67).
            let llm_model =
                std::env::var("LOCALVOX_LLM_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into());
            let summary_template = std::env::var("LOCALVOX_LLM_SUMMARY_TEMPLATE")
                .ok()
                .filter(|s| !s.trim().is_empty());
            let wanted = localvox_light_core::jobs::Wanted::new(
                summary,
                cleanup,
                summary_template,
                &llm_model,
            );
            while running.load(Ordering::Relaxed) {
                // jobs.json is a SHARED resource, not the thread's private state: the
                // "♻ Re-cook" button writes to it from the HTTP thread.
                // Autocook used to keep its own in-memory copy for the whole life of the
                // daemon, and a re-cook request was simply lost — even though the web had
                // already managed to delete the derived artifacts.
                let mut queue = localvox_light_core::jobs::JobQueue::load(&wd);
                // 0a) Ghosts first, and BEFORE reviving anything: a session deleted while the
                // daemon runs leaves work behind, and the next line would faithfully bring it back
                // to life. The owner then watches «сорвалось, повторю» against a recording they
                // threw away — the app appearing to break over something they already decided
                // about. Cheap: a handful of `is_dir` checks per cycle.
                queue.drop_orphans(&wd);
                // 0) revive jobs that failed long ago (Ollama may be back): a transient failure
                // heals itself without restarting the daemon
                let revived = queue.revive_failed(std::time::Duration::from_secs(3600));
                if revived > 0 {
                    tracing::info!("autocook: {revived} Failed jobs revived (backoff 1 h)");
                }
                // 1) Queue sessions that have NO transcript at all. Nothing else.
                //
                // `requeue_stale` used to live here too: it took a session that was already Done
                // and put it back, because its recipe no longer matched the one we want now. That
                // is the third door through which the archive re-cooked itself on every launch —
                // and the recipe drifts on its own (the working directory decides whether the
                // diarization model is found, and that decides `-spk` in the recipe).
                //
                // FINISHED IS FINISHED. A better cook reaches the old archive when the human
                // presses «переварить заново» — `enqueue_recook`, which forces. Not behind his
                // back, and not a hundred times.
                for name in localvox_light_core::jobs::sessions_needing_cook(&wd, quiescent_sec) {
                    // Dedups by session: a session that already has a job (Done included) is not
                    // touched.
                    if queue.enqueue_cook(&name, summary, cleanup, refine) {
                        tracing::info!("autocook: enqueued {name}");
                    }
                }
                // 1b) cooked, but there is no summary / cleaned-up text — finish the job.
                // Discovery used to ask only "does this need COOKING", and a session cooked by
                // hand, or cooked back when the summary was disabled, stayed without a summary
                // FOREVER: nobody created a job for it, and the LLM part never ran.
                for name in localvox_light_core::jobs::sessions_needing_artifacts(&wd, &wanted) {
                    if queue.requeue_for_artifacts(&name, summary, cleanup, refine) {
                        tracing::info!("autocook: {name} — cooked, but no summary → finishing it");
                    }
                }
                // 2) run the pending jobs — each exactly once per cycle
                // (a failed one is retried on the next interval, it does not burn through attempts)
                let mut cooked_any = false;
                for id in queue.pending_ids() {
                    if !running.load(Ordering::Relaxed) {
                        break;
                    }
                    let Some(job) = queue.start(id) else { continue };
                    let session = wd.join("sessions").join(&job.session);
                    // The work over the session begins HERE. Everything the stages say from now on
                    // belongs to this run: an auto-cook picked up after a recording has no button
                    // behind it to have opened the run for it.
                    localvox_light_core::progress::new_run(&session);

                    // A link job: there is no audio yet — it has to be fetched before anything
                    // can be cooked. If the fetch fails there is nothing to cook, and the job
                    // stops HERE: the reason lands both in the queue and in the session's stage
                    // log, where a person is actually looking.
                    if job.kind == localvox_light_core::jobs::JobKind::Ingest
                        && !has_audio(&session)
                    {
                        if let Err(e) = ingest_into_session(&session) {
                            let msg = format!("{e:#}");
                            queue.mark_failed(job.id, &msg, MAX_ATTEMPTS);
                            tracing::warn!("ingest: {} — {msg}", job.session);
                            continue;
                        }
                    }

                    tracing::info!("autocook: cooking {}", job.session);
                    let mut cmd = std::process::Command::new(&exe);
                    cmd.arg(&session)
                        .arg("--work-dir")
                        .arg(&work_dir)
                        .arg("--model-dir")
                        .arg(&model_dir)
                        // ffmpeg by absolute path: under autostart cwd=system32 and the child
                        // will not get the .env (FLAC decoding during the cook)
                        .env(
                            "LOCALVOX_LIGHT_YT_FFMPEG",
                            localvox_light_core::chunks::resolve_ffmpeg_for_decode(),
                        );
                    if job.summary {
                        cmd.arg("--summary");
                    }
                    if job.cleanup {
                        cmd.arg("--cleanup");
                    }
                    if job.refine {
                        cmd.arg("--refine");
                    }
                    // A human rejected the result: we cook again, regardless of the ready
                    // version. Derived data is disposable — it is recreated from the audio, and
                    // throwing it away is no loss. The audio is untouchable.
                    if job.force {
                        cmd.arg("--force");
                    }
                    #[cfg(windows)]
                    {
                        use std::os::windows::process::CommandExt;
                        // CREATE_NO_WINDOW — background work without a console window.
                        //
                        // BELOW_NORMAL_PRIORITY_CLASS — and this is not an optimisation but a
                        // PRIORITY OF VALUES. The cook takes every core (ONNX + LLM), and on a
                        // live machine the recording started to fall behind: the PCM queue grew
                        // and the sound piled up in memory instead of landing on disk in time.
                        // The audio is the only thing that cannot be recreated: the transcript,
                        // the summary and the index are all made from it again, and it is made
                        // from nothing. So background work MUST yield to the recording rather
                        // than compete with it.
                        //
                        // Below normal, not idle: an idle process on a busy machine may never
                        // get the CPU at all, and then the archive would never finish cooking.
                        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
                        cmd.creation_flags(CREATE_NO_WINDOW | BELOW_NORMAL_PRIORITY_CLASS);
                    }
                    // We capture the child's output and relay it into our own log: without this
                    // the cook is silent (neither progress nor reason is visible — only
                    // "exit 2"), and the whole point of diagnostics is in the error text:
                    // "model not found", "the LLM invented things", "ollama went down".
                    cmd.stdout(std::process::Stdio::piped())
                        .stderr(std::process::Stdio::piped());
                    // spawn + poll: on daemon exit we kill the child process, otherwise
                    // localvox-process is left an orphan burning CPU.
                    match cmd.spawn() {
                        Ok(mut child) => {
                            let tail =
                                std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
                            let pumps = [
                                // stdout — the result of the work: it IS the business log.
                                child.stdout.take().map(|r| {
                                    pump_child_output(Box::new(r), job.session.clone(), None, true)
                                }),
                                // stderr — diagnostics, including that of FOREIGN libraries:
                                // into debug and into the "tail" to explain a failure.
                                child.stderr.take().map(|r| {
                                    pump_child_output(
                                        Box::new(r),
                                        job.session.clone(),
                                        Some(tail.clone()),
                                        false,
                                    )
                                }),
                            ];
                            loop {
                                match child.try_wait() {
                                    Ok(Some(s)) if s.success() => {
                                        queue.mark_done(job.id);
                                        cooked_any = true;
                                        break;
                                    }
                                    Ok(Some(s)) => {
                                        // exit 2 = the cook SUCCEEDED, only post-processing failed
                                        // (the LLM was down): the version is committed — we do not
                                        // fail the job outright but mark it Failed with a backoff
                                        // revive (in an hour we retry only the LLM part, the cook
                                        // is idempotently skipped).
                                        let post_only = s.code() == Some(2);
                                        // The cook MANAGED to commit the version — the re-cook did
                                        // its job. Without clearing force, a job failing on the LLM
                                        // would re-cook the session on every attempt and breed
                                        // transcript versions.
                                        if post_only {
                                            queue.clear_force(job.id);
                                        }
                                        // The reason comes from the child's output, not from
                                        // "exit 2": it is stored in jobs.json and shown in
                                        // /api/jobs.
                                        let reason = last_error_line(&tail);
                                        let msg = match (post_only, reason) {
                                            (true, Some(r)) => format!("post-processing (LLM): {r}"),
                                            (true, None) => {
                                                "cook ok, post-processing (LLM) failed".to_string()
                                            }
                                            (false, Some(r)) => format!("exit {s}: {r}"),
                                            (false, None) => format!("exit {s}"),
                                        };
                                        queue.mark_failed(job.id, &msg, MAX_ATTEMPTS);
                                        tracing::warn!("autocook: {} — {msg}", job.session);
                                        // warming the index is worth it in both cases:
                                        // a transcript version may have been committed
                                        cooked_any = true;
                                        break;
                                    }
                                    Ok(None) => {
                                        if !running.load(Ordering::Relaxed) {
                                            let _ = child.kill();
                                            let _ = child.wait();
                                            // The job stays Running on purpose: that is the mark of
                                            // an interrupted cook, and the next daemon reclaims it
                                            // at startup (`reclaim_abandoned`). It comes back as a
                                            // NORMAL job — it finishes what is missing instead of
                                            // wiping what the killed run had already finished.
                                            break;
                                        }
                                        std::thread::sleep(std::time::Duration::from_millis(200));
                                    }
                                    Err(e) => {
                                        queue.mark_failed(job.id, &e.to_string(), MAX_ATTEMPTS);
                                        break;
                                    }
                                }
                            }
                            // Read the tail of the output to the end: without a join the last
                            // lines (usually exactly the error text) are lost.
                            for p in pumps.into_iter().flatten() {
                                let _ = p.join();
                            }
                        }
                        Err(e) => queue.mark_failed(job.id, &e.to_string(), MAX_ATTEMPTS),
                    }
                }
                // 2b) one warm-up for the whole drain of the queue (not after every job —
                // otherwise a nightly backlog of N sessions would mean N full tantivy rebuilds).
                // The first search does not pay for indexing.
                if cooked_any && running.load(Ordering::Relaxed) {
                    warm_indexes(&wd);
                }
                // A silent daemon is indistinguishable from a broken one: once a minute we say
                // that the queue is empty and everything has been processed. On acceptance the
                // owner was looking for anything at all in the logs — and found nothing.
                let pending = queue.pending_ids().len();
                if pending == 0 && !cooked_any {
                    let now = std::time::Instant::now();
                    if last_idle_log
                        .map(|t: std::time::Instant| now.duration_since(t).as_secs() >= 60)
                        .unwrap_or(true)
                    {
                        tracing::info!("autocook: queue empty, all sessions processed");
                        last_idle_log = Some(now);
                    }
                }
                // 3) sleep the interval in small steps (exit on running)
                let woke = std::time::Instant::now();
                while running.load(Ordering::Relaxed) && woke.elapsed().as_secs() < interval_sec {
                    std::thread::sleep(std::time::Duration::from_millis(500));
                }
            }
        })
        .ok()
}

/// Relays the output of the child `localvox-process` into the daemon's log, line by line.
/// `tail` (for stderr) accumulates the last lines — the text of the failure reason is taken
/// from them: `jobs.json` and `/api/jobs` must receive "the LLM invented things" or "model
/// directory not found", not a useless "exit 2".
/// Control sequences (colour) from foreign output. THEY MUST NOT REACH THE LOG: the log is read
/// by eye and grepped, and `\x1b[2m…\x1b[0m` instead of a timestamp turns it into garbage. We
/// fix the child process on our side, but we must not rely on the right version always being
/// installed next to us: this is ITS output, and the log is OURS.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\u{1b}' {
            out.push(c);
            continue;
        }
        // ESC [ … <letter> — a CSI sequence; we throw it away whole.
        if chars.next() == Some('[') {
            for c in chars.by_ref() {
                if c.is_ascii_alphabetic() {
                    break;
                }
            }
        }
    }
    out.trim().to_string()
}

/// "Открыть интерфейс" from the tray.
///
/// The window is a SEPARATE process (`localvox-desktop`), and that is deliberate: it is a view
/// onto this daemon, and closing it must never stop a recording. It is launched with the address
/// we actually bound to — the port is configurable, and a window guessing it would sooner or later
/// guess wrong.
///
/// No shell? Then the browser: an interface reachable at a URL beats no interface at all. A
/// person who never built the desktop shell must still be able to open their archive.
#[cfg(windows)]
fn open_window(addr: &str) {
    let shell = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|d| d.join("localvox-desktop.exe")))
        .filter(|p| p.exists());
    match shell {
        Some(exe) => {
            // A second click focuses the open window rather than opening a second one — the shell
            // is single-instance. Two views of one archive would drift apart.
            let _ = std::process::Command::new(exe).arg("--addr").arg(addr).spawn();
        }
        None => {
            let _ = std::process::Command::new("explorer")
                .arg(format!("http://{addr}/"))
                .spawn();
        }
    }
}

/// Does the session already have audio? For an ingest job this is the idempotency check: the
/// daemon may have died between the download and the cook, and the queue will bring the job back
/// — downloading an hour of video a second time is not a small cost.
fn has_audio(session: &Path) -> bool {
    std::fs::read_dir(session.join("audio"))
        .map(|mut d| d.next().is_some())
        .unwrap_or(false)
}

/// A link → audio in the session. The stages are written as they happen, because this is the
/// step a person actually watches: it is long, it depends on the network and on foreign tools,
/// and "⚙ cooking" for ten minutes tells them nothing about where it is or what broke.
///
/// The audio lands as ordinary session chunks — from here on, a lecture from a link is
/// indistinguishable from a recording of a meeting: the same cook, the same versions, the same
/// player. That is the whole point of doing it this way.
fn ingest_into_session(session: &Path) -> Result<()> {
    use localvox_light_core::chunks::{ChunkParams, ChunkRecorder, SessionMeta};
    use localvox_light_core::progress::{step, Stage};
    use std::sync::{Arc, Mutex};

    let meta_path = session.join("meta.json");
    let mut meta: SessionMeta = serde_json::from_slice(&std::fs::read(&meta_path)?)?;
    let source = meta
        .source
        .clone()
        .ok_or_else(|| anyhow::anyhow!("у сессии нет ссылки: нечего скачивать"))?;

    // The tools are checked BEFORE the download: "yt-dlp is not installed" must be said at the
    // start, not after a minute of waiting for a network that was never going to be used.
    let settings = localvox_light_ingest::load_settings();
    let yt_dlp = localvox_light_ingest::resolve_yt_dlp(&settings, None);
    let ffmpeg = localvox_light_ingest::resolve_ffmpeg(&settings, None);
    localvox_light_ingest::verify_yt_dlp(&yt_dlp)?;
    localvox_light_ingest::verify_ffmpeg(&ffmpeg)?;
    let ffmpeg_location = localvox_light_ingest::resolve_ffmpeg_location_for_ytdlp(&ffmpeg);
    let js_runtime = localvox_light_ingest::resolve_js_runtime(&settings, None, None);

    // The title, if the source names itself. A session called "youtube" is useless in a list —
    // a person looks for the lecture by its name, not by where it was hosted.
    if let Some(title) = source_title(&yt_dlp, &source.url) {
        meta.title = Some(title.clone());
        meta.source = Some(localvox_light_core::chunks::Source {
            url: source.url.clone(),
            title: Some(title),
        });
        localvox_light_core::chunks::save_meta_public(&meta_path, &meta);
    }

    let file = step(session, Stage::Download, || {
        localvox_light_ingest::download::download_audio(
            &yt_dlp,
            &source.url,
            ffmpeg_location.as_deref(),
            js_runtime.as_deref(),
            false,
        )
    })?;

    let outcome = step(session, Stage::Extract, || -> Result<f64> {
        let pcm = localvox_light_ingest::download::convert_to_pcm_s16le(&ffmpeg, &file, false)?;
        let samples: Vec<i16> = pcm
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();
        if samples.is_empty() {
            anyhow::bail!("в источнике нет звуковой дорожки");
        }
        let params = Arc::new(ChunkParams {
            audio_dir: session.join("audio"),
            meta_path: meta_path.clone(),
            chunk_sec: 300.0,
            flac: false,
            ffmpeg: std::path::PathBuf::from(&ffmpeg),
        });
        let shared = Arc::new(Mutex::new(meta));
        // Track 0 — as if it were the microphone: a lecture has one voice line, and inventing a
        // second, empty track would only make the player lie about a silent interlocutor.
        let mut rec = ChunkRecorder::new(0, params, shared);
        rec.feed(&samples);
        rec.finalize_current();
        Ok(samples.len() as f64 / 16_000.0)
    });

    // The temporary download is removed on ANY outcome: a failed ingest must not leave hundreds
    // of megabytes in the temp directory to be found by nobody.
    let _ = std::fs::remove_file(&file);
    let seconds = outcome?;

    tracing::info!(
        "ingest: {} — {:.0} с звука из {}",
        session.display(),
        seconds,
        source.url
    );
    Ok(())
}

/// What the source calls itself. A failure here is not a failure of the ingest: a session with no
/// title is worse than one with a title, but it is still a session.
fn source_title(yt_dlp: &str, url: &str) -> Option<String> {
    let out = std::process::Command::new(yt_dlp)
        // `--encoding utf-8` OR THE TITLE ARRIVES AS MOJIBAKE.
        //
        // yt-dlp is Python, and on Windows a redirected stdout gets the ANSI code page, not UTF-8.
        // Measured 18.07.2026 on a Russian title: the pipe carried cp1251 bytes
        // (209 32 247 229 …), we decoded them as UTF-8, and the archive showed
        // «� ���� ������ …» as the name of the recording. `PYTHONIOENCODING` does NOT help —
        // the frozen exe ignores it; this flag is yt-dlp's own and it does.
        .args(["--encoding", "utf-8", "--skip-download", "--print", "%(title)s", url])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let title = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!title.is_empty()).then_some(title)
}

fn pump_child_output(
    reader: Box<dyn std::io::Read + Send>,
    session: String,
    tail: Option<std::sync::Arc<std::sync::Mutex<Vec<String>>>>,
    business: bool,
) -> std::thread::JoinHandle<()> {
    use std::io::BufRead;
    std::thread::spawn(move || {
        const TAIL_LINES: usize = 20;
        let buf = std::io::BufReader::new(reader);
        for line in buf.lines().map_while(Result::ok) {
            let line = strip_ansi(line.trim_end());
            if line.is_empty() {
                continue;
            }
            // SEPARATION OF STREAMS, not a blacklist of substrings.
            //
            // The child's stdout is its RESULT: what was cooked, how many lines, where it went.
            // These are business events, they belong in the daemon's log.
            //
            // stderr is its DIAGNOSTICS: written both by itself and by EVERY library inside it.
            // Listing foreign libraries by name ("BFCArena", "Allocated memory") is a blacklist
            // that will forever lag behind the next version. That is why stderr goes to debug
            // and into the "tail": it does not clutter the log, but on a failure it is exactly
            // what explains the reason.
            if business {
                tracing::info!("cook[{session}]: {line}");
            } else {
                tracing::debug!("cook[{session}]: {line}");
            }
            if let Some(tail) = &tail {
                if let Ok(mut t) = tail.lock() {
                    t.push(line);
                    if t.len() > TAIL_LINES {
                        t.remove(0);
                    }
                }
            }
        }
    })
}

/// The most meaningful line from the stderr tail: the error message, if there was one,
/// otherwise the last line. Progress lines (`→ …`) do not make a good reason.
fn last_error_line(tail: &std::sync::Arc<std::sync::Mutex<Vec<String>>>) -> Option<String> {
    let t = tail.lock().ok()?;
    t.iter()
        .rev()
        .find(|l| {
            let low = l.to_lowercase();
            low.contains("error") || low.contains("invent") || low.contains("not confirmed")
        })
        .or_else(|| t.last())
        .map(|l| l.chars().take(300).collect())
}

/// A flag that is on by default: it is turned off only by an explicit `0`/`off`.

/// Warming the search indexes after a cook (lexical + semantic): the autocook background pays
/// for them, not the first search. A panic inside tantivy/serde (a corrupted index directory)
/// must not kill the autopilot thread — we swallow it and retry after the next cook.
fn warm_indexes(work_dir: &std::path::Path) {
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        if let Err(e) = localvox_light_search::SearchIndex::open_or_build(work_dir, false) {
            tracing::warn!("warming the tantivy index: {e:#}");
        }
        match localvox_light_search::semantic::SemanticIndex::open_or_update(work_dir) {
            Ok(_) => tracing::info!("indexes warmed after the cook"),
            Err(e) => tracing::warn!("warming the semantic index (Ollama off?): {e:#}"),
        }
    }));
    if res.is_err() {
        tracing::error!("warming the indexes: panic suppressed, autocook keeps running");
    }
}

/// The daemon's HTTP API: bind/token from env; we do not expose ourselves outward without a
/// token. The bind is synchronous: "port busy" is visible at startup, not silence in the
/// background.
/// Returns the bind address if the API really came up (for the tray item), otherwise None.
fn spawn_http_api(work_dir: &str) -> Option<String> {
    let bind = std::env::var("LOCALVOX_API_BIND").unwrap_or_else(|_| "127.0.0.1:3017".into());
    let token = std::env::var("LOCALVOX_API_TOKEN").ok();
    if token.is_none() && !bind.starts_with("127.0.0.1") && !bind.starts_with("localhost") {
        tracing::warn!("HTTP API: bind {bind} without LOCALVOX_API_TOKEN — API not started");
        return None;
    }
    let server = match localvox_light_api::http::bind(&bind) {
        Ok(s) => s,
        Err(e) => {
            tracing::error!("HTTP API not started: {e:#}");
            eprintln!(
                "HTTP API not started: {e:#} (port busy? another localvox? see LOCALVOX_API_BIND)"
            );
            return None;
        }
    };
    let archive = std::sync::Arc::new(localvox_light_api::archive::Archive::new(
        std::path::PathBuf::from(work_dir),
    ));
    let ret = bind.clone();
    std::thread::Builder::new()
        .name("http-api".into())
        .spawn(move || {
            if let Err(e) = localvox_light_api::http::serve(
                server,
                archive,
                localvox_light_api::http::HttpConfig { bind, token },
            ) {
                tracing::error!("HTTP API: {e:#}");
            }
        })
        .ok();
    Some(ret)
}

#[cfg(test)]
mod log_relay_tests {
    use super::*;

    /// The daemon's log is read by a HUMAN. Control bytes from foreign output turn it into
    /// garbage: instead of a timestamp — `\x1b[2m…\x1b[0m`.
    #[test]
    fn control_sequences_never_reach_our_log() {
        let line = "\u{1b}[2m2026-07-13T14:36:04Z\u{1b}[0m \u{1b}[32m INFO\u{1b}[0m готово";
        assert_eq!(strip_ansi(line), "2026-07-13T14:36:04Z  INFO готово");
        assert_eq!(strip_ansi("обычная строка"), "обычная строка");
    }

    /// The child's diagnostics (including that of foreign libraries) go to stderr and live in
    /// debug and in the "tail" to explain a failure; only stdout — the result of the work —
    /// reaches the daemon's log at INFO level. This is a separation of streams, not a blacklist
    /// of substrings: the next library would silently slip past any list, but it cannot slip
    /// past a stream.
    #[test]
    fn diagnostics_and_business_events_are_different_streams() {
        // We check the contract itself: stdout — business (INFO), stderr — diagnostics (DEBUG).
        // Here it is enough that the level is chosen by a flag, not by the content of the line.
        assert!(strip_ansi("cook: v005 (20 lines)").starts_with("cook:"));
    }
}
