use super::*;

fn fixture(root: &Path) -> Args {
    let mut args = crate::tests::fixture(root);
    args.text_only = true;
    fs::write(root.join("input.jsonl"), concat!(
        "{\"source_id\":0,\"start_sec\":0,\"end_sec\":10,\"text\":\"Обсудили задачу.\",\"speaker\":\"PRIVATE_SPEAKER\"}\n"
    )).unwrap();
    fs::write(
        &args.manifest,
        json!({"version":1,"cases":[{
            "name":"sample","transcript":"input.jsonl","lang":"ru",
            "modes":["refine","cleanup","refine-cleanup","summary"]
        }]})
        .to_string(),
    )
    .unwrap();
    args
}

#[test]
fn text_manifest_rejects_bad_paths_modes_and_transcripts_before_creating_output() {
    let root = tempfile::tempdir().unwrap();
    let args = fixture(root.path());
    let valid: Value = serde_json::from_slice(&fs::read(&args.manifest).unwrap()).unwrap();
    for (field, value) in [
        ("name", json!("../escape")),
        ("modes", json!([])),
        ("modes", json!(["refine", "refine"])),
        ("modes", json!(["unknown"])),
        ("transcript", json!("missing.jsonl")),
        ("lang", json!("not-a-language")),
    ] {
        let mut bad = valid.clone();
        bad["cases"][0][field] = value;
        fs::write(&args.manifest, bad.to_string()).unwrap();
        assert!(read_manifest(&args.manifest).is_err(), "{field}");
    }
    fs::write(&args.manifest, valid.to_string()).unwrap();
    for data in ["", "not json", "{\"text\":\"missing timestamps\"}\n"] {
        fs::write(root.path().join("input.jsonl"), data).unwrap();
        assert!(crate::run(args_for(&args), &AtomicBool::new(false)).is_err());
        assert!(!args.output.exists());
    }
}

fn args_for(args: &Args) -> Args {
    Args::try_parse_from([
        "profile",
        "--text-only",
        "--manifest",
        args.manifest.to_str().unwrap(),
        "--worker",
        args.worker.to_str().unwrap(),
        "--model-dir",
        args.model_dir.to_str().unwrap(),
        "--output",
        args.output.to_str().unwrap(),
        "--repeats",
        "1",
        "--timeout-sec",
        "5",
    ])
    .unwrap()
}

#[test]
fn text_seed_is_an_independent_version_and_commands_select_only_the_requested_work() {
    let root = tempfile::tempdir().unwrap();
    let args = fixture(root.path());
    let source = root.path().join("input.jsonl");
    let hash = checksum(&source).unwrap();
    let session = root.path().join("fresh");
    seed(&session, &fs::read(&source).unwrap(), "ru").unwrap();
    let store = VersionStore::open(&session).unwrap();
    assert_eq!(store.best().unwrap().label, "profile-source");
    assert!(!session.join("audio").exists());
    assert!(!session.join("processed.json").exists());
    for (mode, refine, cleanup, phase) in [
        (Mode::Refine, true, false, "text"),
        (Mode::Cleanup, false, true, "text"),
        (Mode::RefineCleanup, true, true, "text"),
        (Mode::Summary, false, false, "summary"),
    ] {
        let command = command(&args, root.path(), &session, mode);
        let argv: Vec<_> = command.get_args().map(|a| a.to_string_lossy()).collect();
        assert_eq!(argv.iter().any(|a| a == "--refine"), refine);
        assert_eq!(argv.iter().any(|a| a == "--cleanup"), cleanup);
        assert!(argv
            .windows(2)
            .any(|a| a[0] == "--worker-phase" && a[1] == phase));
        assert!(command
            .get_envs()
            .all(|(key, _)| !key.to_string_lossy().contains("TOKEN")));
        assert_eq!(command.get_current_dir(), Some(root.path()));
    }
    fs::write(store.resolve(1).unwrap(), "changed copy").unwrap();
    assert_eq!(checksum(&source).unwrap(), hash);
}

#[test]
fn failed_text_worker_retains_a_redacted_checkpoint_and_never_reuses_output() {
    let root = tempfile::tempdir().unwrap();
    let args = fixture(root.path());
    let hash = checksum(&root.path().join("input.jsonl")).unwrap();
    assert!(crate::run(args_for(&args), &AtomicBool::new(false)).is_err());
    let raw = fs::read_to_string(args.output.join("report.json")).unwrap();
    let report: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(report["complete"], false);
    assert_eq!(report["input_preserved"], true);
    assert_eq!(report["runs"].as_array().unwrap().len(), 1);
    assert!(report["runs"][0]["status"] != "success");
    assert!(!raw.contains("PRIVATE_SPEAKER") && !raw.contains("Обсудили"));
    assert!(crate::run(args_for(&args), &AtomicBool::new(false)).is_err());
    assert_eq!(
        fs::read_to_string(args.output.join("report.json")).unwrap(),
        raw
    );
    assert_eq!(checksum(&root.path().join("input.jsonl")).unwrap(), hash);
}

#[test]
#[ignore = "requires LOCALVOX_PROFILE_TEST_WORKER pointing to a built worker; no models/devices"]
fn text_profile_roundtrip_uses_real_workers_and_a_numeric_only_local_llm_fixture() {
    use tiny_http::{Response, Server};
    let root = tempfile::tempdir().unwrap();
    let mut args = fixture(root.path());
    args.worker = std::env::var_os("LOCALVOX_PROFILE_TEST_WORKER")
        .expect("built worker required")
        .into();
    args.timeout_sec = 20;
    let server = Server::http("127.0.0.1:0").unwrap();
    args.llm_base_url = format!("http://{}/v1", server.server_addr().to_ip().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let handle = std::thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            if let Some(mut req) = server.recv_timeout(Duration::from_millis(50)).unwrap() {
                let body: Value = serde_json::from_reader(req.as_reader()).unwrap();
                let prompt = body["messages"][0]["content"].as_str().unwrap();
                let answer = if prompt.contains("[1] Обсудили") {
                    "[1] Обсудили задачу."
                } else {
                    "Обсудили задачу."
                };
                let reply = json!({"message":{"content":answer},"done":true,"done_reason":"stop",
                    "load_duration":0,"prompt_eval_duration":1,"eval_duration":1,"total_duration":2,
                    "prompt_eval_count":20,"eval_count":10});
                req.respond(Response::from_string(reply.to_string()))
                    .unwrap();
            }
        }
    });
    let result = crate::run(args_for_endpoint(&args), &AtomicBool::new(false));
    stop.store(true, Ordering::Relaxed);
    handle.join().unwrap();
    result.unwrap();
    let raw = fs::read_to_string(args.output.join("report.json")).unwrap();
    let report: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(report["success"], true);
    assert_eq!(report["input_preserved"], true);
    let runs = report["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 4);
    for run in runs {
        assert_eq!(run["artifacts_verified"], true);
        assert!(run["llm_trace"]["request_count"].as_u64().unwrap() > 0);
    }
    assert_eq!(runs[2]["llm_trace"]["request_count"], 2);
    assert!(!raw.contains("PRIVATE_SPEAKER") && !raw.contains("Обсудили"));
}

fn args_for_endpoint(args: &Args) -> Args {
    let mut result = args_for(args);
    result.llm_base_url = args.llm_base_url.clone();
    result.timeout_sec = args.timeout_sec;
    result
}
