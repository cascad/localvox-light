//! Точка входа бинарника `localvox-light`. Ядро — крейт `localvox_light_core`.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
        #[cfg(feature = "tui")]
        {
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
            localvox_light_tui::run(
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
        audio_devices,
        None,
        noop_reset_rx,
        running,
        Arc::new(AtomicBool::new(true)),
    )?;
    Ok(())
}
