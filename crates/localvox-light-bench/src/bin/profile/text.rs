//! Fresh transcript copies, production text workers, content-free measurements.
use super::*;
use localvox_light_core::versions::{TranscriptLine, VersionEntry, VersionStore};

#[derive(Clone, Copy, Deserialize, serde::Serialize, Debug)]
#[serde(rename_all = "kebab-case")]
enum Mode {
    Refine,
    Cleanup,
    RefineCleanup,
    Summary,
}
impl Mode {
    fn name(self) -> &'static str {
        match self {
            Self::Refine => "refine",
            Self::Cleanup => "cleanup",
            Self::RefineCleanup => "refine-cleanup",
            Self::Summary => "summary",
        }
    }
    fn artifacts(self) -> &'static [&'static str] {
        match self {
            Self::Refine => &[processing::REFINED],
            Self::Cleanup => &[processing::PROCESSED],
            Self::RefineCleanup => &[processing::REFINED, processing::PROCESSED],
            Self::Summary => &[processing::SUMMARY],
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextManifest {
    version: u32,
    cases: Vec<TextCase>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TextCase {
    name: String,
    transcript: PathBuf,
    lang: String,
    modes: Vec<Mode>,
}

fn read_manifest(path: &Path) -> Result<TextManifest> {
    let mut manifest: TextManifest = serde_json::from_slice(&fs::read(path)?)?;
    ensure!(
        manifest.version == 1 && !manifest.cases.is_empty(),
        "expected text manifest v1"
    );
    let mut names = std::collections::HashSet::new();
    for case in &mut manifest.cases {
        ensure!(
            !case.name.is_empty()
                && case
                    .name
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            "invalid case name"
        );
        ensure!(names.insert(case.name.clone()), "duplicate case name");
        ensure!(!case.modes.is_empty(), "no text modes selected");
        let modes: std::collections::HashSet<_> = case.modes.iter().map(|m| m.name()).collect();
        ensure!(modes.len() == case.modes.len(), "duplicate text mode");
        case.lang = localvox_light_core::lang::normalize(&case.lang)?;
        case.transcript = path
            .parent()
            .unwrap_or(Path::new("."))
            .join(&case.transcript)
            .canonicalize()?;
        let lines = localvox_light_core::artifacts::transcript(&fs::read(&case.transcript)?)?;
        ensure!(
            !lines.is_empty(),
            "text profiling requires a nonempty transcript"
        );
    }
    Ok(manifest)
}

fn seed(session: &Path, bytes: &[u8], lang: &str) -> Result<()> {
    let store = VersionStore::open(session)?;
    let (id, path) = store.next_version("profile-source")?;
    fs::write(&path, bytes)?;
    store.commit(VersionEntry {
        id,
        label: "profile-source".into(),
        file: path.file_name().unwrap().to_string_lossy().into(),
        model: "copied-jsonl".into(),
        params: json!({"profile_input": true}),
        created_at: localvox_light_core::versions::now_rfc3339(),
        parents: vec![],
    })?;
    store.set_best(id)?;
    let meta = localvox_light_core::chunks::SessionMeta {
        started_at: localvox_light_core::versions::now_rfc3339(),
        stopped_at: Some(localvox_light_core::versions::now_rfc3339()),
        sample_rate: 16_000,
        lang: Some(lang.into()),
        ..Default::default()
    };
    fs::write(session.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;
    Ok(())
}

fn command(args: &Args, root: &Path, session: &Path, mode: Mode) -> Command {
    let phase = if matches!(mode, Mode::Summary) {
        JobPhase::Summary
    } else {
        JobPhase::Text
    };
    let mut cmd = worker_command(args, root, session, phase);
    if matches!(mode, Mode::Refine | Mode::RefineCleanup) {
        cmd.arg("--refine");
    }
    if matches!(mode, Mode::Cleanup | Mode::RefineCleanup) {
        cmd.arg("--cleanup");
    }
    cmd
}

fn correction_counts(session: &Path, original: &[TranscriptLine]) -> Value {
    let store = VersionStore::open(session).ok();
    let best = store.as_ref().and_then(VersionStore::best);
    let refined = best.filter(|v| v.label == "refined").and_then(|v| {
        let path = store.as_ref()?.resolve(v.id)?;
        localvox_light_core::artifacts::transcript(&fs::read(path).ok()?).ok()
    });
    let changed = refined.as_ref().map(|lines| {
        lines
            .iter()
            .zip(original)
            .filter(|(a, b)| a.text != b.text)
            .count()
    });
    let readable = localvox_light_core::readable::load(session).ok();
    json!({"refined_lines": refined.as_ref().map(Vec::len), "refined_changed_lines": changed,
        "cleanup_edits": readable.as_ref().map(|r| r.edits.len()),
        "cleanup_omitted": readable.as_ref().map(|r| r.omitted),
        "cleanup_rejected": readable.as_ref().map(|r| r.rejected),
        "counts_do_not_establish_semantic_quality": true})
}

pub(super) fn run(args: &mut Args, cancelled: &AtomicBool) -> Result<PathBuf> {
    ensure!(
        args.queue_runner.is_none(),
        "text-only cannot use the audio queue runner"
    );
    let manifest = read_manifest(&args.manifest)?;
    let experimental = args.experimental_sparse_edits || args.experimental_sparse_lines;
    if experimental {
        ensure!(
            manifest
                .cases
                .iter()
                .all(|c| c.modes.iter().all(|m| matches!(m, Mode::Cleanup))),
            "sparse experiment supports cleanup only"
        );
        for case in &manifest.cases {
            let lines = localvox_light_core::artifacts::transcript(&fs::read(&case.transcript)?)?;
            ensure!(
                lines.iter().all(|l| !l.text.chars().any(char::is_control)),
                "sparse experiment requires single-line source text"
            );
        }
    }
    args.ner_model_dir = args
        .ner_model_dir
        .clone()
        .or_else(|| {
            args.model_dir
                .parent()
                .map(|p| p.join("ner-gliner"))
                .filter(|p| p.is_dir())
        })
        .map(|p| p.canonicalize())
        .transpose()?;
    let inputs: Vec<_> = manifest
        .cases
        .iter()
        .map(|case| {
            let bytes = fs::read(&case.transcript)?;
            let lines = localvox_light_core::artifacts::transcript(&bytes)?;
            Ok(
                json!({"case":case.name,"sha256":checksum(&case.transcript)?,"lines":lines.len(),
            "text_bytes":lines.iter().map(|l| l.text.len()).sum::<usize>(),
            "text_chars":lines.iter().map(|l| l.text.chars().count()).sum::<usize>(),
            "last_end_sec":lines.iter().map(|l|l.end_sec).fold(0.0,f64::max)}),
            )
        })
        .collect::<Result<_>>()?;
    let root = new_output(&args.output)?;
    let mut report = json!({"version":1,"scope":"isolated_text_postprocessing",
        "worker_sha256":checksum(&args.worker)?, "manifest_sha256":checksum(&args.manifest)?,
        "runner_debug_assertions":cfg!(debug_assertions),"os":std::env::consts::OS,
        "llm_model":args.llm_model,"llm_provider":"explicit_local_ollama",
        "ner_model_files":args.ner_model_dir.as_deref().map(model_fingerprints).transpose()?,
        "asr_executed":false,"experimental_sparse_edits":args.experimental_sparse_edits,
        "experimental_sparse_lines":args.experimental_sparse_lines,
        "trace_enabled":args.trace_ollama || experimental,
        "semantic_quality_established":false,"input_preserved":null,"inputs":inputs,"runs":[],"complete":false,"success":false});
    checkpoint(&root, &report)?;
    let trace = if args.experimental_sparse_lines {
        Some(ollama::Trace::sparse_lines(
            &args.llm_base_url,
            Duration::from_secs(args.timeout_sec),
        )?)
    } else if args.experimental_sparse_edits {
        Some(ollama::Trace::sparse_experiment(
            &args.llm_base_url,
            Duration::from_secs(args.timeout_sec),
        )?)
    } else if args.trace_ollama {
        Some(ollama::Trace::start(
            &args.llm_base_url,
            Duration::from_secs(args.timeout_sec),
        )?)
    } else {
        None
    };
    let original_endpoint = args.llm_base_url.clone();
    if let Some(trace) = &trace {
        args.llm_base_url = trace.base_url().into();
    }
    let started = Instant::now();
    let mut all_ok = true;
    'batch: for repeat in 1..=args.repeats {
        for (ci, case) in manifest.cases.iter().enumerate() {
            let bytes = fs::read(&case.transcript)?;
            ensure!(
                checksum(&case.transcript)? == report["inputs"][ci]["sha256"],
                "input changed during profile"
            );
            let original = localvox_light_core::artifacts::transcript(&bytes)?;
            for mode in &case.modes {
                if cancelled.load(Ordering::Relaxed) {
                    all_ok = false;
                    break 'batch;
                }
                let session =
                    root.join("sessions")
                        .join(format!("{}-{}-r{repeat}", case.name, mode.name()));
                seed(&session, &bytes, &case.lang)?;
                let index = trace.as_ref().map_or(0, ollama::Trace::count);
                // Checkpoint pending work so a long LLM call is never reported as completed.
                let ri = report["runs"].as_array().unwrap().len();
                report["runs"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"case":case.name,"mode":mode,"repeat":repeat,"status":"running"}));
                checkpoint(&root, &report)?;
                let mut result = measure(
                    command(args, &root, &session, *mode),
                    Duration::from_secs(args.timeout_sec),
                    cancelled,
                )
                .unwrap_or_else(|_| json!({"status":"spawn_error","wall_sec":null}));
                ollama::redact_phase(&mut result);
                result["case"] = json!(case.name);
                result["mode"] = json!(mode);
                result["repeat"] = json!(repeat);
                let rows = trace.as_ref().map(|trace| trace.since(index));
                if experimental {
                    let fields = [
                        "input_lines",
                        "eligible_lines",
                        "proposed",
                        "accepted",
                        "rejected",
                        "noop",
                        "restored_lines",
                        "protocol_errors",
                    ];
                    let mut totals = serde_json::Map::new();
                    for field in fields {
                        totals.insert(
                            field.into(),
                            json!(rows
                                .as_ref()
                                .expect("sparse adapter is enabled")
                                .iter()
                                .filter_map(|r| r["sparse"][field].as_u64())
                                .sum::<u64>()),
                        );
                    }
                    totals.insert(
                        "omission_is_not_a_model_coverage_measure".into(),
                        json!(true),
                    );
                    result["sparse"] = json!(totals);
                }
                result["llm_trace"] = rows
                    .map(|rows| ollama::summarize(rows, result["wall_sec"].as_f64()))
                    .unwrap_or(Value::Null);
                let ledger = processing::load(&session);
                let verified = mode.artifacts().iter().all(|key| {
                    ledger.artifacts.get(*key).is_some_and(|record| {
                        localvox_light_core::artifacts::verify(&session, key, record).is_ok()
                    })
                });
                result["artifacts_verified"] = json!(verified);
                if result["status"] == "success" && !verified {
                    result["status"] = json!("invalid_artifact");
                }
                result["corrections"] = correction_counts(&session, &original);
                result["timeline"] = timeline(&session);
                ollama::redact_timeline(&mut result["timeline"]);
                all_ok = result["status"] == "success";
                println!(
                    "{} {} r{repeat}: {} · {:?} s · {} requests",
                    case.name,
                    mode.name(),
                    result["status"],
                    result["wall_sec"],
                    result["llm_trace"]["request_count"]
                );
                report["runs"][ri] = result;
                report["batch_wall_sec"] = json!(started.elapsed().as_secs_f64());
                checkpoint(&root, &report)?;
                if !all_ok {
                    break 'batch;
                }
            }
        }
    }
    args.llm_base_url = original_endpoint;
    let preserved = manifest.cases.iter().enumerate().all(|(i, c)| {
        checksum(&c.transcript)
            .ok()
            .is_some_and(|hash| hash == report["inputs"][i]["sha256"])
    });
    let expected =
        manifest.cases.iter().map(|c| c.modes.len()).sum::<usize>() * args.repeats as usize;
    report["input_preserved"] = json!(preserved);
    report["complete"] = json!(all_ok && report["runs"].as_array().unwrap().len() == expected);
    report["success"] = json!(all_ok && preserved);
    checkpoint(&root, &report)?;
    ensure!(
        report["complete"] == true && report["success"] == true,
        "text profile incomplete/failed; see report.json"
    );
    Ok(root)
}

#[cfg(test)]
#[path = "text_tests.rs"]
mod tests;
