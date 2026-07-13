//! `localvox-mcp` — the MCP server of the localvox archive (stdio). Hooking it up to
//! Claude:
//! `claude mcp add localvox -- <path>/localvox-mcp.exe --work-dir <localvox-audio>`
//! Tools: search_transcripts, list_sessions, get_transcript, get_summary,
//! append_note. The protocol goes to stdout, the logs to stderr.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use localvox_light_api::McpServer;

#[derive(Parser)]
#[command(name = "localvox-mcp", about = "MCP server of the localvox archive (stdio)")]
struct Cli {
    /// The localvox work directory (the same one the recording uses)
    #[arg(
        long,
        default_value = "localvox-audio",
        env = "LOCALVOX_LIGHT_AUDIO_DIR"
    )]
    work_dir: PathBuf,
}

fn main() -> Result<()> {
    // A broken line in .env aborts reading the file — everything below is silently lost.
    if let Err(e) = dotenvy::dotenv() {
        if !matches!(e, dotenvy::Error::Io(_)) {
            eprintln!("ERROR in .env: {e}");
            eprintln!("  → the variables AFTER the broken line were NOT read (format: KEY=value or # comment)");
        }
    }
    // the logs go strictly to stderr: stdout belongs to the protocol
    // The filter and the color come from the shared place (see core::cli): someone
    // else's debug output (ORT) does not pour into the log, and ANSI codes are not
    // written where there is no terminal.
    localvox_light_core::cli::init_tracing_tool("info");

    let cli = Cli::parse();
    let server = McpServer::new(cli.work_dir.clone());
    tracing::info!("localvox-mcp: archive {}", cli.work_dir.display());

    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(msg) = serde_json::from_str::<serde_json::Value>(&line) else {
            tracing::warn!("non-JSON line in stdin — skipped");
            continue;
        };
        if let Some(resp) = server.handle(&msg) {
            stdout.write_all(resp.to_string().as_bytes())?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}
