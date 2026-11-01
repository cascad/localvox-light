use super::*;

fn wav(path: &Path, channels: u16, frames: usize) {
    let mut writer = hound::WavWriter::create(
        path,
        hound::WavSpec {
            channels,
            sample_rate: 16_000,
            bits_per_sample: 16,
            sample_format: hound::SampleFormat::Int,
        },
    )
    .unwrap();
    for _ in 0..frames * channels as usize {
        writer.write_sample(0i16).unwrap();
    }
    writer.finalize().unwrap();
}

fn fixture(root: &Path) -> Args {
    wav(&root.join("source.wav"), 1, 1600);
    fs::write(
        root.join("manifest.json"),
        r#"{"version":1,"label":"test","cases":[{"name":"one","audio":"source.wav"}]}"#,
    )
    .unwrap();
    Args {
        manifest: root.join("manifest.json"),
        worker: std::env::current_exe().unwrap(),
        model_dir: root.into(),
        output: root.join("output"),
        repeats: 1,
        timeout_sec: 5,
        llm_base_url: "http://127.0.0.1:11434/v1".into(),
        llm_model: "test".into(),
        trace_ollama: false,
        ner_model_dir: None,
    }
}

#[test]
fn manifest_validation_prevents_path_escape_duplicates_and_invalid_audio() {
    let root = tempfile::tempdir().unwrap();
    let args = fixture(root.path());
    assert_eq!(audio_seconds(&root.path().join("source.wav")).unwrap(), 0.1);
    let hash = checksum(&root.path().join("source.wav")).unwrap();
    for cases in [
        json!([{"name":"../outside","audio":"source.wav"}]),
        json!([{"name":"same","audio":"source.wav"},{"name":"same","audio":"source.wav"}]),
        json!([{"name":"missing","audio":"missing.wav"}]),
    ] {
        fs::write(
            &args.manifest,
            json!({"version":1,"label":"bad","cases":cases}).to_string(),
        )
        .unwrap();
        assert!(read_manifest(&args.manifest).is_err());
    }
    wav(&root.path().join("stereo.wav"), 2, 10);
    wav(&root.path().join("empty.wav"), 1, 0);
    assert!(audio_seconds(&root.path().join("stereo.wav")).is_err());
    assert!(audio_seconds(&root.path().join("empty.wav")).is_err());
    assert_eq!(checksum(&root.path().join("source.wav")).unwrap(), hash);
}

#[test]
fn endpoints_are_explicit_local_http_and_output_is_never_reused() {
    for url in [
        "http://127.0.0.1:11434/v1",
        "http://localhost:1234/v1",
        "http://[::1]:88/v1",
    ] {
        assert!(local_endpoint(url), "{url}");
    }
    for url in [
        "https://127.0.0.1:9",
        "http://cloud.example:80/v1",
        "http://127.0.0.1.evil:9",
        "http://user@localhost:9",
        "http://localhost:0",
        "http://localhost:9/v1?token=x",
    ] {
        assert!(!local_endpoint(url), "{url}");
    }
    let root = tempfile::tempdir().unwrap();
    fs::write(root.path().join("sentinel"), "keep").unwrap();
    assert!(new_output(root.path()).is_err());
    assert_eq!(
        fs::read_to_string(root.path().join("sentinel")).unwrap(),
        "keep"
    );
}

#[test]
fn a_failed_worker_keeps_a_checkpoint_and_does_not_become_a_fast_success() {
    let root = tempfile::tempdir().unwrap();
    let args = fixture(root.path());
    let before = checksum(&root.path().join("source.wav")).unwrap();
    // The test harness executable rejects worker arguments: a real nonzero process exit.
    assert!(run(args, &AtomicBool::new(false)).is_err());
    let report: Value =
        serde_json::from_slice(&fs::read(root.path().join("output/report.json")).unwrap()).unwrap();
    assert_eq!(report["complete"], true);
    assert_eq!(report["all_success"], false);
    assert!(report["model_files"]
        .as_array()
        .unwrap()
        .iter()
        .all(|file| file["file"].is_string()));
    assert_eq!(
        report["runner_sha256"],
        checksum(&std::env::current_exe().unwrap()).unwrap()
    );
    let meta: localvox_light_core::chunks::SessionMeta = serde_json::from_slice(
        &fs::read(root.path().join("output/sessions/one-r1/meta.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(meta.lang.as_deref(), Some("ru"));
    assert!(meta.stopped_at.is_some());
    assert_eq!(report["runs"][0]["phases"][0]["status"], "failed");
    assert!(report["runs"][0]["rtf"].is_null());
    assert!(report["runs"][0]["worker_wall_sec"].is_null());
    assert!(report["queue_wait_sec"].is_null());
    assert_eq!(checksum(&root.path().join("source.wav")).unwrap(), before);
    assert_eq!(
        checksum(
            &root
                .path()
                .join("output/sessions/one-r1/audio/src0_chunk000000.wav")
        )
        .unwrap(),
        before
    );
}

#[test]
fn cancellation_writes_an_incomplete_report_without_starting_a_worker() {
    let root = tempfile::tempdir().unwrap();
    assert!(run(fixture(root.path()), &AtomicBool::new(true)).is_err());
    let report: Value =
        serde_json::from_slice(&fs::read(root.path().join("output/report.json")).unwrap()).unwrap();
    assert_eq!(report["complete"], false);
    assert_eq!(report["runs"], json!([]));
}

#[test]
fn traced_failures_omit_worker_diagnostic_content_and_pin_ner_environment() {
    let root = tempfile::tempdir().unwrap();
    let mut args = fixture(root.path());
    args.trace_ollama = true;
    let command = worker_command(&args, root.path(), root.path(), JobPhase::Summary);
    let ner = command
        .get_envs()
        .find(|(key, _)| *key == "LOCALVOX_NER_MODEL_DIR")
        .unwrap()
        .1
        .unwrap();
    assert_eq!(Path::new(ner), root.path().join("missing-ner"));
    assert!(run(args, &AtomicBool::new(false)).is_err());
    let report: Value =
        serde_json::from_slice(&fs::read(root.path().join("output/report.json")).unwrap()).unwrap();
    let phase = &report["runs"][0]["phases"][0];
    assert!(phase["diagnostic_tail"].is_null());
    assert_eq!(phase["diagnostic_content_omitted"], true);
}

#[test]
fn timeline_retains_real_boundaries_nested_operations_and_unknowns() {
    let root = tempfile::tempdir().unwrap();
    let mut lines = Vec::new();
    for (sec, state) in [(0, "running"), (1, "running"), (3, "done")] {
        lines.push(
            json!({"stage":"transcribe","state":state,"at":format!("2026-11-01T00:00:0{sec}Z")}),
        );
    }
    for (name, elapsed) in [("parent", 2.0), ("child", 1.0)] {
        lines.push(
            json!({"journal":1,"kind":"operation_end","operation":name,"elapsed_sec":elapsed}),
        );
    }
    let text = lines
        .iter()
        .map(Value::to_string)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n{broken";
    fs::write(root.path().join("progress.jsonl"), text).unwrap();
    let data = timeline(root.path());
    assert_eq!(data["stages"][0]["elapsed_sec"], 3.0);
    assert_eq!(data["operations"].as_array().unwrap().len(), 2);
    assert_eq!(data["operation_durations_are_nested_not_additive"], true);
    assert_eq!(data["malformed_records"], 1);
    assert_eq!(data["incomplete_tail"], true);
    assert!(data["model_load_sec"].is_null());
    assert!(data["generation_sec"].is_null());
    let empty = tempfile::tempdir().unwrap();
    assert_eq!(timeline(empty.path())["history_status"], "missing");
    assert_eq!(timeline(empty.path())["stages"], json!([]));
}

fn child_command(mode: &str) -> Command {
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "tests::profile_child",
            "--nocapture",
        ])
        .env("LOCALVOX_PROFILE_TEST_CHILD", mode);
    command
}

#[test]
fn real_child_failure_and_timeout_are_reported_separately() {
    let cancelled = AtomicBool::new(false);
    let failed = measure(child_command("fail"), Duration::from_secs(5), &cancelled).unwrap();
    assert_eq!(failed["status"], "failed");
    let timeout = measure(
        child_command("sleep"),
        Duration::from_millis(150),
        &cancelled,
    )
    .unwrap();
    assert_eq!(timeout["status"], "timeout");
    assert!(timeout["wall_sec"].as_f64().unwrap() < 5.0);
}

#[cfg(windows)]
#[test]
fn windows_reports_observed_peak_and_cpu_after_the_child_exits() {
    let result = measure(
        child_command("memory"),
        Duration::from_secs(5),
        &AtomicBool::new(false),
    )
    .unwrap();
    assert_eq!(result["status"], "success");
    let usage = &result["resources"];
    assert_eq!(usage["availability"], "observed");
    assert!(usage["peak_working_set_bytes"].as_u64().unwrap() >= 32 * 1024 * 1024);
    assert!(usage["cpu_sec"].as_f64().unwrap() >= 0.0);
    assert_eq!(usage["scope"], "worker_process_only");
}

#[test]
#[ignore = "subprocess fixture; launched explicitly by profiler tests"]
fn profile_child() {
    match std::env::var("LOCALVOX_PROFILE_TEST_CHILD").as_deref() {
        Ok("fail") => panic!("synthetic child failure"),
        Ok("sleep") => std::thread::sleep(Duration::from_secs(10)),
        Ok("memory") => {
            let mut memory = vec![0u8; 32 * 1024 * 1024];
            for byte in &mut memory {
                *byte = std::hint::black_box(42);
            }
            let clock = Instant::now();
            while clock.elapsed() < Duration::from_millis(200) {
                std::hint::black_box(&memory);
            }
            std::thread::sleep(Duration::from_millis(200));
            std::hint::black_box(memory);
        }
        _ => panic!("fixture must be launched by a parent test"),
    }
}
