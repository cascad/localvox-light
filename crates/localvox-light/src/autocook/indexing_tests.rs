//! Native BL-005 acceptance, isolated from the user's archive and model services.
use super::*;
use localvox_light_api::{
    archive::Archive,
    http::{respond, HttpConfig},
};
use localvox_light_client::Client;
use localvox_light_core::{
    processing,
    versions::{VersionEntry, VersionStore},
};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    sync::atomic::{AtomicU8, AtomicUsize},
    thread,
};

const OK: u8 = 0;
const HOLD: u8 = 1;
const FAIL: u8 = 2;

struct Endpoint {
    address: String,
    mode: Arc<AtomicU8>,
    requests: Arc<AtomicUsize>,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Endpoint {
    fn start() -> Self {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        let mode = Arc::new(AtomicU8::new(OK));
        let requests = Arc::new(AtomicUsize::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let (m, n, s) = (mode.clone(), requests.clone(), stop.clone());
        let thread = thread::spawn(move || {
            while !s.load(Ordering::Relaxed) {
                let Some(mut request) = server.recv_timeout(Duration::from_millis(40)).unwrap()
                else {
                    continue;
                };
                assert_eq!(request.url(), "/api/embed");
                let mut body = String::new();
                request.as_reader().read_to_string(&mut body).unwrap();
                let body: Value = serde_json::from_str(&body).unwrap();
                n.fetch_add(1, Ordering::Relaxed);
                while m.load(Ordering::Relaxed) == HOLD && !s.load(Ordering::Relaxed) {
                    thread::sleep(Duration::from_millis(10));
                }
                let response = if m.load(Ordering::Relaxed) == FAIL {
                    tiny_http::Response::from_string("fixture Ollama unavailable")
                        .with_status_code(503)
                } else {
                    let vectors = body["input"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|text| {
                            let text = text.as_str().unwrap().to_lowercase();
                            if text.contains("бюджет") {
                                vec![1., 0., 0.]
                            } else if text.contains("срок") {
                                vec![0., 1., 0.]
                            } else {
                                vec![0., 0., 1.]
                            }
                        })
                        .collect::<Vec<_>>();
                    tiny_http::Response::from_string(json!({"embeddings":vectors}).to_string())
                };
                // Cancellation/timeout deliberately closes the worker's connection.
                let _ = request.respond(response);
            }
        });
        Self {
            address,
            mode,
            requests,
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        self.mode.store(OK, Ordering::Relaxed);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

struct Api {
    client: Client,
    stop: Arc<AtomicBool>,
    thread: Option<thread::JoinHandle<()>>,
}
impl Api {
    fn start(root: &Path) -> Self {
        let archive = Archive::new(root.to_path_buf());
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let thread = thread::spawn(move || {
            let config = HttpConfig {
                bind: "127.0.0.1:0".into(),
                token: None,
            };
            thread::scope(|scope| {
                while !stopping.load(Ordering::Relaxed) {
                    let Some(request) = server.recv_timeout(Duration::from_millis(40)).unwrap()
                    else {
                        continue;
                    };
                    let archive = &archive;
                    let config = &config;
                    scope.spawn(move || {
                        let (status, data) = respond(
                            archive,
                            config,
                            request.method(),
                            request.url(),
                            None,
                            true,
                            "",
                        );
                        request
                            .respond(
                                tiny_http::Response::from_string(data.to_string())
                                    .with_status_code(status),
                            )
                            .unwrap();
                    });
                }
            });
        });
        Self {
            client: Client::new(&address, String::new()).unwrap(),
            stop,
            thread: Some(thread),
        }
    }
}
impl Drop for Api {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let result = self.thread.take().unwrap().join();
        if !thread::panicking() {
            result.unwrap();
        }
    }
}

fn wait_for(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !condition() {
        assert!(Instant::now() < deadline, "timeout: {label}");
        thread::sleep(Duration::from_millis(20));
    }
}
struct Stop<'a>(&'a AtomicBool);
impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

fn version(session: &Path, text: &str) -> u32 {
    let store = VersionStore::open(session).unwrap();
    let (id, path) = store.next_version("fixture").unwrap();
    fs::write(
        &path,
        format!(
            "{}\n",
            json!({"source_id":0,"start_sec":12.5,"end_sec":19.,"text":text})
        ),
    )
    .unwrap();
    store
        .commit(VersionEntry {
            id,
            file: path.file_name().unwrap().to_string_lossy().into_owned(),
            label: "fixture".into(),
            model: "fixture".into(),
            params: json!({}),
            created_at: "2026-11-01T00:00:00Z".into(),
            parents: vec![],
        })
        .unwrap();
    processing::record(
        session,
        processing::TRANSCRIPT,
        "fixture",
        processing::Outcome::Ok,
        None,
        Some(id),
    )
    .unwrap();
    id
}
fn ready(root: &Path, name: &str) -> PathBuf {
    let session = root.join("sessions").join(name);
    fs::create_dir_all(&session).unwrap();
    fs::write(
        session.join("meta.json"),
        json!({"title":name,"started_at":"2026-11-01T00:00:00Z","sample_rate":16000,"chunks":[]})
            .to_string(),
    )
    .unwrap();
    version(&session, "Утвердили бюджет проекта");
    fs::write(session.join("summary.md"), "## Решение\n\nПокупаем сервер.").unwrap();
    assert!(processing::inspect(&session, processing::TRANSCRIPT).complete());
    session
}
fn snapshot(dir: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    for entry in fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            files.extend(snapshot(&path));
        } else {
            files.insert(path.clone(), fs::read(path).unwrap());
        }
    }
    files
}
fn retained(root: &Path, files: &BTreeMap<PathBuf, Vec<u8>>) {
    for (path, bytes) in files {
        assert_eq!(
            &fs::read(path).unwrap(),
            bytes,
            "changed {}",
            path.display()
        );
    }
    assert!(processing::inspect(&root.join("sessions/ready"), processing::TRANSCRIPT).complete());
    assert!(!JobQueue::load(root)
        .jobs()
        .iter()
        .any(|job| job.state == JobState::Failed));
}
fn config(root: &Path, address: &str) -> Config {
    // Only this isolated subprocess changes its environment; no .env is present.
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", address);
    std::env::set_var("LOCALVOX_EMBED_MODEL", "fixture-3d");
    std::env::set_var("LOCALVOX_LIGHT_AUTOCOOK_SUMMARY", "off");
    let worker_dir = std::env::current_exe()
        .unwrap()
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let mut cfg = Config::from_env_in(root.to_string_lossy().into_owned(), &worker_dir).unwrap();
    cfg.interval = Duration::from_millis(20);
    cfg.index_timeout = Duration::from_secs(20);
    cfg.preflight(&AtomicBool::new(true)).unwrap();
    cfg
}
fn indexed(root: &Path, text: &str) -> bool {
    fs::read_to_string(root.join("index/semantic.jsonl")).is_ok_and(|body| body.contains(text))
}
fn assert_search(client: &Client, word: &str) {
    for mode in ["lexical", "semantic", "hybrid"] {
        let hits = client.search(word, mode).unwrap();
        assert!(
            hits.iter().any(|hit| hit.session == "ready"
                && hit.kind == "transcript"
                && hit.start_sec == Some(12.5)),
            "{mode}: {hits:?}"
        );
    }
    let summary = client.search("сервер", "lexical").unwrap();
    assert!(summary
        .iter()
        .any(|hit| hit.kind == "summary" && hit.start_sec.is_none()));
}

fn finish_work(root: &Path, name: &str) {
    JobQueue::mutate(root, |queue| {
        let id = queue
            .jobs()
            .iter()
            .find(|job| job.session == name)
            .unwrap()
            .id;
        queue.mark_done(id);
    });
}

fn idle(root: &Path) {
    let endpoint = Endpoint::start();
    let cfg = config(root, &endpoint.address);
    let session = ready(root, "ready");
    let before = snapshot(&session);
    let busy = ready(root, "busy");
    let owner = jobs::try_execution_lock(&busy).unwrap().unwrap();
    JobQueue::mutate(root, |queue| {
        assert!(queue.enqueue_cook("busy", false, false, false))
    });
    let running = AtomicBool::new(true);
    thread::scope(|scope| {
        let _stop = Stop(&running);
        scope.spawn(|| coordinate(&cfg, &running));
        wait_for("coordinator ready", || {
            jobs::coordinator_status().is_some_and(|status| status.state == "ready")
        });
        thread::sleep(Duration::from_millis(200));
        assert_eq!(
            endpoint.requests.load(Ordering::Relaxed),
            0,
            "index started with Pending work"
        );
        assert!(!root.join("index/semantic.jsonl").exists());
        finish_work(root, "busy");
        wait_for("background semantic file", || {
            indexed(root, "Утвердили бюджет")
        });
        assert!(root.join("index/tantivy").is_dir());
        wait_for("worker releases semantic lock", || {
            fs::read_to_string(root.join("index/semantic.jsonl")).is_ok()
        });
        running.store(false, Ordering::Relaxed);
    });
    drop(owner);
    retained(root, &before);
    let api = Api::start(root);
    assert_search(&api.client, "бюджет");
    let id = version(&session, "Утвердили срок проекта");
    assert!(warm_indexes(&cfg, &AtomicBool::new(true)));
    assert!(indexed(root, "Утвердили срок"));
    let body = fs::read_to_string(root.join("index/semantic.jsonl")).unwrap();
    assert!(!body
        .lines()
        .skip(1)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|doc| doc["session"] == "ready"
            && doc["text"]
                .as_str()
                .is_some_and(|text| text.contains("бюджет"))));
    assert!(body
        .lines()
        .skip(1)
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .any(|doc| doc["session"] == "ready" && doc["best_id"] == id));
    assert_search(&api.client, "срок");
}

fn yield_and_resume(root: &Path) {
    let endpoint = Endpoint::start();
    endpoint.mode.store(HOLD, Ordering::Relaxed);
    let cfg = config(root, &endpoint.address);
    let session = ready(root, "ready");
    let before = snapshot(&session);
    let running = AtomicBool::new(true);
    thread::scope(|scope| {
        let worker = scope.spawn(|| warm_indexes(&cfg, &running));
        wait_for("indexer entered embed", || {
            endpoint.requests.load(Ordering::Relaxed) > 0
        });
        ready(root, "new-work");
        JobQueue::mutate(root, |queue| {
            assert!(queue.enqueue_cook("new-work", false, false, false))
        });
        let start = Instant::now();
        assert!(
            !worker.join().unwrap(),
            "cancelled indexing must remain dirty"
        );
        assert!(start.elapsed() < Duration::from_secs(4));
    });
    retained(root, &before);
    endpoint.mode.store(OK, Ordering::Relaxed);
    finish_work(root, "new-work");
    assert!(warm_indexes(&cfg, &running));
    assert!(indexed(root, "Утвердили бюджет"));
    let api = Api::start(root);
    assert_search(&api.client, "бюджет");
    retained(root, &before);
}

fn timeout(root: &Path) {
    let endpoint = Endpoint::start();
    endpoint.mode.store(HOLD, Ordering::Relaxed);
    let mut cfg = config(root, &endpoint.address);
    cfg.index_timeout = Duration::from_secs(3);
    let session = ready(root, "ready");
    let before = snapshot(&session);
    let running = AtomicBool::new(true);
    let start = Instant::now();
    thread::scope(|scope| {
        let _stop = Stop(&running);
        scope.spawn(|| coordinate(&cfg, &running));
        wait_for("hanging embed entered", || {
            endpoint.requests.load(Ordering::Relaxed) > 0
        });
        thread::sleep(cfg.index_timeout + Duration::from_millis(400));
        endpoint.mode.store(FAIL, Ordering::Relaxed);
        thread::sleep(Duration::from_millis(300));
        assert_eq!(
            endpoint.requests.load(Ordering::Relaxed),
            1,
            "timeout created an idle retry loop"
        );
        assert!(!indexed(root, "Утвердили бюджет"));
        running.store(false, Ordering::Relaxed);
    });
    assert!(start.elapsed() < Duration::from_secs(12));
    retained(root, &before);
    let api = Api::start(root);
    assert!(!api.client.search("бюджет", "lexical").unwrap().is_empty());
    assert!(!api.client.transcript("ready").unwrap().is_empty());
    assert!(!api.client.summary("ready").unwrap().blocks.is_empty());
}

fn unavailable(root: &Path) {
    let endpoint = Endpoint::start();
    endpoint.mode.store(FAIL, Ordering::Relaxed);
    let mut cfg = config(root, &endpoint.address);
    let session = ready(root, "ready");
    let before = snapshot(&session);
    assert!(warm_indexes(&cfg, &AtomicBool::new(true)));
    assert!(!indexed(root, "Утвердили бюджет"));
    {
        let api = Api::start(root);
        assert!(!api.client.search("бюджет", "lexical").unwrap().is_empty());
        assert!(api.client.search("бюджет", "semantic").is_err());
        assert!(!api.client.search("бюджет", "hybrid").unwrap().is_empty());
        assert!(!api.client.transcript("ready").unwrap().is_empty());
        assert!(!api.client.summary("ready").unwrap().blocks.is_empty());
    }
    // Connection refusal as well as an HTTP error, with no real Ollama fallback.
    let closed = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", &address);
    cfg.child_env
        .retain(|(key, _)| key != "LOCALVOX_EMBED_BASE_URL");
    cfg.child_env
        .push(("LOCALVOX_EMBED_BASE_URL".into(), address.into()));
    assert!(warm_indexes(&cfg, &AtomicBool::new(true)));
    retained(root, &before);
    // A truly empty archive succeeds without contacting embeddings.
    let empty = root.join("empty");
    fs::create_dir_all(empty.join("sessions")).unwrap();
    let calls = endpoint.requests.load(Ordering::Relaxed);
    {
        let api = Api::start(&empty);
        assert!(api.client.search("бюджет", "semantic").unwrap().is_empty());
    }
    assert_eq!(endpoint.requests.load(Ordering::Relaxed), calls);

    // Recovery builds a real cache. A later failed update must retain its vectors.
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", &endpoint.address);
    cfg.child_env
        .retain(|(key, _)| key != "LOCALVOX_EMBED_BASE_URL");
    cfg.child_env.push((
        "LOCALVOX_EMBED_BASE_URL".into(),
        endpoint.address.clone().into(),
    ));
    endpoint.mode.store(OK, Ordering::Relaxed);
    assert!(warm_indexes(&cfg, &AtomicBool::new(true)));
    let cached = fs::read(root.join("index/semantic.jsonl")).unwrap();
    version(&session, "Утвердили срок проекта");
    endpoint.mode.store(FAIL, Ordering::Relaxed);
    assert!(warm_indexes(&cfg, &AtomicBool::new(true)));
    assert_eq!(fs::read(root.join("index/semantic.jsonl")).unwrap(), cached);
    {
        let api = Api::start(root);
        assert!(api.client.search("срок", "semantic").is_err());
        assert!(!api.client.search("срок", "hybrid").unwrap().is_empty());
    }
    endpoint.mode.store(OK, Ordering::Relaxed);
    assert!(warm_indexes(&cfg, &AtomicBool::new(true)));
    assert!(indexed(root, "Утвердили срок"));
    let api = Api::start(root);
    assert_search(&api.client, "срок");
}

fn busy(root: &Path) {
    let endpoint = Endpoint::start();
    endpoint.mode.store(HOLD, Ordering::Relaxed);
    let cfg = config(root, &endpoint.address);
    let session = ready(root, "ready");
    let before = snapshot(&session);
    let running = AtomicBool::new(true);
    thread::scope(|scope| {
        let _stop = Stop(&running);
        let worker = scope.spawn(|| warm_indexes(&cfg, &running));
        wait_for("background index holds semantic lock", || {
            endpoint.requests.load(Ordering::Relaxed) > 0
        });
        let api = Api::start(root);
        thread::scope(|queries| {
            let (sender, responses) = std::sync::mpsc::channel();
            for _ in 0..4 {
                let client = api.client.clone();
                let sender = sender.clone();
                queries.spawn(move || {
                    let _ = sender.send(client.search("бюджет", "semantic"));
                });
            }
            // Two admitted semantic reads wait on the real worker's file lock;
            // excess requests must refuse promptly instead of filling API threads.
            let first = responses.recv_timeout(Duration::from_secs(3));
            let transcript = api.client.transcript("ready");
            endpoint.mode.store(OK, Ordering::Relaxed);
            assert!(worker.join().unwrap());
            let error = first.unwrap().unwrap_err();
            assert!(format!("{error:#}").contains("занято"), "{error:#}");
            assert!(!transcript.unwrap().is_empty());
        });
        assert!(indexed(root, "Утвердили бюджет"));
        assert_search(&api.client, "бюджет");
    });
    retained(root, &before);
}

#[test]
#[ignore = "private subprocess fixture for native indexing tests"]
fn indexing_fixture() {
    let Ok(case) = std::env::var("LOCALVOX_INDEX_TEST_CASE") else {
        return;
    };
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .try_init();
    let root = std::env::current_dir().unwrap();
    match case.as_str() {
        "idle" => idle(&root),
        "yield" => yield_and_resume(&root),
        "timeout" => timeout(&root),
        "unavailable" => unavailable(&root),
        "busy" => busy(&root),
        _ => panic!("unknown fixture"),
    }
}
fn native(case: &str) {
    let root = tempfile::tempdir().unwrap();
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "autocook::indexing_tests::indexing_fixture",
            "--nocapture",
        ])
        .current_dir(root.path())
        .env_clear();
    for name in [
        "PATH",
        "SystemRoot",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
    ] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
        .env("LOCALVOX_INDEX_TEST_CASE", case)
        .env("TEMP", root.path())
        .env("TMP", root.path());
    let result = localvox_light_process::run(
        command,
        None,
        Limits::new(Duration::from_secs(70)),
        || false,
        |_, _| {},
    )
    .unwrap();
    assert!(
        result.termination.success(),
        "{case}: {:?}\n{}\n{}",
        result.termination,
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    println!("native indexing {case}: passed");
}
macro_rules! native_test {
    ($name:ident, $case:literal) => {
        #[test]
        #[ignore = "requires built sibling localvox-process; loopback fixture, no models/devices"]
        fn $name() {
            native($case);
        }
    };
}
native_test!(idle_indexes_before_search_and_refreshes_best, "idle");
native_test!(new_work_cancels_indexing_then_resume_succeeds, "yield");
native_test!(
    index_timeout_preserves_documents_without_a_retry_loop,
    "timeout"
);
native_test!(
    unavailable_ollama_preserves_documents_and_lexical_search,
    "unavailable"
);
native_test!(
    background_warmup_and_parallel_semantic_reads_preserve_api_access,
    "busy"
);
