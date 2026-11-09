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
    let mut result = Args::try_parse_from([
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
    .unwrap();
    result.experimental_sparse_edits = args.experimental_sparse_edits;
    result.experimental_sparse_lines = args.experimental_sparse_lines;
    result.trace_ollama = args.trace_ollama;
    result
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
    assert_eq!(report["trace_enabled"], false);
    assert!(report["runs"][0]["llm_trace"].is_null());
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
    text_worker_roundtrip(true);
}

#[test]
#[ignore = "requires LOCALVOX_PROFILE_TEST_WORKER; direct local HTTP, no models/devices"]
fn text_profile_direct_endpoint_preserves_all_modes_without_trace() {
    text_worker_roundtrip(false);
}

fn text_worker_roundtrip(traced: bool) {
    use tiny_http::{Response, Server};
    let root = tempfile::tempdir().unwrap();
    let mut args = fixture(root.path());
    args.worker = std::env::var_os("LOCALVOX_PROFILE_TEST_WORKER")
        .expect("built worker required")
        .into();
    args.timeout_sec = 20;
    args.trace_ollama = traced;
    let server = Server::http("127.0.0.1:0").unwrap();
    args.llm_base_url = format!("http://{}/v1", server.server_addr().to_ip().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let paths = Arc::new(std::sync::Mutex::new(Vec::new()));
    let requested_paths = paths.clone();
    let handle = std::thread::spawn(move || {
        while !stopped.load(Ordering::Relaxed) {
            if let Some(mut req) = server.recv_timeout(Duration::from_millis(50)).unwrap() {
                requested_paths.lock().unwrap().push(req.url().to_owned());
                let body: Value = serde_json::from_reader(req.as_reader()).unwrap();
                let prompt = body["messages"][0]["content"].as_str().unwrap();
                let answer = if prompt.contains("[1] Обсудили") {
                    "[1] Обсудили задачу."
                } else {
                    "Обсудили задачу."
                };
                let reply = if traced {
                    json!({"message":{"content":answer},"done":true,"done_reason":"stop",
                    "load_duration":0,"prompt_eval_duration":1,"eval_duration":1,"total_duration":2,
                    "prompt_eval_count":20,"eval_count":10})
                } else {
                    json!({"choices":[{"message":{"role":"assistant","content":answer}}]})
                };
                req.respond(Response::from_string(reply.to_string()))
                    .unwrap();
            }
        }
    });
    let mut configured = args_for_endpoint(&args);
    let result = run(&mut configured, &AtomicBool::new(false));
    stop.store(true, Ordering::Relaxed);
    handle.join().unwrap();
    result.unwrap();
    assert_eq!(configured.llm_base_url, args.llm_base_url);
    let paths = paths.lock().unwrap();
    assert_eq!(paths.len(), 5);
    assert!(paths.iter().all(|path| path
        == if traced {
            "/api/chat"
        } else {
            "/v1/chat/completions"
        }));
    let raw = fs::read_to_string(args.output.join("report.json")).unwrap();
    let report: Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(report["success"], true);
    assert_eq!(report["input_preserved"], true);
    assert_eq!(report["trace_enabled"], traced);
    let runs = report["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 4);
    for run in runs {
        assert_eq!(run["artifacts_verified"], true);
        if traced {
            assert!(run["llm_trace"]["request_count"].as_u64().unwrap() > 0);
        } else {
            assert!(run["llm_trace"].is_null());
        }
    }
    if traced {
        assert_eq!(runs[2]["llm_trace"]["request_count"], 2);
    }
    assert!(!raw.contains("PRIVATE_SPEAKER") && !raw.contains("Обсудили"));
}

fn args_for_endpoint(args: &Args) -> Args {
    let mut result = args_for(args);
    result.llm_base_url = args.llm_base_url.clone();
    result.timeout_sec = args.timeout_sec;
    result
}

#[test]
fn sparse_experiment_rejects_non_cleanup_modes_before_creating_output() {
    let root = tempfile::tempdir().unwrap();
    let mut args = fixture(root.path());
    args.experimental_sparse_edits = true;
    assert!(crate::run(args_for(&args), &AtomicBool::new(false)).is_err());
    assert!(!args.output.exists());
}

#[test]
#[ignore = "requires LOCALVOX_PROFILE_TEST_WORKER; local synthetic LLM, no models/devices"]
fn sparse_real_worker_keeps_bad_edits_and_malformed_answers_as_originals() {
    sparse_worker_fixture(false);
}

#[test]
#[ignore = "requires LOCALVOX_PROFILE_TEST_WORKER; local synthetic LLM, no models/devices"]
fn sparse_lines_real_worker_keeps_bad_edits_and_malformed_answers_as_originals() {
    sparse_worker_fixture(true);
}

fn sparse_worker_fixture(lines: bool) {
    use tiny_http::{Response, Server};
    let root = tempfile::tempdir().unwrap();
    let mut args = fixture(root.path());
    args.worker = std::env::var_os("LOCALVOX_PROFILE_TEST_WORKER")
        .expect("built worker required")
        .into();
    args.experimental_sparse_edits = !lines;
    args.experimental_sparse_lines = lines;
    args.timeout_sec = 20;
    let texts = [
        "Эээ, проект готов.",
        "Письмо не отправлено.",
        "Бюджет 15 000 рублей.",
    ];
    let bytes=texts.iter().enumerate().map(|(i,text)|json!({"source_id":0,"start_sec":i*10,"end_sec":(i+1)*10,"text":text,"speaker":"PRIVATE_SPEAKER"}).to_string()).collect::<Vec<_>>().join("\n")+"\n";
    fs::write(root.path().join("input.jsonl"), bytes.as_bytes()).unwrap();
    fs::write(
        &args.manifest,
        json!({"version":1,"cases":[
        {"name":"accept","transcript":"input.jsonl","lang":"ru","modes":["cleanup"]},
        {"name":"malformed","transcript":"input.jsonl","lang":"ru","modes":["cleanup"]}]})
        .to_string(),
    )
    .unwrap();
    let server = Server::http("127.0.0.1:0").unwrap();
    args.llm_base_url = format!("http://{}/v1", server.server_addr().to_ip().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let handle = std::thread::spawn(move || {
        let mut calls = 0;
        while !stopped.load(Ordering::Relaxed) {
            if let Some(mut req) = server.recv_timeout(Duration::from_millis(50)).unwrap() {
                let body: Value = serde_json::from_reader(req.as_reader()).unwrap();
                assert!(body["format"].is_object());
                let answer = if calls == 0 && lines {
                    json!({"changes":[{"line":1,"text":"Проект готов."},{"line":2,"text":"Письмо отправлено."},{"line":3,"text":"Бюджет 150 000 рублей."}]}).to_string()
                } else if calls == 0 {
                    json!({"edits":[
                    {"line":1,"before":"Эээ, п","after":"П"},
                    {"line":2,"before":"не ","after":""},
                    {"line":3,"before":"15 000","after":"150 000"}]})
                    .to_string()
                } else {
                    "not JSON".into()
                };
                calls += 1;
                let reply = json!({"message":{"content":answer},"done":true,"done_reason":"stop",
                    "load_duration":0,"prompt_eval_duration":1,"eval_duration":1,"total_duration":2,"prompt_eval_count":20,"eval_count":10});
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
    assert_eq!(report["input_preserved"], true);
    assert_eq!(report["runs"][0]["sparse"]["accepted"], 1);
    assert_eq!(report["runs"][0]["sparse"]["rejected"], 2);
    assert_eq!(report["runs"][1]["sparse"]["protocol_errors"], 1);
    for case in ["accept", "malformed"] {
        let session = args
            .output
            .join("sessions")
            .join(format!("{case}-cleanup-r1"));
        let readable = localvox_light_core::readable::load(&session).unwrap();
        assert!(!readable.edits.contains_key(&1));
        assert!(!readable.edits.contains_key(&2));
        assert_eq!(readable.edits.len(), usize::from(case == "accept"));
        assert_eq!(
            fs::read(session.join("transcripts/v001-profile-source.jsonl")).unwrap(),
            bytes.as_bytes()
        );
    }
    assert_eq!(
        report["runs"][0]["llm_trace"]["requests"][0]["ollama"]["eval_count"],
        10
    );
    for secret in ["PRIVATE_SPEAKER", "проект", "отправлено", "15 000"] {
        assert!(!raw.contains(secret));
    }
}

#[test]
#[ignore = "requires LOCALVOX_PROFILE_TEST_WORKER built with current guard; no models/devices"]
fn retention_real_worker_preserves_semantics_and_line_ownership_in_both_modes() {
    use tiny_http::{Response, Server};
    let root = tempfile::tempdir().unwrap();
    let mut args = fixture(root.path());
    args.worker = std::env::var_os("LOCALVOX_PROFILE_TEST_WORKER")
        .expect("built worker required")
        .into();
    args.timeout_sec = 30;
    args.trace_ollama = true;
    let texts = [
        "Эээ, проект готов.",
        "Проект почти готов.",
        "Оплата пока не подтверждена.",
        "Он сказал «эээ».",
        "Сначала сохрани запись, потом удали файл.",
        "Мы перенесём запуск только после",
        "подтверждения оплаты и успешной проверки резервной копии.",
        "Проект гатов.",
        "Данные обновлены.",
        "Ну, как бы, проект готов.",
        "Бюджет 15 000 рублей.",
        "Письмо не отправлено.",
        "Я собрал со всех истории и выбрал публичные.",
        "Работа шла под влиянием команды.",
        "Команда завершили задачу.",
        "Письмо атправлено.",
    ];
    let bytes=texts.iter().enumerate().map(|(i,text)|json!({"source_id":0,"start_sec":i*10,"end_sec":(i+1)*10,"text":text,"speaker":"PRIVATE_SPEAKER"}).to_string()).collect::<Vec<_>>().join("\n")+"\n";
    fs::write(root.path().join("input.jsonl"), &bytes).unwrap();
    fs::write(&args.manifest,json!({"version":1,"cases":[{"name":"retention","transcript":"input.jsonl","lang":"ru","modes":["refine","cleanup"]}]}).to_string()).unwrap();
    let server = Server::http("127.0.0.1:0").unwrap();
    args.llm_base_url = format!("http://{}/v1", server.server_addr().to_ip().unwrap());
    let stop = Arc::new(AtomicBool::new(false));
    let stopped = stop.clone();
    let handle = std::thread::spawn(move || {
        let answer = "[1] Проект готов.\n[2] Проект готов.\n[3] Оплата не подтверждена.\n[4] Он сказал.\n[5] Сначала сохранил запись, потом удалил файл.\n[6] Мы перенесём запуск только после подтверждения оплаты и успешной проверки резервной копии.\n[7] подтверждения оплаты и успешной проверки резервной копии.\n[8] Проект готов.\n[9] Данные обновлены.\n[10] Проект готов.\n[11] Бюджет 150 000 рублей.\n[12] Письмо отправлено.\n[13] Я собрал со всех историй и выбрал публичные.\n[14] Работа шла под влияние команды.\n[15] Команда завершила задачу.\n[16] Письмо отправлено.\n[9] Данные удалены.";
        while !stopped.load(Ordering::Relaxed) {
            if let Some(req) = server.recv_timeout(Duration::from_millis(50)).unwrap() {
                req.respond(Response::from_string(
                    json!({"message":{"content":answer},"done":true,"eval_count":100}).to_string(),
                ))
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
    assert_eq!(report["input_preserved"], true);
    for mode in ["refine", "cleanup"] {
        let session = args
            .output
            .join("sessions")
            .join(format!("retention-{mode}-r1"));
        assert_eq!(
            fs::read_to_string(session.join("transcripts/v001-profile-source.jsonl")).unwrap(),
            bytes
        );
        let actual = if mode == "cleanup" {
            let processed = localvox_light_core::readable::load(&session).unwrap();
            assert_eq!((processed.rejected, processed.omitted), (10, 0));
            texts
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    processed
                        .edits
                        .get(&i)
                        .cloned()
                        .unwrap_or_else(|| text.to_string())
                })
                .collect::<Vec<_>>()
        } else {
            let store = localvox_light_core::versions::VersionStore::open(&session).unwrap();
            assert_eq!(store.best().unwrap().parents, vec![1]);
            let lines = localvox_light_core::versions::read_transcript_lines(
                &store.resolve(store.best().unwrap().id).unwrap(),
            )
            .unwrap();
            for (i, line) in lines.iter().enumerate() {
                assert_eq!(
                    (line.source_id, line.start_sec, line.end_sec),
                    (0, (i * 10) as f64, ((i + 1) * 10) as f64)
                );
                assert_eq!(line.speaker.as_deref(), Some("PRIVATE_SPEAKER"));
            }
            lines.into_iter().map(|line| line.text).collect()
        };
        assert_eq!(actual.len(), texts.len(), "{mode} must preserve every line");
        for (i, text) in actual.iter().enumerate() {
            assert_eq!(
                text,
                match i {
                    0 | 7 | 9 => "Проект готов.",
                    14 => "Команда завершила задачу.",
                    15 => "Письмо отправлено.",
                    _ => texts[i],
                },
                "{mode} line {i}"
            );
        }
    }
    assert!(report["runs"]
        .as_array()
        .unwrap()
        .iter()
        .all(|run| run["artifacts_verified"] == true));
    assert!(!raw.contains("PRIVATE_SPEAKER"));
}
