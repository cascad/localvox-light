//! Exercise the real terminal binary using the platform PTY (ConPTY on Windows).
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};
use std::{
    io::{Read, Write},
    sync::mpsc,
    time::{Duration, Instant},
};

struct ApiFixture {
    address: String,
    stop: Arc<AtomicBool>,
    stops: Arc<AtomicUsize>,
    recording: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl ApiFixture {
    fn new() -> Self {
        Self::with_token(None)
    }
    fn with_token(token: Option<&str>) -> Self {
        let token = token.map(str::to_owned);
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let address = format!("http://{}", server.server_addr());
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let stops = Arc::new(AtomicUsize::new(0));
        let stopped = stops.clone();
        let recording = Arc::new(AtomicBool::new(true));
        let active = recording.clone();
        let thread = std::thread::spawn(move || {
            let fixture: serde_json::Value =
                serde_json::from_str(include_str!("../../../tests/fixtures/workspace.json"))
                    .unwrap();
            while !stopping.load(Ordering::Relaxed) {
                if let Some(request) = server.recv_timeout(Duration::from_millis(50)).unwrap() {
                    let path = request.url().split('?').next().unwrap();
                    if token.as_ref().is_some_and(|token| {
                        !request.headers().iter().any(|h| {
                            h.field.equiv("Authorization")
                                && h.value.as_str() == format!("Bearer {token}")
                        })
                    }) {
                        assert_eq!(
                            path, "/api/health",
                            "no workspace requests before authorized compatibility check"
                        );
                        let _ = request.respond(
                            tiny_http::Response::from_string("{\"error\":\"unauthorized\"}")
                                .with_status_code(401),
                        );
                        continue;
                    }
                    let body = if request.method().as_str() == "POST" {
                        assert_eq!(
                            path, "/api/record/stop",
                            "client must not shut down the daemon or replay an action"
                        );
                        stopped.fetch_add(1, Ordering::Relaxed);
                        active.store(false, Ordering::Relaxed);
                        serde_json::json!({"msg":"Остановлено"})
                    } else {
                        match path {
                            "/api/health" => serde_json::json!({
                                "ok":true,"service":"localvox-light","version":"fixture",
                                "api_protocol":1,"capabilities":["technical-journal-v1","terminal-workspace-v1"]
                            }),
                            "/api/sessions" => fixture["sessions"].clone(),
                            "/api/record" => {
                                let mut record = fixture["record"].clone();
                                record["recording"] = active.load(Ordering::Relaxed).into();
                                record["session"] = "20261031_demo".into();
                                record
                            }
                            "/api/jobs" => fixture["jobs"].clone(),
                            p if p.ends_with("/journal") => serde_json::from_str(include_str!(
                                "../../../tests/fixtures/journal.json"
                            ))
                            .unwrap(),
                            p if p.ends_with("/progress") => fixture["progress"].clone(),
                            p if p.ends_with("/summary") => fixture["summary"].clone(),
                            p if p.ends_with("/versions") => {
                                serde_json::json!({"versions":fixture["versions"]})
                            }
                            p if p.ends_with("/sources") => {
                                serde_json::json!({"sources":fixture["sources"]})
                            }
                            _ => serde_json::json!({}),
                        }
                    };
                    let _ = request.respond(tiny_http::Response::from_string(body.to_string()));
                }
            }
        });
        Self {
            address,
            stop,
            stops,
            recording,
            thread: Some(thread),
        }
    }
}
impl Drop for ApiFixture {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            thread.join().unwrap();
        }
    }
}

struct OwnedChild(Box<dyn portable_pty::Child + Send + Sync>);
impl Drop for OwnedChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn wait_for(
    needle: &str,
    all: &mut Vec<u8>,
    screen: &mut vt100::Parser,
    rx: &mpsc::Receiver<Vec<u8>>,
    writer: &mut dyn Write,
    queries: &mut usize,
    deadline: Instant,
) {
    while !(if needle.starts_with('\x1b') {
        String::from_utf8_lossy(all).contains(needle)
    } else {
        screen.screen().contents().contains(needle)
    }) {
        let remaining = deadline.saturating_duration_since(Instant::now());
        assert!(!remaining.is_zero(), "terminal did not emit {needle}");
        match rx.recv_timeout(remaining) {
            Ok(bytes) => {
                screen.process(&bytes);
                all.extend(bytes);
            }
            Err(e) => panic!(
                "PTY waiting for {needle}: {e}; screen: {}",
                screen.screen().contents()
            ),
        };
        assert!(all.len() < 1024 * 1024, "terminal output is bounded");
        // ConPTY's inherit-cursor handshake asks its host terminal for a DSR reply.
        // A PTY is not itself a terminal emulator: the test host must answer it.
        let count = String::from_utf8_lossy(all).matches("\x1b[6n").count();
        while *queries < count {
            writer.write_all(b"\x1b[1;1R").unwrap();
            writer.flush().unwrap();
            *queries += 1;
        }
    }
}

#[test]
fn real_terminal_accepts_paste_resize_and_restores_screen_on_quit() {
    let api = ApiFixture::new();
    let pty = native_pty_system();
    let pair = pty
        .openpty(PtySize {
            rows: 36,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_localvox-tui"));
    command.args(["--address", &api.address, "--ascii", "--no-color"]);
    command.env("LOCALVOX_API_TOKEN", "");
    let mut child = OwnedChild(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::sync_channel(32);
    let output = std::thread::spawn(move || {
        let mut bytes = [0u8; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 {
                break;
            }
            if tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut all = Vec::new();
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut queries = 0;
    let mut screen = vt100::Parser::new(36, 120, 0);
    wait_for(
        "Архитектура и следующие шаги",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"\t5").unwrap();
    writer.flush().unwrap();
    wait_for(
        "+ 35 lines",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"\r").unwrap();
    writer.flush().unwrap();
    wait_for(
        "Диагностическая строка 1",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    pair.master
        .resize(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    screen.set_size(24, 80);
    writer.write_all(b"i").unwrap();
    writer.flush().unwrap();
    wait_for(
        "URL",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer
        .write_all("\x1b[200~https://example.org/qri/встреча\x1b[201~".as_bytes())
        .unwrap();
    writer.flush().unwrap();
    wait_for(
        "встреча",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    // The reserved stop shortcut still works inside a Unicode editor.
    writer.write_all(b"\x1b[19~").unwrap();
    writer.flush().unwrap();
    while api.stops.load(Ordering::Relaxed) == 0 {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    wait_for(
        "готов к записи",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    // A new server-side recording must survive closing this client.
    api.recording.store(true, Ordering::Relaxed);
    wait_for(
        "ИДЁТ ЗАПИСЬ",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"\x1b[21~").unwrap(); // F10, including an open form
    writer.flush().unwrap();
    wait_for(
        "\x1b[?1049l",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    let status = child.0.wait().unwrap();
    drop(writer);
    drop(pair.master);
    drop(rx);
    output.join().unwrap();
    assert!(
        status.success(),
        "client must exit without killing a daemon"
    );
    let text = String::from_utf8_lossy(&all);
    assert!(text.contains("\x1b[?1049l"), "alternate screen restored");
    assert!(text.contains("\x1b[?2004l"), "bracketed paste restored");
    assert!(api.recording.load(Ordering::Relaxed));
    assert_eq!(api.stops.load(Ordering::Relaxed), 1);
}

#[test]
fn unauthorized_start_can_reconnect_through_the_masked_form_without_replaying_stop() {
    const TOKEN: &str = "synthetic-reconnect-secret";
    let api = ApiFixture::with_token(Some(TOKEN));
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 36,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_localvox-tui"));
    command.args(["--address", &api.address, "--ascii", "--no-color"]);
    command.env("LOCALVOX_API_TOKEN", "");
    let mut child = OwnedChild(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::sync_channel(32);
    let output = std::thread::spawn(move || {
        let mut bytes = [0u8; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut all = Vec::new();
    let mut queries = 0;
    let mut screen = vt100::Parser::new(36, 120, 0);
    let deadline = Instant::now() + Duration::from_secs(15);
    macro_rules! see {
        ($needle:expr) => {
            wait_for(
                $needle,
                &mut all,
                &mut screen,
                &rx,
                &mut writer,
                &mut queries,
                deadline,
            )
        };
    }
    see!("HTTP 401");
    // This control action is rejected locally while disconnected, never queued
    // for automatic replay after the token is entered.
    writer.write_all(b"\x1b[19~:").unwrap();
    writer.flush().unwrap();
    see!("Действия");
    writer.write_all(&[b'j'; 31]).unwrap();
    writer.write_all(b"\r").unwrap();
    writer.flush().unwrap();
    see!("Подключение к демону");
    writer.write_all(b"\t\x1b[200~").unwrap();
    writer.write_all(TOKEN.as_bytes()).unwrap();
    writer.write_all(b"\x1b[201~").unwrap();
    writer.flush().unwrap();
    see!("••••••••");
    assert!(!String::from_utf8_lossy(&all).contains(TOKEN));
    writer.write_all(b"\x13").unwrap();
    writer.flush().unwrap();
    see!("Архитектура и следующие шаги");
    assert_eq!(api.stops.load(Ordering::Relaxed), 0);
    writer.write_all(b"q").unwrap(); // plain exit when the host reserves Ctrl+Q
    writer.flush().unwrap();
    see!("\x1b[?1049l");
    assert!(child.0.wait().unwrap().success());
    drop(writer);
    drop(pair.master);
    drop(rx);
    output.join().unwrap();
    assert!(
        !String::from_utf8_lossy(&all).contains(TOKEN),
        "token must not appear in terminal capture"
    );
    assert_eq!(api.stops.load(Ordering::Relaxed), 0);
}
