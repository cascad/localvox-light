use super::*;
use crate::{
    app::{Action, App, Tab},
    io::Task,
};
use crossterm::event::{KeyEvent, KeyModifiers};
use localvox_light_client::{Session, SummaryBlock};

fn map() -> Keymap {
    Keymap::from_json(
        r#"{"version":1,"keys":{"υ":"y","ς":"q","λ":"j","ι":"l","σ":"/","ω":"v","ψ":"u","δ":"s"}}"#
            .as_bytes(),
    )
    .unwrap()
}
fn key(app: &mut App, c: char, modifiers: KeyModifiers) {
    app.key(KeyEvent::new(KeyCode::Char(c), modifiers));
}

#[test]
fn overrides_precede_defaults_preserve_modifiers_and_support_shift_and_explicit_case() {
    let m =
        Keymap::from_json(r#"{"version":1,"keys":{"a":"j","υ":"y","σ":"/","Υ":"p"}}"#.as_bytes())
            .unwrap();
    for (from, to) in [('a', 'j'), ('A', 'J'), ('υ', 'y'), ('Υ', 'p'), ('Σ', '?')] {
        let event = KeyEvent::new(
            KeyCode::Char(from),
            KeyModifiers::CONTROL | KeyModifiers::SHIFT,
        );
        let mapped = m.translate(event).unwrap();
        assert_eq!(mapped.code, KeyCode::Char(to));
        assert_eq!(
            (mapped.modifiers, mapped.kind),
            (event.modifiers, event.kind)
        );
    }
    assert!(m
        .translate(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE))
        .is_none());
    let mut app = App {
        keymap: m,
        ..Default::default()
    };
    key(&mut app, 'й', KeyModifiers::CONTROL);
    assert!(
        app.quit,
        "default RU/EN remains available for unconfigured symbols"
    );
}

#[test]
fn invalid_files_are_rejected_and_loading_is_bounded() {
    for json in [
        r#"{"version":2,"keys":{}}"#,
        r#"{"version":1,"keys":{},"extra":0}"#,
        r#"{"version":1,"keys":{"xx":"y"}}"#,
        r#"{"version":1,"keys":{"υ":"yy"}}"#,
        r#"{"version":1,"keys":{"υ":"й"}}"#,
        r#"{"version":1,"keys":{"\n":"y"}}"#,
        "{}",
        "not json",
    ] {
        assert!(Keymap::from_json(json.as_bytes()).is_err(), "{json}");
    }
    let many: BTreeMap<char, char> = (0x100..0x181)
        .map(|u| (char::from_u32(u).unwrap(), 'y'))
        .collect();
    assert!(Keymap::from_json(
        &serde_json::to_vec(&serde_json::json!({"version":1,"keys":many})).unwrap()
    )
    .is_err());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("keys.json");
    assert!(Keymap::load(&file).is_err());
    std::fs::write(&file, vec![b' '; 65537]).unwrap();
    assert!(Keymap::load(&file)
        .unwrap_err()
        .to_string()
        .contains("64 KiB"));
    std::fs::write(&file, br#"{"version":1,"keys":{"a":"j"}}"#).unwrap();
    assert!(Keymap::load(&file).is_ok());
}

#[test]
fn mapped_commands_and_control_work_without_translating_forms_or_filter_text() {
    let mut app = App {
        keymap: map(),
        focus: 2,
        tab: Tab::Summary,
        ..Default::default()
    };
    app.summary.blocks = vec![SummaryBlock {
        text: "выбранный блок".into(),
        ..Default::default()
    }];
    key(&mut app, 'υ', KeyModifiers::NONE);
    assert!(matches!(&app.effects[0].1,Task::Copy(s) if s=="выбранный блок"));
    app.effects.clear();
    app.pending_actions.clear();
    app.command(Action::NewAsk);
    key(&mut app, 'υ', KeyModifiers::NONE);
    key(&mut app, 'ς', KeyModifiers::NONE);
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "υς");
    assert!(!app.quit && app.effects.is_empty());
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    app.focus = 1;
    app.sessions = vec![
        Session {
            name: "a".into(),
            ..Default::default()
        },
        Session {
            name: "b".into(),
            ..Default::default()
        },
    ];
    key(&mut app, 'σ', KeyModifiers::NONE);
    for c in "ςλυ".chars() {
        key(&mut app, c, KeyModifiers::NONE);
    }
    assert_eq!(app.archive_filter, "ςλυ");
    assert!(!app.quit);
    key(&mut app, 'ψ', KeyModifiers::CONTROL);
    assert!(app.archive_filter.is_empty() && app.archive_filter_editing);
    app.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    key(&mut app, 'λ', KeyModifiers::NONE);
    assert_eq!(app.selected, 1);
    key(&mut app, 'ι', KeyModifiers::NONE);
    assert_eq!((app.current.as_deref(), app.focus), (Some("b"), 2));
}

#[test]
fn mapped_quit_sequence_and_form_control_submit_use_the_same_projection() {
    let mut app = App {
        keymap: map(),
        ..Default::default()
    };
    key(&mut app, 'ς', KeyModifiers::NONE);
    assert!(!app.quit && app.quit_pending());
    app.tick(100);
    key(&mut app, 'ς', KeyModifiers::NONE);
    assert!(app.quit);
    let mut app = App {
        keymap: map(),
        ..Default::default()
    };
    app.command(Action::NewAsk);
    key(&mut app, 'ς', KeyModifiers::CONTROL);
    assert!(app.quit && app.effects.is_empty());
    let mut app = App {
        keymap: map(),
        ..Default::default()
    };
    app.command(Action::ImportUrl);
    app.paste("https://example.org/video");
    key(&mut app, 'δ', KeyModifiers::CONTROL);
    assert!(app.form.is_none());
    assert!(app
        .effects
        .iter()
        .any(|(_, t)| matches!(t,Task::Ingest(s) if s=="https://example.org/video")));
}
