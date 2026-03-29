//! Точка входа бинарника `localvox-light`. Ядро — крейт `localvox_light_core`.

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

use localvox_light_core::{
    init_tracing, merge_env_bools, print_devices, resolve_audio_from_cli_and_file, run_engine,
    validate_vosk_model, Cli,
};
#[cfg(feature = "tui")]
use localvox_light_core::join_engine_thread;
#[cfg(feature = "tui")]
use localvox_light_core::events::UiMsg;
#[cfg(feature = "tui")]
use localvox_light_core::light_config;

/// Портативная папка: `.env` рядом с exe; `vosk-lib/` в начало поиска нативных библиотек
/// (Linux `LD_LIBRARY_PATH`, macOS `DYLD_LIBRARY_PATH`, Windows `PATH` — после старта процесса).
/// На Windows основная `libvosk.dll` должна быть рядом с exe (загрузчик до `main`); см. build.rs / install-release.
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
        // from_path НЕ перезаписывает уже заданные в ОС переменные — портативный .env тогда «не работает».
        // from_path_override: значения из .env рядом с exe имеют приоритет; явные аргументы CLI по-прежнему сильнее clap.
        if let Err(e) = dotenvy::from_path_override(&dotenv_path) {
            eprintln!(
                "localvox-light: не удалось загрузить {}: {e}",
                dotenv_path.display()
            );
        }
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
    let mut cli = Cli::parse();
    merge_env_bools(&mut cli);
    if cli.no_tui {
        cli.tui = false;
    }
    // Портативная установка без LOCALVOX_LIGHT_TUI в .env: в обычном терминале по умолчанию открываем TUI.
    #[cfg(feature = "tui")]
    if !cli.no_tui
        && io::stdout().is_terminal()
        && std::env::var("LOCALVOX_LIGHT_TUI").is_err()
        && !cli.tui
    {
        cli.tui = true;
    }

    #[cfg(feature = "tui")]
    if cli.tui && !io::stdout().is_terminal() {
        anyhow::bail!(
            "Запрошен TUI (--tui, LOCALVOX_LIGHT_TUI=1 или интерактивный терминал по умолчанию), но stdout не TTY.\n\
             Запустите из Windows Terminal / PowerShell / cmd; для режима только логов: {} --no-tui",
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
            "localvox-light: режим без TUI — запись и логи (Ctrl+C — выход). Для интерфейса: {} --tui",
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

    if cli.tui {
        #[cfg(feature = "tui")]
        {
            let (ui_tx, ui_rx) = crossbeam_channel::unbounded::<UiMsg>();
            let (reset_tx, reset_rx) = crossbeam_channel::unbounded::<()>();
            let record_pcm = Arc::new(AtomicBool::new(true));
            let tui_verbose = cli.verbose;
            let cli_engine = cli.clone();
            let dev_engine = Arc::clone(&devices_shared);
            let rg_engine = Arc::clone(&reload_gen);
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
                        rg_engine,
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
            info!("Session finished.");
            return Ok(());
        }
        #[cfg(not(feature = "tui"))]
        {
            anyhow::bail!(
                "TUI недоступен: пакет собран без фичи `tui`. Используйте default features или `--features tui`."
            );
        }
    }

    let (_noop_reset_tx, noop_reset_rx) = crossbeam_channel::unbounded::<()>();
    run_engine(
        cli,
        devices_shared,
        None,
        noop_reset_rx,
        running,
        Arc::new(AtomicBool::new(true)),
        reload_gen,
    )?;
    Ok(())
}
