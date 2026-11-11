use super::*;

fn control(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::CONTROL));
}

#[test]
fn control_arrows_resize_the_list_without_changing_selection_document_or_audio() {
    let mut a = App {
        current: Some("fixture".into()),
        tab: Tab::Transcript,
        selected: 7,
        list_scroll: 4,
        item: 2,
        scroll: 9,
        archive_filter: "Встреча".into(),
        playback: crate::player::PlaybackState {
            position: 42.0,
            duration: Some(100.0),
            paused: false,
            fragment_end: Some(70.0),
            ..Default::default()
        },
        ..Default::default()
    };
    let playback = a.playback.clone();
    let epoch = a.epoch;
    control(&mut a, KeyCode::Right);
    assert_eq!(a.list_width, 31);
    control(&mut a, KeyCode::Left);
    assert_eq!(a.list_width, 27);
    assert_eq!(
        (a.selected, a.list_scroll, a.item, a.scroll, a.epoch),
        (7, 4, 2, 9, epoch)
    );
    assert_eq!(a.current.as_deref(), Some("fixture"));
    assert_eq!(a.archive_filter, "Встреча");
    assert_eq!(a.playback, playback);
    assert!(a.effects.is_empty() && a.player_effects.is_empty());
    a.focus = 0;
    control(&mut a, KeyCode::Right);
    assert_eq!(a.list_width, 31);
}

#[test]
fn width_limits_reset_compact_layout_and_visible_step_are_deterministic() {
    let mut a = App::default();
    for _ in 0..100 {
        control(&mut a, KeyCode::Right);
    }
    assert_eq!(a.list_width, 80); // 120-column window leaves 40 for the document.
    for _ in 0..100 {
        control(&mut a, KeyCode::Left);
    }
    assert_eq!(a.list_width, 22);
    a.command(Action::ResetListWidth);
    assert_eq!(a.list_width, 27);
    a.list_width = 100;
    a.viewport = (95, 36);
    control(&mut a, KeyCode::Left);
    assert_eq!(a.list_width, 51); // Step from visible 55, not hidden preference100.
    a.viewport = (80, 24);
    control(&mut a, KeyCode::Right);
    assert_eq!(a.list_width, 51);
    assert!(a.status.contains("95"));
    a.command(Action::ResetListWidth);
    assert_eq!(a.list_width, 27);
}

#[test]
fn fields_filter_and_overlays_keep_control_arrows_and_player_keeps_seeking() {
    let mut form = App::default();
    form.command(Action::Search);
    control(&mut form, KeyCode::Right);
    assert_eq!(form.list_width, 27);
    assert!(form.form.is_some());
    for mode in 0..4 {
        let mut a = App::default();
        match mode {
            0 => {
                a.archive_filter_editing = true;
                a.archive_filter = "Заметка".into();
            }
            1 => a.help = true,
            2 => a.palette = true,
            _ => a.console.toggle(),
        }
        control(&mut a, KeyCode::Right);
        assert_eq!(a.list_width, 27, "mode={mode}");
        assert!(a.effects.is_empty() && a.player_effects.is_empty());
    }
    for (focus, modifiers) in [(1, KeyModifiers::ALT), (3, KeyModifiers::CONTROL)] {
        let mut a = App {
            focus,
            current: Some("fixture".into()),
            ..Default::default()
        };
        a.playback.duration = Some(100.0);
        a.key(KeyEvent::new(KeyCode::Right, modifiers));
        assert_eq!(a.list_width, 27);
        assert!(!a.player_effects.is_empty());
    }
    let mut a = App::default();
    let mut key = KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL);
    key.kind = crossterm::event::KeyEventKind::Repeat;
    a.key(key);
    assert_eq!(a.list_width, 31);
    key.kind = crossterm::event::KeyEventKind::Release;
    a.key(key);
    assert_eq!(a.list_width, 31);
}
