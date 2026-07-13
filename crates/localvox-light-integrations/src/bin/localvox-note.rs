//! `localvox-note` — a note into a slot (F2, WP-B3). A slot is the voice name of a
//! destination; the voice «запиши в слот …» (WP-B2) drives the same backend.

use std::path::PathBuf;

use anyhow::{bail, Result};
use clap::Parser;
use localvox_light_integrations::SlotRegistry;

#[derive(Parser)]
#[command(
    name = "localvox-note",
    about = "A note into a slot: localvox-note --slot идеи \"text\" (without --slot — the default slot)"
)]
struct Cli {
    /// Note text
    text: Vec<String>,

    /// Slot name or alias (without it — the slot with default = true)
    #[arg(long, short)]
    slot: Option<String>,

    /// Slot config (otherwise LOCALVOX_SLOTS_CONFIG, then ./slots.toml)
    #[arg(long)]
    config: Option<PathBuf>,

    /// List the available slots and exit
    #[arg(long)]
    list: bool,
}

fn main() -> Result<()> {
    // A broken line in .env makes it abandon reading the file — everything below is silently lost.
    if let Err(e) = dotenvy::dotenv() {
        if !matches!(e, dotenvy::Error::Io(_)) {
            eprintln!("ERROR in .env: {e}");
            eprintln!("  → variables AFTER the broken line were NOT read (format: KEY=value or # comment)");
        }
    }
    let cli = Cli::parse();

    let config = SlotRegistry::default_config_path(cli.config.as_deref());
    let registry = SlotRegistry::load(&config)?;

    if cli.list {
        for name in registry.names() {
            println!("{name}");
        }
        return Ok(());
    }

    let text = cli.text.join(" ");
    if text.trim().is_empty() {
        bail!("empty note text");
    }

    let slot = match &cli.slot {
        Some(q) => registry.resolve(q).ok_or_else(|| {
            anyhow::anyhow!(
                "slot «{q}» not found; available: {}",
                registry.names().join(", ")
            )
        })?,
        None => registry
            .default_slot()
            .ok_or_else(|| anyhow::anyhow!("no slots at all in {}", config.display()))?,
    };

    let dest = slot.write_note(&text)?;
    eprintln!("✓ [{}] → {dest}", slot.name);
    Ok(())
}
