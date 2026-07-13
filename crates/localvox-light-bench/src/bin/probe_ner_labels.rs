//! Probe: HOW THE WORDING OF A LABEL changes the result — on real text from the archive.
//!
//! GLiNER is zero-shot: entity types are given as strings, and it tunes itself to them. The
//! label «человек» catches not only names but also the word «человек», and «помещик», and
//! «крестьяне» — that is a CATEGORY, not a name. And we need exactly the names: only they cannot
//! be rephrased, and only they must not come out of nowhere.
//!
//! We measure on a live lecture (it has both real names and the common noun «люди»).
//!
//! Run: cargo run -p localvox-light-bench --bin probe_ner_labels -- <file.jsonl>

use localvox_light_core::ner::{model_dir, Ner};

/// Words that are NOT names — they are what we measure false positives by.
/// This is not a production dictionary but a measuring reference for ONE text.
const NOT_NAMES: [&str; 8] = [
    "человек",
    "помещик",
    "помещики",
    "помещиков",
    "крестьяне",
    "люди",
    "она",
    "старик",
];

fn main() -> anyhow::Result<()> {
    let file = std::env::args().nth(1).context_msg()?;
    let text: String = std::fs::read_to_string(&file)?
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|v| v["text"].as_str().map(str::to_string))
        .collect::<Vec<_>>()
        .join(" ");

    let ner = Ner::open(&model_dir().ok_or_else(|| anyhow::anyhow!("no model"))?)?;

    // The key idea: give the model a COMPETING label. A zero-shot NER has no "a name or nothing"
    // choice — it is forced to stretch a name over a role («спикер», «помещик»). Give it a
    // separate bucket for roles, and the name stops being the only one.
    let variants: [&[&str]; 6] = [
        &["имя человека"], // as it is now
        &["имя человека", "должность или роль"],
        &["имя человека", "должность", "роль в разговоре"],
        &[
            "собственное имя человека",
            "нарицательное обозначение человека",
        ],
        &["person name", "job title or role"],
        &[
            "имя собственное человека",
            "должность или роль",
            "профессия",
        ],
    ];

    println!("text: {} words\n", text.split_whitespace().count());
    for labels in variants {
        let labels: Vec<String> = labels.iter().map(|s| s.to_string()).collect();
        let ents = ner.extract(&text, &labels)?;
        let primary = &labels[0];

        let mut names = Vec::new();
        let mut junk = Vec::new();
        for e in ents.iter().filter(|e| &e.label == primary) {
            let low = e.text.to_lowercase();
            if NOT_NAMES.iter().any(|w| low == *w || low.starts_with(w)) {
                junk.push(format!("{}({:.2})", e.text, e.score));
            } else {
                names.push(e.text.clone());
            }
        }
        names.sort();
        names.dedup();

        println!("labels: {:?}", labels);
        println!("  names found: {} — {}", names.len(), preview(&names));
        println!(
            "  FALSE (common nouns): {} — {}\n",
            junk.len(),
            if junk.is_empty() {
                "none".to_string()
            } else {
                junk.join(", ")
            }
        );
    }
    Ok(())
}

fn preview(v: &[String]) -> String {
    let head: Vec<&str> = v.iter().take(8).map(String::as_str).collect();
    if v.len() > 8 {
        format!("{}, …", head.join(", "))
    } else {
        head.join(", ")
    }
}

trait Ctx {
    fn context_msg(self) -> anyhow::Result<String>;
}
impl Ctx for Option<String> {
    fn context_msg(self) -> anyhow::Result<String> {
        self.ok_or_else(|| anyhow::anyhow!("pass a transcript file (*.jsonl)"))
    }
}
