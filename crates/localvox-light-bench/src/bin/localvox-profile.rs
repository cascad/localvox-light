//! Isolated, sequential worker-phase measurements. Never reads the user's .env or archive.
use anyhow::{ensure, Context, Result};
use clap::Parser;
use localvox_light_core::{jobs::JobPhase, journal, processing, progress};
use localvox_light_process::{Limits, Termination};
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::Read,
    path::{Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[path = "profile/ollama.rs"]
mod ollama;
#[path = "profile/resources.rs"]
mod resources;

#[derive(Parser)]
#[command(about = "Measure fresh-process worker phases on copied PCM WAV fixtures")]
struct Args {
    #[arg(long)]
    manifest: PathBuf,
    #[arg(long)]
    worker: PathBuf,
    #[arg(long)]
    model_dir: PathBuf,
    /// Must not exist. This tool never overwrites or cleans up an earlier archive.
    #[arg(long)]
    output: PathBuf,
    #[arg(long, default_value_t = 2)]
    repeats: u32,
    #[arg(long, default_value_t = 120)]
    timeout_sec: u64,
    /// Local HTTP endpoint only; no credentials or implicit cloud provider.
    #[arg(long, default_value = "http://127.0.0.1:11434/v1")]
    llm_base_url: String,
    #[arg(long, default_value = "qwen3.5:9b")]
    llm_model: String,
    /// Record content-free native Ollama request timings through an ephemeral local proxy.
    #[arg(long)]
    trace_ollama: bool,
    /// Explicit NER directory; otherwise only the sibling of --model-dir is used.
    #[arg(long)]
    ner_model_dir: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    version: u32,
    label: String,
    cases: Vec<Case>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    name: String,
    audio: PathBuf,
    #[serde(default)]
    summary: bool,
}

fn checksum(path: &Path) -> Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn model_fingerprints(dir: &Path) -> Result<Vec<Value>> {
    let mut paths: Vec<_> = fs::read_dir(dir)?
        .collect::<std::io::Result<Vec<_>>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_file()
                && matches!(
                    path.extension().and_then(|s| s.to_str()),
                    Some("onnx" | "txt" | "yaml" | "yml" | "json")
                )
        })
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|path| {
            Ok(
                json!({"file": path.file_name().unwrap_or_default().to_string_lossy(),
        "sha256": checksum(path)?}),
            )
        })
        .collect()
}

fn audio_seconds(path: &Path) -> Result<f64> {
    let reader = hound::WavReader::open(path).context("reading benchmark WAV")?;
    let spec = reader.spec();
    ensure!(
        spec.channels == 1
            && spec.sample_rate == 16_000
            && spec.bits_per_sample == 16
            && spec.sample_format == hound::SampleFormat::Int,
        "benchmark input must be PCM16 mono 16 kHz"
    );
    let duration = reader.duration();
    ensure!(duration > 0, "benchmark WAV must not be empty");
    // Reading every sample catches truncated WAV before any worker is started.
    let count = reader
        .into_samples::<i16>()
        .try_fold(0usize, |count, sample| sample.map(|_| count + 1))?;
    ensure!(count == duration as usize, "incomplete benchmark WAV");
    Ok(duration as f64 / 16_000.0)
}

fn local_endpoint(url: &str) -> bool {
    // The endpoint is passed to a local test worker, not fetched by this process.
    // Reject userinfo, DNS names, HTTPS/cloud and ambiguous authority spellings.
    let Some(rest) = url.strip_prefix("http://") else {
        return false;
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = authority.rsplit_once(':');
    matches!(host, Some(("127.0.0.1" | "localhost" | "[::1]", port))
        if port.parse::<u16>().is_ok_and(|port| port != 0))
        && !url.contains(['?', '#', '@'])
}

fn read_manifest(path: &Path) -> Result<Manifest> {
    let mut manifest: Manifest = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        manifest.version == 1 && !manifest.cases.is_empty(),
        "expected manifest v1 with cases"
    );
    let mut names = std::collections::HashSet::new();
    for case in &mut manifest.cases {
        ensure!(
            !case.name.is_empty()
                && case.name.len() <= 64
                && case
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_')),
            "case names must be 1..64 ASCII letters, numbers, '-' or '_'"
        );
        ensure!(names.insert(case.name.clone()), "duplicate case name");
        case.audio = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&case.audio)
            .canonicalize()?;
        audio_seconds(&case.audio)?;
    }
    Ok(manifest)
}

fn new_output(path: &Path) -> Result<PathBuf> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent)?;
    }
    fs::create_dir(path).context("output must be a new directory")?;
    let path = path.canonicalize()?;
    fs::create_dir(path.join("temp"))?;
    fs::create_dir(path.join("glossary"))?;
    Ok(path)
}

fn worker_command(args: &Args, root: &Path, session: &Path, phase: JobPhase) -> Command {
    let mut command = Command::new(&args.worker);
    command.env_clear();
    for key in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "HOME",
        "USERPROFILE",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
    ] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    command
        .current_dir(root)
        .env("TEMP", root.join("temp"))
        .env("TMP", root.join("temp"))
        .env("LOCALVOX_DIARIZE", "off")
        .env(
            "LOCALVOX_NER_MODEL_DIR",
            args.ner_model_dir
                .clone()
                .unwrap_or_else(|| root.join("missing-ner")),
        )
        .arg(session)
        .arg("--worker-phase")
        .arg(phase.as_str())
        .arg("--work-dir")
        .arg(root)
        .arg("--model-dir")
        .arg(&args.model_dir)
        .arg("--glossary-dir")
        .arg(root.join("glossary"))
        .arg("--summary-provider")
        .arg("ollama")
        .arg("--llm-base-url")
        .arg(&args.llm_base_url)
        .arg("--llm-model")
        .arg(&args.llm_model);
    command
}

fn measure(mut command: Command, timeout: Duration, cancelled: &AtomicBool) -> Result<Value> {
    // Explicit even before run: the command's environment is part of our isolation contract.
    command.env(localvox_light_process::CONTAINED_ENV, "1");
    let stopped = AtomicBool::new(false);
    let (output, usage) = std::thread::scope(|scope| {
        let stopped = &stopped;
        let mut sampler = None;
        let result = localvox_light_process::run(
            command,
            None,
            Limits::new(timeout),
            || cancelled.load(Ordering::Relaxed),
            |pid, _| {
                if sampler.is_none() {
                    sampler = Some(scope.spawn(move || resources::observe(pid, stopped)));
                }
            },
        );
        stopped.store(true, Ordering::Relaxed);
        let usage = sampler
            .map(|thread| thread.join().unwrap_or_else(|_| resources::unavailable()))
            .unwrap_or_else(resources::unavailable);
        (result, usage)
    });
    let output = output?;
    let status = match output.termination {
        Termination::Exited(code) if code.success() => "success",
        Termination::Exited(_) => "failed",
        Termination::TimedOut => "timeout",
        Termination::Cancelled => "cancelled",
        Termination::OutputLimit => "output_limit",
    };
    let diagnostic_text = String::from_utf8_lossy(&output.stderr);
    let ner_loaded = if diagnostic_text
        .lines()
        .any(|line| line.trim_start().starts_with("name checking:"))
    {
        Some(true)
    } else if diagnostic_text.contains("name checking is OFF")
        || diagnostic_text.contains("NER model failed to load")
    {
        Some(false)
    } else {
        None
    };
    let diagnostic_tail = (status != "success").then(|| {
        String::from_utf8_lossy(&output.stderr)
            .chars()
            .rev()
            .take(2000)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
    });
    Ok(
        json!({"status": status, "wall_sec": output.elapsed.as_secs_f64(),
        "resources": usage,
        "exit_code": match output.termination { Termination::Exited(code) => code.code(), _ => None },
        "stdout_bytes": output.stdout.len(), "stderr_bytes": output.stderr.len(), "diagnostic_tail": diagnostic_tail,
        "ner_loaded_worker_diagnostic": ner_loaded}),
    )
}

fn timeline(session: &Path) -> Value {
    let stages: Vec<_> = progress::fold(&progress::read(session))
        .into_iter()
        .map(|stage| {
            let duration = stage
                .started_at
                .as_deref()
                .zip(stage.ended_at.as_deref())
                .and_then(|(start, end)| {
                    let start = chrono::DateTime::parse_from_rfc3339(start).ok()?;
                    let end = chrono::DateTime::parse_from_rfc3339(end).ok()?;
                    let milliseconds = (end - start).num_milliseconds();
                    (milliseconds >= 0).then_some(milliseconds as f64 / 1000.0)
                });
            let mut value = serde_json::to_value(stage).unwrap();
            value["elapsed_sec"] = json!(duration);
            value
        })
        .collect();
    let history = fs::read_to_string(session.join("progress.jsonl"));
    let history_status = match &history {
        Ok(_) => "available",
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => "missing",
        Err(_) => "unreadable",
    };
    let operation_rows = history.unwrap_or_default();
    let malformed = operation_rows
        .lines()
        .filter(|line| serde_json::from_str::<Value>(line).is_err())
        .count();
    let operations: Vec<_> = operation_rows
        .lines()
        .filter_map(|line| {
            let entry: journal::Entry = serde_json::from_str(line).ok()?;
            (entry.journal == 1 && entry.kind == "operation_end").then(|| {
                json!({
                    "operation": entry.operation, "stage": entry.stage, "label": entry.label,
                    "elapsed_sec": entry.elapsed_sec,
                })
            })
        })
        .collect();
    json!({"stages": stages, "operations": operations, "history_status": history_status,
        "malformed_records": malformed, "incomplete_tail": !operation_rows.is_empty() && !operation_rows.ends_with('\n'),
        "operation_durations_are_nested_not_additive": true,
        "model_load_sec": null, "generation_sec": null, "verification_sec": null})
}

fn checkpoint(root: &Path, report: &Value) -> Result<()> {
    localvox_light_core::artifacts::atomic_write(
        &root.join("report.json"),
        &serde_json::to_vec_pretty(report)?,
    )
}

fn run(mut args: Args, cancelled: &AtomicBool) -> Result<PathBuf> {
    let started = Instant::now();
    ensure!(
        (1..=100).contains(&args.repeats) && args.timeout_sec > 0,
        "invalid repeat/timeout limit"
    );
    ensure!(
        local_endpoint(&args.llm_base_url),
        "summary endpoint must be explicit local HTTP host:port"
    );
    args.worker = args.worker.canonicalize().context("worker path")?;
    args.model_dir = args.model_dir.canonicalize().context("model directory")?;
    let manifest = read_manifest(&args.manifest)?;
    let fingerprints: Vec<_> = manifest
        .cases
        .iter()
        .map(|case| {
            Ok(json!({
                "name": case.name, "audio_sha256": checksum(&case.audio)?,
                "audio_sec": audio_seconds(&case.audio)?, "summary": case.summary,
            }))
        })
        .collect::<Result<_>>()?;
    let model_files = model_fingerprints(&args.model_dir)?;
    if manifest.cases.iter().any(|case| case.summary) {
        if args.ner_model_dir.is_none() {
            args.ner_model_dir = args
                .model_dir
                .parent()
                .map(|parent| parent.join("ner-gliner"))
                .filter(|path| path.is_dir());
        }
        args.ner_model_dir = args
            .ner_model_dir
            .map(|path| path.canonicalize())
            .transpose()?;
    }
    let ner_model_files = args
        .ner_model_dir
        .as_deref()
        .map(model_fingerprints)
        .transpose()?;
    let mut report = json!({"version": 1, "scope": "isolated_sequential_worker_phases",
        "label": manifest.label, "runner_debug_assertions": cfg!(debug_assertions),
        "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
        "logical_cpus": std::thread::available_parallelism().ok().map(|n| n.get()),
        "worker_sha256": checksum(&args.worker)?, "runner_sha256": checksum(&std::env::current_exe()?)?,
        "manifest_sha256": checksum(&args.manifest)?,
        "model_files": model_files, "ner_model_files": ner_model_files, "fingerprints_pre_read_inputs_and_models": true,
        "repeats": args.repeats, "process_policy": "fresh process for each phase; cache state uncontrolled",
        "configuration": {"diarization": false, "cleanup": false, "refine": false,
            "summary_provider": "ollama", "llm_base_url": args.llm_base_url, "llm_model": args.llm_model,
            "timeout_sec": args.timeout_sec, "trace_ollama": args.trace_ollama}, "inputs": fingerprints,
        "queue_wait_sec": null, "external_llm_resources": null,
        "runs": [], "complete": false});
    let root = new_output(&args.output)?;
    let trace = args
        .trace_ollama
        .then(|| ollama::Trace::start(&args.llm_base_url, Duration::from_secs(args.timeout_sec)))
        .transpose()?;
    if let Some(trace) = &trace {
        args.llm_base_url = trace.base_url().to_string();
    }
    report["preflight_sec"] = json!(started.elapsed().as_secs_f64());
    checkpoint(&root, &report)?;
    let batch = Instant::now();
    for repeat in 1..=args.repeats {
        for (case, input) in manifest
            .cases
            .iter()
            .zip(report["inputs"].as_array().unwrap().clone())
        {
            if cancelled.load(Ordering::Relaxed) {
                break;
            }
            let setup = Instant::now();
            let session = root
                .join("sessions")
                .join(format!("{}-r{repeat}", case.name));
            fs::create_dir_all(session.join("audio"))?;
            fs::copy(&case.audio, session.join("audio/src0_chunk000000.wav"))?;
            // No source URL: no download process and no user capture metadata.
            let meta = localvox_light_core::chunks::SessionMeta {
                started_at: localvox_light_core::versions::now_rfc3339(),
                sample_rate: 16_000,
                stopped_at: Some(localvox_light_core::versions::now_rfc3339()),
                lang: Some("ru".into()),
                ..Default::default()
            };
            fs::write(session.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;
            let mut record = json!({"case": case.name, "repeat": repeat,
                "audio_sec": input["audio_sec"], "setup_sec": setup.elapsed().as_secs_f64(), "phases": [],
                "complete": false, "success": false, "worker_wall_sec": null, "rtf": null});
            let run_index = report["runs"].as_array().unwrap().len();
            report["runs"].as_array_mut().unwrap().push(record.clone());
            for phase in [JobPhase::Transcribe, JobPhase::Summary] {
                if phase == JobPhase::Summary && !case.summary {
                    continue;
                }
                let before_requests = trace.as_ref().map_or(0, |trace| trace.count());
                let mut result = match measure(
                    worker_command(&args, &root, &session, phase),
                    Duration::from_secs(args.timeout_sec),
                    cancelled,
                ) {
                    Ok(value) => value,
                    Err(error) => {
                        json!({"status": "spawn_error", "wall_sec": null, "error": error.to_string()})
                    }
                };
                result["llm_trace"] = trace
                    .as_ref()
                    .map(|trace| {
                        ollama::summarize(trace.since(before_requests), result["wall_sec"].as_f64())
                    })
                    .unwrap_or(Value::Null);
                if trace.is_some() {
                    ollama::redact_phase(&mut result);
                }
                result["phase"] = json!(phase.as_str());
                if result["status"] == "success" {
                    let artifact = if phase == JobPhase::Transcribe {
                        processing::TRANSCRIPT
                    } else {
                        processing::SUMMARY
                    };
                    let verification = Instant::now();
                    let artifact = processing::inspect(&session, artifact);
                    result["artifact_state"] = serde_json::to_value(artifact.state)?;
                    result["content_unverified"] = json!(processing::doubts(
                        &session,
                        if phase == JobPhase::Summary {
                            processing::SUMMARY
                        } else {
                            processing::TRANSCRIPT
                        }
                    )
                    .is_some());
                    result["acceptance_verification_sec"] =
                        json!(verification.elapsed().as_secs_f64());
                    if !matches!(
                        artifact.state,
                        processing::ArtifactState::Ready | processing::ArtifactState::Empty
                    ) {
                        result["status"] = json!("invalid_artifact");
                        result["verification_error"] = json!(artifact.reason);
                    }
                }
                let success = result["status"] == "success";
                eprintln!(
                    "{} r{} {}: {} · {:?} s",
                    case.name,
                    repeat,
                    phase.as_str(),
                    result["status"],
                    result["wall_sec"]
                );
                record["phases"].as_array_mut().unwrap().push(result);
                // Preserve an accepted STT measurement even while a long summary is still pending.
                report["runs"][run_index] = record.clone();
                report["batch_wall_sec"] = json!(batch.elapsed().as_secs_f64());
                report["total_wall_sec"] = json!(started.elapsed().as_secs_f64());
                checkpoint(&root, &report)?;
                if !success {
                    break;
                }
            }
            let phases = record["phases"].as_array().unwrap();
            let success = phases.len() == if case.summary { 2 } else { 1 }
                && phases.iter().all(|phase| phase["status"] == "success");
            let wall = phases
                .iter()
                .filter_map(|p| p["wall_sec"].as_f64())
                .sum::<f64>();
            record["success"] = json!(success);
            record["complete"] = json!(true);
            record["timeline"] = timeline(&session);
            if trace.is_some() {
                ollama::redact_timeline(&mut record["timeline"]);
            }
            record["worker_wall_sec"] = if success { json!(wall) } else { Value::Null };
            record["rtf"] = record["worker_wall_sec"]
                .as_f64()
                .map(|wall| json!(wall / input["audio_sec"].as_f64().unwrap()))
                .unwrap_or(Value::Null);
            report["runs"][run_index] = record;
            report["batch_wall_sec"] = json!(batch.elapsed().as_secs_f64());
            report["total_wall_sec"] = json!(started.elapsed().as_secs_f64());
            checkpoint(&root, &report)?;
        }
    }
    report["complete"] = json!(
        !cancelled.load(Ordering::Relaxed)
            && report["runs"].as_array().unwrap().len()
                == manifest.cases.len() * args.repeats as usize
    );
    report["all_success"] = json!(
        report["complete"] == true
            && report["runs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|run| run["success"] == true)
    );
    report["total_wall_sec"] = json!(started.elapsed().as_secs_f64());
    checkpoint(&root, &report)?;
    ensure!(
        report["all_success"] == true,
        "benchmark incomplete/failed; see {}",
        root.join("report.json").display()
    );
    Ok(root.join("report.json"))
}

fn main() -> Result<()> {
    let cancelled = Arc::new(AtomicBool::new(false));
    let signal = cancelled.clone();
    ctrlc::set_handler(move || {
        signal.store(true, Ordering::Relaxed);
    })?;
    println!("{}", run(Args::parse(), &cancelled)?.display());
    Ok(())
}

#[cfg(test)]
#[path = "profile/tests.rs"]
mod tests;
