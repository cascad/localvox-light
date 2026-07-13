//! Probe: what the NER model considers PEOPLE on REAL transcripts from the archive.
//!
//! Six made-up phrases prove nothing. GLiNER's known weakness on Cyrillic is PRECISION (span
//! boundaries and false positives), and a false positive here is expensive: an honest summary
//! goes into quarantine.
//!
//! So we look at live text: whom does the model call a person where there may be no people at
//! all. We have no reference annotation — and we do not need one: it is plain to the eye that
//! «Ла-ла-ла» is not a person.
//!
//! Run: cargo run -p localvox-light-bench --bin probe_ner_real -- <file.jsonl>...

use localvox_light_core::ner::{model_dir, Ner};

fn main() -> anyhow::Result<()> {
    let files: Vec<String> = std::env::args().skip(1).collect();
    anyhow::ensure!(!files.is_empty(), "pass one or more *.jsonl files");

    let dir = model_dir().ok_or_else(|| anyhow::anyhow!("no NER model directory"))?;
    let ner = Ner::open(&dir)?;
    let labels: Vec<String> = ["имя человека"].iter().map(|s| s.to_string()).collect();

    for f in &files {
        let text: String = std::fs::read_to_string(f)?
            .lines()
            .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
            .filter_map(|v| v["text"].as_str().map(str::to_string))
            .collect::<Vec<_>>()
            .join(" ");
        if text.trim().is_empty() {
            continue;
        }
        let words = text.split_whitespace().count();

        // The strict threshold (as for the answer) and the generous one (as for the source) —
        // the price of both is visible.
        let t = std::time::Instant::now();
        let strict = ner.extract(&text, &labels)?;
        let loose = ner.extract_at(&text, &labels, 0.3)?;
        println!(
            "\n=== {} ({words} words, {:.1} s)",
            f.rsplit(['/', '\\']).next().unwrap_or(f),
            t.elapsed().as_secs_f64()
        );
        println!("  threshold 0.5 (ANSWER): {}", show(&strict));
        println!("  threshold 0.3 (SOURCE): {}", show(&loose));
    }
    Ok(())
}

fn show(e: &[localvox_light_core::ner::Entity]) -> String {
    if e.is_empty() {
        return "— no people".into();
    }
    e.iter()
        .map(|x| format!("«{}»({:.2})", x.text, x.score))
        .collect::<Vec<_>>()
        .join("  ")
}
