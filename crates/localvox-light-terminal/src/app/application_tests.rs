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
    for (code, modifiers) in [
        (KeyCode::Char('s'), KeyModifiers::NONE),
        (KeyCode::Char('ы'), KeyModifiers::NONE),
        (KeyCode::Char('ω'), KeyModifiers::NONE),
        (KeyCode::Enter, KeyModifiers::NONE),
        (KeyCode::Char('s'), KeyModifiers::CONTROL),
        (KeyCode::Enter, KeyModifiers::CONTROL),
    ] {
        let mut app = App::default();
        app.preferences.keymap =
            crate::keymap::Keymap::from_json(r#"{"version":1,"keys":{"ω":"s"}}"#.as_bytes())
                .unwrap();
        app.command(Action::Shutdown);
        for key in [
            KeyCode::Tab,
            KeyCode::BackTab,
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
        app.key(KeyEvent::new(code, modifiers));
        app.key(KeyEvent::new(code, modifiers));
        assert!(matches!(
            &app.effects[..],
            [(Resource::Shutdown, Task::Shutdown)]
        ));
        assert!(app.shutdown_started.is_some() && !app.quit);
    }
}

#[test]
fn every_confirmation_is_empty_and_s_or_enter_submits_the_pinned_task_once() {
    for code in [KeyCode::Char('s'), KeyCode::Char('ы'), KeyCode::Enter] {
        for task in [
            Task::DeleteSession("original".into()),
            Task::DeleteAsk("ask".into()),
            Task::DeleteNote(
                "notes".into(),
                localvox_light_client::StoredNote {
                    raw: "original text".into(),
                    source: "original.md".into(),
                    ..Default::default()
                },
            ),
            Task::Recook("original".into(), Scope::All),
            Task::Retry("original".into()),
            Task::Cancel("original".into()),
            Task::SaveSettings([("KEY".into(), Some("value".into()))].into()),
            Task::Confirm("original".into(), "artifact".into()),
        ] {
            let mut app = App::default();
            app.current = Some("original".into());
            app.confirm("Подтвердить действие для original", task.clone());
            assert!(app.form.as_ref().unwrap().is_confirmation());
            assert!(app.form.as_ref().unwrap().fields.is_empty());
            app.current = Some("other".into());
            app.paste("sДА\n");
            for kind in [
                crossterm::event::KeyEventKind::Repeat,
                crossterm::event::KeyEventKind::Release,
            ] {
                let mut event = KeyEvent::new(code, KeyModifiers::NONE);
                event.kind = kind;
                app.key(event);
            }
            app.key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
            assert!(app.effects.is_empty());
            app.key(KeyEvent::new(code, KeyModifiers::NONE));
            assert_eq!(app.effects.len(), 1);
            assert_eq!(
                crate::io::action_resource(&app.effects[0].1),
                crate::io::action_resource(&task)
            );
            if let Task::DeleteSession(target)
            | Task::Recook(target, _)
            | Task::Retry(target)
            | Task::Cancel(target) = &app.effects[0].1
            {
                assert_eq!(target, "original");
            }
            if let Task::DeleteNote(slot, note) = &app.effects[0].1 {
                assert_eq!(slot, "notes");
                assert_eq!(note.source, "original.md");
                assert_eq!(note.raw, "original text");
            }
            app.key(KeyEvent::new(code, KeyModifiers::NONE));
            assert_eq!(app.effects.len(), 1, "pending action must not repeat");
        }
        let mut app = App::default();
        app.settings_edits
            .insert("KEY".into(), Some("draft".into()));
        app.open("Отменить изменения", FormKind::DiscardSettings, &[]);
        app.paste("s\n");
        let mut repeat = KeyEvent::new(code, KeyModifiers::NONE);
        repeat.kind = crossterm::event::KeyEventKind::Repeat;
        app.key(repeat);
        assert!(!app.settings_edits.is_empty());
        app.key(KeyEvent::new(code, KeyModifiers::NONE));
        assert!(app.settings_edits.is_empty() && app.form.is_none() && app.effects.is_empty());
    }
}

#[test]
fn enter_opening_shutdown_does_not_confirm_until_a_new_press() {
    let mut app = App::default();
    app.palette = true;
    app.palette_index = COMMANDS
        .iter()
        .position(|(action, _)| *action == Action::Shutdown)
        .unwrap();
    app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(app.form.as_ref().unwrap().is_shutdown());
    assert!(app.effects.is_empty());
    for kind in [
        crossterm::event::KeyEventKind::Repeat,
        crossterm::event::KeyEventKind::Release,
    ] {
        let mut key = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
        key.kind = kind;
        app.key(key);
    }
    app.paste("\n\rs");
    assert!(app.effects.is_empty());
    app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(matches!(
        &app.effects[..],
        [(Resource::Shutdown, Task::Shutdown)]
    ));
    assert!(!app.quit, "wait for the daemon acknowledgement");

    let mut app = App::default();
    app.command(Action::Shutdown);
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(app.form.is_none() && app.effects.is_empty());
}

#[test]
fn lower_s_opens_settings_then_a_separate_press_confirms_and_text_forms_keep_s() {
    let mut app = App {
        view: View::Settings,
        ..Default::default()
    };
    app.settings_edits
        .insert("KEY".into(), Some("value".into()));
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(app.form.as_ref().unwrap().is_confirmation() && app.effects.is_empty());
    let mut repeat = KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE);
    repeat.kind = crossterm::event::KeyEventKind::Repeat;
    app.key(repeat);
    assert!(app.effects.is_empty());
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert!(matches!(&app.effects[..], [(_, Task::SaveSettings(_))]));
    let mut app = App::default();
    app.open("Текст", FormKind::Start, &[("Название", "")]);
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "s");
    assert!(app.effects.is_empty());
    let mut app = App::default();
    app.open("Текст", FormKind::Note, &[("Текст", "")]);
    app.key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE));
    app.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "s\n");
    assert!(app.effects.is_empty());
}

#[test]
fn settings_delete_and_discard_use_a_compact_confirmation_without_a_text_field() {
    use ratatui::{backend::TestBackend, Terminal};
    for kind in [
        FormKind::Confirm(Task::SaveSettings(Default::default())),
        FormKind::Confirm(Task::DeleteSession("original".into())),
        FormKind::DiscardSettings,
    ] {
        let mut app = App::default();
        app.preferences.ascii = true;
        app.open("Подтвердить original", kind, &[]);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal
            .draw(|frame| crate::render::draw(frame, &app))
            .unwrap();
        let buffer = terminal.backend().buffer();
        let rows = (0..24)
            .map(|y| (0..80).map(|x| buffer[(x, y)].symbol()).collect::<String>())
            .collect::<Vec<_>>();
        let text = rows.join("\n");
        assert!(
            rows[7].contains("Подтвердить original"),
            "a short confirmation must not fill the terminal: {text}"
        );
        assert!(
            text.contains("Enter / s подтвердить") && text.contains("Esc отменить"),
            "{text}"
        );
        assert!(
            !text.contains("Введите") && !text.contains("Tab поле"),
            "{text}"
        );
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
            "Enter / s подтвердить",
            "Esc отменить",
        ] {
            assert!(
                text.contains(expected),
                "{width}: missing {expected}: {text}"
            );
        }
        assert!(!text.contains("Введите") && !text.contains("Tab поле"));
        assert!(!text.contains("Ctrl+S") && !text.contains("Ctrl+Enter"));
    }
}
