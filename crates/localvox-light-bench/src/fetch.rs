//! Downloading reference sets with a single command.
//!
//! We take the test splits through the HuggingFace datasets-server: it hands out the rows (text
//! plus a link to the audio) as plain JSON — no python, no `datasets`, no token needed. We fetch
//! exactly the test splits — the very ones on which GigaAM, T-one and Whisper publish their WER,
//! so our numbers are comparable with theirs.
//!
//! The audio links are signed and short-lived — we download at once, page by page.
//! The LICENCES of the sets differ (Golos has its own, see docs/asr-bench.md): the data is not
//! put into the repository, the `bench/` directory is ignored by git.

use std::path::Path;

use anyhow::{bail, Context, Result};

/// The registry of sets: name → (dataset on HF, config, split, what it is).
const REGISTRY: &[(&str, &str, &str, &str, &str)] = &[
    (
        "golos-crowd",
        "bond005/sberdevices_golos_10h_crowd",
        "default",
        "test",
        "Golos crowd test — reading aloud, 16 kHz; the main reference for Russian ASR",
    ),
    (
        "golos-farfield",
        "bond005/sberdevices_golos_100h_farfield",
        "default",
        "test",
        "Golos farfield test — speech into a smart speaker: reverberation, distant microphone",
    ),
    (
        "ruslibrispeech",
        "bond005/rulibrispeech",
        "default",
        "test",
        "Russian LibriSpeech test — read-out books, long phrases",
    ),
];

pub fn list() {
    println!("sets (localvox-bench fetch --dataset <name>):\n");
    for (name, hf, _, split, what) in REGISTRY {
        println!("  {name:<16} {what}");
        println!("  {:<16} huggingface.co/datasets/{hf} ({split})\n", "");
    }
    println!(
        "Common Voice ru and FLEURS ru require a manual download (gate/limits) — docs/asr-bench.md"
    );
}

pub fn fetch(dataset: &str, out: &Path, limit: usize, offset: usize) -> Result<()> {
    let Some((_, hf, config, split, what)) = REGISTRY.iter().find(|r| r.0 == dataset) else {
        bail!("unknown set «{dataset}» (localvox-bench fetch --list)");
    };
    let audio_dir = out.join("audio");
    std::fs::create_dir_all(&audio_dir)
        .with_context(|| format!("creating {}", audio_dir.display()))?;
    println!("{what}\nsource: huggingface.co/datasets/{hf} [{split}]");

    let mut manifest = String::new();
    let mut got = 0usize;
    let mut pos = offset;
    // datasets-server returns at most 100 rows per request
    while got < limit {
        let take = (limit - got).min(100);
        let url = format!(
            "https://datasets-server.huggingface.co/rows?dataset={}&config={config}&split={split}&offset={pos}&length={take}",
            urlencode(hf)
        );
        let page: serde_json::Value = ureq::get(&url)
            .call()
            .with_context(|| format!("requesting rows of {dataset} (offset {pos})"))?
            .into_json()
            .context("the datasets-server answer is not JSON")?;
        let rows = page["rows"].as_array().cloned().unwrap_or_default();
        if rows.is_empty() {
            println!("the set ran out at row {pos}");
            break;
        }
        for row in &rows {
            let r = &row["row"];
            let text = r["transcription"]
                .as_str()
                .or_else(|| r["text"].as_str())
                .unwrap_or("")
                .trim()
                .to_string();
            let src = r["audio"][0]["src"].as_str().unwrap_or("");
            if text.is_empty() || src.is_empty() {
                continue;
            }
            let name = format!("{:05}.wav", pos + got);
            let path = audio_dir.join(&name);
            if !path.exists() {
                let mut body = Vec::new();
                ureq::get(src)
                    .call()
                    .with_context(|| format!("downloading the audio {name}"))?
                    .into_reader()
                    .read_to_end(&mut body)
                    .context("reading the audio body")?;
                std::fs::write(&path, &body)
                    .with_context(|| format!("writing {}", path.display()))?;
            }
            manifest.push_str(&serde_json::to_string(&serde_json::json!({
                "audio_filepath": format!("audio/{name}"),
                "text": text,
            }))?);
            manifest.push('\n');
            got += 1;
            if got % 25 == 0 {
                eprint!("\r  downloaded {got}/{limit}…");
            }
            if got >= limit {
                break;
            }
        }
        pos += rows.len();
    }
    eprintln!("\r{:30}\r", "");
    if got == 0 {
        bail!("not a single record was downloaded");
    }

    let mpath = out.join("manifest.jsonl");
    std::fs::write(&mpath, manifest).with_context(|| format!("writing {}", mpath.display()))?;
    println!("done: {got} records → {}", mpath.display());
    println!("run it: localvox-bench wer {}", mpath.display());
    Ok(())
}

fn urlencode(s: &str) -> String {
    s.replace('/', "%2F")
}
