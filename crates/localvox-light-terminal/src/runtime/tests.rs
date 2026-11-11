use super::*;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use std::io::{Read, Write};
use std::sync::mpsc;

#[test]
#[ignore = "runs inside a private ConPTY from guard_restores_raw_mode_after_error"]
fn terminal_error_fixture() {
    if std::env::var_os("LOCALVOX_TERMINAL_GUARD_FIXTURE").is_none() {
        return;
    }
    let result: Result<()> = with_terminal(|terminal| {
        let _input = input::Input::new()?;
        terminal.draw(|frame| {
            frame.render_widget(
                ratatui::widgets::Paragraph::new("Ошибка отрисовки"),
                frame.area(),
            )
        })?;
        anyhow::bail!("synthetic failure after terminal initialization")
    });
    assert!(result.is_err());
    assert!(!terminal::is_raw_mode_enabled().unwrap());
    println!("guard-restored");
}

#[test]
fn clipboard_hotkey_and_terminal_paste_have_one_safe_path_without_reading_real_clipboard() {
    use crossterm::event::{KeyEvent, KeyModifiers};
    for key in ['v', 'V', 'м', 'М'] {
        let mut app = App::default();
        dispatch(
            &mut app,
            Event::Key(KeyEvent::new(KeyCode::Char(key), KeyModifiers::CONTROL)),
            || Ok("https://example.org/q:Русский🦀".into()),
        );
        assert!(!app.quit);
        assert!(app.effects.is_empty());
        assert_eq!(
            app.form.unwrap().fields[0].value,
            "https://example.org/q:Русский🦀"
        );
    }
    let mut app = App::default();
    dispatch(
        &mut app,
        Event::Paste("https://example.org/q".into()),
        || panic!("protocol paste must not read clipboard"),
    );
    assert!(app.effects.is_empty());
    let draft = app.form.as_ref().unwrap().fields[0].value.clone();
    dispatch(
        &mut app,
        Event::Key(KeyEvent::new(KeyCode::Char('v'), KeyModifiers::CONTROL)),
        || anyhow::bail!("synthetic unavailable clipboard"),
    );
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, draft);
    assert!(app
        .console
        .text()
        .contains("synthetic unavailable clipboard"));
    assert!(!app.quit);
}

#[test]
fn reserved_actions_and_failed_clipboard_cancel_pending_quit() {
    use crossterm::event::{KeyEvent, KeyModifiers};
    for code in [KeyCode::F(8), KeyCode::Char('v')] {
        let mut app = App::default();
        app.key(KeyEvent::new(KeyCode::Char('q'), KeyModifiers::NONE));
        assert!(app.quit_pending());
        dispatch(
            &mut app,
            Event::Key(KeyEvent::new(
                code,
                if code == KeyCode::F(8) {
                    KeyModifiers::NONE
                } else {
                    KeyModifiers::CONTROL
                },
            )),
            || anyhow::bail!("synthetic clipboard failure"),
        );
        assert!(!app.quit_pending());
    }
}

#[test]
fn guard_restores_raw_mode_after_error() {
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
        "runtime::tests::terminal_error_fixture",
        "--nocapture",
    ]);
    command.env("LOCALVOX_TERMINAL_GUARD_FIXTURE", "1");
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
    let output = std::thread::spawn(move || {
        let mut bytes = [0; 8192];
        while let Ok(n) = reader.read(&mut bytes) {
            if n == 0 || tx.send(bytes[..n].to_vec()).is_err() {
                break;
            }
        }
    });
    let deadline = Instant::now() + Duration::from_secs(15);
    let mut all = Vec::new();
    let mut queries = 0;
    loop {
        let bytes = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .expect("bounded terminal fixture");
        all.extend(bytes);
        assert!(all.len() < 1024 * 1024);
        let text = String::from_utf8_lossy(&all);
        let count = text.matches("\x1b[6n").count();
        while queries < count {
            writer.write_all(b"\x1b[1;1R").unwrap();
            writer.flush().unwrap();
            queries += 1;
        }
        if text.contains("guard-restored") {
            break;
        }
    }
    while child.0.try_wait().unwrap().is_none() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(child.0.wait().unwrap().success());
    let text = String::from_utf8_lossy(&all);
    assert!(
        text.contains("\x1b[?1049l"),
        "alternate screen restored on error"
    );
    assert!(text.contains("\x1b[?2004l"), "paste mode restored on error");
    drop(writer);
    drop(pair.master);
    drop(rx);
    output.join().unwrap();
}
