use super::*;

struct Fixture {
    root: tempfile::TempDir,
    session: PathBuf,
    params: Arc<ChunkParams>,
    meta: Arc<Mutex<SessionMeta>>,
    writer: Option<Arc<Mutex<Writer>>>,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let session = root.path().join("sessions/meeting");
        let audio = session.join("audio");
        fs::create_dir_all(&audio).unwrap();
        let generation = uuid::Uuid::now_v7().to_string();
        let meta = SessionMeta {
            sample_rate: SAMPLE_RATE,
            capture_generation: Some(generation.clone()),
            ..Default::default()
        };
        save_meta_checked(&session.join("meta.json"), &meta).unwrap();
        let writer = Arc::new(Mutex::new(Writer::begin(&session, generation).unwrap()));
        let params = Arc::new(ChunkParams {
            audio_dir: audio,
            meta_path: session.join("meta.json"),
            chunk_sec: 1.0,
            flac: false,
            ffmpeg: "ffmpeg".into(),
        });
        Self {
            root,
            session,
            params,
            meta: Arc::new(Mutex::new(meta)),
            writer: Some(writer),
        }
    }
    fn recorder(&self, id: u8) -> ChunkRecorder {
        ChunkRecorder::for_capture(
            id,
            self.params.clone(),
            self.meta.clone(),
            self.writer.as_ref().unwrap().clone(),
            Arc::new(codec::Conversions::default()),
        )
    }
    fn end(&self, ends: [u64; 2]) -> Result<()> {
        self.writer.as_ref().unwrap().lock().unwrap().end(
            "вручную",
            false,
            [(0, ends[0]), (1, ends[1])].into(),
        )
    }
    fn release(&mut self) {
        // Drop the final producer guard, as after an engine exit.
        assert_eq!(Arc::strong_count(self.writer.as_ref().unwrap()), 1);
        self.writer.take();
    }
}

#[test]
fn closed_ranges_are_not_eof_and_empty_rotations_have_explicit_sequence() {
    let f = Fixture::new();
    let mut mic = f.recorder(0);
    let mut system = f.recorder(1);
    mic.feed(&vec![12; 16000]);
    system.feed(&vec![34; 32000]);
    let first = read(&f.session).unwrap();
    assert_eq!(first.sources[&0].confirmed_end(), 16000);
    assert_eq!(first.sources[&1].confirmed_end(), 32000);
    assert!(first.terminal.is_none());
    assert!(f.end([16000, 32000]).is_err(), "open files forbid EOF");
    mic.finalize_current();
    system.finalize_current();
    f.end([16000, 32000]).unwrap();
    let complete = read(&f.session).unwrap();
    assert_eq!(complete.sources[&0].ranges[1].kind, Kind::Empty);
    assert_eq!(complete.sources[&0].ranges[1].samples, 0);
    assert!(!f.session.join("audio/src0_chunk0002.part").exists());
    assert!(complete.terminal.is_some());
    assert!(is_finalized(&f.session).unwrap());
    assert!(
        f.end([16000, 32000]).is_ok(),
        "same EOF retry is idempotent"
    );
    assert!(f.end([16000, 33000]).is_err());
}

#[test]
fn pause_preserves_generation_and_sample_clock_without_eof() {
    let f = Fixture::new();
    let mut rec = f.recorder(0);
    rec.feed(&vec![7; 8000]);
    rec.finalize_current();
    f.writer.as_ref().unwrap().lock().unwrap().pause().unwrap();
    let paused = read(&f.session).unwrap();
    assert!(paused.paused && paused.terminal.is_none());
    rec.feed(&vec![9; 640]);
    rec.finalize_current();
    let resumed = read(&f.session).unwrap();
    assert!(!resumed.paused);
    assert_eq!(paused.generation, resumed.generation);
    assert_eq!(resumed.sources[&0].ranges[1].start_sample, 8000);
    assert_eq!(resumed.sources[&0].confirmed_end(), 8640);
    f.end([8640, 0]).unwrap();
}

#[test]
fn failed_manifest_publication_cannot_advance_disk_and_retry_keeps_all_updates() {
    let f = Fixture::new();
    let mut rec = f.recorder(0);
    rec.feed(&vec![3; 320]);
    let before = read(&f.session).unwrap();
    let path = f.session.join(FILE);
    let backup = path.with_extension("saved");
    fs::rename(&path, &backup).unwrap();
    fs::create_dir(&path).unwrap();
    rec.feed(&vec![4; 16000 - 320]);
    rec.finalize_current();
    assert!(read(&f.session).is_err());
    let stored: Snapshot = serde_json::from_slice(&fs::read(&backup).unwrap()).unwrap();
    assert_eq!(stored, before);
    assert!(f.end([16000, 0]).is_err());
    fs::remove_dir(&path).unwrap();
    fs::rename(&backup, &path).unwrap();
    f.end([16000, 0]).unwrap();
    let committed = read(&f.session).unwrap();
    assert_eq!(committed.sources[&0].confirmed_end(), 16000);
    assert!(committed.terminal.is_some());
    assert_eq!(committed.sources[&0].ranges.len(), 2);
}

#[test]
fn failed_rename_keeps_part_blocks_eof_and_can_be_verified_on_recovery() {
    let mut f = Fixture::new();
    let mut rec = f.recorder(0);
    let collision = f.session.join("audio/src0_chunk0001.wav");
    fs::create_dir(&collision).unwrap();
    rec.feed(&vec![23; 16000]);
    rec.finalize_current();
    let failed = read(&f.session).unwrap();
    assert_eq!(failed.sources[&0].ranges[0].kind, Kind::Unknown);
    assert_eq!(failed.sources[&0].confirmed_end(), 0);
    assert!(f.end([16000, 0]).is_err());
    assert!(f.session.join("audio/src0_chunk0001.part").is_file());
    fs::remove_dir(&collision).unwrap();
    drop(rec);
    f.release();
    assert_eq!(recover_orphan_chunks(f.root.path()), 1);
    let restored = read(&f.session).unwrap();
    assert!(restored.terminal.unwrap().interrupted);
    assert_eq!(restored.sources[&0].confirmed_end(), 16000);
    assert!(restored.sources[&0].ranges[1].kind == Kind::Empty);
    assert_eq!(wav_samples(&collision).unwrap(), 16000);
}

#[test]
fn recovery_does_not_touch_an_owned_capture_or_infer_eof_from_quiet() {
    let f = Fixture::new();
    let mut rec = f.recorder(0);
    rec.feed(&vec![42; 8000]);
    let before = read(&f.session).unwrap();
    let part = f.session.join("audio/src0_chunk0001.part");
    assert_eq!(recover_orphan_chunks(f.root.path()), 0);
    assert_eq!(read(&f.session).unwrap(), before);
    assert!(part.is_file() && !part.with_extension("wav").exists());
}

#[test]
fn crash_after_audio_rename_before_seal_recovers_exact_saved_samples_once() {
    let mut f = Fixture::new();
    f.writer
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .open(1, 1, 0)
        .unwrap();
    let part = f.session.join("audio/src1_chunk0001.part");
    let mut w = StreamingWav::create(part).unwrap();
    w.write(&[1, 2, 3, 4, 5]).unwrap();
    w.finalize().unwrap(); // no range commit, as after kill
    f.release();
    recover_orphan_chunks(f.root.path());
    let recovered = read(&f.session).unwrap();
    assert_eq!(recovered.sources[&1].confirmed_end(), 5);
    assert!(recovered.terminal.as_ref().unwrap().interrupted);
    recover_orphan_chunks(f.root.path());
    assert_eq!(read(&f.session).unwrap(), recovered);
    let meta: SessionMeta =
        serde_json::from_slice(&fs::read(f.session.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta.chunks.len(), 1);
    assert_eq!(meta.chunks[0].duration_sec, 5.0 / 16000.0);
}

#[test]
fn missing_or_unaccounted_audio_cannot_turn_into_complete_silence() {
    for extra in [false, true] {
        let mut f = Fixture::new();
        let mut rec = f.recorder(0);
        rec.feed(&vec![7; 16000]);
        rec.finalize_current();
        if extra {
            let mut w = StreamingWav::create(f.session.join("audio/src0_chunk0003.part")).unwrap();
            w.write(&[3, 4, 5]).unwrap();
            w.finalize().unwrap();
        } else {
            fs::remove_file(f.session.join("audio/src0_chunk0001.wav")).unwrap()
        }
        drop(rec);
        f.release();
        recover_orphan_chunks(f.root.path());
        assert!(read(&f.session).unwrap().terminal.is_none());
        assert!(!is_finalized(&f.session).unwrap());
    }
}

#[test]
fn recovery_never_shortens_a_known_lost_range_or_discards_an_incomplete_sample() {
    for partial in [false, true] {
        let mut f = Fixture::new();
        f.writer
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .open(0, 1, 0)
            .unwrap();
        let part = f.session.join("audio/src0_chunk0001.part");
        let mut w = StreamingWav::create(part.clone()).unwrap();
        w.write(&[1, 2, 3]).unwrap();
        drop(w);
        if partial {
            let mut file = OpenOptions::new().append(true).open(&part).unwrap();
            file.write_all(&[99]).unwrap();
        } else {
            f.writer
                .as_ref()
                .unwrap()
                .lock()
                .unwrap()
                .close(
                    0,
                    Range {
                        sequence: 1,
                        start_sample: 0,
                        samples: 9,
                        kind: Kind::Unknown,
                        problem: Some("known write failure".into()),
                    },
                )
                .unwrap();
        }
        let before = fs::read(&part).unwrap();
        f.release();
        recover_orphan_chunks(f.root.path());
        assert!(!is_finalized(&f.session).unwrap());
        if partial {
            assert_eq!(fs::read(&part).unwrap(), before)
        } else {
            assert_eq!(read(&f.session).unwrap().sources[&0].ranges[0].samples, 9)
        }
    }
}

#[test]
fn failed_empty_close_can_be_recovered_without_inventing_an_audio_file() {
    let mut f = Fixture::new();
    f.writer
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .open(0, 1, 0)
        .unwrap();
    let part = f.session.join("audio/src0_chunk0001.part");
    drop(StreamingWav::create(part.clone()).unwrap());
    f.writer
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .close(
            0,
            Range {
                sequence: 1,
                start_sample: 0,
                samples: 0,
                kind: Kind::Unknown,
                problem: Some("remove temporarily failed".into()),
            },
        )
        .unwrap();
    f.release();
    recover_orphan_chunks(f.root.path());
    let snapshot = read(&f.session).unwrap();
    assert!(snapshot.terminal.is_some());
    assert_eq!(snapshot.sources[&0].ranges[0].kind, Kind::Empty);
    assert!(!part.exists() && !part.with_extension("wav").exists());
}

#[test]
fn unpublished_empty_close_keeps_proof_and_restart_can_finish_from_the_old_open() {
    let mut f = Fixture::new();
    let mut rec = f.recorder(0);
    rec.feed(&vec![3; 16000]); // rotation opens an empty successor
    let path = f.session.join(FILE);
    let backup = path.with_extension("saved");
    fs::rename(&path, &backup).unwrap();
    fs::create_dir(&path).unwrap();
    rec.finalize_current();
    let part = f.session.join("audio/src0_chunk0002.part");
    assert_eq!(wav_samples(&part).unwrap(), 0);
    drop(rec);
    f.release();
    fs::remove_dir(&path).unwrap();
    fs::rename(&backup, &path).unwrap();
    assert!(read(&f.session).unwrap().sources[&0].open.is_some());
    recover_orphan_chunks(f.root.path());
    let restored = read(&f.session).unwrap();
    assert!(restored.terminal.as_ref().unwrap().interrupted);
    assert_eq!(restored.sources[&0].ranges[1].kind, Kind::Empty);
    assert!(!part.exists());
    recover_orphan_chunks(f.root.path());
    assert_eq!(read(&f.session).unwrap(), restored);
}

#[test]
fn crash_after_empty_seal_before_cleanup_never_blocks_terminal_or_loses_audio() {
    for ended in [false, true] {
        let mut f = Fixture::new();
        let mut rec = f.recorder(0);
        rec.feed(&vec![3; 16000]);
        rec.finalize_current();
        let part = f.session.join("audio/src0_chunk0002.part");
        drop(StreamingWav::create(part.clone()).unwrap());
        if ended {
            f.end([16000, 0]).unwrap();
        }
        drop(rec);
        f.release();
        recover_orphan_chunks(f.root.path());
        let restored = read(&f.session).unwrap();
        assert!(restored.terminal.is_some());
        assert_eq!(restored.sources[&0].confirmed_end(), 16000);
        assert!(!part.exists());
    }
}

#[test]
fn malformed_missing_or_foreign_ledger_never_falls_back_to_legacy() {
    let f = Fixture::new();
    let path = f.session.join(FILE);
    let original = fs::read(&path).unwrap();
    for corrupt in [b"{".to_vec(), b"{}".to_vec()] {
        fs::write(&path, corrupt).unwrap();
        assert!(is_finalized(&f.session).is_err());
    }
    fs::remove_file(&path).unwrap();
    assert!(is_finalized(&f.session).is_err());
    fs::write(&path, &original).unwrap();
    let mut different: Snapshot = serde_json::from_slice(&original).unwrap();
    different.generation = uuid::Uuid::now_v7().to_string();
    fs::write(&path, serde_json::to_vec(&different).unwrap()).unwrap();
    assert!(is_finalized(&f.session).is_err());
    let legacy = f.root.path().join("legacy");
    fs::create_dir(&legacy).unwrap();
    save_meta_checked(&legacy.join("meta.json"), &SessionMeta::default()).unwrap();
    assert!(is_finalized(&legacy).unwrap());
}

#[test]
fn fresh_full_input_refuses_a_lost_sealed_chunk_or_unrecorded_suffix() {
    for extra in [false, true] {
        let f = Fixture::new();
        let mut rec = f.recorder(0);
        rec.feed(&vec![3; 16000]);
        rec.finalize_current();
        f.end([16000, 0]).unwrap();
        validate_final_input(&f.session).unwrap();
        if extra {
            let mut wav =
                StreamingWav::create(f.session.join("audio/src0_chunk0003.part")).unwrap();
            wav.write(&[7, 8, 9]).unwrap();
            wav.finalize().unwrap();
        } else {
            fs::remove_file(f.session.join("audio/src0_chunk0001.wav")).unwrap();
        }
        assert!(validate_final_input(&f.session).is_err());
    }
}

#[test]
fn refused_open_close_pause_and_terminal_commit_never_changes_the_last_disk_snapshot() {
    let f = Fixture::new();
    let mut writer = f.writer.as_ref().unwrap().lock().unwrap();
    for boundary in 0..4 {
        let path = f.session.join(FILE);
        let before = fs::read(&path).unwrap();
        let backup = path.with_extension("saved");
        fs::rename(&path, &backup).unwrap();
        fs::create_dir(&path).unwrap();
        let result = match boundary {
            0 => writer.open(0, 1, 0),
            1 => writer.close(
                0,
                Range {
                    sequence: 1,
                    start_sample: 0,
                    samples: 0,
                    kind: Kind::Empty,
                    problem: None,
                },
            ),
            2 => writer.pause(),
            _ => writer.end("test", false, [(0, 0), (1, 0)].into()),
        };
        assert!(result.is_err());
        assert_eq!(fs::read(&backup).unwrap(), before);
        fs::remove_dir(&path).unwrap();
        fs::rename(&backup, &path).unwrap();
        writer.persist().unwrap(); // retry publication, including the retained logical update
        read(&f.session).unwrap();
    }
    assert!(read(&f.session).unwrap().terminal.is_some());
}

#[test]
fn gaps_overlaps_unknown_outcomes_and_invented_eof_are_rejected() {
    let f = Fixture::new();
    let valid = read(&f.session).unwrap();
    for (seq, start, kind, samples) in [
        (2, 0, Kind::Audio, 1),
        (1, 10, Kind::Audio, 1),
        (1, 0, Kind::Empty, 1),
        (1, 0, Kind::Audio, 0),
        (1, 0, Kind::Unknown, 1),
    ] {
        let mut bad = valid.clone();
        bad.sources.get_mut(&0).unwrap().ranges.push(Range {
            sequence: seq,
            start_sample: start,
            samples,
            kind,
            problem: None,
        });
        assert!(bad.validate().is_err());
    }
    let mut bad = valid;
    bad.terminal = Some(Terminal {
        at: "t".into(),
        reason: "x".into(),
        interrupted: false,
        ends: [(0, 9), (1, 0)].into(),
    });
    assert!(bad.validate().is_err());
}

#[derive(Clone, Copy)]
enum Fault {
    Seek,
    Header,
    Sync,
    Data,
    DataSync,
}
struct FaultFile {
    file: File,
    fault: Fault,
}
impl Write for FaultFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if matches!(self.fault, Fault::Header) && self.file.stream_position()? == 0
            || matches!(self.fault, Fault::Data)
        {
            return Err(std::io::Error::other("injected write refusal"));
        }
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }
}
impl Seek for FaultFile {
    fn seek(&mut self, where_to: SeekFrom) -> std::io::Result<u64> {
        if matches!(self.fault, Fault::Seek) {
            Err(std::io::Error::other("injected seek refusal"))
        } else {
            self.file.seek(where_to)
        }
    }
}
impl WavFile for FaultFile {
    fn sync_all(&self) -> std::io::Result<()> {
        if matches!(self.fault, Fault::Sync) {
            Err(std::io::Error::other("injected sync refusal"))
        } else {
            self.file.sync_all()
        }
    }
    fn sync_data(&self) -> std::io::Result<()> {
        if matches!(self.fault, Fault::DataSync) {
            Err(std::io::Error::other("injected data sync refusal"))
        } else {
            self.file.sync_data()
        }
    }
}

#[test]
fn seek_header_or_sync_failure_never_publishes_a_closed_wav() {
    for fault in [Fault::Seek, Fault::Header, Fault::Sync] {
        let root = tempfile::tempdir().unwrap();
        let part = root.path().join("chunk.part");
        let mut w = StreamingWav::create(part.clone()).unwrap();
        w.write(&[1, 2, 3]).unwrap();
        let w = StreamingWav {
            part_path: w.part_path,
            file: FaultFile {
                file: w.file,
                fault,
            },
            data_bytes: w.data_bytes,
            last_sync: w.last_sync,
            failed: false,
        };
        assert!(w.finalize().is_err());
        assert!(part.is_file() && !part.with_extension("wav").exists());
        assert_eq!(fs::metadata(&part).unwrap().len(), 50);
    }
}

#[test]
fn write_or_periodic_sync_failure_is_sticky_and_keeps_primary_part() {
    for fault in [Fault::Data, Fault::DataSync] {
        let root = tempfile::tempdir().unwrap();
        let part = root.path().join("chunk.part");
        let w = StreamingWav::create(part.clone()).unwrap();
        let mut w = StreamingWav {
            part_path: w.part_path,
            file: FaultFile {
                file: w.file,
                fault,
            },
            data_bytes: 0,
            last_sync: Instant::now() - FSYNC_INTERVAL,
            failed: false,
        };
        assert!(w.write(&[1, 2, 3]).is_err());
        assert!(w.failed);
        assert!(w.write(&[4, 5]).is_err());
        assert!(w.finalize().is_err());
        assert!(part.is_file() && !part.with_extension("wav").exists());
    }
}
