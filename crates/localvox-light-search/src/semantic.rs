//! Semantic search (F4, stage 2; WP-C11): «find by meaning, not by words».
//!
//! Vectors come through Ollama `/api/embed` (the same local infrastructure as the
//! LLM processing; the `nomic-embed-text` model and other embedding models). No
//! native dependencies in the binary: modularity (P4) — semantics is unavailable
//! when Ollama is switched off, lexical tantivy always works.
//!
//! The index is a derived artifact (P9): `<work_dir>/index/semantic.jsonl`, the
//! first line is the header (model/dimension), then one document per line.
//! Incremental by session: only the session whose best version changed
//! (cook/refine/set-best) is reindexed. Ollama vectors are normalized, therefore
//! cosine = dot product.

use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write as _};
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use localvox_light_core::versions::{read_transcript_lines, VersionStore};

/// Batch of requests to /api/embed: we do not blow up a single HTTP call.
const EMBED_BATCH: usize = 32;
/// Cap on the text length of one document (characters) — embedding models cut by
/// tokens anyway, and a gigantic string is slow without any benefit.
const MAX_DOC_CHARS: usize = 2000;

// ─────────────────────────── embed client ───────────────────────────

/// Vectorization client via Ollama `/api/embed`.
pub struct EmbedClient {
    base_url: String,
    model: String,
    agent: ureq::Agent,
}

impl EmbedClient {
    /// Config from env: `LOCALVOX_EMBED_BASE_URL` (default: Ollama on localhost),
    /// `LOCALVOX_EMBED_MODEL` (default: bge-m3).
    ///
    /// **Why bge-m3 and not nomic-embed-text — MEASURED, on the live archive (13.07.2026).**
    /// The owner searched for «капрезе», a word that is not in the archive at all, and got
    /// five confident results, all of them nonsense. The reason is not a missing threshold —
    /// it is that with nomic there is NOTHING TO SET A THRESHOLD ON:
    ///
    /// ```text
    ///   query              in the archive?   nomic top-1   bge-m3 top-1
    ///   «капрезе»          NO                0.830         0.424
    ///   «квартальный…»     NO                0.851         0.444
    ///   «курица»           YES               0.765         0.634
    /// ```
    ///
    /// With nomic the GARBAGE ranks ABOVE the correct hit — its cosines are squeezed into a
    /// narrow band and carry no signal for Russian. With bge-m3 (multilingual, built for
    /// retrieval) garbage lands at 0.42–0.56 and correct hits at 0.58–0.63: there is finally
    /// a gap, and a cut-off threshold becomes meaningful.
    pub fn from_env() -> Self {
        let base_url = std::env::var("LOCALVOX_EMBED_BASE_URL")
            .unwrap_or_else(|_| "http://localhost:11434".into());
        let model = std::env::var("LOCALVOX_EMBED_MODEL").unwrap_or_else(|_| "bge-m3".into());
        let timeout = std::time::Duration::from_secs(
            std::env::var("LOCALVOX_EMBED_TIMEOUT_SEC")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(60),
        );
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            model,
            agent: ureq::AgentBuilder::new().timeout(timeout).build(),
        }
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Vectorizes a batch of texts (order is preserved).
    pub fn embed(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        #[derive(Deserialize)]
        struct EmbedResponse {
            embeddings: Vec<Vec<f32>>,
        }
        let mut out: Vec<Vec<f32>> = Vec::with_capacity(texts.len());
        for chunk in texts.chunks(EMBED_BATCH) {
            let resp: EmbedResponse = self
                .agent
                .post(&format!("{}/api/embed", self.base_url))
                .send_json(serde_json::json!({ "model": self.model, "input": chunk }))
                .with_context(|| format!("embed via {} ({})", self.base_url, self.model))?
                .into_json()
                .context("parsing the /api/embed response")?;
            if resp.embeddings.len() != chunk.len() {
                bail!(
                    "embed returned {} vectors for {} texts",
                    resp.embeddings.len(),
                    chunk.len()
                );
            }
            out.extend(resp.embeddings);
        }
        Ok(out)
    }
}

// ─────────────────────────── index ───────────────────────────

#[derive(Serialize, Deserialize)]
struct Header {
    model: String,
    dim: usize,
}

#[derive(Serialize, Deserialize)]
struct Doc {
    session: String,
    /// id of the best version at the moment of indexing — the incrementality key.
    best_id: u32,
    /// transcript | summary | processed (as in the lexical index).
    #[serde(default = "kind_transcript")]
    kind: String,
    /// The timecode — only on transcript lines; md paragraphs do not have one.
    start_sec: f64,
    /// The END of the fragment. A line is a window of 8-15 seconds, and the human must see
    /// its boundaries: otherwise it is unclear what «▶» plays and when it stops.
    #[serde(default)]
    end_sec: f64,
    text: String,
    v: Vec<f32>,
    /// mtime of the derived md at the moment of indexing — we reindex if
    /// summary/processed were regenerated, even when the best version is the same.
    #[serde(default)]
    md_mtime: u64,
}

fn kind_transcript() -> String {
    "transcript".into()
}

pub struct SemanticHit {
    pub session: String,
    pub kind: String,
    pub start_sec: Option<f64>,
    pub end_sec: Option<f64>,
    pub text: String,
    pub score: f32,
}

pub struct SemanticIndex {
    docs: Vec<Doc>,
    client: EmbedClient,
}

impl SemanticIndex {
    fn index_path(work_dir: &Path) -> PathBuf {
        work_dir.join("index").join("semantic.jsonl")
    }

    /// Opens the index, additionally indexing the sessions that changed (the best
    /// version is not the one that was recorded). A change of the embedding model
    /// invalidates the index as a whole. The rebuild happens under a cross-process
    /// lock (like the tantivy index).
    pub fn open_or_update(work_dir: &Path) -> Result<Self> {
        let client = EmbedClient::from_env();
        let path = Self::index_path(work_dir);
        fs::create_dir_all(path.parent().context("index directory")?)?;
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(path.with_extension("lock"))
            .context("lock file of the semantic index")?;
        // Bounded waiting instead of an indefinite lock(): while a neighbour spends
        // minutes indexing under the exclusive lock, the HTTP thread must not hang —
        // a fast, honest refusal in the style of a 503 «retry later».
        crate::lock_bounded(&lock_file, std::time::Duration::from_secs(10))
            .context("acquiring the lock of the semantic index")?;

        // 1) load what exists (model AND dimension matched — otherwise from scratch)
        let mut docs: Vec<Doc> = Vec::new();
        if let Ok(f) = fs::File::open(&path) {
            let mut lines = BufReader::new(f).lines();
            let header: Option<Header> = lines
                .next()
                .and_then(|l| l.ok())
                .and_then(|l| serde_json::from_str(&l).ok());
            if let Some(h) = header.filter(|h| h.model == client.model) {
                for l in lines.map_while(|l| l.ok()) {
                    if let Ok(d) = serde_json::from_str::<Doc>(&l) {
                        if d.v.len() != h.dim {
                            // a corrupted line or vectors of a different dimension:
                            // a dot over a truncated prefix is garbage, index from scratch
                            tracing::warn!(
                                "semantic: {}D vector with a {}D header — reindexing",
                                d.v.len(),
                                h.dim
                            );
                            docs.clear();
                            break;
                        }
                        docs.push(d);
                    }
                }
            }
        }

        // 2) what is covered: session → (best_id, max mtime of the derived md files)
        let mut covered: HashMap<String, (u32, u64)> = HashMap::new();
        for d in &docs {
            let e = covered.entry(d.session.clone()).or_insert((d.best_id, 0));
            e.0 = d.best_id;
            e.1 = e.1.max(d.md_mtime);
        }

        // 2a) sessions deleted from disk — out of the index: the text must not
        // outlive the removal of its source (privacy/retention, P3).
        let sessions_root = work_dir.join("sessions");
        let alive: std::collections::HashSet<String> = fs::read_dir(&sessions_root)
            .map(|rd| {
                rd.flatten()
                    .filter(|e| e.path().is_dir())
                    .map(|e| e.file_name().to_string_lossy().to_string())
                    .collect()
            })
            .unwrap_or_default();
        let before = docs.len();
        docs.retain(|d| alive.contains(&d.session));
        let mut changed = docs.len() != before;

        // 3) scan of the sessions: where best changed/appeared — reindex the session
        if let Ok(entries) = fs::read_dir(&sessions_root) {
            for e in entries.flatten() {
                let dir = e.path();
                if !dir.is_dir() {
                    continue;
                }
                let name = dir
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                let Some(store) = VersionStore::open(&dir).ok() else {
                    continue;
                };
                let Some(best) = store.best() else {
                    continue; // not cooked — nothing to index
                };
                // The derived md files (summary/processed) are indexed by paragraph —
                // «find by meaning» must work over the summaries too, not only over
                // the raw utterances. The freshness key is their mtime.
                let md_mtime = ["summary.md", "processed.md"]
                    .iter()
                    .filter_map(|f| {
                        fs::metadata(dir.join(f))
                            .and_then(|m| m.modified())
                            .ok()
                            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                            .map(|d| d.as_secs())
                    })
                    .max()
                    .unwrap_or(0);
                let prev = covered.get(&name).copied();
                if prev == Some((best.id, md_mtime)) {
                    continue; // up to date: the same best version and the same derivatives
                }
                // The transcript did not change (the same best), only summary/processed
                // were regenerated → re-embedding thousands of lines for the sake of a
                // dozen paragraphs is not allowed (minutes under the lock). The
                // transcript vectors stay alive.
                let transcript_fresh = prev.is_some_and(|(b, _)| b == best.id);

                // (kind, start_sec, end_sec, text) — what really has to be vectorized
                let mut items: Vec<(String, f64, f64, String)> = Vec::new();
                if !transcript_fresh {
                    let Some(v_path) = store.resolve(best.id) else {
                        continue;
                    };
                    // A transient read error must not throw out the old documents of
                    // the session — we skip it until the next time.
                    let lines = match read_transcript_lines(&v_path) {
                        Ok(l) => l,
                        Err(e) => {
                            tracing::warn!(
                                "semantic: session {name}: transcript not read ({e}) — skipping"
                            );
                            continue;
                        }
                    };
                    items.extend(lines.iter().map(|l| {
                        (
                            "transcript".to_string(),
                            l.start_sec,
                            l.end_sec,
                            l.text.chars().take(MAX_DOC_CHARS).collect::<String>(),
                        )
                    }));
                }
                for (file, kind) in [("summary.md", "summary"), ("processed.md", "processed")] {
                    let Ok(body) = fs::read_to_string(dir.join(file)) else {
                        continue;
                    };
                    for para in body
                        .split("\n\n")
                        .map(str::trim)
                        .filter(|s| !s.is_empty() && !s.starts_with("<!--"))
                    {
                        items.push((
                            kind.to_string(),
                            0.0, // md paragraphs have no timecode
                            0.0,
                            para.chars().take(MAX_DOC_CHARS).collect::<String>(),
                        ));
                    }
                }
                let refs: Vec<&str> = items.iter().map(|(_, _, _, t)| t.as_str()).collect();
                // One problematic session (an embed timeout on a gigantic transcript)
                // does not bring down the whole index: we skip it, the successful
                // sessions are saved in step 4, and the old vectors stay searchable.
                let vectors = match client.embed(&refs) {
                    Ok(v) => v,
                    Err(e) => {
                        tracing::warn!("semantic: session {name} skipped (embed failed): {e:#}");
                        continue;
                    }
                };
                let before_session = docs.len();
                if transcript_fresh {
                    // we drop only the md documents; the transcript vectors are kept
                    docs.retain(|d| !(d.session == name && d.kind != "transcript"));
                    // the incrementality key of the remaining ones has to be refreshed,
                    // otherwise covered would again not add up and the session would be
                    // scanned forever
                    for d in docs.iter_mut().filter(|d| d.session == name) {
                        d.md_mtime = md_mtime;
                    }
                    changed = true; // md_mtime mutated — the file has to be rewritten
                } else {
                    docs.retain(|d| d.session != name); // all documents of the session — out
                }
                let removed_old = docs.len() != before_session;
                for ((kind, start_sec, end_sec, text), v) in items.iter().zip(vectors) {
                    docs.push(Doc {
                        session: name.clone(),
                        best_id: best.id,
                        kind: kind.clone(),
                        start_sec: *start_sec,
                        end_sec: *end_sec,
                        text: text.clone(),
                        v,
                        md_mtime,
                    });
                }
                // An empty session (silence/broken file) yields no documents, and an
                // uncovered one is scanned on every call — without this check every
                // semantic query would rewrite the whole of semantic.jsonl.
                if removed_old || !items.is_empty() {
                    tracing::info!(
                        "semantic: session {name} indexed (best v{:03}, {} docs)",
                        best.id,
                        items.len()
                    );
                    changed = true;
                }
            }
        }

        // 4) atomic write when there are changes
        if changed {
            let dim = docs.first().map(|d| d.v.len()).unwrap_or(0);
            let tmp = path.with_extension("jsonl.tmp");
            {
                let mut w = std::io::BufWriter::new(fs::File::create(&tmp)?);
                writeln!(
                    w,
                    "{}",
                    serde_json::to_string(&Header {
                        model: client.model().to_string(),
                        dim
                    })?
                )?;
                for d in &docs {
                    writeln!(w, "{}", serde_json::to_string(d)?)?;
                }
            }
            fs::rename(&tmp, &path)?;
        }

        Ok(Self { docs, client })
    }

    /// Search by meaning: the query vector → dot product (the vectors are
    /// normalized) → top-k.
    ///
    /// A design boundary: a linear scan over all documents (brute force), without
    /// ANN. For a local archive this is deliberate: ~10⁵ documents × 768D ≈ tens of
    /// ms per dot scan — acceptable. Noticeably beyond that mark one needs
    /// quantization/ANN (sqlite-vec, for example) — and first of all a cache of the
    /// index itself: `open_or_update` re-reads the whole JSONL on every query.
    /// The similarity below which a match is NOT a match.
    ///
    /// Without it the semantic search always returns the top-k nearest vectors — even when
    /// nothing in the archive resembles the query. The owner asked for «капрезе», which was
    /// never said, and got five confident results (all nonsense). «Nothing was found» is a
    /// full-fledged answer, and it must be given by the code.
    ///
    /// The default comes from the measurement (bge-m3, live archive): garbage tops out at
    /// 0.42–0.56, correct hits start at 0.58. The threshold stands between them, and it is
    /// deliberately on the lower side — a missing result is more expensive than an extra one,
    /// because the human sees the extra one and dismisses it himself.
    ///
    /// `LOCALVOX_SEARCH_MIN_SIMILARITY` — the knob. It depends on the model: change the model
    /// and the number must be re-measured, not guessed.
    fn min_similarity() -> f32 {
        std::env::var("LOCALVOX_SEARCH_MIN_SIMILARITY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.55f32)
            .clamp(0.0, 1.0)
    }

    pub fn search(&self, query: &str, k: usize) -> Result<Vec<SemanticHit>> {
        if self.docs.is_empty() {
            return Ok(Vec::new());
        }
        let qv = self
            .embed_query(query)
            .context("vectorizing the query (is Ollama unavailable?)")?;
        // The cosine between vectors of different dimensions is not defined; the zip
        // in dot() would silently truncate to the prefix. The model could have changed
        // its dimension under the same name (a re-upload, a different server) — better
        // a loud error.
        if let Some(d0) = self.docs.first() {
            if qv.len() != d0.v.len() {
                bail!(
                    "the dimension of the embedding model {} has changed: query {}D, index {}D; \
                     delete index/semantic.jsonl to reindex",
                    self.client.model(),
                    qv.len(),
                    d0.v.len()
                );
            }
        }
        let mut scored: Vec<(f32, &Doc)> = self.docs.iter().map(|d| (dot(&qv, &d.v), d)).collect();
        // top-k without a full sort: select_nth is O(N), we sort only the head
        let k = k.min(scored.len());
        if k == 0 {
            return Ok(Vec::new());
        }
        if k < scored.len() {
            scored.select_nth_unstable_by(k - 1, |a, b| b.0.total_cmp(&a.0));
            scored.truncate(k);
        }
        scored.sort_by(|a, b| b.0.total_cmp(&a.0));
        // Everything below the threshold is not a weak match, it is NOT A MATCH. Returning it
        // «just in case» is what turned the semantic search into a generator of confident
        // nonsense.
        let floor = Self::min_similarity();
        let kept = scored.iter().filter(|(s, _)| *s >= floor).count();
        if kept < scored.len() {
            tracing::debug!(
                "semantic: {} of {} hits are below the similarity floor {floor:.2} — dropped",
                scored.len() - kept,
                scored.len()
            );
        }
        Ok(scored
            .into_iter()
            .filter(|(s, _)| *s >= floor)
            .take(k)
            .map(|(score, d)| SemanticHit {
                session: d.session.clone(),
                kind: d.kind.clone(),
                // only transcript lines have a timecode
                start_sec: (d.kind == "transcript").then_some(d.start_sec),
                end_sec: (d.kind == "transcript" && d.end_sec > d.start_sec)
                    .then_some(d.end_sec),
                text: d.text.clone(),
                score,
            })
            .collect())
    }

    fn embed_query(&self, query: &str) -> Result<Vec<f32>> {
        let capped: String = query.chars().take(MAX_DOC_CHARS).collect();
        Ok(self
            .client
            .embed(&[capped.as_str()])?
            .into_iter()
            .next()
            .context("empty embed response")?)
    }
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dot_product_of_normalized_vectors_ranks_similarity() {
        let a = [1.0f32, 0.0];
        let b = [0.9486833f32, 0.31622776]; // ~18°
        let c = [0.0f32, 1.0]; // 90°
        assert!(dot(&a, &b) > dot(&a, &c));
        assert!((dot(&a, &a) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn deleted_session_docs_are_purged_from_index_and_file() {
        let dir = tempfile::tempdir().unwrap();
        let work = dir.path();
        // there are no sessions on disk at all
        fs::create_dir_all(work.join("sessions")).unwrap();
        // but the index contains a document of a «deleted» session
        let idx_dir = work.join("index");
        fs::create_dir_all(&idx_dir).unwrap();
        // The model is taken from the SAME place the index does — otherwise the test would
        // silently stop testing what it thinks: a header with a foreign model makes the index
        // rebuild from scratch, and the deletion path is never reached (that is exactly what
        // happened when the default model changed).
        let header = serde_json::to_string(&Header {
            model: EmbedClient::from_env().model().to_string(),
            dim: 2,
        })
        .unwrap();
        let dead = serde_json::to_string(&Doc {
            session: "20260101_deleted".into(),
            best_id: 1,
            kind: "transcript".into(),
            start_sec: 0.0,
            end_sec: 0.0,
            text: "секретный разговор, который пользователь удалил".into(),
            v: vec![1.0, 0.0],
            md_mtime: 0,
        })
        .unwrap();
        fs::write(
            idx_dir.join("semantic.jsonl"),
            format!("{header}\n{dead}\n"),
        )
        .unwrap();

        // Ollama is not needed: there are no live sessions → embed is not called
        let idx = SemanticIndex::open_or_update(work).unwrap();
        assert!(
            !idx.docs.iter().any(|d| d.session == "20260101_deleted"),
            "the document of a deleted session stayed in the index (privacy)"
        );
        // and the file is rewritten without the dead text
        let body = fs::read_to_string(idx_dir.join("semantic.jsonl")).unwrap();
        assert!(
            !body.contains("секретный разговор"),
            "the text of a deleted session stayed in semantic.jsonl"
        );
    }

    #[test]
    fn header_roundtrip_and_doc_shape() {
        let h = Header {
            model: "m".into(),
            dim: 3,
        };
        let s = serde_json::to_string(&h).unwrap();
        let h2: Header = serde_json::from_str(&s).unwrap();
        assert_eq!(h2.model, "m");
        assert_eq!(h2.dim, 3);
        let d = Doc {
            session: "s".into(),
            best_id: 2,
            kind: "summary".into(),
            start_sec: 1.5,
            end_sec: 0.0,
            text: "т".into(),
            v: vec![0.1, 0.2],
            md_mtime: 42,
        };
        let s = serde_json::to_string(&d).unwrap();
        let d2: Doc = serde_json::from_str(&s).unwrap();
        assert_eq!(d2.best_id, 2);
        assert_eq!(d2.v.len(), 2);
    }
}
