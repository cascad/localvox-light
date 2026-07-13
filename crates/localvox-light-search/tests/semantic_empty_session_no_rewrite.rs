//! A regression test (out of the adversarial review of WP-C11): a session with an
//! empty transcript is never «covered» by the index and used to cause a full
//! rewrite of semantic.jsonl on every open_or_update.
//! Run with --test-threads=1 (env variables are process-global).

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use localvox_light_core::versions::{VersionEntry, VersionStore};
use localvox_light_search::semantic::SemanticIndex;

fn write_session(root: &std::path::Path, name: &str, body: &str) {
    let dir = root.join("sessions").join(name);
    std::fs::create_dir_all(&dir).unwrap();
    let store = VersionStore::open(&dir).unwrap();
    let (id, path) = store.next_version("fast").unwrap();
    std::fs::write(&path, body).unwrap();
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

fn mtime(p: &std::path::Path) -> std::time::SystemTime {
    std::fs::metadata(p).unwrap().modified().unwrap()
}

#[test]
fn empty_transcript_session_does_not_rewrite_index_every_open() {
    // Ollama is not needed: embed(&[]) makes no HTTP calls (chunks over an empty slice)
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", "http://127.0.0.1:1");
    std::env::set_var("LOCALVOX_EMBED_MODEL", "nomic-embed-text");
    let dir = tempfile::tempdir().unwrap();
    let work = dir.path();
    // Silence: cook writes and commits a version even with zero lines (cook.rs:158-193)
    write_session(work, "20260707_silence", "");

    // An existing index (only the header, the model matches)
    let idx_dir = work.join("index");
    std::fs::create_dir_all(&idx_dir).unwrap();
    let idx_path = idx_dir.join("semantic.jsonl");
    std::fs::write(&idx_path, "{\"model\":\"nomic-embed-text\",\"dim\":0}\n").unwrap();
    let mtime0 = mtime(&idx_path);

    std::thread::sleep(std::time::Duration::from_millis(100));
    SemanticIndex::open_or_update(work).unwrap();
    assert_eq!(
        mtime0,
        mtime(&idx_path),
        "semantic.jsonl was rewritten without changes: an empty session is never covered"
    );

    std::thread::sleep(std::time::Duration::from_millis(100));
    SemanticIndex::open_or_update(work).unwrap();
    assert_eq!(
        mtime0,
        mtime(&idx_path),
        "the rewrite repeats on every call"
    );
}

// ─────────── regression: an ordinary session is still indexed ───────────

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A mock /api/embed: any texts → valid vectors. Returns (url, call counter).
fn spawn_mock_embed() -> (String, Arc<AtomicUsize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 4096];
            let header_end = loop {
                match s.read(&mut tmp) {
                    Ok(0) | Err(_) => break None,
                    Ok(n) => {
                        buf.extend_from_slice(&tmp[..n]);
                        if let Some(p) = find_subslice(&buf, b"\r\n\r\n") {
                            break Some(p + 4);
                        }
                    }
                }
            };
            let Some(header_end) = header_end else {
                continue;
            };
            let headers = String::from_utf8_lossy(&buf[..header_end]).to_lowercase();
            let content_length: usize = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            while buf.len() < header_end + content_length {
                match s.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                }
            }
            let body: serde_json::Value =
                serde_json::from_slice(&buf[header_end..header_end + content_length])
                    .unwrap_or(serde_json::Value::Null);
            let n = body["input"].as_array().map(|a| a.len()).unwrap_or(0);
            counter.fetch_add(1, Ordering::SeqCst);
            let vecs: Vec<Vec<f32>> = (0..n).map(|_| vec![1.0, 0.0]).collect();
            let json = serde_json::json!({ "embeddings": vecs }).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = s.write_all(resp.as_bytes());
        }
    });
    (format!("http://127.0.0.1:{port}"), calls)
}

#[test]
fn non_empty_session_indexed_once_then_covered() {
    let (base, calls) = spawn_mock_embed();
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", &base);
    std::env::set_var("LOCALVOX_EMBED_MODEL", "mock-embed");

    let dir = tempfile::tempdir().unwrap();
    let work = dir.path();
    let line = serde_json::json!({
        "source_id": 0, "start_sec": 0.0, "end_sec": 1.0, "text": "обсуждали отчёт"
    });
    write_session(work, "20260708_talk", &format!("{line}\n"));

    // Call 1: the session is indexed, the file is written
    let idx = SemanticIndex::open_or_update(work).unwrap();
    drop(idx);
    let idx_path = work.join("index").join("semantic.jsonl");
    assert!(idx_path.exists(), "the index must be created");
    let body = std::fs::read_to_string(&idx_path).unwrap();
    assert!(
        body.contains("обсуждали отчёт"),
        "the document of the session was not written"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let mtime1 = mtime(&idx_path);

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Call 2: the session is covered — no embed calls, no rewrite
    SemanticIndex::open_or_update(work).unwrap();
    assert_eq!(
        calls.load(Ordering::SeqCst),
        1,
        "a repeated embed of a covered session"
    );
    assert_eq!(mtime1, mtime(&idx_path), "a rewrite without changes");
}
