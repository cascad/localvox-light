//! Actual child worker, capture ledger, native VAD and durable cache; no model/devices.
#![cfg(feature = "asr-fixtures")]
use localvox_light_core::{
    artifacts,
    chunks::{self, capture, planning, ChunkParams, ChunkRecorder, SessionMeta},
    processing,
    versions::VersionStore,
};
use std::{
    collections::BTreeSet,
    fs,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

struct Recording {
    root: tempfile::TempDir,
    owner: Option<Arc<Mutex<capture::Writer>>>,
    recorders: Vec<ChunkRecorder>,
    handle: Option<planning::Handle>,
    planners: planning::Planners,
}
impl Recording {
    fn new(answer: &str, digest: bool) -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions/recorded");
        fs::create_dir_all(session.join("audio")).unwrap();
        let generation = "0199a774-0000-7000-8000-000000000003".to_string();
        let meta = Arc::new(Mutex::new(SessionMeta {
            capture_generation: Some(generation.clone()),
            sample_rate: 16000,
            lang: Some("ru".into()),
            ..Default::default()
        }));
        chunks::save_meta_checked(&session.join("meta.json"), &meta.lock().unwrap()).unwrap();
        let owner = Arc::new(Mutex::new(
            capture::Writer::begin(&session, generation).unwrap(),
        ));
        let planners = planning::Planners::default();
        let handle = planners.attach(owner.clone(), "unused-WAV".into()).unwrap();
        let mut recorders = Vec::new();
        let mut seed = 912871u32;
        let pcm: Vec<i16> = (0..43 * 16000)
            .map(|_| {
                seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (seed >> 16) as i16
            })
            .collect();
        for id in [0, 1] {
            let params = Arc::new(ChunkParams {
                audio_dir: session.join("audio"),
                meta_path: session.join("meta.json"),
                chunk_sec: 300.0,
                flac: false,
                ffmpeg: "unused-WAV".into(),
            });
            let mut recorder = ChunkRecorder::for_capture(
                id,
                params,
                meta.clone(),
                owner.clone(),
                Arc::new(chunks::codec::Conversions::default()),
            );
            if id == 0 {
                recorder.feed(&pcm)
            } else {
                recorder.feed(&vec![0; pcm.len()])
            };
            recorder.finalize_current();
            recorders.push(recorder);
        }
        fs::write(root.path().join("fixture.json"), serde_json::to_vec(&serde_json::json!({"answer":answer,"include_pcm_sha":digest,"calls_file":root.path().join("calls.txt")})).unwrap()).unwrap();
        let recording = Self {
            root,
            owner: Some(owner),
            recorders,
            handle: Some(handle),
            planners,
        };
        recording.wait(false);
        recording
    }
    fn session(&self) -> std::path::PathBuf {
        self.root.path().join("sessions/recorded")
    }
    fn wait(&self, complete: bool) {
        let revision = capture::read(&self.session()).unwrap().revision;
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let plan = fs::read(self.session().join(planning::FILE))
                .ok()
                .and_then(|b| serde_json::from_slice::<planning::Snapshot>(&b).ok());
            if plan.is_some_and(|p| p.capture_revision >= revision && (!complete || p.complete)) {
                break;
            }
            assert!(Instant::now() < deadline, "planner timeout");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn stop(&mut self) {
        self.owner
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .end(
                "fixture Stop",
                false,
                self.recorders
                    .iter()
                    .enumerate()
                    .map(|(id, r)| (id as u8, r.end_samples()))
                    .collect(),
            )
            .unwrap();
        self.handle.take();
        self.wait(true);
        self.recorders.clear();
        self.owner.take();
        drop(std::mem::take(&mut self.planners));
    }
    fn run(&self, prefix: bool, crash: Option<&str>, extra: &[&str]) -> serde_json::Value {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_localvox-process"));
        command
            .arg(self.session())
            .args(["--work-dir"])
            .arg(self.root.path())
            .arg("--fixture-asr")
            .arg(self.root.path().join("fixture.json"))
            .current_dir(self.root.path())
            .env("LOCALVOX_DIARIZE", "off")
            .env("LOCALVOX_LANG", "ru")
            .env_remove("LOCALVOX_ASR_FIXTURE_CRASH_POINT")
            .args(extra);
        if prefix {
            command.arg("--worker-prefix");
        }
        if let Some(point) = crash {
            command.env("LOCALVOX_ASR_FIXTURE_CRASH_POINT", point);
        }
        let mut limits = localvox_light_process::Limits::new(Duration::from_secs(20));
        limits.output_bytes = 128 * 1024;
        let output =
            localvox_light_process::run(command, None, limits, || false, |_, _| {}).unwrap();
        if crash.is_some() {
            assert!(
                matches!(output.termination,localvox_light_process::Termination::Exited(status) if status.code()==Some(73)),
                "missing crash: {:?}: {}",
                output.termination,
                String::from_utf8_lossy(&output.stderr)
            );
            return serde_json::Value::Null;
        }
        assert!(
            output.termination.success(),
            "{:?}: {}",
            output.termination,
            String::from_utf8_lossy(&output.stderr)
        );
        if prefix {
            serde_json::from_slice(&output.stdout).unwrap()
        } else {
            serde_json::Value::Null
        }
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.root.path().join("calls.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
    fn assert_no_final(&self) {
        for file in [
            "versions.json",
            "processing.json",
            "summary.md",
            "processed.md",
        ] {
            assert!(
                !self.session().join(file).exists(),
                "early worker published {file}"
            )
        }
    }
    fn assert_final(&self, empty: bool) {
        let store = VersionStore::open(&self.session()).unwrap();
        let manifest = store.load();
        assert_eq!(manifest.versions.len(), 1);
        let version = &manifest.versions[0];
        assert_eq!(manifest.best, Some(version.id));
        assert!(version.parents.is_empty());
        let (_, lines) = artifacts::source_transcript(&self.session()).unwrap();
        assert_eq!(lines.is_empty(), empty);
        assert!(lines.iter().all(|l| l.source_id == 0
            && l.start_sec >= 0.0
            && l.start_sec < l.end_sec
            && l.end_sec <= 43.0));
        assert!(lines
            .windows(2)
            .all(|pair| pair[0].end_sec <= pair[1].start_sec));
        let log = processing::load(&self.session());
        let record = &log.artifacts[processing::TRANSCRIPT];
        assert_eq!(record.source, Some(version.id));
        assert_eq!(
            record.outcome,
            if empty {
                processing::Outcome::Nothing
            } else {
                processing::Outcome::Ok
            }
        );
        artifacts::verify(&self.session(), processing::TRANSCRIPT, record).unwrap();
        assert!(!self.session().join("summary.md").exists());
    }
}

#[test]
fn child_result_and_checkpoint_crashes_reuse_verified_windows_and_empty_coverage() {
    for empty in [false, true] {
        let mut r = Recording::new(
            if empty {
                ""
            } else {
                "слова из записи"
            },
            !empty,
        );
        r.run(true, Some("after_result"), &[]);
        assert_eq!(r.calls().len(), 1);
        r.assert_no_final();
        r.run(true, Some("after_checkpoint"), &[]);
        let calls = r.calls();
        assert!(calls.len() >= 2);
        assert_eq!(
            calls.len(),
            calls.iter().collect::<BTreeSet<_>>().len(),
            "accepted ASR was repeated after result crash"
        );
        r.assert_no_final();
        let same = r.run(true, None, &[]);
        assert_eq!(same["asr_calls"], 0);
        assert_eq!(same["audio"]["windows"], 0);
        assert_eq!(same["model_loaded"], false);
        assert_eq!(r.calls(), calls);
        r.stop();
        r.run(false, None, &[]);
        r.assert_final(empty);
        let final_calls = r.calls();
        assert_eq!(
            final_calls.len(),
            final_calls.iter().collect::<BTreeSet<_>>().len()
        );
        r.run(false, None, &[]);
        assert_eq!(r.calls(), final_calls);
        r.assert_final(empty);
    }
}

#[test]
fn final_commit_crashes_recover_without_duplicate_asr_or_versions() {
    for point in [
        "after_transcript_file",
        "after_manifest",
        "after_best",
        "after_receipt",
    ] {
        let mut r = Recording::new("слова из записи", true);
        r.run(true, None, &[]);
        r.stop();
        r.run(false, Some(point), &[]);
        let before = r.calls();
        r.run(false, None, &[]);
        assert_eq!(r.calls(), before, "ASR repeated at {point}");
        r.assert_final(false);
    }
}

#[test]
fn force_bypasses_early_cache_and_legacy_without_ledger_still_cooks() {
    let mut r = Recording::new("слова из записи", true);
    r.run(true, None, &[]);
    r.stop();
    let before = r.calls().len();
    r.run(false, None, &["--force"]);
    assert!(r.calls().len() > before);
    r.assert_final(false);
    // Independent legacy archive: preserve audio/meta, no new capture or derived artifacts.
    let legacy = r.root.path().join("legacy");
    fs::create_dir_all(legacy.join("audio")).unwrap();
    for entry in fs::read_dir(r.session().join("audio")).unwrap() {
        let path = entry.unwrap().path();
        fs::copy(&path, legacy.join("audio").join(path.file_name().unwrap())).unwrap();
    }
    let mut meta: SessionMeta =
        serde_json::from_slice(&fs::read(r.session().join("meta.json")).unwrap()).unwrap();
    meta.capture_generation = None;
    chunks::save_meta_checked(&legacy.join("meta.json"), &meta).unwrap();
    let result = localvox_light_core::cook::cook_session_asr_fixture(
        &legacy,
        &Default::default(),
        &r.root.path().join("fixture.json"),
    )
    .unwrap();
    assert!(matches!(
        result,
        localvox_light_core::cook::CookOutcome::Done { .. }
    ));
    assert!(!artifacts::source_transcript(&legacy).unwrap().1.is_empty());
}
