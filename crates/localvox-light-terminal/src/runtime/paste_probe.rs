//! Real OS boundary, using the production input adapter and synthetic data only.
use super::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::{
    io::{Read, Write},
    sync::mpsc,
};

const URL: &str = "https://example.org/q?i=Русский🦀";

#[test]
#[ignore = "private synthetic ConPTY input fixture"]
fn native_paste_fixture() {
    let Some(output) = std::env::var_os("LOCALVOX_INPUT_PROBE") else {
        return;
    };
    let mut events = vec![];
    let mut app = App::default();
    app.focus = 2;
    let mode = || unsafe {
        use windows::Win32::System::Console::*;
        let mut mode = CONSOLE_MODE::default();
        GetConsoleMode(GetStdHandle(STD_INPUT_HANDLE).unwrap(), &mut mode).unwrap();
        mode
    };
    let original = mode();
    with_terminal(|terminal| {
        let mut input = input::Input::new()?;
        terminal.draw(|f| {
            f.render_widget(
                ratatui::widgets::Paragraph::new("input-probe-ready"),
                f.area(),
            )
        })?;
        let end = Instant::now() + Duration::from_secs(8);
        while Instant::now() < end {
            if let Some(event) = input.read(Duration::from_millis(50))? {
                events.push(format!("{event:?}"));
                match event {
                    Event::Key(key) if key.code == KeyCode::F(10) => break,
                    Event::Key(key) if key.kind != KeyEventKind::Release => app.key(key),
                    Event::Paste(text) => app.paste(&text),
                    _ => {}
                }
            }
        }
        Ok(())
    })
    .unwrap();
    assert!(!terminal::is_raw_mode_enabled().unwrap());
    assert_eq!(mode(), original, "all console mode bits must be restored");
    std::fs::write(
        output,
        serde_json::to_vec(&serde_json::json!({
            "events": events, "quit": app.quit, "palette": app.palette,
            "effects": app.effects.len(),
            "value": app.form.as_ref().map(|f| f.fields[0].value.clone()),
        }))
        .unwrap(),
    )
    .unwrap();
    println!("input-probe-finished");
}

#[test]
fn native_paste_preserves_unicode_and_does_not_dispatch_url_as_keys() {
    let root = tempfile::tempdir().unwrap();
    let output = root.path().join("events.json");
    let pair = native_pty_system()
        .openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })
        .unwrap();
    let mut command = CommandBuilder::new(std::env::current_exe().unwrap());
    command.args([
        "--ignored",
        "--exact",
        "runtime::paste_probe::native_paste_fixture",
        "--nocapture",
    ]);
    command.env("LOCALVOX_INPUT_PROBE", &output);
    struct Child(Box<dyn portable_pty::Child + Send + Sync>);
    impl Drop for Child {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut child = Child(pair.slave.spawn_command(command).unwrap());
    drop(pair.slave);
    let mut reader = pair.master.try_clone_reader().unwrap();
    let mut writer = pair.master.take_writer().unwrap();
    let (tx, rx) = mpsc::sync_channel(32);
    let thread = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let end = Instant::now() + Duration::from_secs(15);
    let mut all = vec![];
    let mut queries = 0;
    let mut sent = false;
    loop {
        if let Ok(bytes) = rx.recv_timeout(Duration::from_millis(50)) {
            all.extend(bytes);
        }
        assert!(
            all.len() < 1024 * 1024 && Instant::now() < end,
            "bounded native input fixture"
        );
        let text = String::from_utf8_lossy(&all);
        let count = text.matches("\x1b[6n").count();
        while queries < count {
            writer.write_all(b"\x1b[1;1R").unwrap();
            writer.flush().unwrap();
            queries += 1;
        }
        if !sent && text.contains("input-probe-ready") {
            // Split begin/end sequences across actual reads with intervening idle
            // polls, and split UTF-8 inside the supplementary scalar at the PTY edge.
            for part in ["\x1b[", "200~", URL, "\x1b[20", "1~", "\x1b[21~"] {
                for byte in part.as_bytes() {
                    writer.write_all(&[*byte]).unwrap();
                }
                writer.flush().unwrap();
                std::thread::sleep(Duration::from_millis(80));
            }
            sent = true;
        }
        if text.contains("input-probe-finished") {
            break;
        }
    }
    assert!(child.0.wait().unwrap().success());
    let result: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(result["quit"], false);
    assert_eq!(result["palette"], false);
    assert_eq!(result["effects"], 0);
    assert_eq!(result["value"], URL);
    let events = result["events"].as_array().unwrap();
    assert_eq!(
        events
            .iter()
            .filter(|e| e.as_str().unwrap().starts_with("Paste("))
            .count(),
        1
    );
    assert!(events.iter().any(|e| e.as_str().unwrap().contains("F(10)")));
    drop(writer);
    drop(pair.master);
    drop(rx);
    thread.join().unwrap();
}
