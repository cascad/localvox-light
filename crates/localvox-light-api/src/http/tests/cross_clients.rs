//! Real HTTP/archive and both real clients. Speech/model output is supplied by
//! an explicit synthetic artifact producer; no capture engine or model runs.
use super::*;
use localvox_light_client::Client;
use localvox_light_core::{asks, jobs, journal, processing, progress};
use localvox_light_process::{Limits, ManagedProcess};
use localvox_light_terminal::app::{Action, COMMANDS};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    fs,
    io::{Read, Write},
    path::PathBuf,
    process::Command,
    sync::mpsc,
    time::{Duration, Instant},
};

#[test]
#[ignore = "requires built UI, localvox-tui and installed Playwright Chromium; private HTTP only"]
fn shared_workspace() {
    let root = tempfile::tempdir().unwrap();
    let test = std::env::current_exe().unwrap();
    let copied = root.path().join("api-fixture.exe");
    fs::copy(&test, &copied).unwrap();
    let tui = test
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("localvox-tui.exe");
    assert!(tui.is_file(), "build the terminal bundle first");
    let output = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/test-results/cross-real-api");
    fs::create_dir_all(&output).unwrap();
    for name in [
        "browser-error.txt",
        "browser-failed.png",
        "helper-error.txt",
        "helper-stdout.txt",
        "helper-stderr.txt",
        "helper-status.txt",
        "browser-dom.txt",
        "browser.png",
        "terminal.txt",
        "terminal-notes.txt",
        "terminal.ansi.txt",
        "trace.zip",
        "terminal-timeline.txt",
        "browser-timeline.txt",
    ] {
        match fs::remove_file(output.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => panic!("clearing previous cross-client artifact: {error}"),
        }
    }
    let mut command = Command::new(copied);
    command
        .args([
            "--ignored",
            "--exact",
            "http::tests::cross_clients::shared_workspace_fixture",
            "--nocapture",
        ])
        .current_dir(root.path())
        .env("LOCALVOX_CROSS_ROOT", root.path())
        .env("LOCALVOX_CROSS_TUI", tui)
        .env("LOCALVOX_CROSS_OUTPUT", &output);
    let mut limits = Limits::new(Duration::from_secs(60));
    limits.background = false;
    let result = localvox_light_process::run(command, None, limits, || false, |_, _| {}).unwrap();
    fs::write(output.join("helper-stdout.txt"), result.stdout).unwrap();
    fs::write(output.join("helper-stderr.txt"), &result.stderr).unwrap();
    fs::write(
        output.join("helper-status.txt"),
        format!("{:?}", result.termination),
    )
    .unwrap();
    assert!(
        result.termination.success(),
        "cross-client helper {:?}; see ui/test-results/cross-real-api: {}",
        result.termination,
        String::from_utf8_lossy(&result.stderr)
    );
    let report = fs::read_to_string(root.path().join("passed"))
        .expect("cross-client helper failed; see ui/test-results/cross-real-api");
    assert_eq!(report, "UI+TUI shared HTTP passed");
}

fn produce(root: &Path, name: &str) {
    assert!(!name.contains(['/', '\\']) && !name.starts_with('.'));
    let archive = root.join("archive");
    let dir = archive.join("sessions").join(name);
    let old: Value = serde_json::from_slice(&fs::read(dir.join("meta.json")).unwrap()).unwrap();
    super::workspace::session(&archive, name);
    fs::create_dir_all(dir.join("audio")).unwrap();
    fs::write(dir.join("meta.json"),json!({"title":"Общий архив UI TUI","started_at":"2026-10-31T07:00:00Z","sample_rate":16000,"source":old["source"],"chunks":[{"file":"silence","source_id":0,"start_offset_sec":0,"duration_sec":15}]}).to_string()).unwrap();
    let mut header = [0; 44];
    Archive::new(archive.clone())
        .audio_wav(name, Some((0, Some(43))))
        .unwrap()
        .read_exact(&mut header)
        .unwrap();
    let mut audio = fs::File::create(dir.join("audio/silence.wav")).unwrap();
    audio.write_all(&header).unwrap();
    audio.set_len(44 + 15 * 16000 * 2).unwrap();
    drop(audio);
    for artifact in [
        processing::TRANSCRIPT,
        processing::PROCESSED,
        processing::SUMMARY,
    ] {
        processing::record(
            &dir,
            artifact,
            "cross-client synthetic producer",
            processing::Outcome::Ok,
            None,
            Some(2),
        )
        .unwrap();
    }
    progress::new_run(&dir);
    progress::mark(
        &dir,
        progress::Stage::Summary,
        progress::StageState::Running,
        Some("Synthetic producer; no model inference"),
    );
    {
        let operation = journal::Operation::start(&dir, "summary", "synthetic-producer", None);
        for index in 1..=35 {
            operation.log(&format!("Synthetic detail {index}"));
        }
    }
    for stage in progress::Stage::CHAIN {
        progress::mark(&dir, stage, progress::StageState::Done, None);
    }
    jobs::JobQueue::mutate(&archive, |queue| {
        let ids: Vec<_> = queue
            .jobs()
            .iter()
            .filter(|job| job.session == name)
            .map(|job| job.id)
            .collect();
        for id in ids {
            queue.mark_done(id);
        }
    });
    fs::write(root.join("artifact-ready"), name).unwrap();
}

#[test]
#[ignore = "owned copy only; invoked by shared_workspace"]
fn shared_workspace_fixture() {
    let Some(root) = std::env::var_os("LOCALVOX_CROSS_ROOT").map(PathBuf::from) else {
        return;
    };
    assert_eq!(std::env::current_dir().unwrap(), root);
    assert_eq!(std::env::current_exe().unwrap().parent().unwrap(), root);
    let output = PathBuf::from(std::env::var_os("LOCALVOX_CROSS_OUTPUT").unwrap());
    let panic_output = output.clone();
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = fs::write(panic_output.join("helper-error.txt"), info.to_string());
        original_hook(info);
    }));
    fs::write(
        root.join(".env"),
        "# private cross-client fixture\nLOCALVOX_LLM_MODEL=synthetic-before\n",
    )
    .unwrap();
    fs::write(
        root.join("slots.toml"),
        format!(
            "[slots.\"идеи\"]\npath=\"{}\"\ntemplate=\"- {{{{text}}}}\"\ndefault=true\n",
            root.join("notes.md").to_string_lossy().replace('\\', "/")
        ),
    )
    .unwrap();
    std::env::set_var("LOCALVOX_SLOTS_CONFIG", root.join("slots.toml"));
    std::env::set_var("LOCALVOX_API_TOKEN", "synthetic-cross-token");
    let server = journal_http_fixture(&root.join("archive"), Some("synthetic-cross-token"));
    let client = Client::new(&server.address, "synthetic-cross-token".into()).unwrap();
    assert!(client.health().unwrap().compatible());
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 36,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut terminal_command = CommandBuilder::new(std::env::var_os("LOCALVOX_CROSS_TUI").unwrap());
    terminal_command.args(["--address", &server.address, "--ascii", "--no-color"]);
    terminal_command.cwd(&root);
    struct TerminalChild(Box<dyn portable_pty::Child + Send + Sync>);
    impl Drop for TerminalChild {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut terminal = TerminalChild(pair.slave.spawn_command(terminal_command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::sync_channel(64);
    let reader_thread = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let mut browser_command = Command::new("node");
    browser_command
        .arg(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../ui/tests/integration/actual-api.mjs"),
        )
        .env("LOCALVOX_CROSS_URL", &server.address);
    let mut browser = ManagedProcess::spawn(browser_command).unwrap();
    let trace_step = |stage: &str| {
        let mut trace = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(output.join("terminal-timeline.txt"))
            .unwrap();
        writeln!(trace, "{} {stage}", chrono::Utc::now().timestamp_millis()).unwrap();
    };
    trace_step("browser spawned");
    let mut bytes = Vec::new();
    let mut queries = 0;
    let mut screen = vt100::Parser::new(36, 120, 0);
    let mut produced = false;
    let mut reading = false;
    let mut reconnecting = false;
    let mut reconnect_submitted = false;
    let mut opening = false;
    let mut quitting = false;
    let deadline = Instant::now() + Duration::from_secs(45);
    while !quitting || !String::from_utf8_lossy(&bytes).contains("\x1b[?1049l") {
        if Instant::now() >= deadline {
            fs::write(output.join("terminal.txt"), screen.screen().contents()).unwrap();
            fs::write(output.join("terminal.ansi.txt"), &bytes).unwrap();
            panic!("shared clients did not finish: produced={produced}, reading={reading}, reconnecting={reconnecting}, submitted={reconnect_submitted}, opening={opening}, quitting={quitting}");
        }
        if let Ok(next) = rx.recv_timeout(Duration::from_millis(20)) {
            screen.process(&next);
            bytes.extend(next);
            assert!(bytes.len() < 4 * 1024 * 1024);
        }
        let text = String::from_utf8_lossy(&bytes);
        let count = text.matches("\x1b[6n").count();
        while queries < count {
            writer.write_all(b"\x1b[1;1R").unwrap();
            writer.flush().unwrap();
            queries += 1;
        }
        if !produced && root.join("finish-import").is_file() {
            produce(
                &root,
                fs::read_to_string(root.join("finish-import"))
                    .unwrap()
                    .trim(),
            );
            produced = true;
            trace_step("artifacts produced");
        }
        let asks_root = root.join("archive/asks");
        for row in asks::list(&asks_root) {
            if row.status == asks::AskStatus::Pending {
                let mut ask = asks::load(&asks_root, &row.id).unwrap();
                ask.status = asks::AskStatus::Done;
                ask.answer = Some("Общий ответ UI TUI".into());
                asks::update(&asks_root, &ask).unwrap();
                fs::write(root.join("ask-ready"), &ask.id).unwrap();
            }
        }
        if !reading
            && screen
                .screen()
                .contents()
                .contains("Архитектура клиента согласована")
        {
            reading = true;
            trace_step("terminal read summary");
            pair.master
                .resize(PtySize {
                    rows: 24,
                    cols: 80,
                    pixel_width: 0,
                    pixel_height: 0,
                })
                .unwrap();
            screen.set_size(24, 80);
            // Keep both clients open on the same archive. Keyboard navigation
            // opens the actual notes delivered by the browser, without a mock.
            writer.write_all(b"\t\tj\r\t").unwrap();
            writer.flush().unwrap();
        }
        if reading && !reconnecting && screen.screen().contents().contains("Общая заметка")
        {
            fs::write(root.join("terminal-saw-note"), "actual HTTP note seen").unwrap();
            fs::write(
                output.join("terminal-notes.txt"),
                screen.screen().contents(),
            )
            .unwrap();
            writer.write_all(b":").unwrap();
            let connection = COMMANDS
                .iter()
                .position(|(action, _)| *action == Action::Connection)
                .unwrap();
            let backwards = COMMANDS.len() - connection;
            let (key, steps) = if connection <= backwards {
                (b'j', connection)
            } else {
                (b'k', backwards)
            };
            writer.write_all(&vec![key; steps]).unwrap();
            writer.write_all(b"\r").unwrap();
            writer.flush().unwrap();
            reconnecting = true;
            trace_step("terminal note seen; requested connection form");
        }
        if reconnecting
            && !reconnect_submitted
            && screen.screen().contents().contains("Подключение к демону")
            && screen.screen().contents().contains("HTTP(S)")
        {
            writer.write_all(b"\x13").unwrap();
            writer.flush().unwrap();
            reconnect_submitted = true;
            trace_step("connection form seen; submitted");
        }
        if reconnect_submitted
            && !opening
            && !screen.screen().contents().contains("Подключение к демону")
            && screen.screen().contents().contains("Список")
            && screen.screen().contents().contains("Общий архив UI TUI")
        {
            writer.write_all(b"\t").unwrap();
            writer.flush().unwrap();
            opening = true;
            trace_step("archive list seen; sent Tab");
        }
        if opening
            && !root.join("terminal-reconnected").exists()
            && screen
                .screen()
                .contents()
                .contains("Архитектура клиента согласована")
        {
            fs::write(
                root.join("terminal-reconnected"),
                "same private API verified again",
            )
            .unwrap();
            trace_step("reconnected summary seen; marker saved");
        }
        if browser.has_exited().unwrap() && !quitting {
            trace_step("browser exited");
            fs::write(output.join("terminal.txt"), screen.screen().contents()).unwrap();
            fs::write(output.join("terminal.ansi.txt"), &bytes).unwrap();
            assert!(
                root.join("browser-passed").is_file(),
                "browser integration failed"
            );
            assert!(
                reading && root.join("terminal-reconnected").is_file(),
                "terminal did not read and reconnect to the shared archive"
            );
            fs::write(output.join("terminal.txt"), screen.screen().contents()).unwrap();
            writer.write_all(b"\x11").unwrap();
            writer.flush().unwrap();
            quitting = true;
        }
    }
    assert!(terminal.0.wait().unwrap().success());
    drop(writer);
    drop(pair.master);
    drop(rx);
    reader_thread.join().unwrap();
    fs::write(output.join("terminal.ansi.txt"), &bytes).unwrap();
    assert!(!String::from_utf8_lossy(&bytes).contains("synthetic-cross-token"));
    assert!(
        !client.sessions().unwrap().is_empty(),
        "API/archive must outlive client quit"
    );
    fs::write(root.join("passed"), "UI+TUI shared HTTP passed").unwrap();
}
