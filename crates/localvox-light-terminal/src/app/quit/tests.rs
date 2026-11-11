use super::*;

fn press(app: &mut App, c: char) {
    app.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

#[test]
fn two_presses_confirm_only_within_the_short_monotonic_window() {
    for c in ['q', 'Q', 'й', 'Й'] {
        let mut app = App::default();
        app.tick(100);
        press(&mut app, c);
        assert!(!app.quit && app.quit_pending());
        app.tick(600);
        press(&mut app, c);
        assert!(app.quit && !app.quit_pending());
    }
    let mut app = App::default();
    press(&mut app, 'q');
    app.tick(501);
    assert!(!app.quit_pending() && app.quit_sequence.is_none());
    press(&mut app, 'q');
    assert!(!app.quit && app.quit_pending());
    app.tick(1002);
    press(&mut app, 'q');
    assert!(!app.quit);
}

#[test]
fn another_key_paste_command_or_context_cancels_the_first_press() {
    let mut app = App::default();
    press(&mut app, 'q');
    app.key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
    assert!(!app.quit_pending());
    press(&mut app, 'q');
    assert!(!app.quit);
    app.paste("q");
    assert!(app.quit_sequence.is_none());
    press(&mut app, 'q');
    app.command(Action::ImportUrl);
    assert!(app.quit_sequence.is_none());
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    for change in 0..5 {
        press(&mut app, 'q');
        match change {
            0 => app.focus = 2,
            1 => app.tab = Tab::Progress,
            2 => app.epoch += 1,
            3 => app.help = true,
            _ => app.viewport = (80, 24),
        }
        assert!(!app.quit_pending());
        app.tick(app.now_ms);
        assert!(app.quit_sequence.is_none());
    }
    assert!(!app.quit && app.effects.is_empty());
}

#[test]
fn text_repeat_and_release_never_confirm_plain_exit() {
    let mut app = App::default();
    app.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    press(&mut app, 'q');
    press(&mut app, 'q');
    assert_eq!(app.archive_filter, "qq");
    assert!(!app.quit && !app.quit_pending());
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.command(Action::NewAsk);
    press(&mut app, 'й');
    press(&mut app, 'й');
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "йй");
    assert!(!app.quit && !app.quit_pending());
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    press(&mut app, 'q');
    for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
        app.key(KeyEvent::new_with_kind(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            kind,
        ));
        assert!(!app.quit && app.quit_pending());
    }
    app.tick(501);
    app.key(KeyEvent::new_with_kind(
        KeyCode::Char('q'),
        KeyModifiers::NONE,
        KeyEventKind::Repeat,
    ));
    assert!(!app.quit_pending());
}

#[test]
fn pending_hint_is_visible_then_disappears_without_overwriting_status() {
    use ratatui::{backend::TestBackend, Terminal};
    let mut app = App::default();
    app.status = "Подключено".into();
    press(&mut app, 'q');
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal.draw(|f| crate::render::draw(f, &app)).unwrap();
    let text = format!("{:?}", terminal.backend().buffer());
    assert!(text.contains("q ещё раз — выйти"));
    app.tick(501);
    terminal.draw(|f| crate::render::draw(f, &app)).unwrap();
    assert!(!format!("{:?}", terminal.backend().buffer()).contains("q ещё раз — выйти"));
    assert_eq!(app.status, "Подключено");
}
