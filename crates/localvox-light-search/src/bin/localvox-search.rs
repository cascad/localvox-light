//! `localvox-search` — search over the session archive (F4 stage 1, WP-B4).
//! The index is rebuilt automatically if the session files are newer than it.

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use localvox_light_search::SearchIndex;

#[derive(Parser)]
#[command(
    name = "localvox-search",
    about = "Full-text search over session transcripts, summaries and notes"
)]
struct Cli {
    /// Search query (tantivy syntax: phrases in quotes, AND/OR, -minus)
    query: Vec<String>,

    /// localvox working directory
    #[arg(
        long,
        default_value = "localvox-audio",
        env = "LOCALVOX_LIGHT_AUDIO_DIR"
    )]
    work_dir: PathBuf,

    /// Maximum number of results
    #[arg(long, short = 'n', default_value = "10")]
    limit: usize,

    /// Force a rebuild of the index
    #[arg(long)]
    reindex: bool,

    /// Semantic search «by meaning» (needs Ollama with an embedding model)
    #[arg(long)]
    semantic: bool,
}

fn main() -> Result<()> {
    // A broken line in .env aborts the reading of the file — everything below it is
    // silently lost.
    if let Err(e) = dotenvy::dotenv() {
        if !matches!(e, dotenvy::Error::Io(_)) {
            eprintln!("ERROR in .env: {e}");
            eprintln!("  → the variables AFTER the broken line were NOT read (format: KEY=value or # comment)");
        }
    }
    let cli = Cli::parse();
    let query = cli.query.join(" ");
    if query.trim().is_empty() && !cli.reindex {
        anyhow::bail!("empty query (or --reindex to rebuild the index)");
    }

    if cli.semantic {
        let idx = localvox_light_search::semantic::SemanticIndex::open_or_update(&cli.work_dir)?;
        let hits = idx.search(&query, cli.limit)?;
        if hits.is_empty() {
            eprintln!("nothing found: {query}");
            return Ok(());
        }
        for (i, h) in hits.iter().enumerate() {
            let ts = h
                .start_sec
                .map(|s| format!(" ({:02}:{:02})", (s / 60.0) as u64, s as u64 % 60))
                .unwrap_or_default();
            println!(
                "{}. [{}] {}{} ~{:.2}",
                i + 1,
                h.session,
                h.kind,
                ts,
                h.score
            );
            println!("   {}", h.text.chars().take(160).collect::<String>());
        }
        return Ok(());
    }

    let index = SearchIndex::open_or_build(&cli.work_dir, cli.reindex)?;
    if query.trim().is_empty() {
        eprintln!("index is ready");
        return Ok(());
    }

    let hits = index.search(&query, cli.limit)?;
    if hits.is_empty() {
        eprintln!("nothing found: {query}");
        return Ok(());
    }
    for (i, h) in hits.iter().enumerate() {
        let ts = h
            .start_sec
            .map(|s| format!(" ({:02}:{:02})", (s / 60.0) as u64, s as u64 % 60))
            .unwrap_or_default();
        println!("{}. [{}] {}{}", i + 1, h.session, h.kind, ts);
        println!("   {}", h.snippet);
    }
    Ok(())
}
