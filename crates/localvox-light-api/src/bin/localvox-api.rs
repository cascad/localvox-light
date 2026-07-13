//! `localvox-api` — a local HTTP JSON API over the archive (F6, the foundation of the
//! PWA). By default it listens only on 127.0.0.1; for LAN: `--bind 0.0.0.0:3017` +
//! a mandatory `--token` (or LOCALVOX_API_TOKEN). For the routes see http.rs.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use localvox_light_api::archive::Archive;
use localvox_light_api::http::{run_http, HttpConfig};

#[derive(Parser)]
#[command(name = "localvox-api", about = "Local HTTP API of the localvox archive")]
struct Cli {
    /// The localvox work directory
    #[arg(
        long,
        default_value = "localvox-audio",
        env = "LOCALVOX_LIGHT_AUDIO_DIR"
    )]
    work_dir: PathBuf,

    /// Listen address (for LAN: 0.0.0.0:3017 — then a token is mandatory)
    #[arg(long, default_value = "127.0.0.1:3017", env = "LOCALVOX_API_BIND")]
    bind: String,

    /// Access token (the Authorization: Bearer … header)
    #[arg(long, env = "LOCALVOX_API_TOKEN")]
    token: Option<String>,
}

fn main() -> Result<()> {
    // A broken line in .env aborts reading the file — everything below is silently lost.
    if let Err(e) = dotenvy::dotenv() {
        if !matches!(e, dotenvy::Error::Io(_)) {
            eprintln!("ERROR in .env: {e}");
            eprintln!("  → the variables AFTER the broken line were NOT read (format: KEY=value or # comment)");
        }
    }
    // The filter and the color come from the shared place (see core::cli): someone
    // else's debug output (ORT) does not pour into the log, and ANSI codes are not
    // written where there is no terminal.
    localvox_light_core::cli::init_tracing_tool("info");
    let cli = Cli::parse();

    // exposing the API to the network without a token is forbidden (the archive is
    // private)
    if cli.token.is_none()
        && !cli.bind.starts_with("127.0.0.1")
        && !cli.bind.starts_with("localhost")
    {
        anyhow::bail!(
            "bind {} without a token: set --token/LOCALVOX_API_TOKEN for access from outside localhost",
            cli.bind
        );
    }

    let archive = Arc::new(Archive::new(cli.work_dir));
    run_http(
        archive,
        HttpConfig {
            bind: cli.bind,
            token: cli.token,
        },
    )
}
