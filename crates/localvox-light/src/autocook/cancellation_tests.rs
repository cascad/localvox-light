use super::*;

#[test]
#[ignore = "requires built sibling localvox-process; actual worker with a blocked local HTTP fixture, no speech models/devices"]
fn actual_summary_worker_stops_before_api_acknowledges_and_retains_verified_transcript() {
    for action in ["cancel", "delete"] {
        let (root, _) = super::tests::outcome_fixture(true);
        let session = root.path().join("sessions/result");
        let source = localvox_light_core::versions::VersionStore::open(&session).unwrap();
        let transcript = source.resolve(source.best().unwrap().id).unwrap();
        let before = std::fs::read(&transcript).unwrap();
        let canonical_session = session.canonicalize().unwrap();
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let port = server.server_addr().to_ip().unwrap().port();
        let mut cfg = fixture_config(root.path(), port);
        let (observer, events) = Observer::channel(16);
        cfg.observer = Some(observer);
        let (job, owner) = JobQueue::mutate(root.path(), |queue| {
            queue.reclaim_session("result");
            assert!(queue.requeue_derivatives("result", true, false, false));
            let job = queue.start_phase(0, &[JobPhase::Summary]).unwrap();
            let owner = jobs::control::ManagedExecution::register(
                &session,
                &job,
                jobs::try_execution_lock(&session).unwrap().unwrap(),
            );
            (job, owner)
        });
        let running = AtomicBool::new(true);
        std::thread::scope(|scope| {
            struct Stop<'a>(&'a AtomicBool);
            impl Drop for Stop<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Relaxed);
                }
            }
            let _stop = Stop(&running);
            let cfg = &cfg;
            let running = &running;
            let retained = transcript.clone();
            let expected = before.clone();
            let worker = scope.spawn(move || {
                run_phase_controlled(cfg, running, &job, Some(&owner));
                if owner.cancelled() {
                    finish_cancellation(cfg, running, &job);
                }
                assert!(owner.cancelled());
                assert_eq!(std::fs::read(retained).unwrap(), expected);
                drop(owner);
            });
            let request = server
                .recv_timeout(Duration::from_secs(10))
                .unwrap()
                .unwrap_or_else(|| {
                    panic!(
                        "worker did not reach local model: {:?}",
                        JobQueue::load(root.path()).jobs()
                    )
                });
            assert!(request.url().ends_with("/api/chat"));
            let archive = localvox_light_api::archive::Archive::new(root.path().to_path_buf());
            if action == "delete" {
                archive.delete_session("result", "result").unwrap();
            } else {
                archive.cancel("result").unwrap();
            }
            drop(request);
            worker.join().unwrap();
        });
        let stopped = events
            .try_iter()
            .find(|event| event.kind == "cancelled")
            .unwrap();
        assert!(!stopped.after_has_error);
        assert!(jobs::control::managed_owner(&canonical_session).is_none());
        if action == "delete" {
            assert!(!session.exists());
            assert!(JobQueue::load(root.path()).jobs().is_empty());
            continue;
        }
        assert_eq!(std::fs::read(transcript).unwrap(), before);
        assert!(std::fs::read_to_string(session.join("progress.jsonl"))
            .unwrap()
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .any(|event| event["kind"] == "operation_end" && event["status"] == "cancelled"));
        assert!(localvox_light_core::processing::inspect(
            &session,
            localvox_light_core::processing::TRANSCRIPT
        )
        .complete());
        assert!(!localvox_light_core::processing::inspect(
            &session,
            localvox_light_core::processing::SUMMARY
        )
        .complete());
        let queue = JobQueue::load(root.path());
        assert_eq!(queue.jobs()[0].state, JobState::Cancelled);
        assert!(jobs::try_execution_lock(&session).unwrap().is_some());
        assert!(jobs::control::managed_owner(&session).is_none());
        assert_eq!(stopped.ended_at, queue.jobs()[0].finished_at);
        assert!(!stopped.after_has_error);
    }
}

fn fixture_config(root: &std::path::Path, port: u16) -> Config {
    let worker_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut cfg = Config::from_env_in(root.to_string_lossy().into_owned(), &worker_dir).unwrap();
    cfg.model_dir = root.join("missing-asr");
    cfg.llm_model = "fixture".into();
    cfg.summary_provider = "ollama".into();
    cfg.summary_template = None;
    cfg.child_env.retain(|(key, _)| {
        matches!(
            key.to_str(),
            Some(
                "PATH"
                    | "SystemRoot"
                    | "WINDIR"
                    | "COMSPEC"
                    | "PATHEXT"
                    | "LD_LIBRARY_PATH"
                    | "DYLD_LIBRARY_PATH"
            )
        )
    });
    std::fs::create_dir(root.join("glossary")).unwrap();
    cfg.child_env.extend([
        ("TEMP".into(), root.as_os_str().to_owned()),
        ("TMP".into(), root.as_os_str().to_owned()),
        (
            "LOCALVOX_NER_MODEL_DIR".into(),
            root.join("missing-ner").into_os_string(),
        ),
        (
            "LOCALVOX_LLM_GLOSSARY_DIR".into(),
            root.join("glossary").into_os_string(),
        ),
        (
            "LOCALVOX_LLM_BASE_URL".into(),
            format!("http://127.0.0.1:{port}/:11434/v1").into(),
        ),
    ]);
    cfg
}

#[test]
#[ignore = "requires built sibling localvox-process; standalone CLI against blocked local HTTP, no models/devices"]
fn standalone_cli_cannot_be_cancelled_or_deleted_as_a_managed_worker() {
    let (root, _) = super::tests::outcome_fixture(true);
    let session = root.path().join("sessions/result");
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let cfg = fixture_config(root.path(), server.server_addr().to_ip().unwrap().port());
    let mut command = cfg.command();
    command
        .current_dir(root.path())
        .arg(&session)
        .args([
            "--post-only",
            "--summary",
            "--summary-provider",
            "ollama",
            "--llm-model",
            "fixture",
        ])
        .arg("--work-dir")
        .arg(root.path());
    let running = AtomicBool::new(true);
    std::thread::scope(|scope| {
        struct Stop<'a>(&'a AtomicBool);
        impl Drop for Stop<'_> {
            fn drop(&mut self) {
                self.0.store(false, Ordering::Relaxed);
            }
        }
        let _stop = Stop(&running);
        let running = &running;
        let child = scope.spawn(move || {
            localvox_light_process::run(
                command,
                None,
                Limits::new(Duration::from_secs(15)),
                || !running.load(Ordering::Relaxed),
                |_, _| {},
            )
            .unwrap()
        });
        let request = server
            .recv_timeout(Duration::from_secs(10))
            .unwrap()
            .expect("standalone CLI did not reach the local model");
        assert!(request.url().ends_with("/api/chat"));
        assert!(jobs::try_execution_lock(&session).unwrap().is_none());
        assert!(jobs::control::managed_owner(&session).is_none());
        let before = std::fs::read(root.path().join("jobs.json")).unwrap();
        let store = localvox_light_core::versions::VersionStore::open(&session).unwrap();
        let source = store.resolve(store.best().unwrap().id).unwrap();
        let transcript = std::fs::read(&source).unwrap();
        let archive = localvox_light_api::archive::Archive::new(root.path().to_path_buf());
        assert!(archive
            .cancel("result")
            .unwrap_err()
            .to_string()
            .contains("CLI"));
        assert!(archive
            .delete_session("result", "result")
            .unwrap_err()
            .to_string()
            .contains("CLI"));
        assert_eq!(
            std::fs::read(root.path().join("jobs.json")).unwrap(),
            before
        );
        assert_eq!(std::fs::read(&source).unwrap(), transcript);
        running.store(false, Ordering::Relaxed);
        drop(request);
        assert!(matches!(
            child.join().unwrap().termination,
            Termination::Cancelled
        ));
    });
    assert!(jobs::try_execution_lock(&session).unwrap().is_some());
}
