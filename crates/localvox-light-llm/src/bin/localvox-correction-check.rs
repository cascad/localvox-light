//! Engineering replay of saved proposals. Does not execute a provider or mutate sessions.
use std::io::{self, BufRead, Write};

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Proposal {
    source: String,
    candidate: String,
}

fn main() -> anyhow::Result<()> {
    let mut out = io::BufWriter::new(io::stdout().lock());
    for line in io::stdin().lock().lines() {
        let line = line?;
        anyhow::ensure!(line.len() <= 1024 * 1024, "proposal exceeds limit");
        let proposal: Proposal = serde_json::from_str(&line)?;
        let started = std::time::Instant::now();
        let result =
            localvox_light_llm::correction_guard::check(&proposal.source, &proposal.candidate);
        writeln!(
            out,
            "{}",
            serde_json::json!({"accepted":result.is_ok(),"reason":result.err(),
            "elapsed_ns":started.elapsed().as_nanos()})
        )?;
    }
    Ok(())
}
