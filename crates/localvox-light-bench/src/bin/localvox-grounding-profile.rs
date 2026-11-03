//! Explicit replay of grounding on saved text. No LLM, archive mutation or .env loading.
use anyhow::{bail, ensure, Context, Result};
use clap::Parser;
use localvox_light_core::{artifacts, citations, lexicon::Lexicon, ner::Ner, provenance};
use localvox_light_llm::{
    grounding::{self, Entities, SourceEntities},
    templates,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Instant,
};

#[derive(Parser)]
#[command(about = "Profile existing grounding on a saved transcript/summary; never invokes an LLM")]
struct Args {
    #[arg(long)]
    source: PathBuf,
    #[arg(long)]
    answer: PathBuf,
    #[arg(long)]
    ner_model_dir: PathBuf,
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value = "ru")]
    lang: String,
    #[arg(long, default_value_t = 2, value_parser = clap::value_parser!(u8).range(2..=8))]
    passes: u8,
}

#[derive(Clone, Copy, Default, Serialize)]
struct BackendStats {
    source_calls: u64,
    source_sec: f64,
    source_errors: u64,
    answer_calls: u64,
    answer_sec: f64,
    answer_errors: u64,
}
impl BackendStats {
    fn since(self, old: Self) -> Self {
        Self {
            source_calls: self.source_calls - old.source_calls,
            source_sec: self.source_sec - old.source_sec,
            source_errors: self.source_errors - old.source_errors,
            answer_calls: self.answer_calls - old.answer_calls,
            answer_sec: self.answer_sec - old.answer_sec,
            answer_errors: self.answer_errors - old.answer_errors,
        }
    }
    fn succeeded(self) -> bool {
        self.source_errors == 0 && self.answer_errors == 0
    }
}

#[derive(Default)]
struct Stats(Mutex<BackendStats>);
impl Stats {
    fn snapshot(&self) -> BackendStats {
        *self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
    fn request(&self, source: bool, call: impl FnOnce() -> Result<Vec<String>>) -> Vec<String> {
        let started = Instant::now();
        let result = call();
        let elapsed = started.elapsed().as_secs_f64();
        let mut stats = self.0.lock().unwrap_or_else(|e| e.into_inner());
        if source {
            stats.source_calls += 1;
            stats.source_sec += elapsed;
            stats.source_errors += u64::from(result.is_err());
        } else {
            stats.answer_calls += 1;
            stats.answer_sec += elapsed;
            stats.answer_errors += u64::from(result.is_err());
        }
        // The production Entities boundary also returns an empty Vec on backend failure.
        // The profiler separately marks that failure unknown/incomplete, never successful NER.
        result.unwrap_or_default()
    }
}

struct ObservedNer {
    ner: Ner,
    name: String,
    role: String,
    stats: Stats,
}
impl ObservedNer {
    fn request(&self, text: &str, source: bool) -> Vec<String> {
        let threshold = if source { 0.3 } else { self.ner.threshold() };
        eprintln!(
            "{} NER request started",
            if source { "source" } else { "answer" }
        );
        self.stats.request(source, || {
            self.ner
                .names(text, &self.name, &self.role, threshold)
                .map(|entities| entities.into_iter().map(|e| e.text).collect())
        })
    }
}
impl Entities for ObservedNer {
    fn people(&self, text: &str) -> Vec<String> {
        self.request(text, false)
    }
    fn people_in_source(&self, text: &str) -> Vec<String> {
        self.request(text, true)
    }
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn file_sha(path: &Path) -> Result<String> {
    let mut input = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 65536];
    loop {
        let count = input.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hasher.finalize()))
}
fn model_files(dir: &Path) -> Result<Vec<Value>> {
    let mut paths = fs::read_dir(dir)?
        .map(|e| e.map(|e| e.path()))
        .collect::<std::io::Result<Vec<_>>>()?;
    paths.sort();
    paths
        .into_iter()
        .filter(|p| p.is_file())
        .map(|path| {
            Ok(json!({
                "file":path.file_name().context("model filename missing")?.to_string_lossy(),
                "bytes":fs::metadata(&path)?.len(),"sha256":file_sha(&path)?
            }))
        })
        .collect()
}
fn create_report(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .context("output must be a new writable file")
}
fn save(file: &mut File, report: &Value) -> Result<()> {
    file.seek(SeekFrom::Start(0))?;
    file.set_len(0)?;
    serde_json::to_writer_pretty(&mut *file, report)?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}
fn answer_body(raw: &str) -> String {
    let (_, body) = provenance::split(raw);
    citations::parse(body)
        .into_iter()
        .map(|b| b.text)
        .collect::<Vec<_>>()
        .join("\n")
}

fn run_checks(
    words: &str,
    speech: &str,
    answer: &str,
    inner: &dyn Entities,
    stats: &Stats,
    passes: u8,
    report: &mut Value,
    output: &mut File,
) -> Result<()> {
    let lex = Lexicon::builtin();
    let cached = SourceEntities::new(inner);
    let mut verdicts = Vec::new();
    for pass in 1..=passes {
        eprintln!("grounding check {pass}/{passes} started");
        let before = stats.snapshot();
        let (verdict, timings) =
            grounding::check_profiled(&lex, words, speech, answer, Some(&cached));
        let backend = stats.snapshot().since(before);
        report["checks"]
            .as_array_mut()
            .context("checks array missing")?
            .push(json!({
                "pass":pass,"timings":timings,"backend":backend,
                "number_flags":verdict.numbers.len(),
                "name_flags":backend.succeeded().then_some(verdict.names.len()),
                "ner_succeeded":backend.succeeded()
            }));
        save(output, report)?;
        if !backend.succeeded() {
            report["status"] = json!("ner_request_failed");
            save(output, report)?;
            bail!("NER request failed; numeric partial report preserved");
        }
        verdicts.push(verdict);
    }
    let before = stats.snapshot();
    let started = Instant::now();
    let ordinary = grounding::check_with_entities(&lex, words, speech, answer, &cached);
    let wall_sec = started.elapsed().as_secs_f64();
    let backend = stats.snapshot().since(before);
    let parity = backend.succeeded() && verdicts.iter().all(|v| v == &ordinary);
    report["ordinary_check"] = json!({"wall_sec":wall_sec,"backend":backend,
        "exact_verdict_parity":parity,"number_flags":ordinary.numbers.len(),
        "name_flags":backend.succeeded().then_some(ordinary.names.len())});
    save(output, report)?;
    ensure!(
        parity,
        "ordinary/profiled verdict differs or NER failed; report preserved"
    );
    Ok(())
}

fn run(args: Args) -> Result<()> {
    let started = Instant::now();
    ensure!(!args.output.exists(), "output must be a new file");
    ensure!(
        fs::metadata(&args.source)?.len() <= 32 * 1024 * 1024,
        "source exceeds engineering limit"
    );
    ensure!(
        fs::metadata(&args.answer)?.len() <= 2 * 1024 * 1024,
        "answer exceeds engineering limit"
    );
    let source_bytes = fs::read(&args.source)?;
    let answer_bytes = fs::read(&args.answer)?;
    let mut rows = artifacts::transcript(&source_bytes)?;
    ensure!(!rows.is_empty(), "nonempty transcript required");
    rows.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    let speech = rows
        .iter()
        .map(|r| r.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let answer = answer_body(std::str::from_utf8(&answer_bytes)?);
    let (template_name, template) = templates::for_lang("summary", &args.lang, None)?;
    let words = format!("{template}\n\n{speech}");
    let name = std::env::var("LOCALVOX_NER_LABEL_NAME").unwrap_or_else(|_| "имя человека".into());
    let role =
        std::env::var("LOCALVOX_NER_LABEL_ROLE").unwrap_or_else(|_| "говорящий или роль".into());
    ensure!(!name.trim().is_empty(), "NER name label must be nonempty");
    let fingerprint_started = Instant::now();
    let fingerprints = model_files(&args.ner_model_dir)?;
    let fingerprint_sec = fingerprint_started.elapsed().as_secs_f64();
    let selected = std::env::var_os("LOCALVOX_NER_MODEL_FILE")
        .map(|p| args.ner_model_dir.join(p))
        .or_else(|| {
            ["model_fp16.onnx", "model_q4f16.onnx", "model.onnx"]
                .iter()
                .map(|p| args.ner_model_dir.join(p))
                .find(|p| p.is_file())
        })
        .context("preferred NER filename or explicit LOCALVOX_NER_MODEL_FILE required")?;
    ensure!(
        selected
            .canonicalize()?
            .starts_with(args.ner_model_dir.canonicalize()?),
        "selected model must be within the fingerprinted directory"
    );
    let mut report = json!({"version":1,"scope":"saved_summary_grounding_probe_no_llm",
        "complete":false,"success":false,"status":"starting","llm_executed":false,
        "semantic_quality_established":false,"runner_debug_assertions":cfg!(debug_assertions),
        "os":std::env::consts::OS,"binary_sha256":file_sha(&std::env::current_exe()?)?,
        "source_sha256":sha(&source_bytes),"answer_file_sha256":sha(&answer_bytes),
        "source_lines":rows.len(),"speech_chars":speech.chars().count(),"answer_chars":answer.chars().count(),
        "words_source_sha256":sha(words.as_bytes()),"speech_sha256":sha(speech.as_bytes()),
        "checked_answer_sha256":sha(answer.as_bytes()),"template":template_name,
        "model_files":fingerprints,"selected_model":selected.file_name().unwrap_or_default().to_string_lossy(),
        "model_fingerprinting_sec":fingerprint_sec,"source_threshold":0.3,
        "backend_count_unit":"Ner.names whole-text calls; separate person/role passes and windowing are inside each call",
        "name_label_sha256":sha(name.as_bytes()),"role_label_sha256":sha(role.as_bytes()),"role_label_empty":role.trim().is_empty(),
        "lexicon":"builtin","glossary":"empty","filesystem_cache_cleared":false,
        "timings_are_exclusive_backend_is_nested":true,"checks":[]});
    let mut output = create_report(&args.output)?;
    save(&mut output, &report)?;
    report["preparation_sec"] = json!(started.elapsed().as_secs_f64());
    report["model_fingerprinting_is_part_of_preparation"] = json!(true);
    eprintln!("NER model load started");
    let load_started = Instant::now();
    let ner = match Ner::open(&args.ner_model_dir) {
        Ok(ner) => ner,
        Err(_) => {
            report["status"] = json!("ner_load_failed");
            report["model_load_sec"] = json!(load_started.elapsed().as_secs_f64());
            save(&mut output, &report)?;
            bail!("NER model load failed; numeric partial report preserved");
        }
    };
    report["model_load_sec"] = json!(load_started.elapsed().as_secs_f64());
    if !ner.threshold().is_finite() {
        report["status"] = json!("invalid_ner_threshold");
        save(&mut output, &report)?;
        bail!("nonfinite NER threshold cannot be profiled");
    }
    report["answer_threshold"] = json!(ner.threshold());
    report["status"] = json!("checking");
    save(&mut output, &report)?;
    let observed = ObservedNer {
        ner,
        name,
        role,
        stats: Stats::default(),
    };
    run_checks(
        &words,
        &speech,
        &answer,
        &observed,
        &observed.stats,
        args.passes,
        &mut report,
        &mut output,
    )?;
    let verification_started = Instant::now();
    let preserved = file_sha(&args.source)? == sha(&source_bytes)
        && file_sha(&args.answer)? == sha(&answer_bytes)
        && model_files(&args.ner_model_dir)?
            == report["model_files"]
                .as_array()
                .cloned()
                .unwrap_or_default();
    report["final_verification_sec"] = json!(verification_started.elapsed().as_secs_f64());
    report["input_and_models_preserved"] = json!(preserved);
    report["total_backend"] = json!(observed.stats.snapshot());
    report["total_wall_sec"] = json!(started.elapsed().as_secs_f64());
    let measured_checks: f64 = report["checks"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["timings"]["total_sec"].as_f64())
        .sum();
    let attributed = report["preparation_sec"].as_f64().unwrap()
        + report["model_load_sec"].as_f64().unwrap()
        + report["final_verification_sec"].as_f64().unwrap()
        + report["ordinary_check"]["wall_sec"].as_f64().unwrap()
        + measured_checks;
    report["unattributed_wall_sec"] =
        json!((report["total_wall_sec"].as_f64().unwrap() - attributed).max(0.0));
    report["complete"] = json!(preserved);
    report["success"] = json!(preserved);
    report["status"] = json!(if preserved {
        "success"
    } else {
        "inputs_changed"
    });
    save(&mut output, &report)?;
    ensure!(
        preserved,
        "input/model hashes changed; incomplete report preserved"
    );
    eprintln!("profile complete; exact verdict parity confirmed");
    Ok(())
}

fn main() -> Result<()> {
    run(Args::parse())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fake {
        stats: Stats,
        fail: bool,
    }
    impl Entities for Fake {
        fn people(&self, _: &str) -> Vec<String> {
            self.stats.request(false, || {
                if self.fail {
                    bail!("PRIVATE_ERROR")
                } else {
                    Ok(vec!["PRIVATE_ENTITY".into()])
                }
            })
        }
        fn people_in_source(&self, _: &str) -> Vec<String> {
            self.stats.request(true, || {
                if self.fail {
                    bail!("PRIVATE_ERROR")
                } else {
                    Ok(vec!["PRIVATE_ENTITY".into()])
                }
            })
        }
    }
    #[test]
    fn report_is_numeric_preserves_parity_and_refuses_overwrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.json");
        let mut output = create_report(&path).unwrap();
        let fake = Fake {
            stats: Stats::default(),
            fail: false,
        };
        let mut report = json!({"complete":false,"success":false,"checks":[]});
        run_checks(
            "PRIVATE_SOURCE 150 задач",
            "PRIVATE_SOURCE 150 задач",
            "PRIVATE_ANSWER 151 задача",
            &fake,
            &fake.stats,
            2,
            &mut report,
            &mut output,
        )
        .unwrap();
        assert_eq!(fake.stats.snapshot().source_calls, 1);
        assert_eq!(fake.stats.snapshot().answer_calls, 3);
        assert_eq!(report["ordinary_check"]["exact_verdict_parity"], true);
        assert_eq!(report["checks"][0]["number_flags"], 1);
        assert_eq!(report["checks"][1]["backend"]["source_calls"], 0);
        let before = fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&before).contains("PRIVATE_"));
        assert!(create_report(&path).is_err());
        assert_eq!(fs::read(&path).unwrap(), before);
    }
    #[test]
    fn ner_failure_is_incomplete_and_never_a_successful_empty_tagging() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("report.json");
        let mut output = create_report(&path).unwrap();
        let fake = Fake {
            stats: Stats::default(),
            fail: true,
        };
        let mut report = json!({"complete":false,"success":false,"checks":[]});
        assert!(run_checks(
            "PRIVATE_SOURCE 150 задач",
            "PRIVATE_SOURCE 150 задач",
            "PRIVATE_ANSWER 151 задача",
            &fake,
            &fake.stats,
            2,
            &mut report,
            &mut output
        )
        .is_err());
        assert_eq!(report["complete"], false);
        assert_eq!(report["success"], false);
        assert_eq!(report["checks"][0]["name_flags"], Value::Null);
        assert_eq!(report["checks"][0]["number_flags"], 1);
        assert_eq!(report["status"], "ner_request_failed");
        assert!(!fs::read_to_string(path).unwrap().contains("PRIVATE_"));
    }
    #[test]
    fn saved_artifact_headers_and_citations_never_become_grounding_facts() {
        let raw = "<!-- localvox: summary-ru | модель PRIVATE_MODEL | 2026-11-03T00:00:00Z -->\n\n# Заголовок\nФакт [[150]].";
        assert_eq!(answer_body(raw), "Заголовок\nФакт.");
    }

    #[test]
    fn model_load_failure_keeps_a_partial_report_without_private_error_text() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("source.jsonl");
        let answer = dir.path().join("answer.md");
        let models = dir.path().join("models");
        fs::create_dir(&models).unwrap();
        fs::write(&source, r#"{"source_id":0,"start_sec":0,"end_sec":1,"text":"PRIVATE_SOURCE","speaker":"PRIVATE_SPEAKER"}"#).unwrap();
        fs::write(&answer, "PRIVATE_ANSWER").unwrap();
        fs::write(models.join("model.onnx"), []).unwrap();
        fs::write(models.join("tokenizer.json"), "PRIVATE_TOKENIZER_ERROR").unwrap();
        let output = dir.path().join("profile.json");
        let result = run(Args {
            source,
            answer,
            ner_model_dir: models,
            output: output.clone(),
            lang: "ru".into(),
            passes: 2,
        });
        assert!(result.is_err());
        let raw = fs::read_to_string(output).unwrap();
        let report: Value = serde_json::from_str(&raw).unwrap();
        assert_eq!(report["status"], "ner_load_failed");
        assert_eq!(report["complete"], false);
        assert_eq!(report["success"], false);
        assert!(!raw.contains("PRIVATE_"));
    }

    #[test]
    #[ignore = "requires explicit LOCALVOX_GROUNDING_PROFILE_REFERENCE; read-only saved reference, no models"]
    fn explicit_reference_receipts_verify_with_the_production_transcript_hash() {
        let session = PathBuf::from(
            std::env::var_os("LOCALVOX_GROUNDING_PROFILE_REFERENCE")
                .expect("explicit reference session required"),
        );
        let before = fs::read(session.join("processing.json")).unwrap();
        let log: localvox_light_core::processing::ProcessingLog =
            serde_json::from_slice(&before).unwrap();
        let record = log
            .artifacts
            .get("summary")
            .expect("summary record required");
        let receipt = artifacts::verify(&session, "summary", record).unwrap();
        assert!(receipt.input.is_some());
        assert_eq!(receipt.outputs.len(), 1);
        assert_eq!(fs::read(session.join("processing.json")).unwrap(), before);
    }
}
