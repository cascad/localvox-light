//! Full-text search over the archive (F4, stage 1; WP-B4).
//!
//! Every text artifact of a session is indexed: the lines of the best transcript
//! version (with timecodes), `summary.md` and `processed.md` (by paragraph). The
//! engine is tantivy with Russian stemming («колбаска» finds «колбаски») — the
//! verdict of the tantivy vs sqlite FTS5 spike: FTS5 has no Russian morphology
//! out of the box.
//!
//! The index is a derived artifact (P9): it lives in `<work_dir>/index/tantivy`
//! and is rebuilt from the files as a whole (v1 — full reindex, it is cheap;
//! incrementality — once the archive grows). Detachable (P5): the only input is
//! the session files.

pub mod semantic;

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Schema, Value, FAST, STORED, STRING, TEXT};
use tantivy::tokenizer::{
    Language, LowerCaser, RemoveLongFilter, SimpleTokenizer, Stemmer, StopWordFilter, TextAnalyzer,
};
use tantivy::{doc, Index, IndexWriter, TantivyDocument};

use localvox_light_core::versions::{read_transcript_lines, VersionStore};

/// The derived documents of a session, as text for the index.
///
/// The summary is a file and is indexed as it is written. The readable text is NOT a file any
/// more: it is a delta over a transcript version, and its words are joined at read time. What
/// goes into the index is the SPEECH alone — no speaker labels, no timecodes.
///
/// That is deliberate, not a simplification. The label is the one part of the readable text that
/// moves: rename a source and every line says a different name, while nothing anyone searched for
/// has changed. Indexing it would make the index stale on a rename that touches no artifact at
/// all. And «(00:15)» in the index makes the query «15» match every recording that ran a quarter
/// of a minute.
pub(crate) fn derived_docs(session_dir: &Path) -> Vec<(&'static str, String)> {
    let mut out = Vec::new();
    if let Ok(body) = fs::read_to_string(session_dir.join("summary.md")) {
        out.push(("summary", body));
    }
    if let Ok(lines) = localvox_light_core::readable::lines(session_dir) {
        out.push((
            "processed",
            localvox_light_core::readable::render_speech(&lines),
        ));
    }
    out
}

const TOKENIZER_RU: &str = "lv_ru";

/// Revision of the ANALYZER. It changes — the index is rebuilt.
///
/// Without it, a fix to the analyzer never reached the archive: session files did
/// not change, the index counted as fresh, and search kept working over the old
/// tokens. Exactly the same disease as with the cook recipe, and it is cured the
/// same way — with a revision.
///
/// r2 (13.07.2026): stop words thrown out. Search was finding recordings by «и»,
/// «на», «что» — formally there is a match, but there is no sense in it, and a
/// human rightly did not understand what had been found here.
const INDEX_REV: &str = "r3";

pub struct SearchHit {
    pub session: String,
    /// transcript | summary | processed
    pub kind: String,
    /// Start timecode (sec) for transcript hits; None for md artifacts.
    pub start_sec: Option<f64>,
    /// The END of the fragment. A transcript line is a WINDOW of 8-15 seconds, not a moment:
    /// showing only the start hides what «▶» will actually play and when it will stop.
    pub end_sec: Option<f64>,
    pub snippet: String,
    pub score: f32,
    /// The words that REALLY matched the query — in the exact form they stand in the
    /// text («миграции» for the query «миграция»). The UI highlights them.
    ///
    /// Without this the result looked random: the index searches with stemming, while
    /// the human saw a line that did not contain a single word of the query — and
    /// rightly did not understand what had been found here.
    pub matched: Vec<String>,
}

pub struct SearchIndex {
    index: Index,
    schema_fields: Fields,
    /// Exclusive cross-process lock (`index/tantivy.lock`): while the instance is
    /// alive, nobody will rebuild (remove_dir_all!) the index from under it.
    _lock: fs::File,
}

struct Fields {
    session: tantivy::schema::Field,
    kind: tantivy::schema::Field,
    start_sec: tantivy::schema::Field,
    end_sec: tantivy::schema::Field,
    text: tantivy::schema::Field,
}

fn build_schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let session = b.add_text_field("session", STRING | STORED);
    let kind = b.add_text_field("kind", STRING | STORED);
    let start_sec = b.add_f64_field("start_sec", STORED | FAST);
    let end_sec = b.add_f64_field("end_sec", STORED | FAST);
    let text_opts = (TEXT | STORED).set_indexing_options(
        tantivy::schema::TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER_RU)
            .set_index_option(tantivy::schema::IndexRecordOption::WithFreqsAndPositions),
    );
    let text = b.add_text_field("text", text_opts);
    (
        b.build(),
        Fields {
            session,
            kind,
            start_sec,
            end_sec,
            text,
        },
    )
}

/// The text analyzer — ONE for the whole module: it both indexes and finds the
/// matches for highlighting. Should the two diverge, the highlighting would lie
/// about what the index found.
fn ru_analyzer() -> TextAnalyzer {
    let mut b = TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(40))
        .filter(LowerCaser)
        .dynamic();
    // Stop words — BEFORE the stemmer: the list consists of ordinary word forms («и»,
    // «на», «что»), and after stemming they are no longer those words.
    //
    // Both languages: the archive is bilingual, and the English «the» is just as much
    // noise as the Russian «и». A search that finds a recording by the word «и» finds
    // NOTHING: it returns a match that carries no sense and devalues the whole result
    // list.
    for lang in [Language::Russian, Language::English] {
        if let Some(stop) = StopWordFilter::new(lang) {
            b = b.filter_dynamic(stop);
        }
    }
    b.filter(Stemmer::new(Language::Russian)).build()
}

fn register_ru_tokenizer(index: &Index) {
    index.tokenizers().register(TOKENIZER_RU, ru_analyzer());
}

/// Where in the text the query words stand — WITH MORPHOLOGY TAKEN INTO ACCOUNT (by
/// the same analyzer through which the text got into the index). Returns the bounds
/// in CHARACTERS and the surface form.
fn matches(text: &str, query: &str) -> Vec<(usize, usize, String)> {
    let mut wanted: std::collections::HashSet<String> = Default::default();
    {
        let mut analyzer = ru_analyzer();
        let mut qs = analyzer.token_stream(query);
        while let Some(t) = qs.next() {
            wanted.insert(t.text.clone());
        }
    }
    if wanted.is_empty() {
        return Vec::new();
    }

    let mut out = Vec::new();
    let mut analyzer = ru_analyzer();
    let mut ts = analyzer.token_stream(text);
    while let Some(t) = ts.next() {
        if wanted.contains(&t.text) {
            // Token offsets are in bytes; the UI slices by characters.
            let from = text[..t.offset_from].chars().count();
            let to = text[..t.offset_to].chars().count();
            out.push((from, to, text[t.offset_from..t.offset_to].to_string()));
        }
    }
    out
}

/// The words of the text that matched the query — what the UI highlights. Without
/// repeats and in order of appearance: «миграции, миграцию» is one word shown twice.
pub fn matched_terms(text: &str, query: &str) -> Vec<String> {
    let mut seen: Vec<String> = Vec::new();
    for (_, _, surface) in matches(text, query) {
        if !seen.iter().any(|s| s.eq_ignore_ascii_case(&surface)) {
            seen.push(surface);
        }
    }
    seen
}

impl SearchIndex {
    fn index_dir(work_dir: &Path) -> PathBuf {
        work_dir.join("index").join("tantivy")
    }

    /// Opens the index, rebuilding it if it is missing or older than the session
    /// files. `force` — rebuild unconditionally.
    ///
    /// The index is shared by several processes (daemon, MCP, CLI) — the rebuild
    /// runs under an exclusive file lock, and staleness is re-checked already under
    /// it (a neighbour may have just rebuilt it).
    pub fn open_or_build(work_dir: &Path, force: bool) -> Result<Self> {
        let dir = Self::index_dir(work_dir);
        fs::create_dir_all(dir.parent().unwrap_or(&dir))?;
        let lock_file = fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(false)
            .open(dir.with_extension("lock"))
            .context("index lock file")?;
        lock_bounded(&lock_file, std::time::Duration::from_secs(10))
            .context("acquiring the index lock")?;
        let stale = force || is_stale(work_dir, &dir);
        if stale {
            if dir.exists() {
                fs::remove_dir_all(&dir).context("cleaning up the old index")?;
            }
            fs::create_dir_all(&dir)?;
            let (schema, fields) = build_schema();
            let index = Index::create_in_dir(&dir, schema)?;
            register_ru_tokenizer(&index);
            let me = Self {
                index,
                schema_fields: fields,
                _lock: lock_file,
            };
            let n = me.build(work_dir)?;
            tracing::info!("index rebuilt: {n} documents");
            fs::write(
                dir.join(".built"),
                format!("{INDEX_REV} {}", localvox_light_core::versions::now_rfc3339()),
            )?;
            Ok(me)
        } else {
            let index = Index::open_in_dir(&dir).context("opening the index")?;
            register_ru_tokenizer(&index);
            let (_, fields) = build_schema();
            Ok(Self {
                index,
                schema_fields: fields,
                _lock: lock_file,
            })
        }
    }

    fn build(&self, work_dir: &Path) -> Result<usize> {
        let mut writer: IndexWriter = self.index.writer(64_000_000)?;
        let mut count = 0usize;
        let sessions_root = work_dir.join("sessions");
        let Ok(entries) = fs::read_dir(&sessions_root) else {
            writer.commit()?;
            return Ok(0);
        };
        for e in entries.flatten() {
            let session_dir = e.path();
            if !session_dir.is_dir() {
                continue;
            }
            let session_name = session_dir
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // Lines of the best transcript version
            if let Ok(store) = VersionStore::open(&session_dir) {
                if let Some(best) = store.best() {
                    if let Some(path) = store.resolve(best.id) {
                        if let Ok(lines) = read_transcript_lines(&path) {
                            for l in lines {
                                writer.add_document(doc!(
                                    self.schema_fields.session => session_name.clone(),
                                    self.schema_fields.kind => "transcript",
                                    self.schema_fields.start_sec => l.start_sec,
                                    self.schema_fields.end_sec => l.end_sec,
                                    self.schema_fields.text => l.text,
                                ))?;
                                count += 1;
                            }
                        }
                    }
                }
            }

            // Derived documents — by paragraph
            for (kind, body) in derived_docs(&session_dir) {
                for para in body
                    .split("\n\n")
                    .map(str::trim)
                    .filter(|s| !s.is_empty() && !s.starts_with("<!--"))
                {
                    writer.add_document(doc!(
                        self.schema_fields.session => session_name.clone(),
                        self.schema_fields.kind => kind,
                        self.schema_fields.text => para.to_string(),
                    ))?;
                    count += 1;
                }
            }
        }
        writer.commit()?;
        Ok(count)
    }

    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<SearchHit>> {
        let reader = self.index.reader()?;
        let searcher = reader.searcher();
        let parser = QueryParser::for_index(&self.index, vec![self.schema_fields.text]);
        // The search line is not a query language but a field for a HUMAN. The tantivy
        // parser treats `-`, `+`, `:`, `"` as syntax and rejects whole classes of
        // ordinary phrases outright («Only excluding terms given»), and search answered
        // that with a 400 error — that is, it refused to search. We leave the syntax to
        // those who know it: first we try to parse it as is, and only if that did not
        // work out — we clean it up and search as plain words.
        let q = match parser.parse_query(query) {
            Ok(q) => q,
            Err(e) => {
                let plain = as_words(query);
                if plain.trim().is_empty() {
                    return Ok(Vec::new());
                }
                tracing::debug!(
                    "query «{query}» did not parse as syntax ({e}) — searching as words"
                );
                parser
                    .parse_query(&plain)
                    .with_context(|| format!("parsing the query: {query}"))?
            }
        };
        let top = searcher.search(&q, &TopDocs::with_limit(limit))?;

        let mut hits = Vec::new();
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr)?;
            let get_str = |f| {
                doc.get_first(f)
                    .and_then(|v| v.as_str())
                    .unwrap_or_default()
                    .to_string()
            };
            let text = get_str(self.schema_fields.text);
            hits.push(SearchHit {
                session: get_str(self.schema_fields.session),
                kind: get_str(self.schema_fields.kind),
                start_sec: doc
                    .get_first(self.schema_fields.start_sec)
                    .and_then(|v| v.as_f64()),
                end_sec: doc
                    .get_first(self.schema_fields.end_sec)
                    .and_then(|v| v.as_f64()),
                snippet: make_snippet(&text, query, 160),
                matched: matched_terms(&text, query),
                score,
            });
        }
        Ok(hits)
    }
}

/// A cross-process lock with bounded waiting: while a neighbour spends minutes
/// rebuilding the index, the calling thread (HTTP/auto-cook) must not hang forever —
/// a fast, honest refusal in the style of a 503 «retry later».
pub(crate) fn lock_bounded(f: &fs::File, max: std::time::Duration) -> Result<()> {
    const POLL: std::time::Duration = std::time::Duration::from_millis(200);
    let mut waited = std::time::Duration::ZERO;
    loop {
        match f.try_lock() {
            Ok(()) => return Ok(()),
            Err(std::fs::TryLockError::WouldBlock) => {
                if waited >= max {
                    anyhow::bail!(
                        "busy: the index is being updated by a neighbouring process — retry later"
                    );
                }
                std::thread::sleep(POLL);
                waited += POLL;
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e.into()),
        }
    }
}

/// The index is stale if any session file is not in it by time (mtime newer than the
/// build marker) or if there is no marker at all.
fn is_stale(work_dir: &Path, index_dir: &Path) -> bool {
    let built = index_dir.join(".built");
    // An index built by ANOTHER analyzer is stale, even if not a single session file
    // has changed since then.
    let rev_ok = fs::read_to_string(&built)
        .map(|s| s.split_whitespace().next() == Some(INDEX_REV))
        .unwrap_or(false);
    if !rev_ok {
        return true;
    }
    let Ok(built_meta) = fs::metadata(&built) else {
        return true;
    };
    let Ok(built_time) = built_meta.modified() else {
        return true;
    };
    newest_mtime(&work_dir.join("sessions"))
        .map(|t| t > built_time)
        .unwrap_or(false)
}

/// The freshest mtime of the TEXT artifacts of the sessions. `audio/` and `.part`
/// are skipped: audio is not indexed, and a live recording updates the chunks every
/// few seconds — otherwise an active session would forever invalidate the index
/// (a full rebuild on every warm-up/search).
fn newest_mtime(root: &Path) -> Option<std::time::SystemTime> {
    let mut newest: Option<std::time::SystemTime> = None;
    let entries = fs::read_dir(root).ok()?;
    for e in entries.flatten() {
        let p = e.path();
        let name = e.file_name();
        let t = if p.is_dir() {
            if name == "audio" {
                continue;
            }
            newest_mtime(&p)
        } else {
            if p.extension().and_then(|x| x.to_str()) == Some("part") {
                continue;
            }
            p.metadata().and_then(|m| m.modified()).ok()
        };
        if let Some(t) = t {
            newest = Some(match newest {
                Some(cur) if cur >= t => cur,
                _ => t,
            });
        }
    }
    newest
}

/// The query as PLAIN WORDS: the parser syntax is scrubbed out.
fn as_words(query: &str) -> String {
    const SYNTAX: [char; 14] = [
        '+', '-', ':', '^', '~', '*', '"', '(', ')', '[', ']', '{', '}', '!',
    ];
    query
        .chars()
        .map(|c| if SYNTAX.contains(&c) { ' ' } else { c })
        .collect()
}

/// A window of text around the first REAL match.
///
/// «Real» means found by the same analyzer as the index. Previously the snippet
/// looked for the query word LITERALLY: the index found «миграции» for the query
/// «миграция», while the snippet did not see such a word and showed the BEGINNING OF
/// THE TEXT. The human got a line without a single word of the query in it, and
/// rightly did not understand what had been found here (owner's complaint,
/// 13.07.2026).
pub fn make_snippet(text: &str, query: &str, max_chars: usize) -> String {
    let char_pos = matches(text, query)
        .first()
        .map(|(from, _, _)| *from)
        .unwrap_or(0);
    let chars: Vec<char> = text.chars().collect();
    let start = char_pos.saturating_sub(max_chars / 4);
    let end = (start + max_chars).min(chars.len());
    let mut s: String = chars[start..end].iter().collect();
    if start > 0 {
        s = format!("…{s}");
    }
    if end < chars.len() {
        s.push('…');
    }
    s.replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use localvox_light_core::versions::{now_rfc3339, TranscriptLine, VersionEntry};
    use tempfile::tempdir;

    /// OWNER'S COMPLAINT (13.07.2026): «in most of the results I did not understand
    /// what it found that resembled the query». The reason: the index searches with
    /// STEMMING («миграции» for the query «миграция»), while the snippet looked for the
    /// word LITERALLY — did not find it and showed the BEGINNING OF THE TEXT. The human
    /// saw a line without a single word of the query.
    #[test]
    fn the_snippet_shows_the_match_not_the_beginning_of_the_text() {
        let text = "Начали с планов на квартал и бюджета.                     Потом решили, что миграции займут две недели.";
        let s = make_snippet(text, "миграция", 60);
        assert!(
            s.contains("миграции"),
            "the snippet did not show the word that was found: {s}"
        );
    }

    /// A search that finds a recording by the word «и» finds NOTHING: formally there is
    /// a match, but there is no sense in it. The human looks at the results and does not
    /// understand what was found there — and stops trusting search (owner's complaint,
    /// 13.07.2026).
    #[test]
    fn stop_words_are_not_matches() {
        let dir = tempdir().unwrap();
        make_workdir(dir.path());
        let idx = SearchIndex::open_or_build(dir.path(), true).unwrap();

        // «и», «на», «про» are present in the recordings, but there is nothing to search
        // for by them.
        assert!(
            idx.search("и", 5).unwrap().is_empty(),
            "something was found by the conjunction «и»"
        );
        assert!(idx.search("на", 5).unwrap().is_empty());

        // A meaningful word from the same phrase — is found.
        assert!(!idx.search("колбаски", 5).unwrap().is_empty());

        // And the highlighting stays silent about stop words: to highlight «и» would be
        // to lie that it is exactly what was found.
        assert_eq!(
            matched_terms("Обсудили ресурсные колбаски и диаграммы", "колбаски и планы"),
            vec!["колбаски"]
        );
    }

    /// The search line is a field for a HUMAN, not a query language. The tantivy parser
    /// treats `-` and `"` as syntax and rejects whole ordinary phrases outright — and
    /// search answered that with an error, that is, it refused to search.
    #[test]
    fn a_human_query_with_punctuation_still_searches() {
        let dir = tempdir().unwrap();
        make_workdir(dir.path());
        let idx = SearchIndex::open_or_build(dir.path(), true).unwrap();

        // The parser rejects such a query: «only excluding terms given».
        let hits = idx.search("-колбаски", 5).unwrap();
        assert!(
            !hits.is_empty(),
            "a human query with a hyphen found nothing"
        );

        // Empty after the scrubbing — an honest zero, not an error.
        assert!(idx.search("--- +++", 5).unwrap().is_empty());
    }

    /// The highlighting MUST agree with WHAT THE INDEX FOUND — that is, take morphology
    /// into account. Otherwise it would lie: one word is highlighted while another was
    /// found.
    #[test]
    fn matched_terms_follow_the_same_morphology_as_the_index() {
        let text = "Иван взял миграции на себя, миграцию закончит в пятницу";
        let m = matched_terms(text, "миграция");
        assert_eq!(m, vec!["миграции", "миграцию"], "morphology got lost");

        // There is nothing to highlight for query words that are not in the text — and
        // that is HONEST: this is how a semantic hit is seen as «no word matches».
        assert!(matched_terms(text, "бюджет").is_empty());
        assert!(matched_terms(text, "").is_empty());
    }

    fn make_workdir(dir: &Path) {
        let session = dir.join("sessions/20260711_test");
        fs::create_dir_all(&session).unwrap();
        let store = VersionStore::open(&session).unwrap();
        let (id, path) = store.next_version("gigaam-int8").unwrap();
        let lines = [
            TranscriptLine {
                source_id: 0,
                start_sec: 12.0,
                end_sec: 20.0,
                text: "Обсудили ресурсные колбаски на диаграмме Ганта".into(),
                speaker: None,
            },
            TranscriptLine {
                source_id: 1,
                start_sec: 30.0,
                end_sec: 40.0,
                text: "Про capacity команды поговорим завтра".into(),
                speaker: None,
            },
        ];
        let body: String = lines
            .iter()
            .map(|l| serde_json::to_string(l).unwrap() + "\n")
            .collect();
        fs::write(&path, body).unwrap();
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
        fs::write(
            session.join("summary.md"),
            "<!-- header -->\n\n## Решения\nПеренесли квартальное планирование.\n",
        )
        .unwrap();
    }

    #[test]
    fn finds_stemmed_russian_word_with_timecode() {
        let dir = tempdir().unwrap();
        make_workdir(dir.path());
        let idx = SearchIndex::open_or_build(dir.path(), true).unwrap();
        // «колбаска» ← stemming finds «колбаски»
        let hits = idx.search("колбаска", 10).unwrap();
        assert!(!hits.is_empty(), "stemming did not find «колбаски»");
        assert_eq!(hits[0].kind, "transcript");
        assert_eq!(hits[0].start_sec, Some(12.0));
        assert!(hits[0].snippet.contains("колбаски"));
    }

    #[test]
    fn finds_in_summary_paragraphs() {
        let dir = tempdir().unwrap();
        make_workdir(dir.path());
        let idx = SearchIndex::open_or_build(dir.path(), true).unwrap();
        let hits = idx.search("квартальное планирование", 10).unwrap();
        assert!(hits.iter().any(|h| h.kind == "summary"));
    }

    #[test]
    fn latin_terms_found_too() {
        let dir = tempdir().unwrap();
        make_workdir(dir.path());
        let idx = SearchIndex::open_or_build(dir.path(), true).unwrap();
        let hits = idx.search("capacity", 10).unwrap();
        assert!(!hits.is_empty());
    }

    #[test]
    fn empty_workdir_gives_empty_results() {
        let dir = tempdir().unwrap();
        let idx = SearchIndex::open_or_build(dir.path(), true).unwrap();
        assert!(idx.search("что-нибудь", 5).unwrap().is_empty());
    }
}
