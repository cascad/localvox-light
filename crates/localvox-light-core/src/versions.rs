//! Versions of transcript processing (F8/P2, WP-A2).
//!
//! Every result of processing the primary audio/text is a new version; the sources are
//! never mutated. Files: `<session>/transcripts/vNNN-<label>.jsonl`, the manifest is
//! `<session>/versions.json` (model, parameters, date, parents, the `best` pointer).
//! Consumers: slow-lane ASR jobs (WP-A3), LLM processing (WP-A4), export (WP-A5).
//!
//! A two-phase write: `next_version()` allocates an id and a path → the caller writes the
//! file → `commit()` atomically appends the record to the manifest. An uncommitted file
//! with no record in the manifest is the garbage of an unfinished job and is safe to
//! overwrite.

use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

const MANIFEST: &str = "versions.json";
const TRANSCRIPTS_DIR: &str = "transcripts";

// ─────────────────────────── the cook recipe ───────────────────────────
//
// The defaults for slicing ASR windows live here rather than in `cook` (that one sits
// behind the `onnx` feature), because the auto-cook depends on them too: a session cooked
// with DIFFERENT windows has to be re-cooked. Without this an improvement of the engine
// never reaches the archive — a version with the same label already exists, the cook is
// skipped, and the whole archive stays on the old recipe forever (found by the WP-C14
// review: a change of windows 180→30 s would not have reached a single already cooked
// session).

/// The version label the cook puts by default.
pub const DEFAULT_COOK_LABEL: &str = "gigaam-int8";
/// Hard ceiling of the inference window, sec.
pub const DEFAULT_MAX_WINDOW_SEC: f64 = 15.0;
/// After this duration the window closes at the first VAD silence, sec.
pub const DEFAULT_MIN_CUT_SEC: f64 = 8.0;
/// How much continuous silence counts as a window boundary, ms.
pub const DEFAULT_SILENCE_MS: u32 = 500;

/// The revision of the way we cook — bumped when the ALGORITHM ITSELF changes, not its
/// parameters. Without it an improvement never reaches the archive: the parameters are
/// the same, the recipe matched, the session was skipped.
///
/// r2 (2026-07-12): utterances without a single real word («.», «Т.») no longer get into
/// the transcript — that is recognition noise, not speech.
/// r3 (2026-07-12): a transcript line is a PHRASE with an exact timecode from the CTC
/// frames, and not a whole window of up to 30 s (the player and the search landed at the
/// start of the window).
/// r4 (2026-07-12): numbers are no longer considered noise — GigaAM writes them as digits
/// with a space («1 000 000»), and the noise filter threw such a line out entirely.
/// r5 (2026-07-13): the word timecodes from the CTC frames are fixed.
/// r6 (2026-07-13): slicing into phrases is THROWN OUT — CTC timecodes are unreliable in
/// both directions (the phrase «Что здесь / есть?» drifted apart by 13 s). A line = a
/// window, its boundaries are counted in samples, so the player plays exactly what is
/// written. The windows are reduced to 15/8 s — by the benchmark that also gives the best
/// WER on conversational speech (6.13 % against 7.28 % at 30/15), and the line becomes
/// manageable to look at.
const COOK_REV: &str = "r6";

/// The fingerprint of how the transcript was obtained: label + slicing parameters +
/// algorithm revision. Written into `VersionEntry.params.recipe`; a divergence = it was
/// cooked the old way, which means it is time to re-cook.
///
/// `speakers` — whether the speakers were labelled (diarization). That is part of the
/// recipe, not a decoration: a transcript with names and a transcript without them are
/// DIFFERENT transcripts, and different summaries come out of them. Once a diarization
/// model appears, the archive gets labelled by itself, through the same migration
/// mechanism as a change of language. The suffix is appended ONLY when labelling is
/// enabled, so for those who have no model the recipe stays as it was and no needless
/// re-cook happens.
pub fn cook_recipe(
    label: &str,
    max_window_sec: f64,
    min_cut_sec: f64,
    silence_ms: u32,
    speakers: bool,
) -> String {
    let spk = if speakers { "-spk" } else { "" };
    format!("{label}/w{max_window_sec:.0}-c{min_cut_sec:.0}-s{silence_ms}-{COOK_REV}{spk}")
}

/// The recipe the auto-cook cooks with (without explicit parameters on the command line).
pub fn default_cook_recipe() -> String {
    cook_recipe(
        DEFAULT_COOK_LABEL,
        DEFAULT_MAX_WINDOW_SEC,
        DEFAULT_MIN_CUT_SEC,
        DEFAULT_SILENCE_MS,
        crate::diarize::enabled(),
    )
}

#[derive(Serialize, Deserialize, Default)]
pub struct VersionsManifest {
    pub versions: Vec<VersionEntry>,
    /// The id of the version the derivatives are computed from (summary, index, export).
    pub best: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct VersionEntry {
    pub id: u32,
    /// A short label: `fast`, `gigaam-int8`, `refined`, `merged`…
    pub label: String,
    /// The file name in `transcripts/`.
    pub file: String,
    /// The model/tool that produced the version (`gigaam-v3-e2e-ctc.int8`, `qwen3-30b`…).
    pub model: String,
    /// Arbitrary run parameters (windows, temperature, template…).
    #[serde(default)]
    pub params: serde_json::Value,
    pub created_at: String,
    /// Parent versions (for merge/refine); empty — built from the audio.
    #[serde(default)]
    pub parents: Vec<u32>,
}

/// A line of a transcript version (jsonl): written by the cook (`cook`), read by export
/// and by the LLM processing. The timecodes are in the source's audio timeline.
#[derive(Serialize, Deserialize, Clone)]
pub struct TranscriptLine {
    pub source_id: u8,
    pub start_sec: f64,
    pub end_sec: f64,
    pub text: String,
    /// WHO said it — if we know (diarization, `diarize`).
    ///
    /// Empty means we do not know, and that is an HONEST answer. Right now a line is
    /// marked only by the SOURCE of the sound (`[Я]` / `[Собеседники]`), and on a call of
    /// four people the three others are one faceless «Собеседники». Because of that the
    /// summary cannot write «Иван взял миграцию»: we do not know that it was Иван who
    /// said it.
    ///
    /// A name here is either «Участник 2» (diarization separated the voices but does not
    /// know the people) or a real name (a human named it once, see `profiles`). Inventing
    /// names is forbidden: attributing someone else's words to a living person is not a
    /// typo, it is slander.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
}

/// Reads the jsonl file of a version, sorting the lines by their start.
pub fn read_transcript_lines(path: &std::path::Path) -> std::io::Result<Vec<TranscriptLine>> {
    let text = fs::read_to_string(path)?;
    let mut lines: Vec<TranscriptLine> = Vec::new();
    for raw in text.lines().filter(|l| !l.trim().is_empty()) {
        match serde_json::from_str(raw) {
            Ok(l) => lines.push(l),
            Err(e) => tracing::warn!("a broken transcript line was skipped: {e}"),
        }
    }
    lines.sort_by(|a, b| a.start_sec.total_cmp(&b.start_sec));
    Ok(lines)
}

pub struct VersionStore {
    session_dir: PathBuf,
}

impl VersionStore {
    /// Opens the session's store, creating `transcripts/` if necessary.
    pub fn open(session_dir: impl Into<PathBuf>) -> std::io::Result<Self> {
        let session_dir = session_dir.into();
        fs::create_dir_all(session_dir.join(TRANSCRIPTS_DIR))?;
        Ok(Self { session_dir })
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.session_dir.join(MANIFEST)
    }

    /// A lenient load for READING: a broken/missing manifest → an empty one.
    pub fn load(&self) -> VersionsManifest {
        let Ok(bytes) = fs::read(self.manifest_path()) else {
            return VersionsManifest::default();
        };
        match serde_json::from_slice(&bytes) {
            Ok(m) => m,
            Err(e) => {
                tracing::warn!("versions.json is corrupted ({e}) — starting from an empty one");
                VersionsManifest::default()
            }
        }
    }

    /// Whether the session is cooked by this recipe. We look at ALL the versions, not at
    /// `best`: best usually points at a derivative (`refined`) that has no cook recipe at
    /// all — a check by best would loop the re-cook forever.
    pub fn has_recipe(&self, recipe: &str) -> bool {
        self.load()
            .versions
            .iter()
            .any(|v| v.params.get("recipe").and_then(|r| r.as_str()) == Some(recipe))
    }

    /// A strict load for WRITING: a missing manifest is normal (an empty one), but a
    /// corrupted one is an error. Otherwise a write «from empty» would overwrite the
    /// existing versions and derivatives (P2/P3 — the primary data is immutable).
    fn load_strict(&self) -> std::io::Result<VersionsManifest> {
        let bytes = match fs::read(self.manifest_path()) {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Ok(VersionsManifest::default());
            }
            Err(e) => return Err(e),
        };
        serde_json::from_slice(&bytes).map_err(|e| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!(
                    "versions.json is corrupted — the write is aborted so as not to lose the history: {e}"
                ),
            )
        })
    }

    /// The next free id and the file path for it. The file does not exist yet — the
    /// caller writes it himself, then calls [`commit`](Self::commit).
    /// An error on a corrupted manifest: we do not allocate an id «from empty».
    pub fn next_version(&self, label: &str) -> std::io::Result<(u32, PathBuf)> {
        let manifest = self.load_strict()?;
        let id = manifest.versions.iter().map(|v| v.id).max().unwrap_or(0) + 1;
        let file = format!("v{id:03}-{}.jsonl", sanitize_label(label));
        Ok((id, self.session_dir.join(TRANSCRIPTS_DIR).join(file)))
    }

    /// Atomically appends the version to the manifest. The version file must already be
    /// on disk.
    pub fn commit(&self, entry: VersionEntry) -> std::io::Result<()> {
        let path = self.session_dir.join(TRANSCRIPTS_DIR).join(&entry.file);
        if !path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("the version file was not written: {}", path.display()),
            ));
        }
        let mut manifest = self.load_strict()?;
        if manifest.versions.iter().any(|v| v.id == entry.id) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                format!("version v{} is already in the manifest", entry.id),
            ));
        }
        manifest.versions.push(entry);
        self.save(&manifest)
    }

    /// Marks a version as best (the derivatives are computed from it).
    pub fn set_best(&self, id: u32) -> std::io::Result<()> {
        let mut manifest = self.load_strict()?;
        if !manifest.versions.iter().any(|v| v.id == id) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("there is no version v{id}"),
            ));
        }
        manifest.best = Some(id);
        self.save(&manifest)
    }

    /// The path to a version file by id.
    pub fn resolve(&self, id: u32) -> Option<PathBuf> {
        self.load()
            .versions
            .iter()
            .find(|v| v.id == id)
            .map(|v| self.session_dir.join(TRANSCRIPTS_DIR).join(&v.file))
    }

    /// The working version: the explicit best, otherwise the last committed one.
    pub fn best(&self) -> Option<VersionEntry> {
        let manifest = self.load();
        let id = manifest
            .best
            .or_else(|| manifest.versions.iter().map(|v| v.id).max())?;
        manifest.versions.into_iter().find(|v| v.id == id)
    }

    fn save(&self, manifest: &VersionsManifest) -> std::io::Result<()> {
        let path = self.manifest_path();
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(manifest)?)?;
        fs::rename(&tmp, &path)
    }

    /// Re-label a speaker in ALL versions: «Участник 2» → «Иван».
    ///
    /// The audio does not change because of it, there is nothing to recognize anew — one
    /// label changes. Re-cooking half an hour of audio for the sake of a name would be
    /// mockery, and then nobody would use the names at all, and the summary would stay
    /// about «Участник 2» forever.
    ///
    /// In all versions, not only in the working one: a session has several of them (the
    /// raw one, the cleaned-up one), and a person renamed in one would stay «Участник 2»
    /// in another — that is, two different people in one recording.
    ///
    /// Returns how many lines were re-labelled.
    pub fn relabel_speaker(&self, from: &str, to: &str) -> std::io::Result<usize> {
        let mut total = 0;
        for v in self.load().versions {
            let path = self.session_dir.join(TRANSCRIPTS_DIR).join(&v.file);
            let Ok(lines) = read_transcript_lines(&path) else {
                continue;
            };
            if !lines.iter().any(|l| l.speaker.as_deref() == Some(from)) {
                continue;
            }
            let tmp = path.with_extension("jsonl.tmp");
            {
                let mut w = std::io::BufWriter::new(fs::File::create(&tmp)?);
                for mut l in lines {
                    if l.speaker.as_deref() == Some(from) {
                        l.speaker = Some(to.to_string());
                        total += 1;
                    }
                    serde_json::to_writer(&mut w, &l)?;
                    std::io::Write::write_all(&mut w, b"\n")?;
                }
                std::io::Write::flush(&mut w)?;
            }
            fs::rename(&tmp, &path)?;
        }
        Ok(total)
    }
}

/// The label goes into the file name — only [a-z0-9-], everything else becomes `-`.
fn sanitize_label(label: &str) -> String {
    let s: String = label
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let trimmed = s.trim_matches('-');
    if trimmed.is_empty() {
        "version".into()
    } else {
        trimmed.to_string()
    }
}

pub fn now_rfc3339() -> String {
    chrono::Local::now().to_rfc3339()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;
    use tempfile::tempdir;

    fn entry(id: u32, label: &str, file: &str, parents: Vec<u32>) -> VersionEntry {
        VersionEntry {
            id,
            label: label.into(),
            file: file.into(),
            model: "test-model".into(),
            params: serde_json::json!({}),
            created_at: now_rfc3339(),
            parents,
        }
    }

    fn write_version(store: &VersionStore, label: &str) -> VersionEntry {
        let (id, path) = store.next_version(label).unwrap();
        fs::write(&path, b"{}\n").unwrap();
        let e = entry(
            id,
            label,
            &path.file_name().unwrap().to_string_lossy(),
            vec![],
        );
        store.commit(e.clone()).unwrap();
        e
    }

    /// A person's name changes in ALL versions at once. Otherwise he would stay «Иван» in
    /// the raw transcript and «Участник 2» in the cleaned-up one — two different people in
    /// one recording.
    #[test]
    fn naming_a_voice_renames_it_in_every_version() {
        let dir = tempdir().unwrap();
        let store = VersionStore::open(dir.path()).unwrap();

        let line = |speaker: Option<&str>, text: &str| TranscriptLine {
            source_id: 1,
            start_sec: 0.0,
            end_sec: 1.0,
            text: text.into(),
            speaker: speaker.map(str::to_string),
        };
        for label in ["gigaam-int8", "refined"] {
            let (id, path) = store.next_version(label).unwrap();
            let body: String = [
                line(Some("Участник 2"), "миграцию беру"),
                line(Some("Я"), "хорошо"),
                line(None, "неразборчиво"),
            ]
            .iter()
            .map(|l| serde_json::to_string(l).unwrap() + "\n")
            .collect();
            fs::write(&path, body).unwrap();
            store
                .commit(entry(
                    id,
                    label,
                    &path.file_name().unwrap().to_string_lossy(),
                    vec![],
                ))
                .unwrap();
        }

        assert_eq!(store.relabel_speaker("Участник 2", "Иван").unwrap(), 2);
        for v in store.load().versions {
            let lines =
                read_transcript_lines(&dir.path().join("transcripts").join(&v.file)).unwrap();
            assert_eq!(lines[0].speaker.as_deref(), Some("Иван"));
            assert_eq!(
                lines[0].text, "миграцию беру",
                "the text MUST NOT have been touched"
            );
            assert_eq!(
                lines[1].speaker.as_deref(),
                Some("Я"),
                "someone else's voice was hit"
            );
            assert_eq!(
                lines[2].speaker, None,
                "a name was attributed to a nameless one"
            );
        }
    }

    #[test]
    fn ids_are_sequential_and_files_named_by_label() {
        let dir = tempdir().unwrap();
        let store = VersionStore::open(dir.path()).unwrap();
        let v1 = write_version(&store, "fast");
        let v2 = write_version(&store, "gigaam int8!");
        assert_eq!((v1.id, v2.id), (1, 2));
        assert_eq!(v1.file, "v001-fast.jsonl");
        assert_eq!(v2.file, "v002-gigaam-int8.jsonl");
    }

    #[test]
    fn commit_requires_existing_file() {
        let dir = tempdir().unwrap();
        let store = VersionStore::open(dir.path()).unwrap();
        let e = entry(1, "fast", "v001-fast.jsonl", vec![]);
        assert!(store.commit(e).is_err());
    }

    #[test]
    fn commit_rejects_duplicate_id() {
        let dir = tempdir().unwrap();
        let store = VersionStore::open(dir.path()).unwrap();
        let v1 = write_version(&store, "fast");
        let dup = entry(v1.id, "dup", &v1.file, vec![]);
        assert!(store.commit(dup).is_err());
    }

    #[test]
    fn best_defaults_to_latest_until_set() {
        let dir = tempdir().unwrap();
        let store = VersionStore::open(dir.path()).unwrap();
        let v1 = write_version(&store, "fast");
        let v2 = write_version(&store, "accurate");
        assert_eq!(store.best().unwrap().id, v2.id);
        store.set_best(v1.id).unwrap();
        assert_eq!(store.best().unwrap().id, v1.id);
        // set_best on a non-existent one — an error
        assert!(store.set_best(99).is_err());
    }

    #[test]
    fn resolve_returns_path_inside_transcripts() {
        let dir = tempdir().unwrap();
        let store = VersionStore::open(dir.path()).unwrap();
        let v1 = write_version(&store, "fast");
        let p = store.resolve(v1.id).unwrap();
        assert!(p.ends_with(Path::new("transcripts").join("v001-fast.jsonl")));
        assert!(p.exists());
        assert!(store.resolve(42).is_none());
    }

    #[test]
    fn manifest_survives_reload_and_records_parents() {
        let dir = tempdir().unwrap();
        {
            let store = VersionStore::open(dir.path()).unwrap();
            let v1 = write_version(&store, "fast");
            let v2 = write_version(&store, "accurate");
            let (id3, path3) = store.next_version("merged").unwrap();
            fs::write(&path3, b"{}\n").unwrap();
            store
                .commit(entry(
                    id3,
                    "merged",
                    &path3.file_name().unwrap().to_string_lossy(),
                    vec![v1.id, v2.id],
                ))
                .unwrap();
        }
        // a new store reads the same manifest
        let store = VersionStore::open(dir.path()).unwrap();
        let m = store.load();
        assert_eq!(m.versions.len(), 3);
        assert_eq!(m.versions[2].parents, vec![1, 2]);
    }

    #[test]
    fn corrupted_manifest_degrades_to_empty() {
        let dir = tempdir().unwrap();
        let store = VersionStore::open(dir.path()).unwrap();
        fs::write(store.manifest_path(), b"{ not json").unwrap();
        assert!(store.load().versions.is_empty());
    }
}
