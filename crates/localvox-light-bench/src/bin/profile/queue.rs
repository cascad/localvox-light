//! Stage immutable fixture copies, then use the production queue rather than a parallel recipe.
use super::*;
use localvox_light_core::{
    artifacts,
    chunks::{ChunkEntry, SessionMeta, Source},
    jobs::{JobQueue, JobState},
};

pub fn validate(args: &mut Args, manifest: &Manifest) -> Result<()> {
    let Some(runner) = &mut args.queue_runner else {
        return Ok(());
    };
    *runner = runner.canonicalize().context("queue runner path")?;
    ensure!(
        manifest
            .cases
            .iter()
            .all(|case| case.summary == manifest.cases[0].summary),
        "queue batch must use one summary policy; use separate manifests"
    );
    let expected = runner
        .parent()
        .context("queue runner directory")?
        .join(if cfg!(windows) {
            "localvox-process.exe"
        } else {
            "localvox-process"
        })
        .canonicalize()?;
    ensure!(
        expected == args.worker,
        "queue runner and worker must belong to the same build directory"
    );
    Ok(())
}

fn stage(root: &Path, manifest: &Manifest, repeats: u32) -> Result<Vec<Value>> {
    let mut runs = Vec::new();
    for repeat in 1..=repeats {
        for case in &manifest.cases {
            let started = Instant::now();
            let name = format!("{}-r{repeat}", case.name);
            let session = root.join("sessions").join(&name);
            fs::create_dir_all(session.join("audio"))?;
            let audio_sec = audio_seconds(&case.audio)?;
            fs::copy(&case.audio, session.join("audio/src0_chunk000000.wav"))?;
            let meta = SessionMeta {
                started_at: localvox_light_core::versions::now_rfc3339(),
                stopped_at: Some(localvox_light_core::versions::now_rfc3339()),
                stopped_reason: Some("isolated public benchmark fixture".into()),
                sample_rate: 16_000,
                lang: Some("ru".into()),
                source: Some(Source {
                    url: format!("profile-fixture:{}", case.name),
                    title: None,
                }),
                chunks: vec![ChunkEntry {
                    file: "src0_chunk000000".into(),
                    source_id: 0,
                    start_offset_sec: 0.0,
                    duration_sec: audio_sec,
                    rms: 0,
                    voice_ratio: 0.0,
                }],
                ..Default::default()
            };
            fs::write(session.join("meta.json"), serde_json::to_vec_pretty(&meta)?)?;
            artifacts::begin_preparation(&session)?;
            artifacts::commit_preparation(&session, (audio_sec * 16_000.0).round() as u64)?;
            artifacts::audio(&session)?;
            ensure!(
                JobQueue::mutate(root, |queue| queue.enqueue_cook(
                    &name,
                    case.summary,
                    false,
                    false
                )),
                "queue fixture enqueue failed"
            );
            runs.push(
                json!({"case":case.name,"repeat":repeat,"session":name,"audio_sec":audio_sec,
                "setup_sec":started.elapsed().as_secs_f64(),"complete":false,"success":false}),
            );
        }
    }
    Ok(runs)
}

fn command(args: &Args, root: &Path, summary: bool) -> Command {
    // Reuse the allowlisted environment, replace worker arguments with the headless entry.
    let worker = worker_command(args, root, root, JobPhase::Prepare);
    let mut command = Command::new(args.queue_runner.as_ref().unwrap());
    command.env_clear().envs(
        worker
            .get_envs()
            .filter_map(|(key, value)| value.map(|value| (key, value))),
    );
    command
        .current_dir(root)
        .env("LOCALVOX_ASR_MODEL_DIR", &args.model_dir)
        .env("LOCALVOX_LIGHT_AUTOCOOK", "1")
        .env(
            "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY",
            if summary { "1" } else { "0" },
        )
        .env("LOCALVOX_LIGHT_AUTOCOOK_INTERVAL_SEC", "5")
        .env("LOCALVOX_LIGHT_AUTOCOOK_QUIESCENT_SEC", "15")
        .env("LOCALVOX_LLM_MODEL", &args.llm_model)
        .env("LOCALVOX_LLM_BASE_URL", &args.llm_base_url)
        .env("LOCALVOX_SUMMARY_PROVIDER", "ollama")
        .env("LOCALVOX_GLOSSARY_DIR", root.join("glossary"))
        .arg("--work-dir")
        .arg(root)
        .arg("--worker")
        .arg(&args.worker)
        .arg("--timeout-sec")
        .arg(args.timeout_sec.to_string());
    for key in [
        "LOCALVOX_PREPARE_TIMEOUT_SEC",
        "LOCALVOX_TRANSCRIBE_TIMEOUT_SEC",
        "LOCALVOX_TEXT_TIMEOUT_SEC",
        "LOCALVOX_SUMMARY_TIMEOUT_SEC",
    ] {
        command.env(key, args.timeout_sec.to_string());
    }
    command
}

pub fn run(
    args: &Args,
    manifest: &Manifest,
    root: &Path,
    report: &mut Value,
    trace: Option<&ollama::Trace>,
    started: Instant,
    cancelled: &AtomicBool,
) -> Result<PathBuf> {
    let runner = args.queue_runner.as_ref().unwrap();
    let runner_hash = checksum(runner)?;
    report["scope"] = json!("isolated_production_coordinator_batch");
    report["queue_runner_sha256"] = json!(runner_hash);
    report["process_policy"] = json!("production resource lanes; fresh worker per phase; auto discovery unchanged; disk cache uncontrolled");
    report["configuration"]["coordinator_interval_sec"] = json!(5);
    report["configuration"]["prepare_input"] =
        json!("preconverted_pcm_with_verified_receipt; download_conversion_excluded");
    report["runs"] = json!(stage(root, manifest, args.repeats)?);
    checkpoint(root, report)?;
    let batch = Instant::now();
    // The queue runner handles its own timeout/stop first; supervisor is an outer safety bound.
    let measured = measure_with_scope(
        command(args, root, manifest.cases[0].summary),
        Duration::from_secs(args.timeout_sec.saturating_add(15)),
        cancelled,
        true,
    );
    let mut measured = measured.unwrap_or_else(|_| json!({"status":"spawn_error","wall_sec":null}));
    ollama::redact_phase(&mut measured);
    report["coordinator_process"] = measured;
    report["batch_wall_sec"] = json!(batch.elapsed().as_secs_f64());
    let queue_report = fs::read(root.join("queue-report.json"))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).ok())
        .unwrap_or(Value::Null);
    report["queue"] = queue_report;
    let queue = JobQueue::load(root);
    for record in report["runs"].as_array_mut().unwrap() {
        let name = record["session"].as_str().unwrap();
        let session = root.join("sessions").join(name);
        let job = queue.jobs().iter().find(|job| job.session == name);
        let terminal =
            job.is_some_and(|job| matches!(job.state, JobState::Done | JobState::Failed));
        let verification = job
            .map(|job| localvox_light_core::jobs::verify_phase(&session, job).is_ok())
            .unwrap_or(false);
        record["complete"] = json!(terminal);
        record["success"] =
            json!(job.is_some_and(|job| job.state == JobState::Done) && verification);
        record["timeline"] = timeline(&session);
        ollama::redact_timeline(&mut record["timeline"]);
        record["transcript_state"] =
            serde_json::to_value(processing::inspect(&session, processing::TRANSCRIPT).state)?;
        record["summary_state"] =
            serde_json::to_value(processing::inspect(&session, processing::SUMMARY).state)?;
        record["content_unverified"] =
            json!(processing::doubts(&session, processing::SUMMARY).is_some());
        record["rtf"] = Value::Null; // Queue phase wall includes verification/commit, not just worker execution.
    }
    report["llm_trace"] = trace
        .map(|trace| ollama::summarize(trace.since(0), None))
        .unwrap_or(Value::Null);
    report["worker_sha256_after"] = json!(checksum(&args.worker)?);
    report["queue_runner_sha256_after"] = json!(checksum(runner)?);
    let unchanged = report["worker_sha256_after"] == report["worker_sha256"]
        && report["queue_runner_sha256_after"] == report["queue_runner_sha256"];
    report["executables_unchanged"] = json!(unchanged);
    report["complete"] = json!(
        report["queue"]["complete"] == true
            && matches!(
                report["coordinator_process"]["status"].as_str(),
                Some("success" | "failed")
            )
            && unchanged
            && !cancelled.load(Ordering::Relaxed)
            && report["runs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|run| run["complete"] == true)
    );
    report["all_success"] = json!(
        report["complete"] == true
            && report["runs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|r| r["success"] == true)
    );
    report["total_wall_sec"] = json!(started.elapsed().as_secs_f64());
    checkpoint(root, report)?;
    ensure!(
        report["all_success"] == true,
        "queue benchmark incomplete/failed; see report.json"
    );
    Ok(root.join("report.json"))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn queue_fixture(root: &Path) -> Args {
        let mut args = super::super::tests::fixture(root);
        let bin = root.join("bin");
        fs::create_dir(&bin).unwrap();
        let runner = bin.join("queue-runner");
        let worker = bin.join(if cfg!(windows) {
            "localvox-process.exe"
        } else {
            "localvox-process"
        });
        fs::write(&runner, b"never executed").unwrap();
        fs::write(&worker, b"never executed").unwrap();
        args.queue_runner = Some(runner);
        args.worker = worker.canonicalize().unwrap();
        args
    }

    #[test]
    fn mixed_policy_and_worker_mismatch_fail_before_output_creation() {
        let input = tempfile::tempdir().unwrap();
        let mut args = queue_fixture(input.path());
        let mut manifest = read_manifest(&args.manifest).unwrap();
        assert!(validate(&mut args, &manifest).is_ok());
        manifest.cases.push(Case {
            name: "summary".into(),
            audio: manifest.cases[0].audio.clone(),
            summary: true,
        });
        assert!(validate(&mut args, &manifest).is_err());
        manifest.cases.pop();
        args.worker = std::env::current_exe().unwrap().canonicalize().unwrap();
        assert!(validate(&mut args, &manifest).is_err());
        assert!(!args.output.exists());
    }

    #[test]
    fn cancelled_queue_batch_keeps_pending_jobs_and_cannot_be_a_success() {
        let input = tempfile::tempdir().unwrap();
        let args = queue_fixture(input.path());
        let output = args.output.clone();
        assert!(super::super::run(args, &AtomicBool::new(true)).is_err());
        let report: Value =
            serde_json::from_slice(&fs::read(output.join("report.json")).unwrap()).unwrap();
        assert_eq!(report["complete"], false);
        assert_eq!(report["all_success"], false);
        assert_eq!(report["runs"][0]["success"], false);
        assert_eq!(report["runs"][0]["rtf"], Value::Null);
        assert_eq!(JobQueue::load(&output).jobs()[0].state, JobState::Pending);
        assert!(report["coordinator_process"]
            .get("diagnostic_tail")
            .is_none());
    }

    #[test]
    fn staging_creates_verified_pcm_and_real_jobs_without_mutating_source() {
        let input = tempfile::tempdir().unwrap();
        let args = super::super::tests::fixture(input.path());
        let manifest = read_manifest(&args.manifest).unwrap();
        let hash = checksum(&manifest.cases[0].audio).unwrap();
        let root = new_output(&args.output).unwrap();
        let runs = stage(&root, &manifest, 2).unwrap();
        assert_eq!(runs.len(), 2);
        let queue = JobQueue::load(&root);
        assert_eq!(queue.jobs().len(), 2);
        for job in queue.jobs() {
            assert_eq!(job.phase, JobPhase::Prepare);
            assert_eq!(job.state, JobState::Pending);
            artifacts::audio(&root.join("sessions").join(&job.session)).unwrap();
            assert!(!job.summary && !job.cleanup && !job.refine);
        }
        assert_eq!(hash, checksum(&manifest.cases[0].audio).unwrap());
    }
}
