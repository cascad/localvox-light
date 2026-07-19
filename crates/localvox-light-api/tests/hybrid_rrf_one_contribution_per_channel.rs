//! Regression: the RRF merge in hybrid search gives a document AT MOST ONE
//! contribution per channel. Previously the dedup inside the lexical channel SUMMED
//! the scores: summary and processed with the same paragraph were merged into
//! 1/(60+r1)+1/(60+r2) from a single channel, and the md duplicate outranked the best
//! transcript hit — the hybrid overturned the purely lexical order even when semantics
//! was unavailable.

use localvox_light_api::archive::Archive;
use localvox_light_core::versions::{now_rfc3339, TranscriptLine, VersionEntry, VersionStore};
use std::path::Path;

fn make_workdir(dir: &Path) {
    let session = dir.join("sessions/20260712_rrf");
    std::fs::create_dir_all(&session).unwrap();
    let store = VersionStore::open(&session).unwrap();
    let (id, path) = store.next_version("gigaam-int8").unwrap();
    // A short line with the term — BM25 (length normalization) puts it above the long
    // md paragraphs → lexical rank 0.
    let line = TranscriptLine {
        source_id: 0,
        start_sec: 12.0,
        end_sec: 20.0,
        text: "Обсудили кварталку кратко".into(),
        speaker: None,
    };
    std::fs::write(&path, serde_json::to_string(&line).unwrap() + "\n").unwrap();
    store
        .commit(VersionEntry {
            id,
            label: "gigaam-int8".into(),
            file: path.file_name().unwrap().to_string_lossy().into(),
            model: "m".into(),
            params: serde_json::json!({}),
            created_at: now_rfc3339(),
            parents: vec![],
        })
        .unwrap();
    // The same long paragraph reaching the index through BOTH derived channels (< 160
    // characters — make_snippet will return the same full text, and the dedup by snippet will
    // fire). The summary is a file; the readable text is a delta, so it says this paragraph by
    // being the cleanup of that line.
    let para = "На встрече долго и подробно обсуждали кварталку, сроки, риски, \
бюджеты, кадровые вопросы и планирование на следующий период работы команды.";
    std::fs::write(session.join("summary.md"), format!("{para}\n")).unwrap();
    localvox_light_core::readable::save(
        &session,
        &localvox_light_core::readable::Readable {
            version_id: id,
            edits: [(0usize, para.to_string())].into_iter().collect(),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn hybrid_does_not_double_count_intra_channel_duplicates() {
    // Semantics is unavailable (port 9, discard) → the hybrid degrades to lexical and
    // MUST preserve the lexical order (RRF over a single channel is monotonic).
    std::env::set_var("LOCALVOX_EMBED_BASE_URL", "http://127.0.0.1:9");
    std::env::set_var("LOCALVOX_EMBED_TIMEOUT_SEC", "1");
    let dir = tempfile::tempdir().unwrap();
    make_workdir(dir.path());
    let a = Archive::new(dir.path().to_path_buf());

    // Preconditions of the scenario: transcript first, both md paragraphs in the
    // results.
    let lex = a.search("кварталка", 10).unwrap();
    assert_eq!(lex.len(), 3, "expected transcript + two md paragraphs");
    assert_eq!(
        lex[0].kind, "transcript",
        "tantivy must put the short line first"
    );

    let hyb = a.search_mode("кварталка", 10, "hybrid").unwrap();
    // The summary/processed duplicate is merged for display...
    assert_eq!(hyb.len(), 2, "the md duplicate must be merged into one hit");
    // ...but its score is not doubled: transcript (rank 0, 1/60) stays above the md
    // merge (best rank 1, 1/61) — the lexical order is not overturned.
    assert_eq!(
        hyb[0].kind, "transcript",
        "the intra-channel dedup MUST NOT sum the RRF contributions of one channel"
    );
}
