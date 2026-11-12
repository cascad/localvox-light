//! Exercise the real terminal binary using the platform PTY (ConPTY on Windows).
use localvox_light_terminal::app::{Action, COMMANDS};
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
    activity: Arc<AtomicUsize>,
    list_reads: Arc<AtomicUsize>,
    preview: Arc<AtomicUsize>,
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
        let preview = Arc::new(AtomicUsize::new(0));
        let progress = preview.clone();
        let activity = Arc::new(AtomicUsize::new(0));
        let activity_state = activity.clone();
        let list_reads = Arc::new(AtomicUsize::new(0));
        let reads = list_reads.clone();
        let thread = std::thread::spawn(move || {
            let fixture: serde_json::Value =
                serde_json::from_str(include_str!("../../../tests/fixtures/workspace.json"))
                    .unwrap();
            while !stopping.load(Ordering::Relaxed) {
                if let Some(request) = server.recv_timeout(Duration::from_millis(50)).unwrap() {
                    let path = request.url().split('?').next().unwrap();
                    let mode = activity_state.load(Ordering::Relaxed);
                    if matches!(path, "/api/sessions" | "/api/queue") {
                        reads.fetch_add(1, Ordering::Relaxed);
                        if mode == 5 {
                            let _ = request.respond(
                                tiny_http::Response::from_string("unavailable")
                                    .with_status_code(503),
                            );
                            continue;
                        }
                    }
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
                        let revision = progress.load(Ordering::Relaxed);
                        let info = serde_json::json!({"generation":"pty-g1","revision":revision,"snapshot":format!("snapshot{revision}"),"audio_end_sec":revision*20,"text_end_sec":revision*20,"stt_end_sec":revision*20,"sealed_end_sec":revision*25});
                        let lines:Vec<_>=(0..revision+1).map(|n|serde_json::json!({"id":format!("pty-g1/0/{n}"),"text":format!("Готовый фрагмент {n}"),"start_sec":n*10,"end_sec":(n+1)*10,"source_id":0,"who":"Микрофон"})).collect();
                        match path {
                            "/api/health" => serde_json::json!({
                                "ok":true,"service":"localvox-light","version":"fixture",
                                "api_protocol":1,"capabilities":["technical-journal-v1","terminal-workspace-v1"]
                            }),
                            "/api/sessions" => {
                                if mode == 7 {
                                    serde_json::json!((0..80).map(|n|serde_json::json!({"name":format!("page{n:02}"),"title":format!("Встреча {n:02}"),"cooked":true})).collect::<Vec<_>>())
                                } else {
                                    let mut sessions = fixture["sessions"].clone();
                                    if mode > 0 {
                                        sessions[0]["title"] = "Активная встреча".into();
                                        sessions[0]["job"] = match mode {
                                            1 => "running".into(),
                                            2 => "pending".into(),
                                            4 => "failed".into(),
                                            6 => "waiting_input".into(),
                                            _ => serde_json::Value::Null,
                                        };
                                        sessions[0]["recording"] = matches!(mode, 1 | 6).into();
                                        sessions.as_array_mut().unwrap().push(serde_json::json!({"name":"other","title":"Вторая встреча","cooked":true}));
                                    }
                                    sessions
                                }
                            }
                            "/api/queue" => {
                                if mode > 0 {
                                    let state = match mode {
                                        1 => "running",
                                        2 => "pending",
                                        4 => "failed",
                                        6 => "waiting_input",
                                        _ => "done",
                                    };
                                    serde_json::json!({"queue":[{"session":"20261031_demo","state":state,"phase":"transcribe","position":2}]})
                                } else {
                                    serde_json::json!({"queue":fixture["queue"]})
                                }
                            }
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
                            p if p.ends_with("/transcript") && revision > 0 => {
                                serde_json::json!({"lines":lines,"partial":info})
                            }
                            p if p.ends_with("/processed") && mode == 7 => {
                                serde_json::json!({"from_transcript":true,"lines":[{"text":(0..200).map(|n|format!("Строка {n:03}")).collect::<Vec<_>>().join("\n")}]})
                            }
                            p if p.ends_with("/processed") && revision > 0 => {
                                serde_json::json!({"lines":lines,"partial":info,"from_transcript":true})
                            }
                            p if p.ends_with("/playback") => {
                                serde_json::json!({"duration_sec":120,"snapshot":null,"partial":null})
                            }
                            _ => serde_json::json!({}),
                        }
                    };
                    let _ = request.respond(tiny_http::Response::from_string(body.to_string()));
                }
            }
        });
        Self {
            activity,
            list_reads,
            preview,
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
    wait_until(
        needle,
        |all, screen| {
            if needle.starts_with('\x1b') {
                String::from_utf8_lossy(all).contains(needle)
            } else {
                screen.contents().contains(needle)
            }
        },
        all,
        screen,
        rx,
        writer,
        queries,
        deadline,
    );
}
fn wait_until(
    needle: &str,
    ready: impl Fn(&[u8], &vt100::Screen) -> bool,
    all: &mut Vec<u8>,
    screen: &mut vt100::Parser,
    rx: &mpsc::Receiver<Vec<u8>>,
    writer: &mut dyn Write,
    queries: &mut usize,
    deadline: Instant,
) {
    while !ready(all, screen.screen()) {
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
    exercise_terminal(false);
}

#[test]
fn vox_uses_portable_settings_from_another_directory_and_reuses_the_running_api() {
    exercise_terminal(true);
}

fn exercise_terminal(vox: bool) {
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
    let portable = tempfile::tempdir().unwrap();
    let install = portable.path().join("LocalVox журнал");
    let caller = portable.path().join("caller");
    let mut command = if vox {
        std::fs::create_dir(&install).unwrap();
        std::fs::create_dir(&caller).unwrap();
        let executable = install.join(if cfg!(windows) { "vox.exe" } else { "vox" });
        std::fs::copy(env!("CARGO_BIN_EXE_vox"), &executable).unwrap();
        std::fs::write(
            install.join(".env.defaults"),
            include_str!("../../../.env.defaults"),
        )
        .unwrap();
        std::fs::write(
            install.join(".env"),
            format!(
                "LOCALVOX_API_BIND={}\nLOCALVOX_API_TOKEN=\nLOCALVOX_LIGHT_TUI=off\n",
                api.address.strip_prefix("http://").unwrap()
            ),
        )
        .unwrap();
        std::fs::write(caller.join(".env"), "LOCALVOX_TUI_URL=http://127.0.0.1:1\n").unwrap();
        let mut command = CommandBuilder::new(executable);
        command.cwd(&caller);
        command.args(["--ascii", "--no-color"]);
        command
    } else {
        let mut command = CommandBuilder::new(env!("CARGO_BIN_EXE_localvox-tui"));
        command.args(["--address", &api.address, "--ascii", "--no-color"]);
        command
    };
    // A broken portable-config implementation must never fall back to a real
    // developer daemon on 3017. Only the temporary .env may select the fixture.
    if vox {
        command.env_remove("LOCALVOX_TUI_URL");
    } else {
        command.env("LOCALVOX_TUI_URL", "http://127.0.0.1:1");
    }
    command.env("LOCALVOX_API_BIND", "127.0.0.1:1");
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
    writer.write_all(b"\x1b[1;5C").unwrap(); // Control+Right: wider list, independent of layout.
    writer.flush().unwrap();
    wait_until(
        "wider list boundary",
        |_, s| s.cell(6, 31).is_some_and(|c| c.contents() == "+"),
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"\x1b[1;5D").unwrap();
    writer.flush().unwrap();
    wait_until(
        "restored list boundary",
        |_, s| s.cell(6, 27).is_some_and(|c| c.contents() == "+"),
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    assert_eq!(api.stops.load(Ordering::Relaxed), 0);
    writer.write_all(b"\t\r5").unwrap();
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
    writer.write_all(b"`").unwrap();
    writer.flush().unwrap();
    wait_for(
        "Консоль ошибок",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"`").unwrap();
    writer.flush().unwrap();
    wait_for(
        "F8 stop",
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
        "Импорт",
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
    // Both reserved recording shortcuts work inside an existing Unicode editor.
    writer.write_all(b"\x1b[20~").unwrap(); // F9 replaces the import form
    writer.flush().unwrap();
    wait_for(
        "Название",
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
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
    let config = tempfile::tempdir().unwrap();
    let keymap = config.path().join("keys.json");
    std::fs::write(
        &keymap,
        r#"{"version":1,"keys":{"λ":"j","ω":"q"}}"#.as_bytes(),
    )
    .unwrap();
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
    command.arg("--keymap");
    command.arg(&keymap);
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
    see!("Нет подключения");
    assert!(!screen.screen().contents().contains("HTTP 401"));
    writer.write_all(b"\x1b[24~").unwrap(); // F12 works even in the archive filter
    writer.flush().unwrap();
    see!("HTTP 401");
    assert!(!screen
        .screen()
        .contents()
        .lines()
        .next()
        .unwrap_or("")
        .contains("! 1"));
    writer.write_all(b"\x1b[24~").unwrap(); // F12 works even in the archive filter
    writer.flush().unwrap();
    see!("F8 stop");
    writer.write_all(b"\x1b[1;5C").unwrap();
    writer.flush().unwrap();
    see!("Ширина списка: 31");
    // This control action is rejected locally while disconnected, never queued
    // for automatic replay after the token is entered.
    writer.write_all(b"\x1b[19~:").unwrap();
    writer.flush().unwrap();
    see!("Действия");
    let connection = COMMANDS
        .iter()
        .position(|(action, _)| *action == Action::Connection)
        .unwrap();
    writer.write_all("λ".repeat(connection).as_bytes()).unwrap();
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
    assert_eq!(
        screen.screen().cell(6, 31).unwrap().contents(),
        "+",
        "list width must survive reconnect"
    );
    assert_eq!(api.stops.load(Ordering::Relaxed), 0);
    writer.write_all("\tωω".as_bytes()).unwrap(); // custom mapping must survive reconnect
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

#[test]
fn invalid_keymap_fails_before_entering_the_terminal_or_connecting() {
    let config = tempfile::tempdir().unwrap();
    for name in ["missing.json", "invalid.json"] {
        let path = config.path().join(name);
        if name == "invalid.json" {
            std::fs::write(&path, b"not JSON").unwrap();
        }
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_localvox-tui"))
            .args(["--address", "http://127.0.0.1:1", "--keymap"])
            .arg(path)
            .output()
            .unwrap();
        assert!(!result.status.success());
        let error = String::from_utf8_lossy(&result.stderr);
        assert!(error.contains("клавиш"), "{error}");
        assert!(!result.stdout.windows(8).any(|s| s == b"\x1b[?1049h"));
    }
}

#[test]
fn real_terminal_reads_a_growing_partial_and_keeps_the_old_line_on_its_row() {
    let api = ApiFixture::new();
    api.preview.store(1, Ordering::Relaxed);
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
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut all = Vec::new();
    let mut queries = 0;
    let mut screen = vt100::Parser::new(36, 120, 0);
    let deadline = Instant::now() + Duration::from_secs(20);
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
    see!("Архитектура и следующие шаги");
    writer.write_all(b"\t\r2").unwrap();
    writer.flush().unwrap();
    see!("Готовый фрагмент 1");
    see!("Готовая часть");
    let before = screen
        .screen()
        .contents()
        .lines()
        .position(|l| l.contains("Готовый фрагмент 1"))
        .unwrap();
    writer.write_all(b"\x1b[B").unwrap();
    writer.flush().unwrap();
    api.preview.store(2, Ordering::Relaxed);
    see!("Готовый фрагмент 2");
    let after = screen
        .screen()
        .contents()
        .lines()
        .position(|l| l.contains("Готовый фрагмент 1"))
        .unwrap();
    assert_eq!(before, after);
    assert!(screen.screen().contents().contains("00:40"));
    writer.write_all(b"q").unwrap();
    writer.flush().unwrap();
    see!("q ещё раз — выйти");
    assert!(child.0.try_wait().unwrap().is_none());
    wait_until(
        "quit confirmation expires without another key",
        |_, screen| !screen.contents().contains("q ещё раз — выйти"),
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    assert!(child.0.try_wait().unwrap().is_none());
    writer.write_all(b"qq").unwrap();
    writer.flush().unwrap();
    see!("\x1b[?1049l");
    assert!(child.0.wait().unwrap().success());
    drop(writer);
    drop(pair.master);
    drop(rx);
    output.join().unwrap();
    assert!(api.recording.load(Ordering::Relaxed));
    assert_eq!(api.stops.load(Ordering::Relaxed), 0);
}

#[test]
fn real_terminal_animates_server_activity_filters_names_and_accepts_russian_commands() {
    let api = ApiFixture::new();
    api.activity.store(1, Ordering::Relaxed);
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
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut all = Vec::new();
    let mut queries = 0;
    let mut screen = vt100::Parser::new(36, 120, 0);
    let deadline = Instant::now() + Duration::from_secs(45);
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
    see!("STT · запись");
    let observation_started = Instant::now();
    let requests = api.list_reads.load(Ordering::Relaxed);
    for symbol in ["|", "/", "-", "\\"].into_iter().cycle().take(12) {
        see!(&format!("{symbol} Активная встреча"));
    }
    // The observation can cross normal 3-second polls on a busy host. Each poll
    // reads sessions + queue; allow one pair already in flight at the boundary.
    let elapsed = observation_started.elapsed();
    let allowed_reads = 2 * (elapsed.as_millis() / 3000 + 2) as usize;
    let reads = api.list_reads.load(Ordering::Relaxed) - requests;
    assert!(
        reads <= allowed_reads,
        "animation must not poll the server: {reads} reads in {elapsed:?}, budget {allowed_reads}"
    );
    api.activity.store(2, Ordering::Relaxed);
    see!("в очереди #2");
    see!("> Активная встреча");
    api.activity.store(3, Ordering::Relaxed);
    see!("+ Активная встреча");
    api.activity.store(4, Ordering::Relaxed);
    see!("! Активная встреча");
    api.activity.store(5, Ordering::Relaxed);
    see!("нет обновления");
    see!("? Активная встреча");
    api.activity.store(6, Ordering::Relaxed);
    see!("ждём кусок");
    // First slash enters local search; the next slash and j/k are literal input.
    writer.write_all(b"//jk").unwrap();
    writer.flush().unwrap();
    see!("Список · 0/2");
    see!("/ /jk_");
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    wait_until(
        "search closed before returning to navigation",
        |_, screen| {
            screen.contents().contains("Список · / поиск") && !screen.contents().contains("/ /jk_")
        },
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"jk").unwrap();
    writer
        .write_all("/\x1b[200~вТоРаЯ\x1b[201~".as_bytes())
        .unwrap();
    writer.flush().unwrap();
    see!("Список · 1/2");
    wait_until(
        "filtered archive frame",
        |_, screen| {
            let list = (0..36)
                .map(|row| {
                    (0..27)
                        .filter_map(|col| screen.cell(row, col))
                        .map(|c| c.contents())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n");
            list.contains("Вторая встреча") && !list.contains("Активная встреча")
        },
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all("\rп".as_bytes()).unwrap();
    writer.flush().unwrap();
    see!("Перейти ко времени");
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    // Observe the closed editor before another printable key; an ESC prefix
    // immediately followed by a character can legitimately mean Alt+character.
    while screen.screen().contents().contains("Перейти ко времени") {
        let bytes = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        screen.process(&bytes);
        all.extend(bytes);
        assert!(all.len() < 1024 * 1024);
    }
    writer.write_all(b".").unwrap();
    writer.flush().unwrap();
    see!("lexical / semantic / hybrid");
    writer.write_all("п.й".as_bytes()).unwrap();
    writer.flush().unwrap();
    see!("п.й");
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    wait_until(
        "closed search editor",
        |_, screen| !screen.contents().contains("lexical / semantic / hybrid"),
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    see!("Esc меню");
    assert!(screen.screen().contents().contains("Список · 1/2"));
    writer.write_all(b"\x1b").unwrap();
    writer.flush().unwrap();
    see!("Enter/l раздел");
    assert!(screen.screen().contents().contains("Список · 1/2"));
    writer.write_all(b"\t\r").unwrap();
    writer.flush().unwrap();
    see!("Esc список");
    api.activity.store(7, Ordering::Relaxed);
    // Leave details, explicitly edit and clear the saved filter. Idle Esc now
    // returns to the sections menu without discarding the filter.
    writer.write_all(b"\x1b[Z/\x1b").unwrap();
    writer.flush().unwrap();
    see!("Встреча 00");
    writer.write_all(b"\x1b[1~").unwrap();
    writer.flush().unwrap();
    see!("> + Встреча 00");
    writer.write_all(b"\x1b[6~").unwrap();
    writer.flush().unwrap();
    see!("> + Встреча 09");
    writer.write_all(b"\x1b[5~").unwrap();
    writer.flush().unwrap();
    see!("> + Встреча 00");
    writer.write_all(b"\r3").unwrap();
    writer.flush().unwrap();
    see!("Строка 000");
    writer.write_all(b"\x1b[6~").unwrap();
    writer.flush().unwrap();
    wait_until(
        "one document page",
        |_, screen| {
            (28..40)
                .filter_map(|x| screen.cell(9, x))
                .map(|c| c.contents())
                .collect::<String>()
                .starts_with("Строка 023")
        },
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
    wait_until(
        "compact footer",
        |_, screen| {
            (1..12)
                .filter_map(|x| screen.cell(23, x))
                .map(|c| c.contents())
                .collect::<String>()
                .starts_with("F8 stop")
        },
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"\x1b[6~").unwrap();
    writer.flush().unwrap();
    wait_until(
        "resized document page",
        |_, screen| {
            (1..13)
                .filter_map(|x| screen.cell(9, x))
                .map(|c| c.contents())
                .collect::<String>()
                .starts_with("Строка 036")
        },
        &mut all,
        &mut screen,
        &rx,
        &mut writer,
        &mut queries,
        deadline,
    );
    writer.write_all(b"\x11").unwrap();
    writer.flush().unwrap();
    see!("\x1b[?1049l");
    assert!(child.0.wait().unwrap().success());
    drop(writer);
    drop(pair.master);
    drop(rx);
    output.join().unwrap();
    assert_eq!(api.stops.load(Ordering::Relaxed), 0);
}
