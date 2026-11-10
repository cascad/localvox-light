use super::*;
use localvox_light_core::{
    chunks::{self, capture, planning, ChunkParams, ChunkRecorder, SessionMeta},
    incremental::Published,
};
use std::{fs, sync::Mutex};

struct Producer {
    root: tempfile::TempDir,
    owner: Option<Arc<Mutex<capture::Writer>>>,
    recorders: Vec<ChunkRecorder>,
    planner: Option<planning::Handle>,
    planners: planning::Planners,
    seed: u32,
}
impl Producer {
    fn new(evidence: Option<&std::path::Path>) -> Self {
        let root = match evidence {
            Some(path) => tempfile::tempdir_in(path).unwrap(),
            None => tempfile::tempdir().unwrap(),
        };
        let session = root.path().join("sessions/recorded");
        fs::create_dir_all(session.join("audio")).unwrap();
        let generation = "0199a774-0000-7000-8000-000000000004".to_string();
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
        let planner = planners.attach(owner.clone(), "unused-WAV".into()).unwrap();
        let recorders = [0, 1]
            .into_iter()
            .map(|id| {
                ChunkRecorder::for_capture(
                    id,
                    Arc::new(ChunkParams {
                        audio_dir: session.join("audio"),
                        meta_path: session.join("meta.json"),
                        chunk_sec: 300.0,
                        flac: false,
                        ffmpeg: "unused-WAV".into(),
                    }),
                    meta.clone(),
                    owner.clone(),
                    Arc::new(chunks::codec::Conversions::default()),
                )
            })
            .collect();
        fs::write(root.path().join(jobs::RECORDING_MARKER), "recorded").unwrap();
        Self {
            root,
            owner: Some(owner),
            recorders,
            planner: Some(planner),
            planners,
            seed: 48731,
        }
    }
    fn session(&self) -> PathBuf {
        self.root.path().join("sessions/recorded")
    }
    fn feed(&mut self, seconds: usize) {
        let pcm: Vec<i16> = (0..seconds * 16000)
            .map(|_| {
                self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (self.seed >> 16) as i16
            })
            .collect();
        self.recorders[0].feed(&pcm);
        self.recorders[0].finalize_current();
        self.recorders[1].feed(&vec![0; pcm.len()]);
        self.recorders[1].finalize_current();
        let revision = capture::read(&self.session()).unwrap().revision;
        wait(|| {
            fs::read(self.session().join(planning::FILE))
                .ok()
                .and_then(|b| serde_json::from_slice::<planning::Snapshot>(&b).ok())
                .is_some_and(|p| p.capture_revision >= revision)
        });
    }
    fn stop(&mut self) {
        self.owner
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .end(
                "coordinator fixture Stop",
                false,
                self.recorders
                    .iter()
                    .enumerate()
                    .map(|(id, r)| (id as u8, r.end_samples()))
                    .collect(),
            )
            .unwrap();
        self.planner.take();
        self.recorders.clear();
        self.owner.take();
        drop(std::mem::take(&mut self.planners));
        fs::remove_file(self.root.path().join(jobs::RECORDING_MARKER)).unwrap();
    }
    fn calls(&self) -> Vec<String> {
        fs::read_to_string(self.root.path().join("calls.txt"))
            .unwrap_or_default()
            .lines()
            .map(str::to_owned)
            .collect()
    }
    fn state(&self) -> Option<JobState> {
        JobQueue::load(self.root.path())
            .jobs()
            .first()
            .map(|j| j.state)
    }
    fn config(&self) -> Config {
        let worker = std::env::var_os("LOCALVOX_TEST_ASR_WORKER")
            .map(PathBuf::from)
            .expect("build asr-fixtures worker and select LOCALVOX_TEST_ASR_WORKER");
        let mut cfg = Config::from_env_in(
            self.root.path().to_string_lossy().into(),
            worker.parent().unwrap(),
        )
        .unwrap();
        cfg.exe = worker;
        let fixture = self.root.path().join("fixture.json");
        fs::write(&fixture,serde_json::to_vec(&serde_json::json!({"answer":"слова из записи","include_pcm_sha":true,"calls_file":self.root.path().join("calls.txt"),"delay_ms":50,"fault_file":self.root.path().join("asr-fault.txt")})).unwrap()).unwrap();
        cfg.fixture_args = vec!["--fixture-asr".into(), fixture.into_os_string()];
        cfg.interval = Duration::from_millis(30);
        cfg.quiescent_sec = 1;
        cfg.index_timeout = Duration::from_secs(2);
        cfg.timeouts = [Duration::from_secs(15); 4];
        cfg.child_env.retain(|(key, _)| {
            matches!(
                key.to_str(),
                Some(
                    "PATH"
                        | "SystemRoot"
                        | "WINDIR"
                        | "COMSPEC"
                        | "PATHEXT"
                        | "LD_LIBRARY_PATH"
                        | "DYLD_LIBRARY_PATH"
                )
            )
        });
        cfg.child_env.extend([
            ("TEMP".into(), self.root.path().as_os_str().to_owned()),
            ("TMP".into(), self.root.path().as_os_str().to_owned()),
            ("LOCALVOX_DIARIZE".into(), "off".into()),
            ("LOCALVOX_LANG".into(), "ru".into()),
        ]);
        cfg
    }
}
fn wait(mut predicate: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while !predicate() {
        assert!(Instant::now() < deadline, "coordinator fixture timeout");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
#[ignore = "requires explicit asr-fixtures worker; temporary real coordinator/capture/worker/API, no models/devices"]
fn finite_prefix_coordinator_handles_growth_stop_disable_cancel_and_retry() {
    let _guard = super::tests::POST_ENV_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let keys = [
        "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY",
        "LOCALVOX_LIGHT_INCREMENTAL_TRANSCRIBE",
    ];
    struct Restore(Vec<(String, Option<String>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            localvox_light_core::settings::apply_live(&self.0);
        }
    }
    let _restore = Restore(
        keys.iter()
            .map(|k| (k.to_string(), std::env::var(k).ok()))
            .collect(),
    );
    // Opt-in retention for the isolated fault series; ordinary tests clean up as before.
    let evidence = std::env::var_os("LOCALVOX_TEST_PREFIX_EVIDENCE").map(PathBuf::from);
    if let Some(path) = &evidence {
        fs::create_dir(path).expect("evidence output must be a new directory");
    }
    for action in [
        "stop",
        "disable",
        "cancel",
        "timeout",
        "asr_error",
        "missing",
        "changed",
    ] {
        let fault = matches!(action, "timeout" | "asr_error" | "missing" | "changed");
        localvox_light_core::settings::apply_live(&[
            (keys[0].into(), Some("off".into())),
            (keys[1].into(), Some("on".into())),
        ]);
        let mut producer = Producer::new(evidence.as_deref());
        producer.feed(43);
        let mut cfg = producer.config();
        if action == "timeout" {
            cfg.timeouts[1] = Duration::from_secs(2);
        }
        let running = AtomicBool::new(true);
        cfg.preflight(&running).unwrap();
        std::thread::scope(|scope| {
            struct Stop<'a>(&'a AtomicBool);
            impl Drop for Stop<'_> {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Relaxed)
                }
            }
            let _stop = Stop(&running);
            let coordinator = scope.spawn(|| coordinate(&cfg, &running));
            wait(|| producer.state() == Some(JobState::WaitingInput));
            let published = Published::current(&producer.session()).unwrap().unwrap();
            assert!(!published
                .read(&producer.session())
                .unwrap()
                .lines
                .is_empty());
            assert!(!producer.session().join("versions.json").exists());
            let before = producer.calls();
            assert!(before.len() > 0);
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(producer.calls(), before, "same horizon respawned");
            if fault {
                localvox_light_core::settings::apply_live(&[(keys[1].into(), Some("off".into()))]);
                producer.feed(35);
                let path = producer.session().join("audio/src0_chunk0002.wav");
                let original = fs::read(&path).unwrap();
                let fault_path = producer.root.path().join("asr-fault.txt");
                match action {
                    "timeout" => fs::write(&fault_path, "wait").unwrap(),
                    "asr_error" => fs::write(&fault_path, "fail").unwrap(),
                    "missing" => fs::remove_file(&path).unwrap(),
                    "changed" => {
                        let mut changed = original.clone();
                        changed[44] ^= 1;
                        fs::write(&path, changed).unwrap();
                    }
                    _ => unreachable!(),
                }
                localvox_light_core::settings::apply_live(&[(keys[1].into(), Some("on".into()))]);
                wait(|| producer.state() == Some(JobState::Failed));
                assert_eq!(
                    Published::current(&producer.session()).unwrap().unwrap(),
                    published,
                    "failed suffix changed published prefix"
                );
                let archive =
                    localvox_light_api::archive::Archive::new(producer.root.path().to_owned());
                assert_eq!(
                    archive
                        .transcript("recorded")
                        .unwrap()
                        .partial
                        .unwrap()
                        .snapshot
                        .as_deref(),
                    Some(published.snapshot.as_str())
                );
                assert!(!producer.session().join("versions.json").exists());
                assert!(producer.owner.is_some());
                wait(|| {
                    jobs::try_execution_lock(&producer.session())
                        .unwrap()
                        .is_some()
                });
                fs::write(&path, original).unwrap();
                if fault_path.exists() {
                    fs::remove_file(fault_path).unwrap();
                }
                archive.retry("recorded").unwrap();
                wait(|| producer.state() == Some(JobState::WaitingInput));
            } else if action == "disable" {
                localvox_light_core::settings::apply_live(&[(keys[1].into(), Some("off".into()))]);
                producer.feed(35);
                std::thread::sleep(Duration::from_millis(200));
                assert_eq!(producer.calls(), before, "disabled early work ran");
            } else {
                producer.feed(35);
                wait(|| producer.state() == Some(JobState::Running));
            }
            let archive =
                localvox_light_api::archive::Archive::new(producer.root.path().to_owned());
            if action == "cancel" {
                archive.cancel("recorded").unwrap();
                assert_eq!(producer.state(), Some(JobState::Cancelled));
                assert!(
                    producer.owner.is_some(),
                    "processing cancel stopped capture"
                );
                producer.feed(3);
                let cancelled_calls = producer.calls();
                producer.stop();
                std::thread::sleep(Duration::from_millis(250));
                assert_eq!(producer.state(), Some(JobState::Cancelled));
                assert_eq!(producer.calls(), cancelled_calls);
                assert!(!producer.session().join("versions.json").exists());
                archive.retry("recorded").unwrap();
            } else {
                producer.stop();
            }
            wait(|| producer.state() == Some(JobState::Done));
            let (_, lines) =
                localvox_light_core::artifacts::source_transcript(&producer.session()).unwrap();
            assert!(!lines.is_empty());
            assert!(!producer.session().join("summary.md").exists());
            assert_eq!(JobQueue::load(producer.root.path()).jobs().len(), 1);
            if action != "cancel" && !fault {
                let calls = producer.calls();
                assert_eq!(
                    calls.len(),
                    calls
                        .iter()
                        .collect::<std::collections::BTreeSet<_>>()
                        .len(),
                    "ASR repeated after {action}"
                );
            }
            if fault {
                let calls = producer.calls();
                for hash in &before {
                    assert_eq!(
                        calls.iter().filter(|next| *next == hash).count(),
                        1,
                        "accepted prefix ASR repeated after {action}"
                    );
                }
            }
            running.store(false, Ordering::Relaxed);
            coordinator.join().unwrap();
        });
        if let Some(path) = &evidence {
            let report = serde_json::json!({
                "scenario": action,
                "assertions_passed": true,
                "archive": producer.root.path(),
                "capture": capture::read(&producer.session()).unwrap(),
                "published": Published::current(&producer.session()).unwrap(),
                "jobs": JobQueue::load(producer.root.path()).jobs(),
                "asr_input_hashes": producer.calls(),
            });
            localvox_light_core::artifacts::atomic_write(
                &path.join(format!("{action}.json")),
                &serde_json::to_vec_pretty(&report).unwrap(),
            )
            .unwrap();
            let _retained_archive = producer.root.keep();
        }
    }
}
