//! Real mpv + HTTP Range on generated PCM. No microphone, archive or audible output.
use localvox_light_client::Client;
use localvox_light_terminal::player::{Mpv, PlaybackCommand, PlaybackState, Player};
use std::{
    io::Read,
    path::PathBuf,
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        Arc, Mutex,
    },
    time::{Duration, Instant},
};
use tiny_http::{Header, Response, Server};

struct ServerGuard {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

struct Wave {
    position: u64,
    end: u64,
    header: [u8; 44],
    sent: Arc<AtomicU64>,
}
impl Read for Wave {
    fn read(&mut self, out: &mut [u8]) -> std::io::Result<usize> {
        let count = (self.end - self.position).min(out.len() as u64) as usize;
        if count == 0 {
            return Ok(0);
        }
        out[..count].fill(0);
        for (i, byte) in out[..count].iter_mut().enumerate() {
            let pos = self.position + i as u64;
            if pos < 44 {
                *byte = self.header[pos as usize];
            }
        }
        self.position += count as u64;
        self.sent.fetch_add(count as u64, Ordering::Relaxed);
        Ok(count)
    }
}
fn wave_header() -> [u8; 44] {
    let mut h = [0; 44];
    let data = 16000u32 * 2 * 3600;
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(data + 36).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&1u16.to_le_bytes());
    h[24..28].copy_from_slice(&16000u32.to_le_bytes());
    h[28..32].copy_from_slice(&32000u32.to_le_bytes());
    h[32..34].copy_from_slice(&2u16.to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}
fn wait(player: &mut Mpv, condition: impl Fn(&PlaybackState) -> bool) -> PlaybackState {
    let deadline = Instant::now() + Duration::from_secs(7);
    loop {
        let state = player.state().unwrap();
        if condition(&state) {
            return state;
        }
        assert!(
            Instant::now() < deadline,
            "player condition timed out: {state:?}"
        );
        std::thread::sleep(Duration::from_millis(30));
    }
}
fn wait_worker(player: &Player, condition: impl Fn(&PlaybackState) -> bool) -> PlaybackState {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let state = player
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        assert!(state.error.is_none(), "{state:?}");
        if condition(&state) {
            return state;
        }
    }
}

#[test]
#[ignore = "requires real mpv: npm --prefix ui run setup:mpv; cargo test -p localvox-light-terminal --test media_playback -- --ignored"]
fn real_player_auth_range_pause_and_repeated_fragment() {
    let executable = std::env::var_os("LOCALVOX_MPV")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/tools/mpv/mpv.exe")
        });
    assert!(
        executable.is_file(),
        "install the real backend before this acceptance test"
    );
    let server = Server::http("127.0.0.1:0").unwrap();
    let address = format!("http://{}", server.server_addr());
    let stop = Arc::new(AtomicBool::new(false));
    let stopping = stop.clone();
    let sent = Arc::new(AtomicU64::new(0));
    let server_sent = sent.clone();
    let ranges = Arc::new(Mutex::new(Vec::new()));
    let server_ranges = ranges.clone();
    let thread = std::thread::spawn(move || {
        let mut requests = vec![];
        let length = 16000u64 * 2 * 3600 + 44;
        while !stopping.load(Ordering::Relaxed) {
            if let Some(request) = server.recv_timeout(Duration::from_millis(100)).unwrap() {
                assert!(request
                    .headers()
                    .iter()
                    .any(|h| h.field.equiv("Authorization")
                        && h.value.as_str() == "Bearer synthetic-media-secret"));
                if request.url() == "/api/sessions/missing/audio.wav" {
                    request
                        .respond(Response::from_string("missing").with_status_code(404))
                        .unwrap();
                    continue;
                }
                if request.url().ends_with("/playback") {
                    request
                        .respond(
                            Response::from_string(
                                r#"{"duration_sec":3600,"snapshot":null,"partial":null}"#,
                            )
                            .with_header(
                                Header::from_bytes("Content-Type", "application/json").unwrap(),
                            ),
                        )
                        .unwrap();
                    continue;
                }
                assert!([
                    "/api/sessions/demo/audio.wav",
                    "/api/sessions/other/audio.wav"
                ]
                .contains(&request.url()));
                let from = request
                    .headers()
                    .iter()
                    .find(|h| h.field.equiv("Range"))
                    .and_then(|h| h.value.as_str().strip_prefix("bytes="))
                    .and_then(|s| s.split('-').next())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(0)
                    .min(length);
                server_ranges.lock().unwrap().push(from);
                let sent = server_sent.clone();
                requests.push(std::thread::spawn(move || {
                    let mut response = Response::new(
                        206.into(),
                        vec![
                            Header::from_bytes("Content-Type", "audio/wav").unwrap(),
                            Header::from_bytes("Accept-Ranges", "bytes").unwrap(),
                            Header::from_bytes(
                                "Content-Range",
                                format!("bytes {from}-{}/{length}", length - 1),
                            )
                            .unwrap(),
                        ],
                        Wave {
                            position: from,
                            end: length,
                            header: wave_header(),
                            sent,
                        },
                        Some((length - from) as usize),
                        None,
                    );
                    response.add_header(Header::from_bytes("Connection", "close").unwrap());
                    let _ = request.respond(response);
                }));
            }
        }
        for request in requests {
            request.join().unwrap();
        }
    });
    let server_guard = ServerGuard {
        stop,
        thread: Some(thread),
    };
    let client = Client::new(&address, "synthetic-media-secret".into()).unwrap();
    let media = client.media("demo");
    let mut player = Mpv::start(&executable, true).unwrap();
    player
        .load(&media.url, &media.headers, 1800.0, None)
        .unwrap();
    let state = wait(&mut player, |s| !s.idle && s.position >= 1800.0);
    assert!(state.duration.unwrap() > 3599.0);
    let first_load = sent.load(Ordering::Relaxed);
    assert!(
        first_load < 16000 * 2 * 3600 / 2,
        "playback must start before loading half of the generated recording"
    );
    player
        .command(serde_json::json!(["set_property", "pause", true]))
        .unwrap();
    let paused = wait(&mut player, |s| s.paused);
    std::thread::sleep(Duration::from_millis(100));
    assert!((player.state().unwrap().position - paused.position).abs() < 0.1);
    player.seek(1850.0).unwrap();
    let sought = wait(&mut player, |s| s.position >= 1849.9 && s.position < 1850.1);
    assert!(sought.paused, "seek must preserve pause");
    assert_eq!(
        player
            .command(serde_json::json!(["get_property", "path"]))
            .unwrap(),
        media.url
    );
    player
        .load(&media.url, &media.headers, 12.0, Some(14.0))
        .unwrap();
    wait(&mut player, |s| !s.idle && s.position >= 12.0);
    player
        .command(serde_json::json!(["set_property", "pause", true]))
        .unwrap();
    player.seek(20.0).unwrap();
    // mpv's paused audio clock includes its output buffer (under 250 ms for
    // this null-audio fixture); this checks the interval and retained pause.
    assert!(wait(&mut player, |s| (s.position - 20.0).abs() < 0.25).paused);
    player
        .command(serde_json::json!(["set_property", "pause", false]))
        .unwrap();
    wait(&mut player, |s| !s.idle && s.position > 20.2);
    for _ in 0..2 {
        player
            .load(&media.url, &media.headers, 12.0, Some(12.45))
            .unwrap();
        wait(&mut player, |s| !s.idle && s.position >= 12.0);
        wait(&mut player, |s| s.idle);
    }
    let other = client.media("other");
    player
        .load(&other.url, &other.headers, 7.0, Some(7.45))
        .unwrap();
    wait(&mut player, |s| {
        !s.idle && s.position >= 7.0 && s.position < 8.0
    });
    assert_eq!(
        player
            .command(serde_json::json!(["get_property", "path"]))
            .unwrap(),
        other.url
    );
    wait(&mut player, |s| s.idle);
    let missing = client.media("missing");
    player
        .load(&missing.url, &missing.headers, 0.0, None)
        .unwrap();
    wait(&mut player, |s| s.error.is_some());
    player.stop().unwrap();
    assert!(player.state().unwrap().error.is_none());
    drop(player);
    let worker = Player::with_backend(client.clone(), executable, true).unwrap();
    assert!(worker.submit(PlaybackCommand::Load {
        session: "demo".into(),
        start: 100.0,
        end: None
    }));
    wait_worker(&worker, |s| {
        s.session.as_deref() == Some("demo") && !s.idle && s.position >= 100.0
    });
    assert!(worker.submit(PlaybackCommand::Toggle));
    wait_worker(&worker, |s| s.paused && !s.idle);
    assert!(worker.submit(PlaybackCommand::Seek {
        session: "demo".into(),
        position: 150.0
    }));
    wait_worker(&worker, |s| {
        s.paused && s.position >= 149.9 && s.position < 150.1
    });
    assert!(worker.submit(PlaybackCommand::Stop));
    assert!(worker.submit(PlaybackCommand::Load {
        session: "other".into(),
        start: 20.0,
        end: Some(20.8)
    }));
    wait_worker(&worker, |s| {
        s.session.as_deref() == Some("other") && !s.idle && s.position >= 20.0
    });
    let finished = wait_worker(&worker, |s| s.session.as_deref() == Some("other") && s.idle);
    assert_eq!(
        finished.position, 20.8,
        "the fragment endpoint remains visible"
    );
    assert!(finished.duration.is_some());
    assert!(worker.submit(PlaybackCommand::Stop));
    wait_worker(&worker, |s| s.session.is_none() && s.idle);
    assert!(worker.submit(PlaybackCommand::Load {
        session: "missing".into(),
        start: 0.0,
        end: None
    }));
    let deadline = Instant::now() + Duration::from_secs(10);
    let unavailable = loop {
        let state = worker
            .rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        if state.session.as_deref() == Some("missing") && state.error.is_some() {
            break state;
        }
    };
    let error = unavailable.error.unwrap();
    assert!(!error.contains("нужен mpv") && !error.contains("synthetic-media-secret"));
    assert!(worker.submit(PlaybackCommand::Load {
        session: "demo".into(),
        start: 1.0,
        end: Some(1.3)
    }));
    wait_worker(&worker, |state| {
        state.session.as_deref() == Some("demo") && !state.idle && state.error.is_none()
    });
    drop(worker);
    drop(server_guard);
    assert!(
        ranges
            .lock()
            .unwrap()
            .iter()
            .any(|offset| *offset > 50_000_000),
        "far seek must use HTTP Range"
    );
    eprintln!(
        "media acceptance: bytes={}, Range offsets={:?}",
        sent.load(Ordering::Relaxed),
        ranges.lock().unwrap()
    );
    assert!(
        sent.load(Ordering::Relaxed) < 16000 * 2 * 3600,
        "seeks and a session switch must not preload one full 115 MB generated WAV"
    );
}
