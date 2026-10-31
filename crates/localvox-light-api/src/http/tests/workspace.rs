//! The real client against real HTTP/archive operations, without a running engine or models.
use super::*;
use localvox_light_client::{AskRequest, Client, Scope};
use localvox_light_core::{
    jobs, readable,
    versions::{VersionEntry, VersionStore},
};

pub(super) fn session(root: &Path, name: &str) -> Vec<std::path::PathBuf> {
    let dir = root.join("sessions").join(name);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("meta.json"),json!({"started_at":"2026-10-31T07:00:00Z","sample_rate":16000,"title":"Тестовая встреча","chunks":[]}).to_string()).unwrap();
    let store = VersionStore::open(&dir).unwrap();
    fs::write(dir.join("speakers.json"),json!({"members":[{"id":0,"label":"Участник 1","embedding":[1.0,0.0],"speech_sec":1.5,"owner":false,"manually_named":false}]}).to_string()).unwrap();
    let mut files = Vec::new();
    for _ in 0..2 {
        let (id, path) = store.next_version("synthetic").unwrap();
        fs::write(&path,json!({"source_id":0,"start_sec":0.5,"end_sec":2.0,"text":"Согласовали архитектуру клиента","speaker":"Участник 1"}).to_string()+"\n").unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "synthetic".into(),
                file: path.file_name().unwrap().to_str().unwrap().into(),
                model: "fixture".into(),
                params: json!({}),
                created_at: "2026-10-31T07:00:00Z".into(),
                parents: vec![],
            })
            .unwrap();
        files.push(path);
    }
    readable::save(
        &dir,
        &readable::Readable {
            version_id: 2,
            edits: [(0, "Согласовали архитектуру клиента.".into())]
                .into_iter()
                .collect(),
            ..Default::default()
        },
    )
    .unwrap();
    fs::write(
        dir.join("summary.md"),
        "## Решение\nАрхитектура клиента согласована. [[0]]",
    )
    .unwrap();
    files
}

#[test]
fn parallel_short_imports_complete_once_and_release_their_connections() {
    let directory = tempfile::tempdir().unwrap();
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    std::thread::scope(|scope| {
        let mut workers = vec![];
        for lane in 0..4 {
            let address = server.address.clone();
            workers.push(scope.spawn(move || {
                let client = Client::new(&address, "synthetic-token".into()).unwrap();
                (0..16)
                    .map(|index| {
                        client
                            .ingest(&format!("https://example.test/short-{lane}-{index}"))
                            .unwrap()
                            .session
                            .unwrap()
                    })
                    .collect::<Vec<_>>()
            }));
        }
        let names = workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(names.len(), 64);
    });
    let queue = jobs::JobQueue::load(directory.path());
    assert_eq!(
        queue.jobs().len(),
        64,
        "mutating requests must not be replayed"
    );
    assert_eq!(
        fs::read_dir(directory.path().join("sessions"))
            .unwrap()
            .count(),
        64
    );
}

#[test]
fn raw_transcript_is_readable_without_cleanup_through_the_real_api() {
    let directory = tempfile::tempdir().unwrap();
    session(directory.path(), "raw-only");
    let dir = directory.path().join("sessions/raw-only");
    fs::remove_file(dir.join("processed.json")).unwrap();
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    let transcript = client.transcript("raw-only").unwrap();
    let readable = client.readable("raw-only").unwrap();
    assert_eq!(transcript.len(), readable.lines.len());
    assert!(readable.from_transcript && readable.provenance.is_none());
    for (raw, text) in transcript.iter().zip(&readable.lines) {
        assert_eq!(raw.text, text.text);
        assert_eq!(
            (raw.start_sec, raw.end_sec, &raw.who),
            (text.start_sec, text.end_sec, &text.who)
        );
        assert!(text.original.is_none());
    }
    assert!(!dir.join("processed.json").exists());
    fs::write(dir.join("processed.json"), "{broken").unwrap();
    assert!(client.readable("raw-only").is_err());
}

#[test]
fn progress_explains_missing_job_and_corrupt_queue_after_stt() {
    let directory = tempfile::tempdir().unwrap();
    session(directory.path(), "raw-only");
    let dir = directory.path().join("sessions/raw-only");
    fs::remove_file(dir.join("summary.md")).unwrap();
    fs::remove_file(dir.join("processed.json")).unwrap();
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    let state = client.progress("raw-only").unwrap();
    assert_eq!(state.execution.unwrap().state, "untracked");
    fs::write(directory.path().join("jobs.json"), "{broken").unwrap();
    let state = client.progress("raw-only").unwrap();
    assert!(state.queue_error.is_some());
    assert_eq!(state.execution.unwrap().state, "unavailable");
}

#[test]
fn document_actions_preserve_transcripts_and_enforce_worker_ownership_through_http() {
    let directory = tempfile::tempdir().unwrap();
    let files = session(directory.path(), "demo");
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    assert!(client.health().unwrap().compatible());
    assert_eq!(client.sessions().unwrap()[0].name, "demo");
    assert_eq!(client.jobs().unwrap().running, 0);
    assert!(client.langs().is_ok());
    assert_eq!(client.versions("demo").unwrap().len(), 2);
    assert_eq!(
        client.transcript("demo").unwrap()[0].text,
        "Согласовали архитектуру клиента."
    );
    assert!(!client.summary("demo").unwrap().blocks.is_empty());
    assert!(!client.readable("demo").unwrap().lines.is_empty());
    let original = files
        .iter()
        .map(|p| fs::read(p).unwrap())
        .collect::<Vec<_>>();
    client.set_best("demo", 1).unwrap();
    assert!(client
        .versions("demo")
        .unwrap()
        .iter()
        .any(|v| v.best && v.id == 1));
    assert!(
        !directory.path().join("jobs.json").exists(),
        "best is not an LLM request"
    );
    for (path, bytes) in files.iter().zip(&original) {
        assert_eq!(fs::read(path).unwrap(), *bytes);
    }
    client.set_best("demo", 2).unwrap();
    let doc = directory.path().join("sessions/demo/processed.json");
    let readable_before = fs::read(&doc).unwrap();
    client.name_source("demo", 0, "Лектор").unwrap();
    assert_eq!(client.sources("demo").unwrap()[0].name, "Лектор");
    client.name_source("demo", 0, "").unwrap();
    assert!(!client.sources("demo").unwrap()[0].custom);
    assert_eq!(fs::read(&doc).unwrap(), readable_before);
    client.name_speaker("demo", "Участник 1", "Анна").unwrap();
    assert!(client
        .speakers("demo")
        .unwrap()
        .iter()
        .any(|p| p.name == "Анна"));
    for (path, before) in files.iter().zip(&original) {
        let mut before: Value = serde_json::from_slice(before).unwrap();
        let mut after: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
        before.as_object_mut().unwrap().remove("speaker");
        after.as_object_mut().unwrap().remove("speaker");
        assert_eq!(before, after, "renaming must preserve speech and timecodes");
    }
    let dir = directory.path().join("sessions/demo");
    let lease = jobs::try_execution_lock(&dir).unwrap().unwrap();
    assert!(client.set_best("demo", 1).is_err());
    assert!(client.name_source("demo", 0, "Занято").is_err());
    assert!(client.set_lang("demo", "en").is_err());
    assert!(client.recook("demo", Scope::Summary).is_err());
    drop(lease);
    let meta_before = fs::read(dir.join("meta.json")).unwrap();
    assert!(
        client.set_lang("demo", "en").is_err(),
        "missing audio/model must fail before invalidation"
    );
    assert_eq!(fs::read(dir.join("meta.json")).unwrap(), meta_before);
    assert_eq!(fs::read(&doc).unwrap(), readable_before);
    assert!(
        client.recook("demo", Scope::All).is_err(),
        "no source audio"
    );
    client.recook("demo", Scope::Summary).unwrap();
    assert_eq!(
        jobs::JobQueue::load(directory.path()).jobs()[0].phase,
        jobs::JobPhase::Summary
    );
    assert!(doc.exists(), "summary-only keeps readable text");
    client.recook("demo", Scope::Text).unwrap();
    assert_eq!(
        jobs::JobQueue::load(directory.path()).jobs()[0].phase,
        jobs::JobPhase::Text
    );
    assert!(files.iter().all(|p| p.exists()));
    let queue = client.queue().unwrap();
    assert_eq!(queue[0].session, "demo");
    let imported = client
        .ingest("https://example.org/synthetic-video")
        .unwrap()
        .session
        .unwrap();
    assert!(!imported.is_empty());
    let queue = client.queue().unwrap();
    assert_eq!(queue[0].session, "demo");
    assert!(queue.iter().any(|q| q.session == imported));
    assert!(!client.record().unwrap().engine);
}

#[test]
fn notes_and_asks_roundtrip_with_explicit_targets_and_provider() {
    let _env = crate::SLOTS_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    struct Env(Option<std::ffi::OsString>);
    impl Drop for Env {
        fn drop(&mut self) {
            if let Some(value) = &self.0 {
                std::env::set_var("LOCALVOX_SLOTS_CONFIG", value);
            } else {
                std::env::remove_var("LOCALVOX_SLOTS_CONFIG");
            }
        }
    }
    let _restore = Env(std::env::var_os("LOCALVOX_SLOTS_CONFIG"));
    let directory = tempfile::tempdir().unwrap();
    let config = directory.path().join("slots.toml");
    let note_path = directory.path().join("ideas.md");
    fs::write(
        &config,
        format!(
            "[slots.\"идеи\"]\npath = \"{}\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n",
            note_path.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    std::env::set_var("LOCALVOX_SLOTS_CONFIG", &config);
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    assert_eq!(client.slots().unwrap(), vec!["идеи"]);
    client.note("Проверить диапазоны 🦀", "").unwrap();
    let slots = client.slot_notes().unwrap();
    assert!(slots[0].readable);
    let note = &slots[0].notes[0];
    assert_eq!(note.text, "Проверить диапазоны 🦀");
    client.delete_note("идеи", note).unwrap();
    assert!(client.slot_notes().unwrap()[0].notes.is_empty());
    // An actual destination failure must remain an HTTP error; unreadable is
    // different from a readable slot containing no notes. Only a temp path.
    let blocked = directory.path().join("blocked");
    fs::write(&blocked, "a file cannot be a destination directory").unwrap();
    fs::write(
        &config,
        format!(
            "[slots.\"идеи\"]\npath = \"{}\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n",
            blocked
                .join("ideas.md")
                .to_string_lossy()
                .replace('\\', "/")
        ),
    )
    .unwrap();
    assert!(client.note("Сохранить мой черновик", "идеи").is_err());
    fs::write(
        &config,
        format!(
            "[slots.\"идеи\"]\npath = \"{}\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n",
            note_path.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    fs::write(&note_path, [0xff, 0xfe]).unwrap();
    let unavailable = client.slot_notes().unwrap();
    assert!(unavailable[0].error.is_some());
    assert!(unavailable[0].notes.is_empty());
    let created = client
        .create_ask(&AskRequest {
            text: "Синтетический материал 🦀".into(),
            provider: "ollama".into(),
            prompt: "Выдели решение".into(),
            input_name: Some("встреча.txt".into()),
        })
        .unwrap();
    assert_eq!(created.status, "pending");
    assert_eq!(created.provider, "ollama");
    let record = client.get_ask(&created.id).unwrap();
    assert_eq!(record.input.as_deref(), Some("Синтетический материал 🦀"));
    assert_eq!(record.input_name.as_deref(), Some("встреча.txt"));
    assert_eq!(client.asks().unwrap()[0].id, created.id);
    client.delete_ask(&created.id).unwrap();
    assert!(client.asks().unwrap().is_empty());
}

#[test]
fn terminal_ask_file_validates_before_post_and_reads_saved_status_transitions() {
    use localvox_light_core::asks::{self, AskStatus};
    use localvox_light_terminal::io::{perform, Data, Task};
    let directory = tempfile::tempdir().unwrap();
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    let file = directory.path().join("встреча.txt");
    let task = || Task::CreateAskFile {
        path: file.to_string_lossy().into(),
        provider: "ollama".into(),
        prompt: "Выдели решение".into(),
        input_name: String::new(),
    };
    assert!(perform(&client, task()).is_err());
    for bytes in [
        vec![0xff, 0xfe],
        vec![],
        vec![b'a'; 1024 * 1024 + 1],
        vec![b'a'; 400_001],
    ] {
        fs::write(&file, bytes).unwrap();
        assert!(perform(&client, task()).is_err());
        assert!(client.asks().unwrap().is_empty());
    }
    fs::write(&file, "\u{feff}Русский текст 🦀\nВторая строка").unwrap();
    let Data::Ask(created) = perform(&client, task()).unwrap() else {
        panic!("expected a saved ask")
    };
    assert_eq!(created.input_name.as_deref(), Some("встреча.txt"));
    assert_eq!(created.provider, "ollama");
    assert_eq!(created.status, "pending");
    let saved = client.get_ask(&created.id).unwrap();
    assert_eq!(
        saved.input.as_deref(),
        Some("Русский текст 🦀\nВторая строка")
    );
    let root = directory.path().join(asks::DIR);
    let mut ask = asks::load(&root, &created.id).unwrap();
    for (state, label) in [
        (AskStatus::Running, "running"),
        (AskStatus::Done, "done"),
        (AskStatus::Failed, "failed"),
    ] {
        ask.status = state;
        ask.answer = if state == AskStatus::Done {
            Some("Синтетический ответ".into())
        } else {
            None
        };
        ask.error = if state == AskStatus::Failed {
            Some("synthetic provider failure".into())
        } else {
            None
        };
        asks::update(&root, &ask).unwrap();
        let current = client.get_ask(&created.id).unwrap();
        assert_eq!(current.status, label);
        assert_eq!(current.answer, ask.answer);
        assert_eq!(current.error, ask.error);
        assert_eq!(
            client.asks().unwrap().len(),
            1,
            "status reads never repeat POST"
        );
    }
}

#[test]
fn confirmation_http_propagates_integrity_failures_instead_of_claiming_no_doubt() {
    use localvox_light_core::processing::{self, Outcome};
    let directory = tempfile::tempdir().unwrap();
    session(directory.path(), "demo");
    let dir = directory.path().join("sessions/demo");
    let summary = fs::read(dir.join("summary.md")).unwrap();
    processing::record(
        &dir,
        processing::SUMMARY,
        "synthetic",
        Outcome::Unverified,
        Some("Проверьте формулировку".into()),
        Some(2),
    )
    .unwrap();
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    let lease = jobs::try_execution_lock(&dir).unwrap().unwrap();
    assert!(client.confirm("demo", "summary").is_err());
    drop(lease);
    fs::write(dir.join("summary.md"), "changed after validation").unwrap();
    assert!(client.confirm("demo", "summary").is_err());
    assert!(processing::doubts(&dir, processing::SUMMARY).is_some());
    fs::write(dir.join("summary.md"), summary).unwrap();
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        let handle = fs::OpenOptions::new()
            .read(true)
            .share_mode(1)
            .open(dir.join("processing.json"))
            .unwrap();
        assert!(client.confirm("demo", "summary").is_err());
        assert!(processing::doubts(&dir, processing::SUMMARY).is_some());
        drop(handle);
    }
    client.confirm("demo", "summary").unwrap();
    assert!(processing::doubts(&dir, processing::SUMMARY).is_none());
    assert!(client
        .confirm("demo", "summary")
        .unwrap()
        .msg
        .unwrap()
        .contains("и так не было"));
}

#[test]
fn session_deletion_requires_its_named_confirmation_and_a_free_worker() {
    let directory = tempfile::tempdir().unwrap();
    session(directory.path(), "demo");
    session(directory.path(), "other");
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    assert!(client
        .request::<Value>(
            "DELETE",
            &["api", "sessions", "demo"],
            &[],
            Some(json!({"confirm":"other"})),
            std::time::Duration::from_secs(3)
        )
        .is_err());
    let dir = directory.path().join("sessions/demo");
    assert!(dir.exists());
    let lease = jobs::try_execution_lock(&dir).unwrap().unwrap();
    assert!(client.delete_session("demo").is_err());
    assert!(dir.exists());
    drop(lease);
    assert_eq!(
        client.delete_session("demo").unwrap().deleted.as_deref(),
        Some("demo")
    );
    assert!(!dir.exists());
    assert_eq!(client.sessions().unwrap()[0].name, "other");
}

#[test]
fn search_modes_and_chat_do_not_turn_two_channel_failures_into_no_results() {
    let directory = tempfile::tempdir().unwrap();
    // Both indexes fail before any embedding call, independent of machine settings.
    fs::write(directory.path().join("index"), "not a directory").unwrap();
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    for mode in ["lexical", "semantic", "hybrid"] {
        assert!(client.search(" ", mode).unwrap().is_empty());
        assert!(client.search("архитектура", mode).is_err());
    }
    assert!(
        client.ask("архитектура").is_err(),
        "an unavailable search must not become a no-information answer"
    );
}

#[test]
fn hybrid_returns_lexical_hits_when_the_semantic_index_is_unavailable() {
    let directory = tempfile::tempdir().unwrap();
    session(directory.path(), "demo");
    fs::create_dir_all(directory.path().join("index/semantic.lock")).unwrap();
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    let hits = client.search("архитектура", "hybrid").unwrap();
    assert!(!hits.is_empty());
    assert!(hits.iter().all(|h| h.session == "demo"));
    assert!(hits
        .iter()
        .any(|h| h.start_sec.is_some() && !h.matched.is_empty()));
    assert!(client.search("архитектура", "semantic").is_err());
}

#[test]
fn grounded_chat_and_route_keep_sources_and_destinations_explicit_with_a_mock_model() {
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;
    let _env = crate::SLOTS_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    struct Env(Vec<(&'static str, Option<std::ffi::OsString>)>);
    impl Drop for Env {
        fn drop(&mut self) {
            for (key, value) in &self.0 {
                if let Some(value) = value {
                    std::env::set_var(key, value);
                } else {
                    std::env::remove_var(key);
                }
            }
        }
    }
    let _restore = Env([
        "LOCALVOX_LLM_BASE_URL",
        "LOCALVOX_EMBED_BASE_URL",
        "LOCALVOX_SLOTS_CONFIG",
    ]
    .into_iter()
    .map(|key| (key, std::env::var_os(key)))
    .collect());
    let directory = tempfile::tempdir().unwrap();
    session(directory.path(), "demo");
    fs::create_dir_all(directory.path().join("index/semantic.lock")).unwrap();
    let note = directory.path().join("ideas.md");
    let config = directory.path().join("slots.toml");
    fs::write(
        &config,
        format!(
            "[slots.\"идеи\"]\npath = \"{}\"\ntemplate = \"- {{{{text}}}}\"\ndefault = true\n",
            note.to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    std::env::set_var("LOCALVOX_SLOTS_CONFIG", config);
    let model = Server::http("127.0.0.1:0").unwrap();
    std::env::set_var(
        "LOCALVOX_LLM_BASE_URL",
        format!("http://{}/v1", model.server_addr()),
    );
    std::env::set_var(
        "LOCALVOX_EMBED_BASE_URL",
        format!("http://{}", model.server_addr()),
    );
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let calls = Arc::new(AtomicUsize::new(0));
    let called = calls.clone();
    let fail = Arc::new(AtomicBool::new(false));
    let failing = fail.clone();
    let thread = std::thread::spawn(move || {
        while !stopping.load(Ordering::Relaxed) {
            if let Some(mut request) = model.recv_timeout(Duration::from_millis(50)).unwrap() {
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                called.fetch_add(1, Ordering::Relaxed);
                if failing.load(Ordering::Relaxed) {
                    request
                        .respond(
                            Response::from_string("synthetic unavailable model")
                                .with_status_code(503),
                        )
                        .unwrap();
                    continue;
                }
                if request.url() == "/api/embed" {
                    let input: Value = serde_json::from_str(&body).unwrap();
                    let vectors =
                        vec![vec![1.0, 0.0, 0.0]; input["input"].as_array().unwrap().len()];
                    request
                        .respond(
                            Response::from_string(json!({"embeddings":vectors}).to_string())
                                .with_header(
                                    Header::from_bytes("Content-Type", "application/json").unwrap(),
                                ),
                        )
                        .unwrap();
                    continue;
                }
                assert_eq!(request.url(), "/v1/chat/completions");
                let answer = if body.contains("Есть разделы для заметок") {
                    "идеи | Подходит теме\nвыдуманный слот | недопустимо"
                } else {
                    "Согласовали архитектуру клиента [1] [999]."
                };
                request
                    .respond(
                        Response::from_string(
                            json!({"choices":[{"message":{"role":"assistant","content":answer}}]})
                                .to_string(),
                        )
                        .with_header(
                            Header::from_bytes("Content-Type", "application/json").unwrap(),
                        ),
                    )
                    .unwrap();
            }
        }
    });
    struct Model {
        stop: Arc<AtomicBool>,
        thread: Option<std::thread::JoinHandle<()>>,
    }
    impl Drop for Model {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
            if let Some(thread) = self.thread.take() {
                thread.join().unwrap();
            }
        }
    }
    let _model = Model {
        stop,
        thread: Some(thread),
    };
    let server = journal_http_fixture(directory.path(), Some("synthetic-token"));
    let client = Client::new(&server.address, "synthetic-token".into()).unwrap();
    let answer = client.ask("архитектура").unwrap();
    assert!(!answer.sources.is_empty());
    assert!(answer
        .sources
        .iter()
        .all(|s| s.session == "demo" && s.n != 999));
    let before = calls.load(Ordering::Relaxed);
    let empty = client.ask("неизвестноеслово987xyz").unwrap();
    assert!(empty.sources.is_empty());
    assert_eq!(
        calls.load(Ordering::Relaxed),
        before,
        "no fragments means no model call"
    );
    let hints = client.route("Идея для архитектуры").unwrap();
    assert_eq!(hints.len(), 1);
    assert_eq!(hints[0].slot, "идеи");
    assert_eq!(hints[0].reason.as_deref(), Some("Подходит теме"));
    assert!(!note.exists(), "route never writes a note");
    fs::remove_dir(directory.path().join("index/semantic.lock")).unwrap();
    let semantic = client.search("концепция", "semantic").unwrap();
    assert!(!semantic.is_empty());
    assert!(semantic.iter().all(|h| h.score.is_some()));
    let hybrid = client.search("архитектура", "hybrid").unwrap();
    assert!(hybrid
        .iter()
        .any(|h| h.why.as_deref() == Some("слова + смысл")));
    fail.store(true, Ordering::Relaxed);
    assert!(client.search("концепция", "semantic").is_err());
    assert!(client.ask("архитектура").is_err());
    assert!(client.route("архитектура").is_err());
}
