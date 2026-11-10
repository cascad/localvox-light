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
fn typing_and_paste_filter_titles_without_switching_documents_or_dispatching_commands() {
    let mut a = fixture();
    a.paste("пЛаН");
    assert_eq!(a.list_len(), 2);
    assert_eq!(a.sessions.len(), 3);
    assert_eq!(a.current.as_deref(), Some("a"));
    assert!(a.effects.is_empty() && a.player_effects.is_empty() && a.form.is_none());
    a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
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
