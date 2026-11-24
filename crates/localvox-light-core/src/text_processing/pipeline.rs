//! Durable, text-only derivatives. Acoustic caches and source versions stay immutable.
use super::{sbert, validate_lines, validate_text, TextProcessor};
use crate::{
    artifacts,
    incremental::Identity,
    processing, progress,
    versions::{self, TranscriptLine, VersionEntry, VersionStore},
};
use anyhow::{ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Instant,
};

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[derive(Clone)]
struct ModelSpec {
    directory: PathBuf,
    key: String,
}
impl ModelSpec {
    fn resolve() -> Result<Self> {
        let directory = crate::model_paths::default_dir("sbert");
        ensure!(
            crate::model_paths::component("sbert").ready(&directory),
            "SbertPuncCase не готов: {}; выполните vox setup",
            directory.display()
        );
        Self::at(directory)
    }
    fn at(directory: PathBuf) -> Result<Self> {
        let files: BTreeMap<_, _> = ["model.safetensors", "tokenizer.json", "config.json"]
            .into_iter()
            .map(|name| (name.to_string(), directory.join(name)))
            .collect();
        let key = Identity::from_files(
            sbert::ENGINE.into(),
            "ru".into(),
            "punctuation/v1".into(),
            files,
            || false,
        )?
        .fingerprint()?;
        Ok(Self { directory, key })
    }
    fn load(&self) -> Result<Box<dyn TextProcessor>> {
        let model = sbert::SbertPuncCase::new(&self.directory)?;
        ensure!(
            Self::at(self.directory.clone())?.key == self.key,
            "модель пунктуации изменилась при загрузке"
        );
        Ok(Box::new(model))
    }
}

/// A loaded, immutable model remains pinned while processing the same active session.
/// Reuse its verified identity, not a cached path/mtime. New owners/loadings rehash files.
#[derive(Default)]
pub struct Runtime {
    owner: Option<PathBuf>,
    spec: Option<ModelSpec>,
    processor: Option<Box<dyn TextProcessor>>,
}
impl Runtime {
    pub fn clear(&mut self) {
        self.processor = None;
        self.spec = None;
        self.owner = None;
    }
    fn selection(&mut self, session: &Path) -> Result<ModelSpec> {
        if self.owner.as_deref() == Some(session) && self.processor.is_some() {
            return self.spec.clone().context("loaded punctuation identity");
        }
        let spec = ModelSpec::resolve()?;
        if self.owner.as_deref() != Some(session)
            || self.spec.as_ref().is_none_or(|s| s.key != spec.key)
        {
            self.clear();
        }
        self.owner = Some(session.into());
        self.spec = Some(spec.clone());
        Ok(spec)
    }
    pub fn partial(
        &mut self,
        session: &Path,
        snapshot: &mut crate::incremental::Snapshot,
    ) -> Result<()> {
        if !crate::transcription::Choice::for_session(session)?.punctuation {
            self.clear();
            return Ok(());
        }
        let raw = snapshot
            .original_lines
            .as_ref()
            .unwrap_or(&snapshot.lines)
            .clone();
        if raw.is_empty() {
            return Ok(());
        }
        let mut operation = crate::journal::Operation::start(
            session,
            "transcribe",
            "Пунктуация готовой части · SbertPuncCase",
            None,
        );
        let result = (|| {
            let spec = self.selection(session)?;
            let source: Vec<_> = raw
                .iter()
                .map(|line| TranscriptLine {
                    source_id: line.source_id,
                    start_sec: line.start_sample as f64 / 16000.0,
                    end_sec: line.end_sample as f64 / 16000.0,
                    text: line.text.clone(),
                    speaker: None,
                })
                .collect();
            let styled = process_lines(
                session,
                &source,
                &spec.key,
                &mut self.processor,
                || spec.load(),
                None,
            )?;
            let mut lines = raw.clone();
            for (line, styled) in lines.iter_mut().zip(styled) {
                line.text = styled.text;
            }
            snapshot.lines = lines;
            snapshot.original_lines = Some(raw);
            snapshot.text_identity = Some(spec.key);
            Ok(())
        })();
        if result.is_err() {
            operation.failed();
        }
        result
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Cached {
    source: String,
    model: String,
    text: String,
    checksum: String,
}

fn process_lines<F>(
    session: &Path,
    raw: &[TranscriptLine],
    key: &str,
    processor: &mut Option<Box<dyn TextProcessor>>,
    factory: F,
    stage: Option<progress::Stage>,
) -> Result<Vec<TranscriptLine>>
where
    F: FnOnce() -> Result<Box<dyn TextProcessor>>,
{
    ensure!(
        key.len() == 64 && key.bytes().all(|b| b.is_ascii_hexdigit()),
        "invalid punctuation cache identity"
    );
    let root = session.join("punctuation/cache").join(key);
    fs::create_dir_all(&root)?;
    let mut factory = Some(factory);
    let mut result = Vec::with_capacity(raw.len());
    let mut signal = Instant::now();
    for (index, line) in raw.iter().enumerate() {
        let source = hash(line.text.as_bytes());
        let path = root.join(format!("{source}.json"));
        let cached = fs::read(&path)
            .ok()
            .and_then(|b| serde_json::from_slice::<Cached>(&b).ok())
            .filter(|c| {
                c.source == source
                    && c.model == key
                    && c.checksum == hash(c.text.as_bytes())
                    && validate_text(&line.text, &c.text).is_ok()
            });
        let text = if let Some(cached) = cached {
            cached.text
        } else {
            if processor.is_none() {
                *processor = Some(factory.take().context("punctuation factory consumed")?()?);
            }
            let text = processor
                .as_deref()
                .context("punctuation model")?
                .process(&line.text)?;
            validate_text(&line.text, &text)?;
            let value = Cached {
                source,
                model: key.into(),
                checksum: hash(text.as_bytes()),
                text: text.clone(),
            };
            artifacts::atomic_write(&path, &serde_json::to_vec(&value)?)?;
            text
        };
        let mut output = line.clone();
        output.text = text;
        result.push(output);
        if signal.elapsed().as_secs() >= 1 {
            if let Some(stage) = stage {
                progress::progress(session, stage, (index + 1) as u32, raw.len() as u32);
            }
            signal = Instant::now();
        }
    }
    validate_lines(raw, &result)?;
    Ok(result)
}

pub fn process_session(session: &Path) -> Result<Option<u32>> {
    ensure!(
        crate::transcription::Choice::for_session(session)?.punctuation,
        "пунктуация не включена для этой записи"
    );
    process_session_with(session, || {
        let spec = ModelSpec::resolve()?;
        Ok((spec.key.clone(), move || spec.load()))
    })
}

/// Injection boundary for model-free workflow regressions; not selected by configuration.
pub fn process_session_with<S, F>(session: &Path, selection: S) -> Result<Option<u32>>
where
    S: FnOnce() -> Result<(String, F)>,
    F: FnOnce() -> Result<Box<dyn TextProcessor>>,
{
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(session.join(".cook.lock"))?;
    lock.lock().context("punctuation version lock")?;
    progress::mark(
        session,
        progress::Stage::Punctuation,
        progress::StageState::Running,
        Some("Пунктуация и регистр · SbertPuncCase"),
    );
    let mut operation = crate::journal::Operation::start(
        session,
        "punctuation",
        "Пунктуация и регистр · SbertPuncCase",
        None,
    );
    let result = (|| {
        let raw = processing::inspect(session, processing::TRANSCRIPT);
        ensure!(raw.complete(), "нет проверенной исходной расшифровки");
        let store = VersionStore::open(session)?;
        let source = raw.source;
        let original = source
            .map(|id| -> Result<_> {
                let path = store
                    .resolve(id)
                    .context("исходная ASR-версия не найдена")?;
                let bytes = fs::read(path)?;
                Ok((bytes.clone(), artifacts::transcript(&bytes)?))
            })
            .transpose()?;
        let Some((bytes, lines)) =
            original.filter(|(_, lines)| lines.iter().any(|l| !l.text.trim().is_empty()))
        else {
            let reason = "Распознанной речи нет; пунктуация пропущена";
            processing::record(
                session,
                processing::PUNCTUATED,
                "punctuation/no-speech",
                processing::Outcome::Nothing,
                Some(reason.into()),
                source,
            )?;
            progress::mark(
                session,
                progress::Stage::Punctuation,
                progress::StageState::Skipped,
                Some(reason),
            );
            return Ok(source);
        };
        let source = source.context("ASR version id")?;
        let (key, factory) = selection()?;
        let input = hash(&bytes);
        let recipe = format!("{}/{key}/{input}", sbert::ENGINE);
        for entry in store.load_strict()?.versions {
            if entry.label == processing::PUNCTUATED
                && entry.parents == [source]
                && entry.params["recipe"].as_str() == Some(recipe.as_str())
            {
                if let Some(lines_path) = store.resolve(entry.id) {
                    if fs::read(lines_path)
                        .ok()
                        .filter(|bytes| {
                            entry.params["output_sha256"].as_str() == Some(hash(bytes).as_str())
                        })
                        .and_then(|bytes| artifacts::transcript(&bytes).ok())
                        .is_some_and(|derived| validate_lines(&lines, &derived).is_ok())
                    {
                        store.set_best(entry.id)?;
                        processing::record(
                            session,
                            processing::PUNCTUATED,
                            &recipe,
                            processing::Outcome::Ok,
                            None,
                            Some(entry.id),
                        )?;
                        progress::mark(
                            session,
                            progress::Stage::Punctuation,
                            progress::StageState::Done,
                            Some("Пунктуация уже сохранена и проверена"),
                        );
                        return Ok(Some(entry.id));
                    }
                }
            }
        }
        // A failed new derivative must leave a readable original selected.
        store.set_best(source)?;
        let styled = process_lines(
            session,
            &lines,
            &key,
            &mut None,
            factory,
            Some(progress::Stage::Punctuation),
        )?;
        let (id, path) = store.next_version(processing::PUNCTUATED)?;
        let mut data = Vec::new();
        for line in &styled {
            serde_json::to_writer(&mut data, line)?;
            data.push(b'\n');
        }
        artifacts::atomic_write(&path, &data)?;
        store.commit(VersionEntry {
            id,
            label: processing::PUNCTUATED.into(),
            file: path
                .file_name()
                .context("version filename")?
                .to_string_lossy()
                .into(),
            model: sbert::ENGINE.into(),
            params: serde_json::json!({"recipe":recipe,"model_identity":key,"input_sha256":input,"output_sha256":hash(&data)}),
            created_at: versions::now_rfc3339(),
            parents: vec![source],
        })?;
        store.set_best(id)?;
        processing::record(
            session,
            processing::PUNCTUATED,
            &recipe,
            processing::Outcome::Ok,
            None,
            Some(id),
        )?;
        progress::progress(
            session,
            progress::Stage::Punctuation,
            styled.len() as u32,
            styled.len() as u32,
        );
        progress::mark(
            session,
            progress::Stage::Punctuation,
            progress::StageState::Done,
            Some("Пунктуация сохранена; исходная версия сохранена отдельно"),
        );
        Ok(Some(id))
    })();
    if let Err(error) = &result {
        operation.failed();
        progress::mark(
            session,
            progress::Stage::Punctuation,
            progress::StageState::Failed,
            Some(&format!("{error:#}")),
        );
        processing::record(
            session,
            processing::PUNCTUATED,
            "punctuation/failed",
            processing::Outcome::Failed,
            Some(format!("{error:#}")),
            None,
        )
        .with_context(|| format!("пунктуация: {error:#}; не удалось сохранить отказ"))?;
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };

    struct Formatter(Arc<AtomicUsize>);
    impl TextProcessor for Formatter {
        fn name(&self) -> &str {
            "fixture"
        }
        fn process(&self, text: &str) -> Result<String> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Ok(format!("{}.", text.to_uppercase()))
        }
    }
    fn source(model: &str, text: &str) -> (tempfile::TempDir, Vec<u8>) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("meta.json"),
            br#"{"started_at":"2026-11-24T00:00:00Z","sample_rate":16000,"chunks":[]}"#,
        )
        .unwrap();
        let store = VersionStore::open(dir.path()).unwrap();
        let (id, path) = store.next_version("raw").unwrap();
        let line = TranscriptLine {
            source_id: 1,
            start_sec: 2.5,
            end_sec: 7.25,
            text: text.into(),
            speaker: Some("Speaker 2".into()),
        };
        let bytes = serde_json::to_vec(&line).unwrap();
        fs::write(&path, &bytes).unwrap();
        store
            .commit(VersionEntry {
                id,
                label: "raw".into(),
                file: path.file_name().unwrap().to_string_lossy().into(),
                model: model.into(),
                params: serde_json::json!({}),
                created_at: versions::now_rfc3339(),
                parents: vec![],
            })
            .unwrap();
        store.set_best(id).unwrap();
        processing::record(
            dir.path(),
            processing::TRANSCRIPT,
            "raw",
            processing::Outcome::Ok,
            None,
            Some(id),
        )
        .unwrap();
        (dir, bytes)
    }
    #[test]
    fn each_asr_keeps_its_original_and_a_separate_punctuation_version_with_exact_coordinates() {
        for model in ["GigaAM", "T-one", "Vosk"] {
            let (dir, original) = source(model, "привет API 2026");
            let calls = Arc::new(AtomicUsize::new(0));
            let key = hash(b"fixture-model");
            let make = || {
                let calls = calls.clone();
                let key = key.clone();
                move || {
                    Ok((key, move || {
                        Ok(Box::new(Formatter(calls)) as Box<dyn TextProcessor>)
                    }))
                }
            };
            let derived = process_session_with(dir.path(), make()).unwrap().unwrap();
            assert_eq!(derived, 2);
            assert_eq!(process_session_with(dir.path(), make()).unwrap(), Some(2));
            assert_eq!(calls.load(Ordering::SeqCst), 1);
            let store = VersionStore::open(dir.path()).unwrap();
            assert_eq!(fs::read(store.resolve(1).unwrap()).unwrap(), original);
            let lines =
                artifacts::transcript(&fs::read(store.resolve(2).unwrap()).unwrap()).unwrap();
            assert_eq!(lines[0].text, "ПРИВЕТ API 2026.");
            assert_eq!(
                (lines[0].source_id, lines[0].start_sec, lines[0].end_sec),
                (1, 2.5, 7.25)
            );
            assert_eq!(lines[0].speaker.as_deref(), Some("Speaker 2"));
            assert_eq!(store.load_strict().unwrap().versions[1].parents, [1]);
            assert!(processing::is_done(dir.path(), processing::PUNCTUATED));
            // Selecting the original is supported; it does not corrupt the derived receipt.
            store.set_best(1).unwrap();
            assert!(processing::is_done(dir.path(), processing::PUNCTUATED));
        }
    }
    #[test]
    fn changing_words_or_model_failure_never_overwrites_the_source_or_commits_a_derivative() {
        struct Wrong;
        impl TextProcessor for Wrong {
            fn name(&self) -> &str {
                "wrong"
            }
            fn process(&self, _: &str) -> Result<String> {
                Ok("совсем другое 999".into())
            }
        }
        let (dir, bytes) = source("GigaAM", "привет API 2026");
        let result = process_session_with(dir.path(), || {
            Ok((hash(b"bad-model"), || {
                Ok(Box::new(Wrong) as Box<dyn TextProcessor>)
            }))
        });
        assert!(result.is_err());
        let store = VersionStore::open(dir.path()).unwrap();
        assert_eq!(store.load_strict().unwrap().versions.len(), 1);
        assert_eq!(store.best().unwrap().id, 1);
        assert_eq!(fs::read(store.resolve(1).unwrap()).unwrap(), bytes);
        assert!(processing::is_done(dir.path(), processing::TRANSCRIPT));
        assert_eq!(
            processing::inspect(dir.path(), processing::PUNCTUATED).state,
            processing::ArtifactState::Failed
        );
    }
    #[test]
    fn cache_checksums_and_model_identity_are_verified_before_reuse() {
        let (dir, _) = source("Vosk", "проверка 123");
        let store = VersionStore::open(dir.path()).unwrap();
        let raw = artifacts::transcript(&fs::read(store.resolve(1).unwrap()).unwrap()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let first = hash(b"model1");
        let second = hash(b"model2");
        let mut engine = None;
        for key in [&first, &first, &second] {
            process_lines(
                dir.path(),
                &raw,
                key,
                &mut engine,
                || Ok(Box::new(Formatter(calls.clone()))),
                None,
            )
            .unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let cache = dir
            .path()
            .join("punctuation/cache")
            .join(&first)
            .join(format!("{}.json", hash(raw[0].text.as_bytes())));
        let mut value: serde_json::Value =
            serde_json::from_slice(&fs::read(&cache).unwrap()).unwrap();
        value["text"] = "Проверка 123?".into();
        fs::write(&cache, serde_json::to_vec(&value).unwrap()).unwrap();
        let fixed = process_lines(
            dir.path(),
            &raw,
            &first,
            &mut engine,
            || unreachable!(),
            None,
        )
        .unwrap();
        assert_eq!(fixed[0].text, "ПРОВЕРКА 123.");
        assert_eq!(calls.load(Ordering::SeqCst), 3);
    }
    #[test]
    fn empty_speech_never_resolves_or_loads_a_model() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("meta.json"),
            br#"{"started_at":"2026-11-24T00:00:00Z","sample_rate":16000,"chunks":[]}"#,
        )
        .unwrap();
        processing::record(
            dir.path(),
            processing::TRANSCRIPT,
            "empty",
            processing::Outcome::Nothing,
            Some("Нет речи".into()),
            None,
        )
        .unwrap();
        let result = process_session_with(
            dir.path(),
            || -> Result<(String, fn() -> Result<Box<dyn TextProcessor>>)> {
                panic!("model must not be resolved")
            },
        )
        .unwrap();
        assert!(result.is_none());
        assert_eq!(
            processing::inspect(dir.path(), processing::PUNCTUATED).state,
            processing::ArtifactState::Empty
        );
    }

    #[test]
    fn changed_punctuation_version_is_not_recaptured_as_a_verified_model_output() {
        let (dir, original) = source("GigaAM", "проверка 123");
        let calls = Arc::new(AtomicUsize::new(0));
        let run = || {
            process_session_with(dir.path(), || {
                let calls = calls.clone();
                Ok((hash(b"model"), move || {
                    Ok(Box::new(Formatter(calls)) as Box<dyn TextProcessor>)
                }))
            })
            .unwrap()
            .unwrap()
        };
        let first = run();
        let store = VersionStore::open(dir.path()).unwrap();
        let path = store.resolve(first).unwrap();
        let bytes = fs::read_to_string(&path).unwrap().replace("123.", "123?");
        fs::write(&path, bytes).unwrap();
        assert!(!processing::is_done(dir.path(), processing::PUNCTUATED));
        let repaired = run();
        assert_ne!(repaired, first);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "verified text cache avoids another inference"
        );
        assert_eq!(fs::read(store.resolve(1).unwrap()).unwrap(), original);
        let lines =
            artifacts::transcript(&fs::read(store.resolve(repaired).unwrap()).unwrap()).unwrap();
        assert_eq!(lines[0].text, "ПРОВЕРКА 123.");
        assert!(processing::is_done(dir.path(), processing::PUNCTUATED));
    }
}
