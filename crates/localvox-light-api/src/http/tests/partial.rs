use super::*;
use localvox_light_core::{
    chunks::{self, capture, planning, ChunkParams, ChunkRecorder, SessionMeta},
    incremental::{consume_windows, Cache, Horizon, Identity, Published},
};
use std::{
    collections::BTreeMap,
    io::{Read, Seek, Write},
    sync::Mutex,
    time::{Duration, Instant},
};

struct Producer {
    root: tempfile::TempDir,
    owner: Arc<Mutex<capture::Writer>>,
    recorders: Vec<ChunkRecorder>,
    _planner: planning::Handle,
    _planners: planning::Planners,
    pcm: Vec<i16>,
    cache: Cache,
    after: BTreeMap<u8, u64>,
    seed: u32,
}
impl Producer {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions/recorded");
        fs::create_dir_all(session.join("audio")).unwrap();
        let generation = "0199a774-0000-7000-8000-000000000005".to_string();
        let meta = Arc::new(Mutex::new(SessionMeta {
            capture_generation: Some(generation.clone()),
            sample_rate: 16000,
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
        let model = root.path().join("synthetic.model");
        fs::write(&model, b"fixture model").unwrap();
        let identity = Identity::from_files(
            "fixture".into(),
            "ru".into(),
            "fixture".into(),
            [("model".into(), model)].into(),
            || false,
        )
        .unwrap();
        let cache = Cache::open(&session, &identity).unwrap();
        Self {
            root,
            owner,
            recorders,
            _planner: planner,
            _planners: planners,
            pcm: vec![],
            cache,
            after: BTreeMap::new(),
            seed: 73631,
        }
    }
    fn session(&self) -> std::path::PathBuf {
        self.root.path().join("sessions/recorded")
    }
    fn feed(&mut self, seconds: usize) -> Published {
        let pcm: Vec<_> = (0..seconds * 16000)
            .map(|_| {
                self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
                (self.seed >> 16) as i16
            })
            .collect();
        self.recorders[0].feed(&pcm);
        self.recorders[0].finalize_current();
        self.recorders[1].feed(&vec![0; pcm.len()]);
        self.recorders[1].finalize_current();
        self.pcm.extend(pcm);
        let revision = capture::read(&self.session()).unwrap().revision;
        let deadline = Instant::now() + Duration::from_secs(10);
        let horizon = loop {
            if let Ok(h) = Horizon::read(&self.session()) {
                if h.revision() >= revision {
                    break h;
                }
            }
            assert!(Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        };
        let _execution = localvox_light_core::jobs::try_execution_lock(&self.session())
            .unwrap()
            .unwrap();
        let mut accepted = vec![];
        consume_windows(
            &self.session(),
            &horizon,
            &self.after,
            || false,
            |window| {
                accepted.push(
                    self.cache
                        .resolve(window, |_| Ok("слова из готового фрагмента".into()))?
                        .0,
                );
                Ok(())
            },
        )
        .unwrap();
        let checkpoint = self.cache.checkpoint(&horizon, &accepted).unwrap();
        self.after = checkpoint
            .sources
            .iter()
            .map(|(id, s)| (*id, s.end_sample))
            .collect();
        self.cache.publish(&self.session(), &horizon).unwrap()
    }
}
fn bytes(url: &str, range: Option<&str>) -> std::io::Result<(u16, Vec<u8>, String)> {
    let mut req = ureq::get(url)
        .timeout(Duration::from_secs(5))
        .set("Authorization", "Bearer fixture-token");
    if let Some(range) = range {
        req = req.set("Range", range)
    }
    let response = req.call().map_err(std::io::Error::other)?;
    let status = response.status();
    let content_range = response.header("Content-Range").unwrap_or("").to_string();
    let mut bytes = vec![];
    response.into_reader().read_to_end(&mut bytes)?;
    Ok((status, bytes, content_range))
}

#[test]
fn partial_http_growth_keeps_text_audio_revision_and_exact_range_across_a_file_seam() {
    let mut p = Producer::new();
    let first = p.feed(43);
    let server = journal_http_fixture(p.root.path(), Some("fixture-token"));
    let client =
        localvox_light_client::Client::new(&server.address, "fixture-token".into()).unwrap();
    let first_doc = client.transcript_document("recorded").unwrap();
    let first_info = first_doc.partial.unwrap();
    assert_eq!(
        first_info.snapshot.as_deref(),
        Some(first.snapshot.as_str())
    );
    assert!(!first_doc.lines.is_empty());
    let readable = client.readable("recorded").unwrap();
    assert!(readable.from_transcript);
    assert_eq!(
        readable.lines.iter().map(|l| &l.text).collect::<Vec<_>>(),
        first_doc.lines.iter().map(|l| &l.text).collect::<Vec<_>>()
    );
    assert!(localvox_light_core::export::export_session(
        &p.session(),
        localvox_light_core::export::ExportFormat::Txt
    )
    .is_err());
    assert!(!p.session().join("versions.json").exists());
    let old_media = client.media_snapshot("recorded", Some(&first.snapshot));
    let (_, header, _) = bytes(&old_media.url, Some("bytes=0-43")).unwrap();
    let old_length = u32::from_le_bytes(header[40..44].try_into().unwrap()) as u64 + 44;
    let second = p.feed(20);
    assert_ne!(first.snapshot, second.snapshot);
    let latest = client.transcript_document("recorded").unwrap();
    let latest_info = latest.partial.unwrap();
    assert!(latest_info.follows(&first_info));
    assert!(latest.lines.len() > first_doc.lines.len());
    assert!(latest
        .lines
        .iter()
        .zip(&first_doc.lines)
        .all(|(new, old)| new.id == old.id && new.text == old.text));
    let (_, _, content_range) = bytes(&old_media.url, Some("bytes=0-43")).unwrap();
    assert!(
        content_range.ends_with(&format!("/{old_length}")),
        "old snapshot grew"
    );
    let media = client.media_snapshot("recorded", Some(&second.snapshot));
    let from = 42 * 16000;
    let to = 44 * 16000;
    let range = format!("bytes={}-{}", 44 + from * 2, 44 + to * 2 - 1);
    let (status, data, _) = bytes(&media.url, Some(&range)).unwrap();
    assert_eq!(status, 206);
    let expected: Vec<_> = p.pcm[from..to]
        .iter()
        .flat_map(|s| s.to_le_bytes())
        .collect();
    assert_eq!(data, expected);
    let peaks: Value = ureq::get(&format!(
        "{}/api/sessions/recorded/peaks?snapshot={}&buckets=32",
        server.address, first.snapshot
    ))
    .set("Authorization", "Bearer fixture-token")
    .call()
    .unwrap()
    .into_json()
    .unwrap();
    assert_eq!(
        peaks["duration_sec"].as_f64().unwrap(),
        first_info.audio_end_sec
    );
    assert_eq!(peaks["peaks"].as_array().unwrap().len(), 32);
    assert!(
        bytes(&client.media("recorded").url, None).is_err(),
        "mutable capture audio was served"
    );
    let invalid = client.media_snapshot("recorded", Some("../../meta"));
    assert!(bytes(&invalid.url, None).is_err());
    // Corrupt an actual needed PCM byte, preserve its size/header. Never substitute zeros.
    let mut file = fs::OpenOptions::new()
        .write(true)
        .open(p.session().join("audio/src0_chunk0002.wav"))
        .unwrap();
    file.seek(std::io::SeekFrom::Start(44)).unwrap();
    file.write_all(&[91, 92]).unwrap();
    drop(file);
    assert!(
        bytes(&media.url, Some(&range)).is_err(),
        "corrupt input became successful audio"
    );
    assert!(
        bytes(&old_media.url, Some("bytes=44-200")).is_ok(),
        "unrelated old prefix stopped playing"
    );
    let _keep_capture_alive = &p.owner;
}

#[test]
#[ignore = "requires actual FFmpeg; generated sealed PCM, no devices/models"]
fn real_flac_media_snapshot_keeps_pcm_and_waits_for_an_owned_response() {
    let mut p = Producer::new();
    let first = p.feed(43);
    let archive = Archive::new(p.root.path().into());
    let ffmpeg = std::path::PathBuf::from(chunks::resolve_ffmpeg_for_decode());
    let wav = p.session().join("audio/src0_chunk0001.wav");
    let mut stream = archive
        .audio_wav_snapshot(
            "recorded",
            Some((44, Some(44 + 100))),
            Some(&first.snapshot),
        )
        .unwrap();
    assert!(chunks::codec::convert(&ffmpeg, &wav, || false, Duration::from_millis(80)).is_err());
    assert!(wav.is_file());
    let mut bytes = vec![];
    stream.read_to_end(&mut bytes).unwrap();
    drop(stream);
    for source in [0, 1] {
        chunks::codec::convert(
            &ffmpeg,
            &p.session().join(format!("audio/src{source}_chunk0001.wav")),
            || false,
            Duration::from_secs(15),
        )
        .unwrap();
    }
    assert!(!wav.exists());
    let mut converted = archive
        .audio_wav_snapshot(
            "recorded",
            Some((44, Some(44 + 100))),
            Some(&first.snapshot),
        )
        .unwrap();
    let mut actual = vec![];
    converted.read_to_end(&mut actual).unwrap();
    assert_eq!(actual, bytes);
    let peaks = archive
        .peaks_snapshot("recorded", 32, &first.snapshot)
        .unwrap();
    assert_eq!(
        peaks["duration_sec"],
        first.read(&p.session()).unwrap().audio_end as f64 / 16000.0
    );
}

#[test]
#[ignore = "requires actual mpv; real temporary HTTP/partial snapshots, null audio, no devices/models"]
fn real_mpv_keeps_old_partial_playing_and_loads_new_audio_only_by_command() {
    use localvox_light_terminal::player::{PlaybackCommand, PlaybackState, Player};
    let mut p = Producer::new();
    let first = p.feed(43);
    let server = journal_http_fixture(p.root.path(), Some("fixture-token"));
    let client =
        localvox_light_client::Client::new(&server.address, "fixture-token".into()).unwrap();
    let executable = std::env::var_os("LOCALVOX_MPV")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../target/tools/mpv/mpv.exe")
        });
    let player = Player::with_backend(client, executable, true).unwrap();
    let wait = |predicate: &dyn Fn(&PlaybackState) -> bool| {
        let deadline = Instant::now() + Duration::from_secs(12);
        loop {
            let state = player
                .rx
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .expect("mpv timeout");
            assert!(state.error.is_none(), "{state:?}");
            if predicate(&state) {
                break state;
            }
        }
    };
    assert!(player.submit(PlaybackCommand::Load {
        session: "recorded".into(),
        start: 1.0,
        end: None
    }));
    let first_state = wait(&|state| !state.paused && !state.idle && state.position >= 1.0);
    assert_eq!(
        first_state.snapshot.as_deref(),
        Some(first.snapshot.as_str())
    );
    let second = p.feed(20);
    assert_ne!(first.snapshot, second.snapshot);
    let still = wait(&|state| state.position > first_state.position + 0.2);
    assert_eq!(still.snapshot, first_state.snapshot);
    assert_eq!(still.duration, first_state.duration);
    assert!(player.submit(PlaybackCommand::Toggle));
    let paused = wait(&|s| s.paused && !s.idle);
    assert_eq!(paused.snapshot, first_state.snapshot);
    assert!(player.submit(PlaybackCommand::Load {
        session: "recorded".into(),
        start: 44.0,
        end: Some(45.0)
    }));
    let next = wait(&|s| !s.paused && !s.idle && s.position >= 44.0);
    assert_eq!(next.snapshot.as_deref(), Some(second.snapshot.as_str()));
    assert!(next.duration.unwrap() > first_state.duration.unwrap());
    let ended = wait(&|s| s.idle);
    assert!(ended.position >= 45.0);
    assert_eq!(ended.snapshot, next.snapshot);
    assert!(player.submit(PlaybackCommand::Stop));
}
