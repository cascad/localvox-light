//! An integration test of the grounding check on the REFERENCE CORPUS.
//!
//! **Why this is possible and why it matters.** The check knows nothing about sound: its
//! input is a transcript (text) and the model's answer. The microphone, the VAD and the ASR
//! have nothing to do with this layer. Which means the whole layer can be lifted out into
//! deterministic tests: no recording, no LLM, not a single megabyte of audio. Milliseconds
//! and CI.
//!
//! We measure BOTH failures, because both are expensive:
//!   * `verdict = "clean"`   — an honest summary declared an invention (a false accusation);
//!   * `verdict = "flagged"` — an invention passed as a fact (a miss).
//! A system that never accuses is just as useless as one that always does.
//!
//! The NER model is optional: if it is absent (`models/ner-gliner`), the test honestly
//! skips the cases that are impossible to catch without it, and says so.
//! The corpus: `assets/corpus/*.toml` (there is a README about the format there too).

use serde::Deserialize;

#[derive(Deserialize)]
struct Corpus {
    name: String,
    #[serde(default = "ru")]
    lang: String,
    transcript: String,
    #[serde(default, rename = "case")]
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    name: String,
    /// `clean` — there must be no accusations; `flagged` — an accusation MUST be there.
    verdict: String,
    answer: String,
    /// Which names/numbers exactly MUST end up in the accusation (a subset).
    #[serde(default)]
    names: Vec<String>,
    #[serde(default)]
    numbers: Vec<String>,
    /// Without the NER model there is NOTHING to catch this case with (a name at the start
    /// of a phrase, a homonym). No model — the case is skipped, and the test says so out
    /// loud.
    #[serde(default)]
    needs_ner: bool,
}

/// The live NER model as the source of people — if it is on the disk.
struct Gliner {
    ner: localvox_light_core::ner::Ner,
    name: String,
    role: String,
}

impl localvox_light_llm::grounding::Entities for Gliner {
    fn people(&self, text: &str) -> Vec<String> {
        self.at(text, 0.5)
    }
    fn people_in_source(&self, text: &str) -> Vec<String> {
        // The recording is read GENEROUSLY: a spurious person there is harmless.
        self.at(text, 0.3)
    }
}

impl Gliner {
    fn at(&self, text: &str, threshold: f32) -> Vec<String> {
        self.ner
            .names(text, &self.name, &self.role, threshold)
            .map(|v| v.into_iter().map(|e| e.text).collect())
            .unwrap_or_default()
    }
}

fn open_ner() -> Option<Gliner> {
    // `cargo test` sets the cwd to the CRATE's directory, while the model lies in the root
    // of the repository. We look in both places: first as configured, then from the root.
    let dir = localvox_light_core::ner::model_dir().or_else(|| {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../models/ner-gliner");
        root.is_dir().then_some(root)
    })?;
    let ner = localvox_light_core::ner::Ner::open(&dir).ok()?;
    Some(Gliner {
        ner,
        name: "имя человека".to_string(),
        role: "говорящий или роль".to_string(),
    })
}

fn ru() -> String {
    "ru".to_string()
}

fn corpus_dir() -> std::path::PathBuf {
    // from crates/localvox-light-llm/ up to the root of the repository
    std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/corpus")
}

#[test]
fn grounding_holds_on_the_reference_corpus() {
    let dir = corpus_dir();
    let mut files: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("corpus {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("toml"))
        .collect();
    files.sort();
    assert!(!files.is_empty(), "the corpus is empty: {}", dir.display());

    let lex = localvox_light_core::lexicon::active();
    let ner = open_ner();
    if ner.is_none() {
        eprintln!(
            "WARNING: there is no NER model (models/ner-gliner) — the cases that are \
             impossible to catch without it were SKIPPED. Full run: scripts/setup-ner.ps1"
        );
    }

    let mut failures: Vec<String> = Vec::new();
    let mut checked = 0usize;
    let mut skipped = 0usize;

    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let c: Corpus = toml::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        let speech = c.transcript.trim();
        // The base of the check is THE SAME as in prod: the template + the speech. The
        // words of our own template are not an invention (we gave them to the model
        // ourselves), and without the template the test would be checking something other
        // than what actually runs.
        let template = localvox_light_llm::templates::for_lang("summary", &c.lang, None)
            .map(|(_, t)| t)
            .unwrap_or_default();
        let base = format!(
            "{template}
{speech}"
        );

        for case in &c.cases {
            if case.needs_ner && ner.is_none() {
                skipped += 1;
                continue;
            }
            checked += 1;
            let answer = case.answer.trim();
            let u = match &ner {
                Some(n) => localvox_light_llm::grounding::check_with_entities(
                    lex, &base, speech, answer, n,
                ),
                None => localvox_light_llm::grounding::check(&base, answer),
            };
            let flagged = !u.is_empty();

            let want_flag = match case.verdict.as_str() {
                "clean" => false,
                "flagged" => true,
                other => panic!("{}: unknown verdict «{other}»", c.name),
            };

            if flagged != want_flag {
                failures.push(format!(
                    "[{}] {}\n     expected: {}\n     got:      {}",
                    c.name,
                    case.name,
                    if want_flag {
                        "an accusation"
                    } else {
                        "silence"
                    },
                    if flagged {
                        format!("an accusation — {}", u.describe())
                    } else {
                        "silence".into()
                    }
                ));
                continue;
            }

            // We accused — but was it for THE RIGHT THING?
            for n in &case.names {
                if !u.names.iter().any(|x| x == n) {
                    failures.push(format!(
                        "[{}] {}\n     the name «{n}» was not caught (caught: {:?})",
                        c.name, case.name, u.names
                    ));
                }
            }
            for n in &case.numbers {
                if !u.numbers.iter().any(|x| x == n) {
                    failures.push(format!(
                        "[{}] {}\n     the number «{n}» was not caught (caught: {:?})",
                        c.name, case.name, u.numbers
                    ));
                }
            }
        }
    }

    if !failures.is_empty() {
        panic!(
            "

CORPUS: {checked} cases, {} failures{}

{}
",
            failures.len(),
            skip_note(skipped),
            failures.join(
                "

"
            )
        );
    }
    eprintln!("corpus: {checked} cases — all passed{}", skip_note(skipped));
}

fn skip_note(skipped: usize) -> String {
    if skipped == 0 {
        String::new()
    } else {
        format!(" (skipped without the NER model: {skipped})")
    }
}
