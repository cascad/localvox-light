//! Explicit native smoke on saved PUBLIC PCM, temporary sessions only.
use localvox_light_core::{
    artifacts,
    chunks::{self, capture, planning, ChunkParams, ChunkRecorder, SessionMeta},
    versions::VersionStore,
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

fn wait_plan(session: &Path, revision: u64, complete: bool) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let plan = fs::read(session.join(planning::FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<planning::Snapshot>(&b).ok());
        if plan.is_some_and(|p| p.capture_revision >= revision && (!complete || p.complete)) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "capture planner did not reach the native horizon"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
fn worker(root: &Path, session: &Path, model: &Path, prefix: bool) -> serde_json::Value {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_localvox-process"));
    command
        .arg(session)
        .arg("--work-dir")
        .arg(root)
        .arg("--model-dir")
        .arg(model)
        .current_dir(root)
        .env("LOCALVOX_DIARIZE", "off")
        .env("LOCALVOX_LANG", "ru");
    if prefix {
        command.arg("--worker-prefix");
    }
    let mut limits = localvox_light_process::Limits::new(Duration::from_secs(90));
    limits.output_bytes = 128 * 1024;
    let output = localvox_light_process::run(command, None, limits, || false, |_, _| {}).unwrap();
    assert!(
        output.termination.success(),
        "native worker failed: {:?}\n{}",
        output.termination,
        String::from_utf8_lossy(&output.stderr)
    );
    if prefix {
        serde_json::from_slice(&output.stdout).unwrap()
    } else {
        serde_json::json!({"stdout":String::from_utf8_lossy(&output.stdout)})
    }
}
fn native_input() -> (Vec<i16>, PathBuf) {
    let source = PathBuf::from(
        std::env::var_os("LOCALVOX_TEST_PUBLIC_PCM").expect("set explicit saved public PCM path"),
    );
    let model = PathBuf::from(
        std::env::var_os("LOCALVOX_TEST_ASR_MODEL").expect("set native ASR model directory"),
    );
    assert!(
        chunks::codec::samples(&source).unwrap() > 750 * 16000,
        "public fixture needs 600–750 s interval"
    );
    let bytes = chunks::codec::range(
        &source,
        600 * 16000,
        750 * 16000,
        Path::new("unused-for-WAV"),
    )
    .unwrap();
    (
        bytes
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect(),
        model,
    )
}
fn recorder(
    id: u8,
    session: &Path,
    meta: Arc<Mutex<SessionMeta>>,
    owner: Option<Arc<Mutex<capture::Writer>>>,
) -> ChunkRecorder {
    let params = Arc::new(ChunkParams {
        audio_dir: session.join("audio"),
        meta_path: session.join("meta.json"),
        chunk_sec: 300.0,
        flac: false,
        ffmpeg: "unused-for-WAV".into(),
    });
    match owner {
        Some(owner) => ChunkRecorder::for_capture(
            id,
            params,
            meta,
            owner,
            Arc::new(chunks::codec::Conversions::default()),
        ),
        None => ChunkRecorder::new(id, params, meta),
    }
}

#[test]
#[ignore = "requires real GigaAM weights and explicitly selected saved public PCM; no devices/LLM"]
fn native_worker_reuses_prefix_and_final_text_matches_fresh_offline_recognition() {
    let (pcm, model) = native_input();
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("sessions/recorded");
    fs::create_dir_all(session.join("audio")).unwrap();
    let generation = "0199a774-0000-7000-8000-000000000002".to_string();
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
    let handle = planners
        .attach(owner.clone(), "unused-for-WAV".into())
        .unwrap();
    let mut mic = recorder(0, &session, meta.clone(), Some(owner.clone()));
    let mut system = recorder(1, &session, meta.clone(), Some(owner.clone()));
    mic.feed(&pcm[..90 * 16000]);
    mic.finalize_current();
    system.feed(&vec![0; 90 * 16000]);
    system.finalize_current();
    wait_plan(&session, capture::read(&session).unwrap().revision, false);
    let first = worker(root.path(), &session, &model, true);
    assert!(first["asr_calls"].as_u64().unwrap() > 0);
    assert!(first["checkpoint"]["common_end"].as_u64().unwrap() > 0);
    assert!(!session.join("versions.json").exists());
    assert!(!session.join("processing.json").exists());
    assert!(!session.join("summary.md").exists());
    let same = worker(root.path(), &session, &model, true);
    assert_eq!(same["asr_calls"], 0);
    assert_eq!(same["audio"]["windows"], 0);
    assert_eq!(same["model_loaded"], false);
    mic.feed(&pcm[90 * 16000..]);
    mic.finalize_current();
    system.feed(&vec![0; 60 * 16000]);
    system.finalize_current();
    wait_plan(&session, capture::read(&session).unwrap().revision, false);
    let second = worker(root.path(), &session, &model, true);
    assert!(second["asr_calls"].as_u64().unwrap() > 0);
    owner
        .lock()
        .unwrap()
        .end(
            "native smoke Stop",
            false,
            [(0, mic.end_samples()), (1, system.end_samples())].into(),
        )
        .unwrap();
    drop(handle);
    wait_plan(&session, capture::read(&session).unwrap().revision, true);
    drop(mic);
    drop(system);
    drop(owner);
    drop(planners);
    worker(root.path(), &session, &model, false);
    let (_, source) = artifacts::source_transcript(&session).unwrap();
    let manifest = VersionStore::open(&session).unwrap().load();
    assert_eq!(manifest.versions.len(), 1);
    let final_version = &manifest.versions[0];
    let final_calls = final_version.params["asr_cache"]["calls"].as_u64().unwrap();
    let hits = final_version.params["asr_cache"]["hits"].as_u64().unwrap();
    assert!(hits > 0);
    assert!(!session.join("summary.md").exists());
    let offline = root.path().join("sessions/offline");
    fs::create_dir_all(offline.join("audio")).unwrap();
    let meta = Arc::new(Mutex::new(SessionMeta {
        sample_rate: 16000,
        lang: Some("ru".into()),
        ..Default::default()
    }));
    chunks::save_meta_checked(&offline.join("meta.json"), &meta.lock().unwrap()).unwrap();
    let mut mic = recorder(0, &offline, meta.clone(), None);
    let mut system = recorder(1, &offline, meta, None);
    mic.feed(&pcm);
    mic.finalize_current();
    system.feed(&vec![0; pcm.len()]);
    system.finalize_current();
    drop(mic);
    drop(system);
    worker(root.path(), &offline, &model, false);
    let (_, offline_source) = artifacts::source_transcript(&offline).unwrap();
    assert_eq!(source.len(), offline_source.len());
    assert!(
        source
            .iter()
            .zip(&offline_source)
            .all(|(a, b)| a.source_id == b.source_id
                && a.start_sec == b.start_sec
                && a.end_sec == b.end_sec
                && a.text == b.text),
        "native cached text/timing differs from fresh offline"
    );
    let offline_manifest = VersionStore::open(&offline).unwrap().load();
    let offline_calls = offline_manifest.versions[0].params["asr_cache"]["calls"]
        .as_u64()
        .unwrap();
    assert_eq!(
        first["asr_calls"].as_u64().unwrap() + second["asr_calls"].as_u64().unwrap() + final_calls,
        offline_calls,
        "a verified window was recognized twice or skipped"
    );
    let report = serde_json::json!({"first":first,"unchanged":same,"second":second,"final_asr_calls":final_calls,"final_cache_hits":hits,"offline_asr_calls":offline_calls,"final_lines":source.len(),"text_and_timing_equal":true});
    if let Some(path) = std::env::var_os("LOCALVOX_TEST_EVIDENCE") {
        fs::write(path, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
    }
    println!("{report}");
}
