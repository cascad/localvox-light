//! Explicit local-model acceptance; no download, user archive, audio capture or LLM.
use localvox_light_core::{
    artifacts, processing,
    transcription::{Backend, Choice},
    versions::VersionStore,
};
use std::{fs, path::PathBuf, process::Command, time::Duration};

#[test]
#[ignore = "requires local T-one, SbertPuncCase, ffmpeg and LOCALVOX_TEST_ASR_AUDIO; no downloads/devices"]
fn real_tone_worker_with_optional_punctuation_keeps_raw_and_timed_derivative() {
    let audio = PathBuf::from(
        std::env::var_os("LOCALVOX_TEST_ASR_AUDIO").expect("LOCALVOX_TEST_ASR_AUDIO"),
    );
    assert!(audio.is_file());
    let tone = std::env::var_os("LOCALVOX_TONE_MODEL_DIR").expect("LOCALVOX_TONE_MODEL_DIR");
    let sbert = std::env::var_os("LOCALVOX_SBERT_MODEL_DIR").expect("LOCALVOX_SBERT_MODEL_DIR");
    let root = tempfile::tempdir().unwrap();
    for punctuation in [false, true] {
        let work = root.path().join(if punctuation { "on" } else { "off" });
        fs::create_dir_all(&work).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_localvox-process"));
        command
            .current_dir(&work)
            .arg("--work-dir")
            .arg(&work)
            .arg("--ingest")
            .arg(&audio)
            .env("LOCALVOX_ASR_BACKEND", "tone")
            .env(
                "LOCALVOX_ASR_PUNCTUATION",
                if punctuation { "on" } else { "off" },
            )
            .env("LOCALVOX_TONE_MODEL_DIR", &tone)
            .env("LOCALVOX_SBERT_MODEL_DIR", &sbert)
            .env("LOCALVOX_LANG", "ru")
            .env("LOCALVOX_DIARIZE", "off")
            .env("RAYON_NUM_THREADS", "2");
        let result = localvox_light_process::run(
            command,
            None,
            localvox_light_process::Limits::new(Duration::from_secs(120)),
            || false,
            |_, _| {},
        )
        .unwrap();
        assert!(
            result.termination.success(),
            "{:?}\n{}\n{}",
            result.termination,
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let sessions: Vec<_> = fs::read_dir(work.join("sessions"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(sessions.len(), 1);
        let session = &sessions[0];
        assert_eq!(
            Choice::for_session(session).unwrap(),
            Choice {
                backend: Backend::Tone,
                punctuation
            }
        );
        let raw = processing::inspect(session, processing::TRANSCRIPT);
        assert!(raw.complete());
        let store = VersionStore::open(session).unwrap();
        let original =
            artifacts::transcript(&fs::read(store.resolve(raw.source.unwrap()).unwrap()).unwrap())
                .unwrap();
        assert!(!original.is_empty());
        let view = localvox_light_core::progress::read(session);
        if punctuation {
            let derived = processing::inspect(session, processing::PUNCTUATED);
            assert!(derived.complete());
            assert_ne!(derived.source, raw.source);
            let lines = artifacts::transcript(
                &fs::read(store.resolve(derived.source.unwrap()).unwrap()).unwrap(),
            )
            .unwrap();
            localvox_light_core::text_processing::validate_lines(&original, &lines).unwrap();
            assert_eq!(store.best().unwrap().id, derived.source.unwrap());
            assert!(view.iter().any(|e| e.stage
                == localvox_light_core::progress::Stage::Punctuation
                && e.state == localvox_light_core::progress::StageState::Done));
        } else {
            assert!(store
                .load()
                .versions
                .iter()
                .all(|v| v.label != processing::PUNCTUATED));
            assert!(view
                .iter()
                .all(|e| e.stage != localvox_light_core::progress::Stage::Punctuation));
        }
    }
}
