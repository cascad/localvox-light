use super::*;

fn confirmed() -> App {
    let mut app = App::default();
    app.address = "http://127.0.0.1:43017".into();
    app.record.recording = true;
    app.command(Action::Shutdown);
    assert!(app.form.as_ref().unwrap().title.contains(&app.address));
    assert!(app.effects.is_empty());
    assert!(app.form.as_ref().unwrap().fields.is_empty());
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::CONTROL));
    assert!(matches!(
        &app.effects[..],
        [(Resource::Shutdown, Task::Shutdown)]
    ));
    assert!(!app.quit);
    app
}

#[test]
fn empty_confirmation_accepts_submit_keys_once_and_ignores_text_navigation_and_paste() {
    for code in [KeyCode::Char('s'), KeyCode::Enter] {
        let mut app = App::default();
        app.command(Action::Shutdown);
        for key in [
            KeyCode::Tab,
            KeyCode::BackTab,
            KeyCode::Enter,
            KeyCode::Up,
            KeyCode::Char('д'),
            KeyCode::Char('q'),
        ] {
            app.key(KeyEvent::new(key, KeyModifiers::NONE));
        }
        app.paste("ДА\nhttps://example.test/meeting");
        assert!(app.effects.is_empty() && !app.quit);
        assert!(matches!(
            app.form.as_ref().unwrap().kind,
            FormKind::Shutdown
        ));
        assert!(app.form.as_ref().unwrap().fields.is_empty());
        app.key(KeyEvent::new(code, KeyModifiers::CONTROL));
        app.key(KeyEvent::new(code, KeyModifiers::CONTROL));
        assert!(matches!(
            &app.effects[..],
            [(Resource::Shutdown, Task::Shutdown)]
        ));
        assert!(app.shutdown_started.is_some() && !app.quit);
    }
}

#[test]
fn ordinary_quit_and_cancel_never_request_daemon_shutdown() {
    let mut app = App::default();
    app.command(Action::Shutdown);
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.form.is_none() && app.effects.is_empty());
    app.key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
    assert!(app.quit && app.effects.is_empty());
}

#[test]
fn full_exit_waits_and_does_not_reconnect_or_send_a_second_action() {
    let mut app = confirmed();
    app.tick(5200);
    assert!(app.status.contains("00:05") && app.status.contains(&app.address));
    app.command(Action::Connection);
    assert!(app.form.is_none() && app.connection_request.is_none());
    app.command(Action::StopRecording);
    app.command(Action::Shutdown);
    assert_eq!(app.effects.len(), 1);
    // An earlier action still releases its bookkeeping, without hiding shutdown.
    app.pending_actions.insert(Resource::Control);
    app.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Control,
        result: Err("previous stop failed".into()),
    });
    assert!(!app.pending_actions.contains(&Resource::Control));
    assert!(app.status.contains("Завершение LocalVox"));
    app.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Shutdown,
        result: Ok(Data::Shutdown),
    });
    assert!(app.quit && app.shutdown_started.is_none());
}

#[test]
fn failure_or_rejected_submission_keeps_client_open_and_preserves_confirmation() {
    let mut app = confirmed();
    app.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Shutdown,
        result: Err("engine: final chunk failed".into()),
    });
    assert!(!app.quit && app.shutdown_started.is_none());
    assert!(app.form.is_some() && app.errors.contains_key(&Resource::Shutdown));
    assert_eq!(app.effects.len(), 1, "no automatic retry");
    let mut app = confirmed();
    app.submission_rejected(Resource::Shutdown);
    assert!(!app.quit && app.shutdown_started.is_none() && app.form.is_some());
}

#[test]
fn shutdown_confirmation_shows_target_warning_and_keys_at_normal_terminal_widths() {
    for width in [80, 120] {
        let mut app = App::default();
        app.address = "http://127.0.0.1:43017".into();
        app.command(Action::Shutdown);
        let backend = ratatui::backend::TestBackend::new(width, 30);
        let mut terminal = ratatui::Terminal::new(backend).unwrap();
        terminal.draw(|f| crate::render::draw(f, &app)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        for expected in [
            app.address.as_str(),
            "Запись закроется",
            "Очередь продолжится",
            "Ctrl+S / Ctrl+Enter",
            "завершить",
            "Esc отменить",
        ] {
            assert!(
                text.contains(expected),
                "{width}: missing {expected}: {text}"
            );
        }
        assert!(!text.contains("Введите") && !text.contains("Tab поле"));
    }
}
