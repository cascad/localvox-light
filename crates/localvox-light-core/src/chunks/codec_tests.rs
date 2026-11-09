use super::*;

fn wav(audio: &Path, name: &str, n: usize) -> PathBuf {
    fs::create_dir_all(audio).unwrap();
    let mut writer = StreamingWav::create(audio.join(format!("{name}.part"))).unwrap();
    let samples = (0..n)
        .map(|i| ((i * 1703) % 65536) as i16)
        .collect::<Vec<_>>();
    writer.write(&samples).unwrap();
    writer.finalize().unwrap().unwrap()
}
fn fake_command(output: &Path, mode: &str) -> std::process::Command {
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "chunks::codec::tests::codec_child",
            "--nocapture",
        ])
        .env("LOCALVOX_CODEC_CHILD", mode)
        .env("LOCALVOX_CODEC_OUTPUT", output);
    command
}

#[test]
#[ignore = "subprocess fixture, explicitly run by codec tests"]
fn codec_child() {
    let Some(mode) = std::env::var_os("LOCALVOX_CODEC_CHILD") else {
        return;
    };
    let output = PathBuf::from(std::env::var_os("LOCALVOX_CODEC_OUTPUT").unwrap());
    if mode == "empty" {
        fs::write(output, []).unwrap();
        return;
    }
    if mode == "header_only" {
        let mut bytes = vec![0; 42];
        bytes[..4].copy_from_slice(b"fLaC");
        bytes[4] = 0x80;
        bytes[7] = 34;
        let packed = (16000u64 << 44) | (15u64 << 36) | 4;
        bytes[18..26].copy_from_slice(&packed.to_be_bytes());
        fs::write(output, bytes).unwrap();
        return;
    }
    if mode == "hang" {
        loop {
            fs::write(&output, b"alive").unwrap();
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    panic!("invalid fixture mode");
}

#[test]
fn successful_exit_and_existing_file_are_not_proof_of_flac_audio() {
    for mode in ["empty", "header_only"] {
        let root = tempfile::tempdir().unwrap();
        let audio = root.path().join("audio");
        let source = wav(&audio, "src0_chunk0001", 16000);
        let original = fs::read(&source).unwrap();
        let make = |_: &Path, args: &[std::ffi::OsString]| {
            fake_command(Path::new(args.last().unwrap()), mode)
        };
        assert!(convert_with(
            Path::new("unused"),
            &source,
            &|| false,
            Duration::from_secs(5),
            &make
        )
        .is_err());
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(!source.with_extension("flac").exists());
    }
}

#[test]
fn codec_timeout_and_cancellation_stop_owned_child_and_keep_wav() {
    for cancel in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let audio = root.path().join("audio");
        let source = wav(&audio, "src0_chunk0001", 16000);
        let original = fs::read(&source).unwrap();
        let heartbeat = root.path().join("heartbeat");
        let make = |_: &Path, _: &[std::ffi::OsString]| fake_command(&heartbeat, "hang");
        let start = Instant::now();
        assert!(convert_with(
            Path::new("unused"),
            &source,
            &|| cancel && start.elapsed() > Duration::from_millis(200),
            Duration::from_millis(500),
            &make
        )
        .is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
        assert_eq!(fs::read(&source).unwrap(), original);
        assert!(!source.with_extension("flac").exists());
        if heartbeat.exists() {
            let before = fs::metadata(&heartbeat).unwrap().modified().unwrap();
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(
                fs::metadata(&heartbeat).unwrap().modified().unwrap(),
                before
            );
        }
    }
}

#[test]
fn reader_and_converter_share_one_lock_independent_of_extension_or_zero_padding() {
    let root = tempfile::tempdir().unwrap();
    let audio = root.path().join("audio");
    let source = wav(&audio, "src0_chunk0001", 5);
    with_read(&audio, "src0_chunk0001", |selected| {
        assert_eq!(selected, source);
        assert!(acquire(
            &audio,
            "src0_chunk1",
            true,
            Instant::now() + Duration::from_millis(50),
            &|| false
        )
        .is_err());
        let _other_reader = acquire(
            &audio,
            "src0_chunk0001",
            false,
            Instant::now() + Duration::from_millis(50),
            &|| false,
        )?;
        Ok(())
    })
    .unwrap();
    let _writer = acquire(
        &audio,
        "src0_chunk1",
        true,
        Instant::now() + Duration::from_millis(50),
        &|| false,
    )
    .unwrap();
    assert!(acquire(
        &audio,
        "src0_chunk0001",
        false,
        Instant::now() + Duration::from_millis(50),
        &|| false
    )
    .is_err());
}

#[test]
fn no_tool_keeps_wav_and_pcm_streaming_is_bounded_and_exact() {
    let root = tempfile::tempdir().unwrap();
    let audio = root.path().join("audio");
    let source = wav(&audio, "src0_chunk0001", 100000);
    let bytes = fs::read(&source).unwrap();
    let mut actual = Vec::new();
    let mut largest = 0;
    stream(
        &source,
        Path::new("unused"),
        || false,
        |part| {
            largest = largest.max(part.len());
            actual.extend_from_slice(part);
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(actual, bytes[44..]);
    assert!(largest <= 64 * 1024);
    assert!(convert(
        Path::new("no-such-localvox-ffmpeg"),
        &source,
        || false,
        Duration::from_secs(1)
    )
    .is_err());
    assert_eq!(fs::read(source).unwrap(), bytes);
    assert!(key("../escape").is_err());
    assert!(key("").is_err());
}

#[test]
fn bounded_conversion_owner_stops_waiters_and_keeps_capture_owned_until_they_exit() {
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("session");
    let audio = session.join("audio");
    fs::create_dir_all(&audio).unwrap();
    let generation = uuid::Uuid::now_v7().to_string();
    save_meta_checked(
        &session.join("meta.json"),
        &SessionMeta {
            capture_generation: Some(generation.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let owner = Arc::new(Mutex::new(
        capture::Writer::begin(&session, generation).unwrap(),
    ));
    let source = wav(&audio, "src0_chunk0001", 16000);
    with_read(&audio, "src0_chunk0001", |_| {
        let conversions = Conversions::default();
        for _ in 0..3 {
            conversions.spawn(
                "not-run-while-reader-held".into(),
                source.clone(),
                Some(owner.clone()),
            );
        }
        assert!(conversions.tasks.lock().unwrap().len() <= 2);
        drop(owner);
        assert!(capture::try_mutation_lock(&session)?.is_none());
        let started = Instant::now();
        drop(conversions);
        assert!(started.elapsed() < Duration::from_secs(3));
        assert!(capture::try_mutation_lock(&session)?.is_some());
        assert!(source.is_file());
        Ok(())
    })
    .unwrap();
}

#[test]
fn flac_header_does_not_allow_unknown_counts_formats_or_excessive_allocations() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("input.flac");
    for packed in [
        (16000u64 << 44) | (15 << 36),
        (48000u64 << 44) | (15 << 36) | 5,
        (16000u64 << 44) | (1 << 41) | (15 << 36) | 5,
        (16000u64 << 44) | (23 << 36) | 5,
        (16000u64 << 44) | (15 << 36) | 0xF_FFFF_FFFF,
    ] {
        let mut h = [0; 42];
        h[..4].copy_from_slice(b"fLaC");
        h[4] = 0x80;
        h[7] = 34;
        h[18..26].copy_from_slice(&packed.to_be_bytes());
        fs::write(&path, h).unwrap();
        assert!(samples(&path).is_err());
    }
}

#[test]
#[ignore = "requires real ffmpeg, generated PCM only; no models or devices"]
fn real_flac_preserves_all_samples_and_ranges_and_rejects_short_or_changed_audio() {
    let ffmpeg = PathBuf::from(resolve_ffmpeg_for_decode());
    for mode in ["normal", "short", "changed"] {
        let root = tempfile::tempdir().unwrap();
        let audio = root.path().join("audio");
        let source = wav(&audio, "src0_chunk0001", 32117);
        let original = fs::read(&source).unwrap();
        if mode != "normal" {
            let other = wav(&audio, "other", if mode == "short" { 2000 } else { 32117 });
            if mode == "changed" {
                let mut b = fs::read(&other).unwrap();
                b[50] ^= 1;
                fs::write(&other, b).unwrap();
            }
            let status = std::process::Command::new(&ffmpeg)
                .args(["-v", "error", "-nostdin", "-y", "-i"])
                .arg(other)
                .arg(source.with_extension("flac"))
                .status()
                .unwrap();
            assert!(status.success());
            assert!(convert(&ffmpeg, &source, || false, Duration::from_secs(5)).is_err());
            assert_eq!(fs::read(&source).unwrap(), original);
            continue;
        }
        convert(&ffmpeg, &source, || false, Duration::from_secs(10)).unwrap();
        assert!(!source.exists());
        let decoded = with_read(&audio, "src0_chunk0001", |path| {
            let mut decoded = Vec::new();
            stream(
                path,
                &ffmpeg,
                || false,
                |p| {
                    decoded.extend_from_slice(p);
                    Ok(())
                },
            )?;
            for (a, b) in [(0, 1), (17, 15999), (15999, 16001), (32001, 32117)] {
                assert_eq!(
                    range(path, a, b, &ffmpeg)?,
                    original[44 + a as usize * 2..44 + b as usize * 2]
                );
            }
            Ok(decoded)
        })
        .unwrap();
        assert_eq!(decoded, original[44..]);
    }
}

#[test]
#[ignore = "requires real ffmpeg; concurrent reads/conversion and capture recovery"]
fn real_conversion_waits_for_reader_and_recovered_capture_accepts_verified_flac() {
    let ffmpeg = PathBuf::from(resolve_ffmpeg_for_decode());
    let root = tempfile::tempdir().unwrap();
    let session = root.path().join("sessions/test");
    let audio = session.join("audio");
    fs::create_dir_all(&audio).unwrap();
    let generation = uuid::Uuid::now_v7().to_string();
    save_meta_checked(
        &session.join("meta.json"),
        &SessionMeta {
            sample_rate: 16000,
            capture_generation: Some(generation.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let mut writer = capture::Writer::begin(&session, generation).unwrap();
    writer.open(0, 1, 0).unwrap();
    let source = wav(&audio, "src0_chunk0001", 16000);
    writer
        .close(
            0,
            capture::Range {
                sequence: 1,
                start_sample: 0,
                samples: 16000,
                kind: capture::Kind::Audio,
                problem: None,
            },
        )
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let src = source.clone();
    let ff = ffmpeg.clone();
    let child = with_read(&audio, "src0_chunk0001", |_| {
        let child = std::thread::spawn(move || {
            tx.send(()).unwrap();
            convert(&ff, &src, || false, Duration::from_secs(5))
        });
        rx.recv().unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert!(source.is_file());
        assert!(!source.with_extension("flac").exists());
        Ok(child)
    })
    .unwrap();
    child.join().unwrap().unwrap();
    drop(writer);
    recover_orphan_chunks(root.path());
    assert!(
        capture::read(&session)
            .unwrap()
            .terminal
            .unwrap()
            .interrupted
    );
    let meta: SessionMeta =
        serde_json::from_slice(&fs::read(session.join("meta.json")).unwrap()).unwrap();
    assert_eq!(meta.chunks[0].duration_sec, 1.0);
}
