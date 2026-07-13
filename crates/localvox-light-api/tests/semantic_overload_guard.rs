//! An adversarial repro → regression test: semantic search (re)indexes the archive on
//! the HTTP thread under an exclusive lock (semantic.rs). Before the fix, parallel
//! semantic requests queued up on the lock, ate MAX_INFLIGHT=32 and drove ALL
//! endpoints (including /api/health) into 503. After the fix: SemanticGuard admits at
//! most 2 (one indexes, one waits for the lock), the rest get a fast 503 "busy", and
//! health/lexical stay alive.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use localvox_light_api::archive::Archive;
use localvox_light_api::http::{bind, serve, HttpConfig};
use localvox_light_core::versions::{VersionEntry, VersionStore};

/// A mock Ollama /api/embed: it accepts connections and NEVER answers — a cold/hung
/// model. We keep the connections alive until the end of the process.
fn spawn_stuck_embed() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let mut held: Vec<TcpStream> = Vec::new();
        for stream in listener.incoming().flatten() {
            held.push(stream); // we do not answer, we just hold
        }
    });
    format!("http://127.0.0.1:{port}")
}

fn write_session(root: &std::path::Path, name: &str, text: &str) {
    let dir = root.join("sessions").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let store = VersionStore::open(&dir).unwrap();
    let (id, path) = store.next_version("fast").unwrap();
    let line = serde_json::json!({"source_id":0,"start_sec":0.0,"end_sec":1.0,"text":text});
    std::fs::write(&path, format!("{line}\n")).unwrap();
    store
        .commit(VersionEntry {
            id,
            label: "fast".into(),
            file: path.file_name().unwrap().to_string_lossy().to_string(),
            model: "test".into(),
            params: serde_json::json!({}),
            created_at: "t".into(),
            parents: vec![],
        })
        .unwrap();
}

fn raw_get(port: u16, path: &str) -> TcpStream {
    let mut s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    let req = format!("GET {path} HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    s
}

fn status_of(mut s: TcpStream) -> Option<u16> {
    s.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut buf = [0u8; 512];
    let n = s.read(&mut buf).ok()?;
    let line = String::from_utf8_lossy(&buf[..n]).to_string();
    line.split_whitespace().nth(1)?.parse().ok()
}

#[test]
fn semantic_flood_does_not_starve_other_endpoints() {
    let base = spawn_stuck_embed();
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", &base);
    std::env::set_var("LOCALVOX_EMBED_MODEL", "mock-embed");
    // the embed timeout is large — like the default 60 s; the test does not wait that
    // long
    std::env::set_var("LOCALVOX_EMBED_TIMEOUT_SEC", "600");

    let dir = tempfile::tempdir().unwrap();
    // one session not yet cooked into the index — there is something to index
    write_session(dir.path(), "20260712_big", "обсуждали планы на квартал");

    let archive = Arc::new(Archive::new(dir.path().to_path_buf()));
    let server = bind("127.0.0.1:0").unwrap();
    let port = server.server_addr().to_ip().unwrap().port();
    let cfg = HttpConfig {
        bind: format!("127.0.0.1:{port}"),
        token: None,
    };
    std::thread::spawn(move || serve(server, archive, cfg));

    // before the flood health is alive
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(
        status_of(raw_get(port, "/api/health")),
        Some(200),
        "health must work before the flood"
    );

    // 32 semantic requests: #1 hangs in embed (holding the lock), #2 — on lock(), the
    // rest MUST bounce off with a fast 503 from SemanticGuard.
    let mut held: Vec<TcpStream> = Vec::new();
    for _ in 0..32 {
        held.push(raw_get(
            port,
            "/api/search?q=%D0%BF%D0%BB%D0%B0%D0%BD&mode=semantic",
        ));
    }
    std::thread::sleep(Duration::from_secs(2)); // the server works through the connections

    // the cheap endpoints are alive: the flood did not eat the inflight budget
    let health = status_of(raw_get(port, "/api/health"));
    let lexical = status_of(raw_get(port, "/api/search?q=x"));
    assert_eq!(
        (health, lexical),
        (Some(200), Some(200)),
        "the semantic flood MUST NOT starve the other endpoints; health={health:?}, lexical={lexical:?}"
    );

    // out of the 32 semantic ones: 2 hang under the guard (embed/lock — read timeout →
    // None), 30 got a fast 503 "busy"
    let mut busy = 0;
    let mut hung = 0;
    for s in held {
        match status_of(s) {
            Some(503) => busy += 1,
            None => hung += 1,
            other => panic!("unexpected status of a semantic request: {other:?}"),
        }
    }
    assert_eq!(
        (busy, hung),
        (30, 2),
        "expected 30×503 and 2 hanging under the guard"
    );
}
