//! Entry point of the `localvox-light` binary. The core is the `localvox_light_core` crate.

mod autocook;

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
    // Background mode: the interface lives at the HTTP address, not in the terminal. Autostart
    // (HKCU\Run, launchd) has no console at all — a TUI enabled in .env must not kill the daemon.
    //
    // The one-shot commands are in the same list, and that is a defect being fixed, not a
    // precaution: with `LOCALVOX_LIGHT_TUI=1` in .env, `--doctor` and `--list-devices` piped
    // anywhere died with «stdout is not a TTY» — the doctor refusing to speak because there is no
    // terminal to draw a full-screen interface it was never asked for.
    if cli.daemon || cli.doctor || cli.list_devices || cli.update_yt_dlp {
        cli.tui = false;
    }
    // Portable install without LOCALVOX_LIGHT_TUI in .env: in a normal terminal we open the TUI
    // by default.
    #[cfg(feature = "tui")]
    if !cli.no_tui
        && !cli.daemon
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

    // BEFORE validate_vosk_model — the doctor exists precisely for the case where something is
    // missing. A check that refuses to run until everything is in place answers only the
    // question nobody asks.
    if cli.doctor {
        std::process::exit(run_doctor(&cli));
    }

    // One-shot maintenance, same as --doctor: does not need models, so it runs before validation.
    if cli.update_yt_dlp {
        std::process::exit(run_update_yt_dlp());
    }

    validate_vosk_model(&cli)?;

    init_tracing(cli.debug, cli.tui);

    if !cli.tui && !cli.daemon {
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

    if cli.daemon {
        return run_daemon_mode(cli, devices_shared, reload_gen, running);
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
            let engine_result = join_engine_thread(engine_handle, Duration::from_secs(2));
            // drain the voice queue: accepted "запиши…" commands are appended to the slots
            drop(voice_hook);
            drain_voice(voice_handle);
            engine_result?;
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

/// `--doctor`: does this installation actually work, and if not — what to do about it.
///
/// The checks themselves live in `core::doctor` and are pure. Here is the composition root: the
/// environment is read, the paths are resolved, and the two questions that need a socket are
/// asked — «is anything listening where the LLM should be» and «is our port free».
///
/// Exit code, so a script can branch: 0 — works, 1 — runs but cannot do everything, 2 — will not
/// work.
fn run_doctor(cli: &Cli) -> i32 {
    use localvox_light_core::doctor::{self, State};

    let vosk = std::path::PathBuf::from(localvox_light_core::cli::normalized_model_path(cli));
    let layout = doctor::Layout::from_env(vosk, std::path::PathBuf::from(&cli.audio_dir));
    let mut findings = doctor::inspect(&layout);
    findings.push(probe_llm(&layout));
    findings.push(probe_api_port(&layout));
    findings.push(probe_yt_dlp());

    println!("localvox — проверка установки\n");
    for f in &findings {
        println!("  [{}] {}", f.state.mark(), f.what);
        println!("         {}", f.detail);
        if let Some(fix) = &f.fix {
            println!("         → {fix}");
        }
    }

    let worst = doctor::worst(&findings);
    println!();
    match worst {
        State::Ok => {
            println!("Всё на месте.");
            0
        }
        State::Warn => {
            println!("Работать будет, но не всё: строки с «ЖДЁТ» говорят, чего именно не будет.");
            1
        }
        State::Fail => {
            println!("Не заработает: сначала строки с «НЕТ».");
            2
        }
    }
}

/// Отвечает ли что-нибудь там, где должна быть Ollama.
///
/// Именно TCP-проверка, и об её границе сказано вслух: она НЕ проверяет, что модель скачана.
/// Проверка, которая молчит о том, чего не смотрела, — та же ложь, только вежливая.
fn probe_llm(l: &localvox_light_core::doctor::Layout) -> localvox_light_core::doctor::Finding {
    use localvox_light_core::doctor::{Finding, State};
    let host = l
        .llm_base_url
        .split("://")
        .nth(1)
        .unwrap_or(&l.llm_base_url)
        .split('/')
        .next()
        .unwrap_or_default()
        .to_string();
    let alive = std::net::ToSocketAddrs::to_socket_addrs(&host)
        .map(|addrs| {
            addrs.into_iter().any(|a| {
                std::net::TcpStream::connect_timeout(&a, std::time::Duration::from_millis(700))
                    .is_ok()
            })
        })
        .unwrap_or(false);
    if alive {
        Finding {
            what: "LLM (сводка и чистовик)".into(),
            state: State::Ok,
            detail: format!(
                "{host} отвечает; модель в настройках: {}. Скачана ли она — здесь НЕ проверяется, \
                 это видно по первой же варке",
                l.llm_model
            ),
            fix: None,
        }
    } else {
        Finding {
            what: "LLM (сводка и чистовик)".into(),
            state: State::Warn,
            detail: format!("{host} не отвечает — расшифровка будет, сводки и чистовика нет"),
            fix: Some(format!(
                "запустить Ollama и скачать модель: ollama pull {}",
                l.llm_model
            )),
        }
    }
}

/// Свободен ли порт, на котором поднимется интерфейс. Занятый порт — самая частая причина
/// «демон работает, а страница не открывается», и узнать о ней надо до запуска, а не после.
fn probe_api_port(l: &localvox_light_core::doctor::Layout) -> localvox_light_core::doctor::Finding {
    use localvox_light_core::doctor::{Finding, State};
    match std::net::TcpListener::bind(&l.api_bind) {
        Ok(_) => Finding {
            what: "Порт интерфейса".into(),
            state: State::Ok,
            detail: format!("{} свободен", l.api_bind),
            fix: None,
        },
        Err(e) => Finding {
            what: "Порт интерфейса".into(),
            state: State::Warn,
            detail: format!("{} занят ({e}) — возможно, демон уже запущен", l.api_bind),
            fix: Some("остановить прежний демон или задать другой LOCALVOX_API_BIND".into()),
        },
    }
}

/// The version yt-dlp reports (`2026.08.16` or `2026.08.16.020253`), or `None` if it is missing or
/// did not answer. A live probe — runs the binary — so it lives here, not in the pure `doctor`.
fn yt_dlp_version(bin: &str) -> Option<String> {
    std::process::Command::new(bin)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Freshness of yt-dlp. YouTube breaks old versions every few weeks (HTTP 403 on download), so a
/// stale binary is the quiet cause of «ingest by link stopped working». Running the binary and
/// reading «now» is entropy — hence here, appended to the pure `inspect` like the LLM/port probes.
fn probe_yt_dlp() -> localvox_light_core::doctor::Finding {
    use localvox_light_core::doctor;
    let settings = localvox_light_ingest::load_settings();
    let yt_dlp = localvox_light_ingest::resolve_yt_dlp(&settings, None);
    let version = yt_dlp_version(&yt_dlp);
    let age_days = version
        .as_deref()
        .and_then(doctor::parse_yt_dlp_date)
        .map(|d| (chrono::Local::now().date_naive() - d).num_days());
    doctor::yt_dlp_finding(&yt_dlp, version.as_deref(), age_days)
}

/// `--update-yt-dlp`: update yt-dlp in place to the nightly channel and exit.
///
/// PROBLEM CLASS: keeping an external, frequently-breaking tool fresh. yt-dlp solves this itself —
/// `--update-to nightly` rewrites its own binary atomically for this OS — so we do NOT hand-roll a
/// downloader; we invoke its updater, the same way we invoke it for downloads. Nightly, not stable:
/// YouTube fixes land there first, and stable can sit unchanged for weeks while 403s pile up.
fn run_update_yt_dlp() -> i32 {
    let _ = tracing_subscriber::fmt::try_init();
    let settings = localvox_light_ingest::load_settings();
    let yt_dlp = localvox_light_ingest::resolve_yt_dlp(&settings, None);
    let before = yt_dlp_version(&yt_dlp);
    println!(
        "yt-dlp: {} — обновляю по месту до nightly ({} --update-to nightly)",
        before.as_deref().unwrap_or("не найден"),
        yt_dlp
    );
    match std::process::Command::new(&yt_dlp)
        .args(["--update-to", "nightly"])
        .status()
    {
        Ok(s) if s.success() => {
            let after = yt_dlp_version(&yt_dlp);
            println!(
                "Готово: {} → {}",
                before.as_deref().unwrap_or("?"),
                after.as_deref().unwrap_or("?")
            );
            0
        }
        Ok(s) => {
            // The common failure is the GitHub API rate limit (403) that yt-dlp's own updater uses
            // for the version check — transient, and the honest fallback is a direct asset download.
            eprintln!(
                "yt-dlp вернул код {s}. Частая причина — лимит GitHub API (403 rate limit) в самом \
                 апдейтере. Повторите позже или скачайте свежий бинарь вручную из \
                 https://github.com/yt-dlp/yt-dlp-nightly-builds/releases/latest и положите на место {yt_dlp}"
            );
            1
        }
        Err(e) => {
            eprintln!("не удалось запустить {yt_dlp}: {e}");
            2
        }
    }
}

/// How often a waiting loop re-reads `running`. Short enough that «Выход» feels immediate,
/// long enough to be invisible on a power meter.
const IDLE_TICK: std::time::Duration = std::time::Duration::from_millis(300);

/// Everything localvox IS when nobody is looking: capture, the voice module, background cooking
/// and the HTTP API.
///
/// It owns no interface. The tray icon and the window are VIEWS onto this, and on a machine with
/// neither — a Mac, a Linux box, a login session started by launchd — the product must be exactly
/// as complete. That is the whole reason this type exists: all of it used to live inside
/// `run_tray_mode`, behind `#[cfg(windows)]`, so everywhere else `--tray` was an error message and
/// the daemon was a dictaphone with no archive, no interface and no cooking.
///
/// Shutdown is ordered and bounded (see [`Daemon::shutdown`]) — an unattended process must be able
/// to stop as deliberately as it starts.
struct Daemon {
    running: Arc<AtomicBool>,
    /// Capture on/off, without tearing the streams down. Shared with the engine.
    ///
    /// Only the tray touches it today, so off Windows the daemon records or does not record and
    /// there is no third state. That is a REAL GAP, not a platform quirk: pause belongs in the
    /// HTTP API, where every interface can reach it. Until it is there, the honest thing is a
    /// suppression that says so out loud rather than a shrug.
    #[cfg_attr(not(windows), allow(dead_code))]
    record_pcm: Arc<AtomicBool>,
    /// Where the interface actually answers — already normalised to something connectable
    /// (`0.0.0.0` is an address to LISTEN on, never one to open). `None`: the API did not come up.
    api_addr: Option<String>,
    work_dir: String,
    /// Why the engine stopped, if it stopped on its own. Set once, by the engine thread.
    ///
    /// The engine dying is a STATE OF THE DAEMON, not a message to a user interface. It used to be
    /// a `TrayMsg`, which meant only the tray could ever learn of it — a headless daemon would go
    /// on reporting itself healthy with nothing recording.
    fatal: Arc<std::sync::Mutex<Option<String>>>,
    engine: std::thread::JoinHandle<()>,
    autocook: Option<std::thread::JoinHandle<()>>,
    voice: Option<std::thread::JoinHandle<()>>,
    /// Processes «Спросить у LLM» requests one at a time in the background — the async lifecycle
    /// that lets the button return at once (WP: ask section as a 1-stage pipeline).
    ask_worker: std::thread::JoinHandle<()>,
}

impl Daemon {
    /// Bring the parts up. The API comes first on purpose: the archive is readable whether or not
    /// anything is being recorded, and a failure to capture must not take the archive down with
    /// it.
    fn start(
        cli: &Cli,
        devices_shared: Arc<RwLock<localvox_light_core::LightDeviceConfig>>,
        reload_gen: Arc<std::sync::atomic::AtomicU64>,
        running: Arc<AtomicBool>,
    ) -> Result<Daemon> {
        let api_addr = spawn_http_api(&cli.audio_dir).map(|bind| connectable(&bind));

        let (voice_hook, voice, voice_status) = init_voice(None);
        eprintln!("Voice module: {voice_status}");

        // Autocook (WP-C7): the daemon finishes cooking closed sessions in the background itself.
        let autocook = autocook::spawn(cli.audio_dir.clone(), running.clone());

        // Ask-worker: the same idea for «Спросить у LLM» — a background thread drains pending
        // requests one at a time, so the button returns instantly and the answer arrives later.
        let ask_worker = spawn_ask_worker(cli.audio_dir.clone(), running.clone());

        let record_pcm = Arc::new(AtomicBool::new(true));
        let fatal = Arc::new(std::sync::Mutex::new(None));
        let (_reset_tx, reset_rx) = crossbeam_channel::unbounded::<()>();
        let cli_engine = cli.clone();
        let r_engine = running.clone();
        let record_engine = Arc::clone(&record_pcm);
        let fatal_engine = Arc::clone(&fatal);
        let engine = std::thread::Builder::new()
            .name("engine".into())
            .spawn(move || {
                if let Err(e) = run_engine(
                    cli_engine,
                    devices_shared,
                    None,
                    reset_rx,
                    r_engine.clone(),
                    record_engine,
                    reload_gen,
                    voice_hook,
                ) {
                    tracing::error!("Engine stopped: {e:#}");
                    if let Ok(mut slot) = fatal_engine.lock() {
                        *slot = Some(format!("{e:#}"));
                    }
                    // Without capture the daemon is lying about what it does. It stops.
                    r_engine.store(false, Ordering::SeqCst);
                }
            });
        // The engine is the LAST thing started, so it is the only one that can fail with the
        // others already running. Bailing out with `?` here would leave the autocook thread
        // polling for ever and the voice thread waiting on a queue nobody will close — a process
        // that failed to start and never finished exiting.
        let engine = match engine {
            Ok(h) => h,
            Err(e) => {
                // running=false makes the ask-worker exit on its next tick too; we join it so no
                // thread outlives a failed start.
                running.store(false, Ordering::SeqCst);
                if let Some(h) = autocook {
                    if let Err(e) = localvox_light_core::cli::join_component_thread(
                        h,
                        std::time::Duration::from_secs(10),
                        "autocook",
                    ) {
                        tracing::error!("{e:#}");
                    }
                }
                let _ = ask_worker.join();
                drain_voice(voice);
                return Err(anyhow::Error::new(e).context("spawning the recording engine"));
            }
        };

        Ok(Daemon {
            running,
            record_pcm,
            api_addr,
            work_dir: cli.audio_dir.clone(),
            fatal,
            engine,
            autocook,
            voice,
            ask_worker,
        })
    }

    fn is_running(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Ask for shutdown. Off Windows the same thing arrives as a signal, straight into `running`.
    #[cfg_attr(not(windows), allow(dead_code))]
    fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
    }

    /// Flip capture; returns the state it landed in (`true` — recording).
    #[cfg_attr(not(windows), allow(dead_code))]
    fn toggle_pause(&self) -> bool {
        let now = !self.record_pcm.load(Ordering::Relaxed);
        self.record_pcm.store(now, Ordering::Relaxed);
        tracing::info!("recording {}", if now { "resumed" } else { "paused" });
        now
    }

    /// Block until something asks us to stop — a signal, the tray, or the engine dying.
    ///
    /// On Windows the tray's own message loop does this waiting, so this one is unused there.
    #[cfg_attr(windows, allow(dead_code))]
    fn run_until_stopped(self) -> Result<()> {
        while self.is_running() {
            std::thread::sleep(IDLE_TICK);
        }
        self.shutdown()
    }

    /// Stop in an order that leaves nothing behind.
    ///
    /// Autocook first: its poll loop sees `running == false`, kills the child `localvox-process`
    /// and exits — otherwise the cook is orphaned and burns a core with nobody left to read its
    /// output. Then the engine, which closes the current chunk and drops the voice hook, which
    /// closes the voice queue, which lets the voice thread finish its last note.
    fn shutdown(self) -> Result<()> {
        self.running.store(false, Ordering::SeqCst);
        let mut errors = Vec::new();
        if let Some(h) = self.autocook {
            if let Err(e) = localvox_light_core::cli::join_component_thread(
                h,
                std::time::Duration::from_secs(10),
                "autocook",
            ) {
                tracing::error!("{e:#}");
                errors.push(e.to_string());
            }
        }
        if let Err(e) =
            localvox_light_core::join_engine_thread(self.engine, std::time::Duration::from_secs(10))
        {
            tracing::error!("{e:#}");
            errors.push(e.to_string());
        }
        // Best-effort: if idle, the worker exits within its poll tick; if it is mid-model-call it
        // may take longer, and then we proceed rather than block shutdown — the ask stays `Running`
        // and the next startup's reclaim revives it.
        join_bounded(
            self.ask_worker,
            std::time::Duration::from_secs(5),
            "ask-worker",
        );
        drain_voice(self.voice);
        let fatal = self.fatal.lock().ok().and_then(|g| g.clone());
        if let Some(e) = fatal {
            errors.push(format!("recording engine died: {e}"));
        }
        anyhow::ensure!(errors.is_empty(), "{}", errors.join("; "));
        Ok(())
    }
}

/// Join a thread but never longer than `wait` — a background worker mid-blocking-call must not hold
/// shutdown hostage. These optional workers recover interrupted work at the next startup.
fn join_bounded(handle: std::thread::JoinHandle<()>, wait: std::time::Duration, what: &str) {
    let (tx, rx) = std::sync::mpsc::sync_channel::<()>(0);
    std::thread::spawn(move || {
        let _ = handle.join();
        let _ = tx.send(());
    });
    if rx.recv_timeout(wait).is_err() {
        tracing::debug!("{what}: still busy at shutdown — leaving it, work resumes on restart");
    }
}

/// The ask-worker: drains `asks/` of pending «Спросить у LLM» requests, one at a time, in the
/// background. On startup it revives anything left `Running` by a dead daemon (self-healing, like a
/// session's cook). Failures are recorded on the ask itself, not retried in a loop.
fn spawn_ask_worker(work_dir: String, running: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::Builder::new()
        .name("ask-worker".into())
        .spawn(move || {
            let archive =
                localvox_light_api::archive::Archive::new(std::path::PathBuf::from(&work_dir));
            let revived = archive.reclaim_running_asks();
            if revived > 0 {
                tracing::info!(
                    "ask-worker: {revived} request(s) abandoned by a dead daemon — replaying"
                );
            }
            while running.load(Ordering::Relaxed) {
                match archive.next_pending_ask() {
                    Some(id) => {
                        tracing::info!("ask-worker: processing {id}");
                        match archive.process_ask(&id) {
                            Ok(a) if a.error.is_some() => {
                                tracing::warn!(
                                    "ask-worker: {id} failed: {}",
                                    a.error.unwrap_or_default()
                                )
                            }
                            Ok(_) => tracing::info!("ask-worker: {id} done"),
                            Err(e) => tracing::warn!("ask-worker: {id}: {e:#}"),
                        }
                    }
                    // Nothing waiting — poll again shortly. Cheap: a directory scan.
                    None => std::thread::sleep(std::time::Duration::from_secs(2)),
                }
            }
        })
        .expect("spawn ask-worker thread")
}

/// An address to CONNECT to, from an address we LISTEN on. `0.0.0.0` / `[::]` mean "every
/// interface" to a listener and nothing at all to a client — handing it to the window or the
/// browser produces a connection that cannot succeed.
fn connectable(bind: &str) -> String {
    match bind.parse::<std::net::SocketAddr>() {
        Ok(a) if a.ip().is_unspecified() => format!("127.0.0.1:{}", a.port()),
        _ => bind.to_string(),
    }
}

/// Background mode where there is no tray to attach: the interface is the HTTP address, and the
/// process is stopped by a signal (Ctrl+C, `SIGTERM` from launchd/systemd) — both already route
/// into `running` through the handler installed in `main`.
#[cfg(not(windows))]
fn run_daemon_mode(
    cli: Cli,
    devices_shared: Arc<RwLock<localvox_light_core::LightDeviceConfig>>,
    reload_gen: Arc<std::sync::atomic::AtomicU64>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    let daemon = Daemon::start(&cli, devices_shared, reload_gen, running)?;
    // The two facts an operator needs from a process with no window: where to look at it, and
    // where its data lives. Both, at startup, in the log — because under launchd this is the only
    // thing anyone will ever see of it.
    match daemon.api_addr.as_deref() {
        Some(addr) => eprintln!("localvox-light: daemon — interface at http://{addr}/"),
        None => eprintln!(
            "localvox-light: daemon — WITHOUT the HTTP interface (see the log: port busy? \
             LOCALVOX_API_BIND without LOCALVOX_API_TOKEN?)"
        ),
    }
    eprintln!("localvox-light: archive at {}", daemon.work_dir);
    daemon.run_until_stopped()
}

/// Messages to the tray from its menu callbacks (the main loop owns TrayItem).
#[cfg(windows)]
enum TrayMsg {
    TogglePause,
    ToggleAutostart,
    Quit,
}

#[cfg(windows)]
fn autostart_label(enabled: bool) -> &'static str {
    if enabled {
        "Автозапуск при входе: ✓"
    } else {
        "Автозапуск при входе: —"
    }
}

/// Ids of the two menu items whose LABEL is state: they say what the daemon is doing, so they
/// have to be rewritten when it changes.
#[cfg(windows)]
struct StatefulItems {
    pause: u32,
    autostart: u32,
}

/// The tray menu, over an already-running daemon.
///
/// «Открыть интерфейс» appears only if the API really came up: an item that leads to a refused
/// connection is worse than a missing one — it blames the person for clicking.
#[cfg(windows)]
fn build_tray_menu(
    tray: &mut tray_item::TrayItem,
    msg_tx: &crossbeam_channel::Sender<TrayMsg>,
    daemon: &Daemon,
) -> Result<StatefulItems> {
    let pause = {
        let t = msg_tx.clone();
        tray.inner_mut()
            .add_menu_item_with_id("Пауза", move || {
                let _ = t.send(TrayMsg::TogglePause);
            })
            .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?
    };
    {
        let dir = daemon.work_dir.clone();
        tray.add_menu_item("Открыть папку архива", move || {
            let _ = std::process::Command::new("explorer").arg(&dir).spawn();
        })
        .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?;
    }
    if let Some(addr) = daemon.api_addr.clone() {
        tray.add_menu_item("Открыть интерфейс", move || {
            open_window(&addr)
        })
        .map_err(|e| anyhow::anyhow!("tray menu: {e}"))?;
    }
    let autostart = {
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
    Ok(StatefulItems { pause, autostart })
}

/// Background mode on Windows: the same daemon as everywhere, with a tray icon attached as its
/// control surface.
///
/// The ICON is raised before anything starts: if there is going to be no way to reach this
/// process, we find out while there is still nothing to reach — not after capture has begun.
#[cfg(windows)]
fn run_daemon_mode(
    cli: Cli,
    devices_shared: Arc<RwLock<localvox_light_core::LightDeviceConfig>>,
    reload_gen: Arc<std::sync::atomic::AtomicU64>,
    running: Arc<AtomicBool>,
) -> Result<()> {
    use tray_item::{IconSource, TrayItem};

    let mut tray = TrayItem::new("localvox — идёт запись", IconSource::Resource("tray-icon"))
        .map_err(|e| anyhow::anyhow!("tray: {e} (exe built without assets/localvox.ico?)"))?;

    let daemon = Daemon::start(&cli, devices_shared, reload_gen, running)?;

    let (msg_tx, msg_rx) = crossbeam_channel::unbounded::<TrayMsg>();
    // A half-built menu leaves a daemon with no way to stop it. We take it down deliberately
    // rather than letting `?` drop the threads on the floor.
    let items = match build_tray_menu(&mut tray, &msg_tx, &daemon) {
        Ok(items) => items,
        Err(e) => {
            daemon.stop();
            let _ = daemon.shutdown();
            return Err(e);
        }
    };

    eprintln!("localvox-light: background mode — icon in the tray (exit via the tray menu)");

    while daemon.is_running() {
        // The timeout is what notices `running` going false without a click — the engine dying,
        // or a signal.
        match msg_rx.recv_timeout(IDLE_TICK) {
            Ok(TrayMsg::TogglePause) => {
                let (item, tip) = if daemon.toggle_pause() {
                    ("Пауза", "localvox — идёт запись")
                } else {
                    ("Продолжить запись", "localvox — ПАУЗА")
                };
                let _ = tray.inner_mut().set_menu_item_label(item, items.pause);
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
                            if target { "enabled" } else { "disabled" }
                        );
                        let _ = tray
                            .inner_mut()
                            .set_menu_item_label(autostart_label(target), items.autostart);
                    }
                    Err(e) => tracing::warn!("autostart: {e}"),
                }
            }
            Ok(TrayMsg::Quit) => daemon.stop(),
            Err(_) => {} // timeout — re-check running
        }
    }
    daemon.shutdown()
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
            let _ = std::process::Command::new(exe)
                .arg("--addr")
                .arg(addr)
                .spawn();
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
