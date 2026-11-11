use super::*;
use localvox_light_client::{QueueItem, RetryPlan};
use ratatui::{backend::TestBackend, Terminal};

fn fixture(state: Option<&str>) -> App {
    let mut a = App {
        focus: 1,
        now_ms: 100,
        ..Default::default()
    };
    a.sessions = vec![Session {
        name: "demo".into(),
        title: Some("Встреча".into()),
        job: state.map(str::to_owned),
        ..Default::default()
    }];
    if let Some(state) = state {
        a.queue = vec![QueueItem {
            session: "demo".into(),
            state: state.into(),
            phase: Some("transcribe".into()),
            position: (state == "pending").then_some(2),
            ..Default::default()
        }];
    }
    a
}
fn detail(a: &App) -> String {
    lines(a, &a.sessions[0], Theme::for_app(a))[1].to_string()
}
#[test]
fn server_states_have_distinct_static_outcomes_and_only_running_animates() {
    for (state, symbol, label) in [
        ("running", "⠙", "STT"),
        ("pending", "◷", "в очереди #2"),
        ("waiting_input", "◦", "ждём кусок"),
        ("failed", "!", "ошибка"),
        ("cancelling", "◌", "остановка"),
        ("cancelled", "■", "отменено"),
        ("foreign", "?", "статус неизвестен"),
    ] {
        let a = fixture(Some(state));
        let status = describe(&a, &a.sessions[0]);
        assert_eq!(status.symbol(false, 100), symbol, "{state}");
        assert!(detail(&a).contains(label), "{state}: {}", detail(&a));
        assert_eq!(animation_frame(&a).is_some(), state == "running");
    }
    let mut a = fixture(None);
    assert!(detail(&a).contains("нет STT"));
    a.sessions[0].cooked = true;
    assert_eq!(describe(&a, &a.sessions[0]).symbol(false, 0), "✓");
    assert!(animation_frame(&a).is_none());
}
#[test]
fn recording_never_hides_processing_or_waiting_and_retry_is_not_running() {
    for state in ["running", "pending", "waiting_input", "failed"] {
        let mut a = fixture(Some(state));
        a.sessions[0].recording = true;
        assert!(detail(&a).contains("запись"));
        assert_eq!(animation_frame(&a).is_some(), state == "running");
    }
    let mut a = fixture(None);
    a.sessions[0].recording = true;
    assert_eq!(describe(&a, &a.sessions[0]).symbol(false, 0), "●");
    assert!(animation_frame(&a).is_none());
    let mut a = fixture(Some("failed"));
    for (state, label) in [
        ("retry_wait", "ждём повтор"),
        ("blocked", "повтор не подтверждён"),
        ("stalled", "повтор не подтверждён"),
    ] {
        a.queue[0].retry = Some(RetryPlan {
            state: state.into(),
            ..Default::default()
        });
        assert!(detail(&a).contains(label));
        assert!(animation_frame(&a).is_none());
    }
}
#[test]
fn stale_queue_details_are_not_attached_to_a_different_session_state() {
    let mut a = fixture(Some("running"));
    a.queue[0].state = "pending".into();
    a.queue[0].phase = Some("summary".into());
    assert!(detail(&a).contains("обработка") && !detail(&a).contains("сводка"));
    a.queue[0].state = "running".into();
    a.queue[0].session = "other".into();
    assert!(!detail(&a).contains("сводка"));
    a.queue[0].session = "demo".into();
    assert!(detail(&a).contains("сводка"));
}
#[test]
fn failed_poll_freezes_retained_rows_until_recovery() {
    for resource in [Resource::Sessions, Resource::Queue] {
        let mut a = fixture(Some("running"));
        a.errors.insert(resource, "503".into());
        let status = describe(&a, &a.sessions[0]);
        assert_eq!(status.symbol(false, 100), "?");
        assert_eq!(status.symbol(false, 900), "?");
        assert!(detail(&a).contains("нет обновления"));
        assert!(animation_frame(&a).is_none());
        a.select_session("another".into());
        a.switch(View::Queue);
        a.switch(View::Archive);
        assert!(a.errors.contains_key(&resource));
        assert!(
            animation_frame(&a).is_none(),
            "navigation does not recover a failed poll"
        );
        a.errors.remove(&resource);
        assert!(animation_frame(&a).is_some());
        assert_eq!(a.sessions[0].title(), "Встреча");
    }
}
#[test]
fn frames_have_one_column_and_hidden_or_idle_archives_do_not_request_animation() {
    let mut a = fixture(Some("running"));
    for ascii in [false, true] {
        let status = describe(&a, &a.sessions[0]);
        for n in 0..20 {
            assert_eq!(Line::from(status.symbol(ascii, n * 100)).width(), 1);
        }
    }
    assert_eq!(animation_frame(&a), Some(1));
    a.now_ms = 199;
    assert_eq!(animation_frame(&a), Some(1));
    a.now_ms = 200;
    assert_eq!(animation_frame(&a), Some(2));
    a.viewport = (80, 24);
    a.focus = 2;
    assert!(animation_frame(&a).is_none());
    a.focus = 1;
    assert!(animation_frame(&a).is_some());
    for overlay in 0..3 {
        a.help = overlay == 0;
        a.palette = overlay == 1;
        a.console.open = overlay == 2;
        assert!(animation_frame(&a).is_none());
    }
    a.help = false;
    a.palette = false;
    a.console.open = false;
    a.view = View::Queue;
    assert!(animation_frame(&a).is_none());
}
fn buffer(a: &App) -> ratatui::buffer::Buffer {
    let (w, h) = a.viewport;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| super::super::draw(f, a)).unwrap();
    terminal.backend().buffer().clone()
}
#[test]
fn animation_changes_only_the_badge_cell_and_keeps_selection_scroll_and_effects() {
    let mut a = fixture(Some("running"));
    a.scroll = 7;
    let before = buffer(&a);
    a.now_ms = 200;
    let after = buffer(&a);
    let changed: Vec<_> = before
        .content
        .iter()
        .zip(&after.content)
        .filter(|(x, y)| x != y)
        .collect();
    assert_eq!(changed.len(), 1);
    assert_eq!(a.selected, 0);
    assert_eq!(a.scroll, 7);
    assert_eq!(a.focus, 1);
    assert!(a.effects.is_empty());
}
#[test]
fn activity_visuals_preserve_semantic_colors_in_selected_rows_and_terminal_modes() {
    for mode in ["dark", "light", "16", "mono", "ascii"] {
        let mut a = fixture(Some("running"));
        a.preferences.light = mode == "light";
        a.preferences.color16 = mode == "16";
        a.preferences.monochrome = mode == "mono";
        a.preferences.ascii = mode == "ascii";
        a.sessions[0].recording = true;
        for state in ["pending", "waiting_input", "failed", "cancelled", "ready"] {
            a.sessions.push(Session {
                name: state.into(),
                title: Some(format!("Встреча {state}")),
                job: Some(state.into()),
                cooked: state == "ready",
                ..Default::default()
            });
        }
        if mode == "ascii" {
            a.viewport = (80, 24);
        }
        let image = buffer(&a);
        let t = Theme::for_app(&a);
        let badge = image
            .content
            .iter()
            .find(|c| {
                c.symbol() == if a.preferences.ascii { "/" } else { "⠙" } && c.bg == t.selection
            })
            .unwrap();
        assert_eq!(badge.fg, t.command, "{mode}");
        assert_eq!(badge.bg, t.selection, "selected badge: {mode}");
        if a.preferences.monochrome {
            assert!(image
                .content
                .iter()
                .all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
        }
        insta::assert_debug_snapshot!(format!("activity_{mode}"), image);
    }
}

#[test]
fn filtered_and_empty_archive_are_visible_without_hiding_the_open_document() {
    let mut a = fixture(Some("running"));
    a.current = Some("demo".into());
    a.sessions.push(Session {
        name: "other".into(),
        title: Some("Другая встреча".into()),
        cooked: true,
        ..Default::default()
    });
    a.key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Char('/'),
        crossterm::event::KeyModifiers::NONE,
    ));
    a.paste("другая");
    assert!(
        animation_frame(&a).is_none(),
        "hidden running rows do not animate"
    );
    let image = buffer(&a);
    let text = image.content.iter().map(|c| c.symbol()).collect::<String>();
    assert!(text.contains("1/2") && text.contains("другая"));
    assert_eq!(a.current.as_deref(), Some("demo"));
    insta::assert_debug_snapshot!("filtered_archive", image);
    a.paste(" нет совпадений");
    let image = buffer(&a);
    let text = image.content.iter().map(|c| c.symbol()).collect::<String>();
    assert!(text.contains("Нет совпадений") && text.contains("0/2"));
    insta::assert_debug_snapshot!("empty_filtered_archive", image);
}
