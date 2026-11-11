use super::*;

fn fixture() -> App {
    App {
        focus: 1,
        current: Some("a".into()),
        sessions: [
            ("a", "Совет команды"),
            ("b", "План Q3"),
            ("c", "ДРУГОЙ план"),
        ]
        .into_iter()
        .map(|(id, title)| Session {
            name: id.into(),
            title: Some(title.into()),
            ..Default::default()
        })
        .collect(),
        ..Default::default()
    }
}
fn key(a: &mut App, c: char) {
    a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
}

#[test]
fn navigation_letters_and_arrows_select_without_starting_search_or_opening_documents() {
    for code in [KeyCode::Char('j'), KeyCode::Char('о'), KeyCode::Down] {
        let mut a = fixture();
        a.key(KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(a.selected, 1);
        assert!(!a.archive_filter_editing && a.archive_filter.is_empty());
        assert_eq!(a.current.as_deref(), Some("a"));
        assert!(a.effects.is_empty() && a.player_effects.is_empty());
    }
    for code in [KeyCode::Char('k'), KeyCode::Char('л'), KeyCode::Up] {
        let mut a = fixture();
        a.selected = 2;
        a.key(KeyEvent::new(code, KeyModifiers::NONE));
        assert_eq!(a.selected, 1);
        assert!(!a.archive_filter_editing && a.archive_filter.is_empty());
    }
    let mut a = fixture();
    key(&mut a, 'z');
    assert!(!a.archive_filter_editing && a.archive_filter.is_empty());
}

#[test]
fn vim_right_opens_the_visible_row_only_in_idle_archive_list() {
    for c in ['l', 'д'] {
        let mut a = fixture();
        key(&mut a, '/');
        a.paste("план");
        a.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
        a.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        a.selected = 1;
        key(&mut a, c);
        assert_eq!(a.current.as_deref(), Some("c"));
        assert_eq!(a.focus, 2);
        assert_eq!(a.archive_filter, "план");

        let mut a = fixture();
        key(&mut a, '/');
        key(&mut a, c);
        assert_eq!(a.archive_filter, c.to_string());
        assert_eq!(a.current.as_deref(), Some("a"));
        assert!(a.effects.is_empty());

        let mut a = fixture();
        a.sessions.clear();
        key(&mut a, c);
        assert_eq!(a.focus, 1);
        assert!(a.effects.is_empty());

        let mut a = fixture();
        a.focus = 2;
        key(&mut a, c);
        assert_eq!(a.current.as_deref(), Some("a"));
        assert!(a.effects.is_empty());

        let mut a = fixture();
        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            a.key(KeyEvent::new(KeyCode::Char(c), modifiers));
            assert_eq!(a.focus, 1);
            assert!(a.effects.is_empty());
        }
        a.command(Action::NewAsk);
        key(&mut a, c);
        assert_eq!(a.form.as_ref().unwrap().fields[0].value, c.to_string());
        assert!(a.effects.is_empty());
    }
}

#[test]
fn vim_horizontal_keys_match_document_arrows_and_remain_literal_in_input() {
    for (left, right) in [('h', 'l'), ('р', 'д')] {
        let mut a = fixture();
        a.focus = 2;
        a.tab = Tab::Progress;
        a.journal.entries = vec![localvox_light_client::JournalEntry {
            id: "entry".into(),
            run: "run".into(),
            operation: Some("op".into()),
            kind: "operation_start".into(),
            stage: "summary".into(),
            ..Default::default()
        }];
        key(&mut a, right);
        assert!(a.expanded && a.journal.expanded.contains("run/op"));
        key(&mut a, left);
        assert!(!a.expanded && !a.journal.expanded.contains("run/op"));
        assert_eq!(a.focus, 2);
        assert_eq!(a.current.as_deref(), Some("a"));
        assert!(a.effects.is_empty() && a.player_effects.is_empty());

        a.command(Action::NewAsk);
        key(&mut a, left);
        key(&mut a, right);
        assert_eq!(
            a.form.as_ref().unwrap().fields[0].value,
            format!("{left}{right}")
        );
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        a.focus = 1;
        key(&mut a, '/');
        key(&mut a, left);
        key(&mut a, right);
        assert_eq!(a.archive_filter, format!("{left}{right}"));
        assert_eq!(a.focus, 1);
        assert!(a.effects.is_empty());

        let mut a = fixture();
        a.view = View::Queue;
        a.focus = 2;
        key(&mut a, right);
        assert!(!a.expanded);
        a.view = View::Archive;
        for modifiers in [KeyModifiers::CONTROL, KeyModifiers::ALT] {
            a.key(KeyEvent::new(KeyCode::Char(right), modifiers));
            assert!(!a.expanded);
        }
        a.palette = true;
        key(&mut a, right);
        assert!(a.palette && !a.expanded && a.effects.is_empty());
    }
}

#[test]
fn first_slash_starts_search_and_further_slashes_and_command_letters_are_literal() {
    for first in ['/', '.'] {
        let mut a = fixture();
        key(&mut a, first);
        assert!(a.archive_filter_editing && a.archive_filter.is_empty());
        assert!(a.form.is_none() && a.effects.is_empty());
        for c in "/jkqgп:? .".chars() {
            key(&mut a, c);
        }
        assert_eq!(a.archive_filter, "/jkqgп:? .");
        assert!(!a.quit && !a.palette && !a.help && a.form.is_none());
        assert!(a.effects.is_empty() && a.player_effects.is_empty());
        a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!a.archive_filter_editing && a.archive_filter.is_empty());
        key(&mut a, 'j');
        assert_eq!(a.selected, 1);
    }
}

#[test]
fn leaving_search_preserves_query_and_reentry_does_not_insert_its_trigger() {
    let mut a = fixture();
    key(&mut a, '/');
    a.paste("план");
    a.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(a.focus, 2);
    assert!(!a.archive_filter_editing);
    a.key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
    assert_eq!(a.focus, 1);
    key(&mut a, 'j');
    assert_eq!(a.selected, 1);
    assert_eq!(a.archive_filter, "план");
    key(&mut a, '/');
    assert!(a.archive_filter_editing);
    assert_eq!(a.archive_filter, "план");
    key(&mut a, '/');
    assert_eq!(a.archive_filter, "план/");
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(
        a.archive_filter_editing,
        "no result must keep editing available"
    );
    assert_eq!(a.current.as_deref(), Some("a"));
    a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(a.list_len(), 3);
    key(&mut a, '/');
    a.paste("план");
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(a.current.as_deref(), Some("b"));
    assert!(!a.archive_filter_editing);
}

#[test]
fn active_search_keeps_global_exit_and_player_focus_but_not_plain_q() {
    let mut a = fixture();
    key(&mut a, '/');
    key(&mut a, 'q');
    assert!(!a.quit);
    a.key(KeyEvent::new(KeyCode::Char('г'), KeyModifiers::CONTROL));
    assert!(a.archive_filter_editing && a.archive_filter.is_empty());
    a.key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
    assert_eq!(a.focus, 3);
    assert!(!a.archive_filter_editing);
    for exit in [
        KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE),
        KeyEvent::new(KeyCode::Char('q'), KeyModifiers::CONTROL),
    ] {
        let mut a = fixture();
        key(&mut a, '/');
        a.key(exit);
        assert!(a.quit);
    }
}
#[test]
fn typing_and_paste_filter_titles_without_switching_documents_or_dispatching_commands() {
    let mut a = fixture();
    key(&mut a, '/');
    a.paste("пЛаН");
    assert_eq!(a.list_len(), 2);
    assert_eq!(a.sessions.len(), 3);
    assert_eq!(a.current.as_deref(), Some("a"));
    assert!(a.effects.is_empty() && a.player_effects.is_empty() && a.form.is_none());
    a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    key(&mut a, '/');
    for c in "qjgп 3".chars() {
        key(&mut a, c);
    }
    assert_eq!(a.archive_filter, "qjgп 3");
    assert!(!a.quit && a.form.is_none() && a.list_len() == 0);
    a.key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
    assert_eq!(a.archive_filter, "qjgп ");
    a.key(KeyEvent::new(KeyCode::Char('г'), KeyModifiers::CONTROL));
    assert_eq!(a.list_len(), 3);
    a.paste("План\n\x1b[200~");
    assert!(!a.archive_filter.chars().any(char::is_control));
    assert!(a.effects.is_empty());
}
#[test]
fn refresh_retains_visible_identity_and_enter_opens_the_filtered_row() {
    let mut a = fixture();
    key(&mut a, '/');
    a.paste("план");
    a.selected = 1;
    let mut updated = a.sessions.clone();
    updated.swap(1, 2);
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Sessions,
        result: Ok(Data::Sessions(updated)),
    });
    assert_eq!(a.visible_sessions()[a.selected].name, "c");
    assert_eq!(a.archive_filter, "план");
    assert_eq!(a.current.as_deref(), Some("a"));
    assert!(
        a.archive_filter_editing,
        "refresh must not leave input mode"
    );
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(a.current.as_deref(), Some("c"));
    assert_eq!(a.focus, 2);
    a.select_session("a".into());
    assert!(
        a.archive_filter.is_empty(),
        "an explicit external target must be visible"
    );
}
#[test]
fn no_match_enter_is_inert_and_recovery_reset_keeps_the_full_archive() {
    let mut a = fixture();
    key(&mut a, '/');
    a.paste("нет такого имени");
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(a.current.as_deref(), Some("a"));
    assert!(a.effects.is_empty());
    a.errors.insert(Resource::Sessions, "offline".into());
    a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(a.list_len(), 3);
    assert_eq!(a.sessions.len(), 3);
    assert!(a.errors.contains_key(&Resource::Sessions));
}
#[test]
fn russian_command_positions_work_outside_input_but_forms_keep_their_text() {
    for (en, ru) in [('g', 'п'), ('/', '.'), ('i', 'ш'), ('n', 'т')] {
        let mut a = fixture();
        a.focus = 2;
        key(&mut a, en);
        let expected = a.form.as_ref().unwrap().title.clone();
        let mut b = fixture();
        b.focus = 2;
        key(&mut b, ru);
        assert_eq!(b.form.as_ref().unwrap().title, expected, "{en}/{ru}");
    }
    let mut a = fixture();
    a.focus = 2;
    a.command(Action::ImportUrl);
    for c in "п.й g/".chars() {
        key(&mut a, c);
    }
    assert_eq!(a.form.as_ref().unwrap().fields[0].value, "п.й g/");
    assert!(!a.quit);
    let mut a = fixture();
    a.key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
    assert!(a.palette);
    assert!(a.archive_filter.is_empty());
    let mut a = fixture();
    a.key(KeyEvent::new(KeyCode::Char('й'), KeyModifiers::CONTROL));
    assert!(a.quit);
    let mut a = fixture();
    a.key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
    assert!(a.quit);
}
#[test]
fn every_command_letter_preserves_its_shift_in_both_layouts() {
    for (ru, en) in "йцукенгшщзфывапролдячсмить"
        .chars()
        .zip("qwertyuiopasdfghjklzxcvbnm".chars())
    {
        for upper in [false, true] {
            let ru = if upper {
                ru.to_uppercase().next().unwrap()
            } else {
                ru
            };
            let en = if upper { en.to_ascii_uppercase() } else { en };
            assert_eq!(
                command_key(KeyEvent::new(KeyCode::Char(ru), KeyModifiers::NONE)).code,
                KeyCode::Char(en)
            );
        }
    }
}
