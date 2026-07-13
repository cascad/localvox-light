//! An adversarial repro → regression test: Header.dim must be checked.
//! Before the fix: on a change of the embedding model's dimension under the same
//! name, search() returned Ok with score=1.0 from a dot() of truncated prefixes
//! (vectors from different spaces). After the fix: (1) search() gives a loud error
//! when the dimension of the query and of the index do not match; (2) a line with a
//! vector of the wrong dimension invalidates the index as a whole → a full reindex →
//! self-heal.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use localvox_light_core::versions::{VersionEntry, VersionStore};
use localvox_light_search::semantic::SemanticIndex;

fn find_subslice(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// A mock /api/embed with a switchable dimension (emulates a re-upload of the model
/// under the same name). dim=4: "alpha"→[0,1,0,0], otherwise [1,0,0,0]. dim=2: [0,1].
fn spawn_mock_embed(dim: Arc<AtomicUsize>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
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
            let inputs: Vec<String> = body["input"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|v| v.as_str().unwrap_or_default().to_string())
                        .collect()
                })
                .unwrap_or_default();
            let d = dim.load(Ordering::SeqCst);
            let vecs: Vec<Vec<f32>> = inputs
                .iter()
                .map(|t| {
                    if d == 4 {
                        if t.contains("alpha") {
                            vec![0.0, 1.0, 0.0, 0.0]
                        } else {
                            vec![1.0, 0.0, 0.0, 0.0]
                        }
                    } else {
                        vec![0.0, 1.0]
                    }
                })
                .collect();
            let json = serde_json::json!({ "embeddings": vecs }).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                json.len(),
                json
            );
            let _ = s.write_all(resp.as_bytes());
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

#[test]
fn dim_change_under_same_model_name_fails_loud_and_bad_row_triggers_reindex() {
    let dim = Arc::new(AtomicUsize::new(4));
    let base = spawn_mock_embed(dim.clone());
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", &base);
    std::env::set_var("LOCALVOX_EMBED_MODEL", "nomic-embed-text");
    // This test is about the dimension changing, not about the similarity floor: the mock
    // returns synthetic vectors whose cosines mean nothing. Leaving the floor on would make
    // the test silently check something else.
    std::env::set_var("LOCALVOX_SEARCH_MIN_SIMILARITY", "0.0");

    let work_dir = tempfile::tempdir().unwrap();
    let work = work_dir.path();
    write_session(work, "20260701_alpha", "alpha разговор про отчёт");
    write_session(work, "20260702_beta", "beta совсем другая тема");

    // Phase 1: the index is built by the «old» model, dim=4 in the header; search is ok.
    let idx = SemanticIndex::open_or_update(work).unwrap();
    let hits = idx.search("alpha запрос", 10).unwrap();
    assert_eq!(hits.len(), 2);
    assert!(hits[0].text.contains("alpha"));
    let idx_file = work.join("index").join("semantic.jsonl");
    let body = std::fs::read_to_string(&idx_file).unwrap();
    assert!(
        body.lines().next().unwrap().contains("\"dim\":4"),
        "header: {body}"
    );

    // Phase 2: the model was «rolled over» under the same name — embed is now
    // 2-dimensional. The sessions did not change → the old 4D vectors remain; search
    // MUST fail loudly instead of returning a dot of truncated prefixes (before the
    // fix: Ok, score=1.0).
    dim.store(2, Ordering::SeqCst);
    let idx = SemanticIndex::open_or_update(work).unwrap();
    let err = match idx.search("любой запрос", 10) {
        Err(e) => e,
        Ok(_) => panic!("search MUST reject a dimension mismatch"),
    };
    let msg = format!("{err:#}");
    assert!(msg.contains("dimension"), "the wrong error: {msg}");
    assert!(
        msg.contains("2D") && msg.contains("4D"),
        "no dimensions in the error: {msg}"
    );

    // Phase 3: a line with a vector of the wrong dimension (corruption/a mixed index)
    // → the whole index is invalidated, a full reindex with the current model (2D) →
    // self-heal: the header says dim=2, search works again.
    let bad_row =
        r#"{"session":"20260701_alpha","best_id":1,"start_sec":0.0,"text":"x","v":[0.1,0.2,0.3]}"#;
    let mut body = std::fs::read_to_string(&idx_file).unwrap();
    body.push_str(bad_row);
    body.push('\n');
    std::fs::write(&idx_file, body).unwrap();

    let idx = SemanticIndex::open_or_update(work).unwrap();
    let body = std::fs::read_to_string(&idx_file).unwrap();
    assert!(
        body.lines().next().unwrap().contains("\"dim\":2"),
        "after the reindex the header must be 2D: {body}"
    );
    let hits = idx.search("любой запрос", 10).unwrap();
    assert_eq!(hits.len(), 2, "after the self-heal search works again");
}
