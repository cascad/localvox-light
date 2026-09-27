//! Artifact validation and publication. A successful process exit is not a commit.
//! Receipts live in processing.json; files are published before their receipt.

use std::fs;
use std::io::Write;
use std::path::{Component, Path};

use anyhow::{bail, ensure, Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::processing::{Outcome, Record, PROCESSED, REFINED, SUMMARY, TRANSCRIPT};
use crate::versions::{TranscriptLine, VersionEntry, VersionsManifest};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FileStamp {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
    /// Speaker labels may be renamed without changing the recognized speech.
    #[serde(default)]
    pub transcript: bool,
}

impl FileStamp {
    fn matches(&self, current: &Self) -> bool {
        self.path == current.path
            && self.transcript == current.transcript
            && self.sha256 == current.sha256
            && (self.transcript || self.bytes == current.bytes)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Receipt {
    pub outputs: Vec<FileStamp>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input: Option<FileStamp>,
}

fn relative_path(path: &str) -> Result<&Path> {
    let path = Path::new(path);
    ensure!(
        !path.as_os_str().is_empty()
            && path.components().all(|c| matches!(c, Component::Normal(_))),
        "путь артефакта должен находиться внутри записи"
    );
    Ok(path)
}

/// Strict parsing for acceptance; the lenient reader remains useful for displaying damaged data.
pub fn transcript(bytes: &[u8]) -> Result<Vec<TranscriptLine>> {
    let text = std::str::from_utf8(bytes).context("расшифровка не UTF-8")?;
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(i, line)| {
            let parsed: TranscriptLine = serde_json::from_str(line)
                .with_context(|| format!("повреждена строка {} расшифровки", i + 1))?;
            ensure!(
                parsed.start_sec.is_finite()
                    && parsed.end_sec.is_finite()
                    && parsed.start_sec >= 0.0
                    && parsed.end_sec >= parsed.start_sec,
                "некорректные таймкоды строки {}",
                i + 1
            );
            Ok(parsed)
        })
        .collect()
}

fn stamp(session: &Path, path: &str, is_transcript: bool) -> Result<FileStamp> {
    let bytes = fs::read(session.join(relative_path(path)?))
        .with_context(|| format!("артефакт {path} недоступен"))?;
    let hash = if is_transcript {
        let lines = transcript(&bytes)?;
        // Names are presentation data: VersionStore::relabel_speaker changes them in place.
        let speech: Vec<_> = lines
            .iter()
            .map(|l| (l.source_id, l.start_sec, l.end_sec, &l.text))
            .collect();
        Sha256::digest(serde_json::to_vec(&speech)?).to_vec()
    } else {
        Sha256::digest(&bytes).to_vec()
    };
    Ok(FileStamp {
        path: path.into(),
        bytes: bytes.len() as u64,
        sha256: hash.iter().map(|b| format!("{b:02x}")).collect(),
        transcript: is_transcript,
    })
}

fn version(
    session: &Path,
    id: u32,
) -> Result<(VersionEntry, FileStamp, Vec<TranscriptLine>, Option<u32>)> {
    let manifest: VersionsManifest =
        serde_json::from_slice(&fs::read(session.join("versions.json"))?)
            .context("повреждён versions.json")?;
    let best = manifest
        .best
        .or_else(|| manifest.versions.iter().map(|v| v.id).max());
    let entry = manifest
        .versions
        .into_iter()
        .find(|v| v.id == id)
        .with_context(|| format!("версия расшифровки v{id} отсутствует в манифесте"))?;
    relative_path(&entry.file)?;
    let path = format!("transcripts/{}", entry.file);
    let fingerprint = stamp(session, &path, true)?;
    let lines = transcript(&fs::read(session.join(&path))?)?;
    Ok((entry, fingerprint, lines, best))
}

/// Read the selected transcript strictly, including existing receipts when available.
/// Legacy nonempty versions without receipts are structurally readable, not hash-verified.
pub fn source_transcript(session: &Path) -> Result<(VersionEntry, Vec<TranscriptLine>)> {
    let manifest: VersionsManifest = serde_json::from_slice(
        &fs::read(session.join("versions.json")).context("нет манифеста расшифровки")?,
    )
    .context("повреждён versions.json")?;
    let id = manifest
        .best
        .or_else(|| manifest.versions.iter().map(|v| v.id).max())
        .context("нет исходной расшифровки")?;
    let (entry, _, lines, _) = version(session, id)?;
    let log = crate::processing::load_strict(session)?;
    let mut confirmed_empty = false;
    for artifact in [TRANSCRIPT, REFINED] {
        if let Some(record) = log.artifacts.get(artifact).filter(|r| r.source == Some(id)) {
            verify(session, artifact, record)?;
            confirmed_empty |= record.outcome == Outcome::Nothing;
        }
    }
    ensure!(
        lines.iter().any(|line| !line.text.trim().is_empty()) || confirmed_empty,
        "пустая расшифровка не имеет подтверждения отсутствия речи"
    );
    Ok((entry, lines))
}

/// Build the evidence before committing the receipt. No status is inferred from filenames alone.
pub fn capture(
    session: &Path,
    artifact: &str,
    outcome: Outcome,
    source: Option<u32>,
    detail: Option<&str>,
) -> Result<Receipt> {
    ensure!(
        outcome != Outcome::Failed,
        "ошибка не является подтверждением результата"
    );
    let empty = outcome == Outcome::Nothing;
    if empty {
        ensure!(
            detail.is_some_and(|s| !s.trim().is_empty()),
            "для пустого результата нужна причина"
        );
    }
    let source_version = source.map(|id| version(session, id)).transpose()?;
    if empty && source_version.is_none() && artifact == TRANSCRIPT {
        let bytes = fs::read(session.join("meta.json"))?;
        let meta: crate::chunks::SessionMeta = serde_json::from_slice(&bytes)?;
        ensure!(
            meta.chunks.is_empty(),
            "звук есть, но нет подтверждённой расшифровки"
        );
        return Ok(Receipt {
            outputs: vec![],
            input: Some(stamp(session, "meta.json", false)?),
        });
    }
    let (entry, input, lines, best) =
        source_version.context("не указана исходная версия расшифровки")?;
    match artifact {
        TRANSCRIPT => {
            ensure!(
                empty || lines.iter().any(|l| !l.text.trim().is_empty()),
                "расшифровка пуста"
            );
            ensure!(
                !empty || lines.iter().all(|l| l.text.trim().is_empty()),
                "пустой результат STT содержит распознанную речь"
            );
            Ok(Receipt {
                outputs: vec![input],
                input: None,
            })
        }
        REFINED => {
            if empty {
                ensure!(
                    best == Some(entry.id),
                    "пустой результат относится к другой рабочей версии"
                );
                return Ok(Receipt {
                    outputs: vec![],
                    input: Some(input),
                });
            }
            ensure!(
                entry.label == "refined" && best == Some(entry.id),
                "исправленная версия не является рабочей"
            );
            ensure!(!lines.is_empty(), "исправленная расшифровка пуста");
            let parent = entry
                .parents
                .first()
                .context("исправленная версия не указывает оригинал")?;
            Ok(Receipt {
                outputs: vec![input],
                input: Some(version(session, *parent)?.1),
            })
        }
        PROCESSED | SUMMARY => {
            ensure!(
                best == Some(entry.id),
                "результат сделан из другой рабочей версии расшифровки"
            );
            if empty {
                return Ok(Receipt {
                    outputs: vec![],
                    input: Some(input),
                });
            }
            let path = if artifact == PROCESSED {
                crate::readable::FILE
            } else {
                "summary.md"
            };
            let output = stamp(session, path, false)?;
            if artifact == PROCESSED {
                let readable = crate::readable::load(session)?;
                ensure!(
                    readable.version_id == entry.id,
                    "читаемый текст ссылается на другую версию"
                );
                ensure!(
                    readable
                        .edits
                        .iter()
                        .all(|(i, text)| *i < lines.len() && !text.trim().is_empty()),
                    "читаемый текст содержит некорректные номера или пустые строки"
                );
            } else {
                let body = fs::read_to_string(session.join(path))?;
                ensure!(
                    !crate::provenance::split(&body).1.trim().is_empty(),
                    "сводка пуста"
                );
            }
            Ok(Receipt {
                outputs: vec![output],
                input: Some(input),
            })
        }
        _ => bail!("неизвестный артефакт {artifact}"),
    }
}

pub fn verify(session: &Path, artifact: &str, record: &Record) -> Result<Receipt> {
    let current = capture(
        session,
        artifact,
        record.outcome,
        record.source,
        record.detail.as_deref(),
    )?;
    if let Some(saved) = &record.receipt {
        ensure!(
            saved.outputs.len() == current.outputs.len()
                && saved
                    .outputs
                    .iter()
                    .zip(&current.outputs)
                    .all(|(a, b)| a.matches(b)),
            "результат изменён после подтверждения (контрольная сумма)"
        );
        ensure!(
            match (&saved.input, &current.input) {
                (Some(a), Some(b)) => a.matches(b),
                (None, None) => true,
                _ => false,
            },
            "исходный текст изменён после обработки (контрольная сумма)"
        );
    }
    Ok(current)
}

#[derive(Serialize, Deserialize)]
struct PreparedAudio {
    complete: bool,
    source_url: String,
    sample_count: u64,
    files: Vec<FileStamp>,
}

/// Published BEFORE writing chunks. A crash after the first chunk must not turn a partial video
/// into a completed download. Old archives without this marker retain structural validation.
pub fn begin_preparation(session: &Path) -> Result<()> {
    let meta: crate::chunks::SessionMeta =
        serde_json::from_slice(&fs::read(session.join("meta.json"))?)?;
    let source = meta.source.context("у сессии нет источника")?;
    atomic_write(
        &session.join("prepare.json"),
        &serde_json::to_vec_pretty(&PreparedAudio {
            complete: false,
            source_url: source.url,
            sample_count: 0,
            files: vec![],
        })?,
    )
}

/// Commit only after every decoded sample was registered and its audio file can be verified.
pub fn commit_preparation(session: &Path, sample_count: u64) -> Result<()> {
    let mut receipt: PreparedAudio =
        serde_json::from_slice(&fs::read(session.join("prepare.json"))?)?;
    let meta: crate::chunks::SessionMeta =
        serde_json::from_slice(&fs::read(session.join("meta.json"))?)?;
    ensure!(
        meta.source.as_ref().map(|s| s.url.as_str()) == Some(receipt.source_url.as_str()),
        "источник изменился во время загрузки"
    );
    let saved_samples = meta
        .chunks
        .iter()
        .map(|c| c.duration_sec * meta.sample_rate as f64)
        .sum::<f64>();
    ensure!(
        sample_count > 0 && (saved_samples - sample_count as f64).abs() < 1.0,
        "аудио сохранено не полностью"
    );
    receipt.files = audio_files(session)?
        .iter()
        .map(|p| stamp(session, p, false))
        .collect::<Result<_>>()?;
    receipt.sample_count = sample_count;
    receipt.complete = true;
    atomic_write(
        &session.join("prepare.json"),
        &serde_json::to_vec_pretty(&receipt)?,
    )
}

/// The durable product of Prepare is the complete registered audio, not a temporary download.
pub fn audio(session: &Path) -> Result<Vec<String>> {
    inspect_audio(session, true)
}

/// UI polling checks the commit, file headers and sizes without rehashing hours of audio.
/// Phase acceptance and reuse always call `audio`, which also checks the recorded content hashes.
pub fn audio_overview(session: &Path) -> Result<Vec<String>> {
    inspect_audio(session, false)
}

fn inspect_audio(session: &Path, verify_hashes: bool) -> Result<Vec<String>> {
    let receipt: Option<PreparedAudio> = match fs::read(session.join("prepare.json")) {
        Ok(bytes) => {
            Some(serde_json::from_slice(&bytes).context("повреждено подтверждение загрузки")?)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    if let Some(receipt) = &receipt {
        ensure!(receipt.complete, "сохранение аудио было прервано");
    }
    let files = audio_files(session)?;
    if let Some(receipt) = receipt {
        let meta: crate::chunks::SessionMeta =
            serde_json::from_slice(&fs::read(session.join("meta.json"))?)?;
        ensure!(
            meta.source.as_ref().map(|s| s.url.as_str()) == Some(receipt.source_url.as_str()),
            "источник аудио изменился"
        );
        let samples = meta
            .chunks
            .iter()
            .map(|c| c.duration_sec * meta.sample_rate as f64)
            .sum::<f64>();
        ensure!(
            (samples - receipt.sample_count as f64).abs() < 1.0,
            "состав аудио изменился"
        );
        ensure!(
            files.len() == receipt.files.len(),
            "число файлов аудио изменилось"
        );
        for (path, recorded) in files.iter().zip(&receipt.files) {
            ensure!(
                path == &recorded.path && fs::metadata(session.join(path))?.len() == recorded.bytes,
                "файл аудио изменился после подтверждения: {path}"
            );
            if verify_hashes {
                ensure!(
                    recorded.matches(&stamp(session, path, false)?),
                    "контрольная сумма аудио изменилась: {path}"
                );
            }
        }
    }
    Ok(files)
}

fn audio_files(session: &Path) -> Result<Vec<String>> {
    let meta: crate::chunks::SessionMeta =
        serde_json::from_slice(&fs::read(session.join("meta.json"))?)?;
    ensure!(!meta.chunks.is_empty(), "звуковая дорожка не сохранена");
    meta.chunks
        .iter()
        .map(|chunk| {
            relative_path(&chunk.file)?;
            ensure!(
                chunk.duration_sec.is_finite() && chunk.duration_sec > 0.0,
                "некорректная длительность аудио"
            );
            let base = session.join("audio").join(&chunk.file);
            let wav = base.with_extension("wav");
            if wav.exists() {
                let reader = hound::WavReader::open(&wav).context("повреждён WAV")?;
                let spec = reader.spec();
                ensure!(
                    spec.sample_rate > 0
                        && spec.sample_rate == meta.sample_rate
                        && spec.channels == 1
                        && spec.bits_per_sample == 16
                        && spec.sample_format == hound::SampleFormat::Int,
                    "WAV не соответствует формату зарегистрированной дорожки"
                );
                let seconds = reader.duration() as f64 / spec.sample_rate as f64;
                ensure!(
                    seconds + 0.05 >= chunk.duration_sec,
                    "WAV короче зарегистрированной дорожки"
                );
                let data_bytes = reader.len() as u64 * u64::from(spec.bits_per_sample / 8);
                use std::io::Seek;
                let data_offset = reader.into_inner().stream_position()?;
                ensure!(
                    fs::metadata(&wav)?.len() >= data_offset + data_bytes,
                    "WAV обрезан после записи заголовка"
                );
                return Ok(format!("audio/{}.wav", chunk.file));
            }
            let flac = base.with_extension("flac");
            use std::io::Read;
            let mut f = fs::File::open(&flac).context("файл аудио отсутствует")?;
            let mut header = [0; 4];
            f.read_exact(&mut header)?;
            ensure!(
                &header == b"fLaC" && f.metadata()?.len() > 42,
                "повреждён заголовок FLAC"
            );
            Ok(format!("audio/{}.flac", chunk.file))
        })
        .collect()
}

/// Publish on the same filesystem; failure leaves the previous artifact intact.
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("у артефакта нет каталога")?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(bytes)?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .map_err(|e| e.error)
        .with_context(|| format!("сохранение {}", path.display()))?;
    #[cfg(unix)]
    fs::File::open(parent)?.sync_all()?;
    Ok(())
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::{processing, versions::VersionStore};

    #[test]
    fn partial_ingest_is_not_ready_until_complete_audio_is_committed() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("audio")).unwrap();
        let wav = dir.path().join("audio/src0_chunk0001.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..1600 {
            writer.write_sample(42i16).unwrap();
        }
        writer.finalize().unwrap();
        fs::write(dir.path().join("meta.json"), serde_json::to_vec(&serde_json::json!({
            "started_at": "t", "sample_rate": 16000,
            "source": {"url": "https://example.test/video", "title": null},
            "chunks": [{"file": "src0_chunk0001", "source_id": 0, "start_offset_sec": 0, "duration_sec": 0.1}]
        })).unwrap()).unwrap();
        assert!(audio(dir.path()).is_ok(), "legacy audio remains readable");
        begin_preparation(dir.path()).unwrap();
        assert!(
            audio(dir.path()).is_err(),
            "one complete chunk is not the whole video"
        );
        assert!(
            commit_preparation(dir.path(), 3200).is_err(),
            "only half the samples were saved"
        );
        assert!(
            audio_overview(dir.path()).is_err(),
            "the UI must not accept the partial ingest"
        );
        commit_preparation(dir.path(), 1600).unwrap();
        assert!(audio(dir.path()).is_ok());
        assert!(audio_overview(dir.path()).is_ok());
        // Same header and length, different audio content: the receipt must detect it.
        let mut bytes = fs::read(&wav).unwrap();
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&wav, bytes).unwrap();
        assert!(audio(dir.path()).is_err());
    }

    #[test]
    fn truncated_audio_cannot_finish_preparation() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("audio")).unwrap();
        let wav = dir.path().join("audio/src0_chunk0001.wav");
        let mut writer = hound::WavWriter::create(
            &wav,
            hound::WavSpec {
                channels: 1,
                sample_rate: 16000,
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            },
        )
        .unwrap();
        for _ in 0..1600 {
            writer.write_sample(42i16).unwrap();
        }
        writer.finalize().unwrap();
        fs::write(
            dir.path().join("meta.json"),
            serde_json::to_vec(&serde_json::json!({
                "started_at": "t", "sample_rate": 16000, "chunks": [{"file": "src0_chunk0001",
                    "source_id": 0, "start_offset_sec": 0, "duration_sec": 0.1}]
            }))
            .unwrap(),
        )
        .unwrap();
        assert!(audio(dir.path()).is_ok());
        let len = fs::metadata(&wav).unwrap().len();
        fs::OpenOptions::new()
            .write(true)
            .open(&wav)
            .unwrap()
            .set_len(len - 2)
            .unwrap();
        assert!(
            audio(dir.path()).is_err(),
            "even a missing final sample must be rejected"
        );
    }

    pub fn fixture(session: &Path) -> std::path::PathBuf {
        let store = VersionStore::open(session).unwrap();
        let (id, path) = store.next_version("raw").unwrap();
        fs::write(&path, "{\"source_id\":0,\"start_sec\":0,\"end_sec\":5,\"text\":\"hello\",\"speaker\":\"S1\"}\n").unwrap();
        store
            .commit(VersionEntry {
                id,
                file: path.file_name().unwrap().to_str().unwrap().into(),
                label: "raw".into(),
                model: "fixture".into(),
                params: serde_json::json!({}),
                created_at: "2026-09-27T00:00:00Z".into(),
                parents: vec![],
            })
            .unwrap();
        fs::write(session.join("summary.md"), "A real summary").unwrap();
        crate::readable::save(
            session,
            &crate::readable::Readable {
                version_id: id,
                ..Default::default()
            },
        )
        .unwrap();
        path
    }

    #[test]
    fn completion_requires_actual_output_and_source() {
        let dir = tempfile::tempdir().unwrap();
        assert!(
            processing::record(dir.path(), SUMMARY, "test", Outcome::Ok, None, Some(1)).is_err()
        );
        let path = fixture(dir.path());
        fs::remove_file(dir.path().join("summary.md")).unwrap();
        assert!(
            processing::record(dir.path(), SUMMARY, "test", Outcome::Ok, None, Some(1)).is_err()
        );
        fs::write(&path, "{truncated").unwrap();
        assert!(
            processing::record(dir.path(), TRANSCRIPT, "test", Outcome::Ok, None, Some(1)).is_err()
        );
        assert!(!dir.path().join("processing.json").exists());
    }

    #[test]
    fn hashes_detect_same_length_output_and_input_changes() {
        let dir = tempfile::tempdir().unwrap();
        let input = fixture(dir.path());
        processing::record(dir.path(), SUMMARY, "test", Outcome::Ok, None, Some(1)).unwrap();
        fs::write(dir.path().join("summary.md"), "B real summary").unwrap();
        assert_eq!(
            processing::inspect(dir.path(), SUMMARY).state,
            processing::ArtifactState::Invalid
        );
        fs::write(dir.path().join("summary.md"), "A real summary").unwrap();
        assert!(processing::is_done(dir.path(), SUMMARY));
        let changed = fs::read_to_string(&input)
            .unwrap()
            .replace("hello", "other");
        fs::write(&input, changed).unwrap();
        assert_eq!(
            processing::inspect(dir.path(), SUMMARY).state,
            processing::ArtifactState::Invalid
        );
    }

    #[test]
    fn speaker_rename_preserves_speech_receipts() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        processing::record(dir.path(), TRANSCRIPT, "test", Outcome::Ok, None, Some(1)).unwrap();
        processing::record(dir.path(), PROCESSED, "test", Outcome::Ok, None, Some(1)).unwrap();
        VersionStore::open(dir.path())
            .unwrap()
            .relabel_speaker("S1", "Long human name")
            .unwrap();
        assert!(processing::is_done(dir.path(), TRANSCRIPT));
        assert!(processing::is_done(dir.path(), PROCESSED));
    }

    #[test]
    fn cleanup_must_reference_the_correct_version_and_lines() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        let mut readable = crate::readable::load(dir.path()).unwrap();
        readable.version_id = 2;
        crate::readable::save(dir.path(), &readable).unwrap();
        assert!(capture(dir.path(), PROCESSED, Outcome::Ok, Some(1), None).is_err());
        readable.version_id = 1;
        readable.edits.insert(7, "out of range".into());
        crate::readable::save(dir.path(), &readable).unwrap();
        assert!(capture(dir.path(), PROCESSED, Outcome::Ok, Some(1), None).is_err());
    }

    #[test]
    fn corrupt_ledger_is_never_overwritten_by_record_forget_or_confirm() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        fs::write(dir.path().join("processing.json"), "{corrupt").unwrap();
        assert!(
            processing::record(dir.path(), SUMMARY, "test", Outcome::Ok, None, Some(1)).is_err()
        );
        processing::forget(dir.path(), &[SUMMARY]);
        assert!(!processing::confirm(dir.path(), SUMMARY));
        assert_eq!(
            fs::read_to_string(dir.path().join("processing.json")).unwrap(),
            "{corrupt"
        );
    }

    #[test]
    fn legacy_receipt_requires_real_files_too() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        processing::record(
            dir.path(),
            SUMMARY,
            "test",
            Outcome::Unverified,
            None,
            Some(1),
        )
        .unwrap();
        let mut log = processing::load(dir.path());
        log.artifacts.get_mut(SUMMARY).unwrap().receipt = None;
        fs::write(
            dir.path().join("processing.json"),
            serde_json::to_vec(&log).unwrap(),
        )
        .unwrap();
        assert!(processing::is_done(dir.path(), SUMMARY));
        fs::remove_file(dir.path().join("summary.md")).unwrap();
        assert!(!processing::is_done(dir.path(), SUMMARY));
    }

    #[test]
    fn no_result_needs_a_reason_and_an_existing_input() {
        let dir = tempfile::tempdir().unwrap();
        fixture(dir.path());
        assert!(capture(dir.path(), SUMMARY, Outcome::Nothing, Some(1), None).is_err());
        assert!(capture(
            dir.path(),
            SUMMARY,
            Outcome::Nothing,
            Some(2),
            Some("no speech")
        )
        .is_err());
        processing::record(
            dir.path(),
            SUMMARY,
            "test",
            Outcome::Nothing,
            Some("no meaningful speech".into()),
            Some(1),
        )
        .unwrap();
        assert_eq!(
            processing::inspect(dir.path(), SUMMARY).state,
            processing::ArtifactState::Empty
        );
    }
}
