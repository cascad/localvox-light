use super::*;

struct Fixture {
    _root: tempfile::TempDir,
    session: PathBuf,
    owner: Option<Arc<Mutex<capture::Writer>>>,
    params: Arc<ChunkParams>,
    meta: Arc<Mutex<SessionMeta>>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("session");
        fs::create_dir_all(session.join("audio")).unwrap();
        let generation = uuid::Uuid::now_v7().to_string();
        let meta = SessionMeta {
            capture_generation: Some(generation.clone()),
            sample_rate: SAMPLE_RATE,
            ..Default::default()
        };
        save_meta_checked(&session.join("meta.json"), &meta).unwrap();
        let owner = Arc::new(Mutex::new(
            capture::Writer::begin(&session, generation).unwrap(),
        ));
        Self {
            params: Arc::new(ChunkParams {
                audio_dir: session.join("audio"),
                meta_path: session.join("meta.json"),
                chunk_sec: 300.0,
                flac: false,
                ffmpeg: PathBuf::from(resolve_ffmpeg_for_decode()),
            }),
            meta: Arc::new(Mutex::new(meta)),
            session,
            owner: Some(owner),
            _root: root,
        }
    }
    fn recorder(&self, source: u8) -> ChunkRecorder {
        ChunkRecorder::for_capture(
            source,
            self.params.clone(),
            self.meta.clone(),
            self.owner.as_ref().unwrap().clone(),
            Arc::new(codec::Conversions::default()),
        )
    }
    fn planner(&self) -> Planner {
        Planner::new(
            &self.session,
            capture::read(&self.session).unwrap().generation,
            Recipe::default(),
            PathBuf::from(resolve_ffmpeg_for_decode()),
        )
        .unwrap()
    }
    fn end(&self, a: u64, b: u64) {
        self.owner
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .end("test", false, [(0, a), (1, b)].into())
            .unwrap();
    }
    fn attach(&self, group: &Planners) -> Handle {
        group
            .attach(
                self.owner.as_ref().unwrap().clone(),
                PathBuf::from(resolve_ffmpeg_for_decode()),
            )
            .unwrap()
    }
    fn snapshot(&self) -> Snapshot {
        serde_json::from_slice(&fs::read(self.session.join(FILE)).unwrap()).unwrap()
    }
}

fn wait(mut done: impl FnMut() -> bool) {
    let start = Instant::now();
    while !done() {
        assert!(
            start.elapsed() < Duration::from_secs(5),
            "planner did not reach the expected state"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn windows(f: &Fixture) -> BTreeMap<u8, Vec<Descriptor>> {
    let snapshot = f.snapshot();
    let recipe = snapshot.recipe.fingerprint().unwrap();
    snapshot
        .sources
        .iter()
        .map(|(id, source)| {
            let mut covered = 0;
            let records = source
                .windows
                .iter()
                .map(|r| {
                    assert_eq!(r.start_sample, covered);
                    let path = f.session.join(format!(
                        "capture/plan/{recipe}/src{id}_{}-{}.json",
                        r.start_sample, r.end_sample
                    ));
                    let bytes = fs::read(path).unwrap();
                    assert_eq!(r.sha256, format!("{:x}", Sha256::digest(&bytes)));
                    let record: Descriptor = serde_json::from_slice(&bytes).unwrap();
                    assert_eq!(record.generation, snapshot.generation);
                    assert_eq!(record.recipe, recipe);
                    assert_eq!(record.source, *id);
                    assert_eq!(record.start_sample, r.start_sample);
                    assert_eq!(record.end_sample, r.end_sample);
                    assert!(record.end_sample > record.start_sample);
                    covered = record.end_sample;
                    record
                })
                .collect();
            assert_eq!(covered, source.closed_end);
            assert!(source.closed_end <= source.input_end);
            (*id, records)
        })
        .collect()
}

fn pcm(n: usize) -> Vec<i16> {
    let mut state = 47u32;
    (0..n)
        .map(|i| {
            state = state.wrapping_mul(1664525).wrapping_add(1013904223);
            if i / 320 % 127 >= 120 {
                0
            } else {
                (state >> 16) as i16
            }
        })
        .collect()
}

#[test]
fn sealed_transport_and_pause_preserve_native_vad_windows_and_only_eof_flushes() {
    let f = Fixture::new();
    let audio = pcm(35 * 16000 + 117);
    let mut rec = f.recorder(0);
    let mut planner = f.planner();
    let mut seed = 7usize;
    let mut at = 0;
    while at < audio.len() {
        seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
        let end = audio.len().min(at + 1 + seed % 22001);
        rec.feed(&audio[at..end]);
        rec.finalize_current();
        assert!(!planner.advance(&|| false).unwrap());
        assert_eq!(f.snapshot().sources[&0].input_end, end as u64);
        assert_eq!(
            f.snapshot().common_end,
            0,
            "unseen second source cannot become silence"
        );
        let before = f.snapshot();
        f.owner.as_ref().unwrap().lock().unwrap().pause().unwrap();
        assert!(!planner.advance(&|| false).unwrap());
        assert_eq!(
            f.snapshot().sources,
            before.sources,
            "pause cannot flush the VAD tail"
        );
        at = end;
    }
    f.end(audio.len() as u64, 0);
    assert!(planner.advance(&|| false).unwrap());
    let actual = windows(&f).remove(&0).unwrap();
    let mut vad = Vad::new_with_rate_and_mode(SampleRate::Rate16kHz, VadMode::LowBitrate);
    let mut windower = Recipe::default().windower();
    let mut expected = Vec::new();
    let mut emit = |w: Window<'_>| {
        let bytes: Vec<_> = w.pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
        expected.push((
            w.start_sample,
            w.start_sample + w.pcm.len() as u64,
            w.kind,
            format!("{:x}", Sha256::digest(bytes)),
        ));
        Ok(())
    };
    windower
        .feed_all(
            &audio,
            &mut |frame| vad.is_voice_segment(frame).unwrap_or(true),
            &mut emit,
        )
        .unwrap();
    windower.finish_all(&mut emit).unwrap();
    assert_eq!(
        actual
            .into_iter()
            .map(|r| (r.start_sample, r.end_sample, r.kind, r.pcm_sha256))
            .collect::<Vec<_>>(),
        expected
    );
    assert!(f.snapshot().complete);
    assert_eq!(f.snapshot().common_end, audio.len() as u64);
}

#[test]
fn new_seals_do_not_redecode_the_prefix_and_late_source_limits_common_coverage() {
    let f = Fixture::new();
    let mut mic = f.recorder(0);
    let mut system = f.recorder(1);
    let mut planner = f.planner();
    mic.feed(&pcm(20 * 16000 + 79));
    mic.finalize_current();
    planner.advance(&|| false).unwrap();
    let first = f.session.join("audio/src0_chunk0001.wav");
    let saved = first.with_extension("saved");
    fs::rename(&first, &saved).unwrap();
    mic.feed(&pcm(20 * 16000 + 43));
    mic.finalize_current();
    planner.advance(&|| false).unwrap(); // first file is deliberately unavailable during this step
    assert_eq!(f.snapshot().sources[&0].input_end, 40 * 16000 + 122);
    assert_eq!(f.snapshot().common_end, 0);
    system.feed(&pcm(16 * 16000));
    system.finalize_current();
    planner.advance(&|| false).unwrap();
    let plan = f.snapshot();
    assert_eq!(
        plan.common_end,
        plan.sources.values().map(|s| s.closed_end).min().unwrap()
    );
    assert!(plan.common_end < plan.sources[&0].closed_end);
    fs::rename(saved, first).unwrap();
    f.end(mic.end_samples(), system.end_samples());
    planner.advance(&|| false).unwrap();
    assert_eq!(f.snapshot().common_end, mic.end_samples());
    windows(&f);
}

#[test]
fn unknown_interval_blocks_that_source_without_throwing_away_the_other() {
    let f = Fixture::new();
    let mut mic = f.recorder(0);
    let mut system = f.recorder(1);
    fs::create_dir(f.session.join("audio/src0_chunk0001.wav")).unwrap();
    mic.feed(&pcm(16000));
    mic.finalize_current();
    mic.feed(&pcm(16000));
    mic.finalize_current();
    system.feed(&pcm(20 * 16000));
    system.finalize_current();
    let mut planner = f.planner();
    assert!(!planner.advance(&|| false).unwrap());
    let snapshot = f.snapshot();
    assert_eq!(snapshot.sources[&0].sequence, 0);
    assert_eq!(snapshot.sources[&0].input_end, 0);
    assert_eq!(snapshot.sources[&1].input_end, 20 * 16000);
    assert_eq!(snapshot.common_end, 0);
    assert!(!snapshot.complete);
}

#[test]
fn owned_planner_threads_are_bounded_and_busy_planning_does_not_prevent_recording() {
    let a = Fixture::new();
    let b = Fixture::new();
    let c = Fixture::new();
    let group = Planners::default();
    let first = a.attach(&group);
    let second = b.attach(&group);
    assert!(group
        .attach(c.owner.as_ref().unwrap().clone(), "unused".into())
        .is_err());
    assert_eq!(group.tasks.lock().unwrap().len(), 2);
    let mut recorder = c.recorder(0);
    recorder.feed(&pcm(17000));
    recorder.finalize_current();
    c.end(recorder.end_samples(), 0);
    assert!(capture::is_finalized(&c.session).unwrap());
    drop(first);
    drop(second);
    wait(|| group.tasks.lock().unwrap().iter().all(|t| t.is_finished()));
    let third = c.attach(&group);
    wait(|| c.session.join(FILE).is_file());
    assert!(c.snapshot().complete);
    drop(third);
}

#[test]
fn replay_accepts_identical_descriptors_but_never_overwrites_a_changed_input() {
    let f = Fixture::new();
    let mut rec = f.recorder(0);
    rec.feed(&pcm(20 * 16000));
    rec.finalize_current();
    f.end(rec.end_samples(), 0);
    f.planner().advance(&|| false).unwrap();
    let first = fs::read(f.session.join(FILE)).unwrap();
    f.planner().advance(&|| false).unwrap();
    assert_eq!(fs::read(f.session.join(FILE)).unwrap(), first);
    let path = f.session.join("audio/src0_chunk0001.wav");
    let mut bytes = fs::read(&path).unwrap();
    bytes[50] ^= 1;
    fs::write(path, bytes).unwrap();
    assert!(f.planner().advance(&|| false).is_err());
    assert_eq!(fs::read(f.session.join(FILE)).unwrap(), first);
}

#[test]
fn coalesced_notifications_and_stop_reach_the_last_disk_horizon_without_more_pcm() {
    let f = Fixture::new();
    let group = Planners::default();
    let handle = f.attach(&group);
    assert!(group
        .attach(f.owner.as_ref().unwrap().clone(), "unused".into())
        .is_err());
    let mut rec = f.recorder(0);
    for _ in 0..20 {
        rec.feed(&pcm(1100));
        rec.finalize_current();
        for _ in 0..100 {
            let _ = handle.signal.try_send(());
            assert!(handle.signal.len() <= 1);
        }
    }
    f.end(rec.end_samples(), 0);
    drop(handle);
    wait(|| {
        fs::read(f.session.join(FILE))
            .ok()
            .and_then(|b| serde_json::from_slice::<Snapshot>(&b).ok())
            .is_some_and(|s| s.complete)
    });
    assert_eq!(f.snapshot().sources[&0].input_end, 22000);
    assert_eq!(f.snapshot().sources[&0].sequence, 20);
    windows(&f);
}

#[test]
fn refused_planner_publication_is_journaled_and_primary_recording_continues() {
    let f = Fixture::new();
    fs::create_dir_all(f.session.join(FILE)).unwrap();
    let group = Planners::default();
    let handle = f.attach(&group);
    wait(|| group.tasks.lock().unwrap().iter().all(|t| t.is_finished()));
    let mut rec = f.recorder(0);
    rec.feed(&pcm(17000));
    rec.finalize_current();
    f.end(rec.end_samples(), 0);
    assert!(capture::is_finalized(&f.session).unwrap());
    assert_eq!(
        capture::wav_samples(&f.session.join("audio/src0_chunk0001.wav")).unwrap(),
        17000
    );
    assert!(fs::read_to_string(f.session.join("progress.jsonl"))
        .unwrap()
        .contains("Раннее планирование остановлено"));
    drop(handle);
}

#[test]
fn shutdown_without_pcm_exits_quickly_without_an_error_or_a_fabricated_eof() {
    let mut f = Fixture::new();
    let group = Planners::default();
    let handle = f.attach(&group);
    wait(|| f.session.join(FILE).is_file());
    f.owner.take();
    assert!(capture::try_mutation_lock(&f.session).unwrap().is_none());
    drop(handle);
    let start = Instant::now();
    drop(group);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(capture::try_mutation_lock(&f.session).unwrap().is_some());
    assert!(capture::read(&f.session).unwrap().terminal.is_none());
    assert!(!f.snapshot().complete);
    let log = fs::read_to_string(f.session.join("progress.jsonl")).unwrap_or_default();
    assert!(!log.contains("Раннее планирование остановлено"));
}

#[test]
fn blocked_decoder_keeps_capture_ownership_after_stop_until_shutdown_releases_it() {
    let mut f = Fixture::new();
    let mut rec = f.recorder(0);
    rec.feed(&pcm(17000));
    rec.finalize_current();
    f.end(rec.end_samples(), 0);
    drop(rec);
    codec::with_read(&f.session.join("audio"), "src0_chunk0001", |_| Ok(())).unwrap();
    let sidecar = fs::read_dir(f.session.join(".audio-locks"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .open(sidecar)
        .unwrap();
    lock.lock().unwrap();
    let group = Planners::default();
    let handle = f.attach(&group);
    f.owner.take();
    drop(handle);
    assert!(capture::try_mutation_lock(&f.session).unwrap().is_none());
    let start = Instant::now();
    drop(group);
    assert!(start.elapsed() < Duration::from_secs(2));
    assert!(capture::try_mutation_lock(&f.session).unwrap().is_some());
    assert!(f.session.join("audio/src0_chunk0001.wav").exists());
}

#[test]
#[ignore = "requires real ffmpeg, generated PCM only"]
fn real_planner_seam_through_verified_flac_matches_the_continuous_plan() {
    let f = Fixture::new();
    let mut rec = f.recorder(0);
    let audio = pcm(33 * 16000 + 217);
    rec.feed(&audio[..17 * 16000 + 31]);
    rec.finalize_current();
    rec.feed(&audio[17 * 16000 + 31..]);
    rec.finalize_current();
    f.end(rec.end_samples(), 0);
    f.planner().advance(&|| false).unwrap();
    let expected = windows(&f);
    for seq in 1..=2 {
        codec::convert(
            &PathBuf::from(resolve_ffmpeg_for_decode()),
            &f.session.join(format!("audio/src0_chunk{seq:04}.wav")),
            || false,
            Duration::from_secs(10),
        )
        .unwrap();
        assert!(codec::verify_conversion_cancelled(
            &f.session.join("audio"),
            &format!("src0_chunk{seq:04}"),
            capture::read(&f.session).unwrap().sources[&0].ranges[(seq - 1) as usize].samples,
            &capture::read(&f.session).unwrap().generation,
            &|| true,
        )
        .is_err());
    }
    f.planner().advance(&|| false).unwrap();
    assert_eq!(windows(&f), expected);
}
