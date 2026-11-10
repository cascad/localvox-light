use super::*;
use ratatui::{backend::TestBackend, Terminal};

fn text_app(size: (u16, u16)) -> App {
    let mut a = App {
        focus: 2,
        tab: Tab::Readable,
        viewport: size,
        follow_selection: false,
        ..Default::default()
    };
    a.readable.from_transcript = true;
    a.readable.lines = vec![TranscriptLine {
        text: (0..200)
            .map(|n| format!("Строка {n:03}"))
            .collect::<Vec<_>>()
            .join("\n"),
        ..Default::default()
    }];
    a
}
fn key(a: &mut App, code: KeyCode) {
    a.key(KeyEvent::new(code, KeyModifiers::NONE));
}
fn body_top(a: &App) -> String {
    let (w, h) = a.viewport;
    let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
    terminal.draw(|f| crate::render::draw(f, a)).unwrap();
    let x = if w < 95 { 1 } else { 28 };
    (x..w.min(x + 15))
        .map(|col| terminal.backend().buffer()[(col, 9)].symbol())
        .collect()
}
#[test]
fn document_pages_use_actual_body_height_and_reach_the_last_line() {
    for (size, page) in [((80, 24), 13), ((120, 36), 25), ((160, 48), 37)] {
        let mut a = text_app(size);
        assert_eq!(crate::render::document_dimensions(&a).1, page);
        assert!(body_top(&a).starts_with("Исходная"));
        key(&mut a, KeyCode::PageDown);
        assert_eq!(a.scroll, page);
        assert!(body_top(&a).starts_with(&format!("Строка {:03}", page - 2)));
        key(&mut a, KeyCode::PageUp);
        assert_eq!(a.scroll, 0);
        for _ in 0..30 {
            key(&mut a, KeyCode::PageDown);
        }
        assert_eq!(a.scroll, crate::render::document_metrics(&a).0 - page);
        key(&mut a, KeyCode::Home);
        key(&mut a, KeyCode::End);
        assert_eq!(a.scroll, crate::render::document_metrics(&a).0 - page);
        assert!(a.effects.is_empty() && a.player_effects.is_empty());
    }
}
#[test]
fn resize_wrapped_text_and_other_focus_do_not_change_the_reading_target_or_sound() {
    let mut a = text_app((120, 36));
    a.readable.lines[0].text = "длинная строка ".repeat(1000);
    a.item = 4;
    a.playback.position = 17.0;
    key(&mut a, KeyCode::PageDown);
    assert_eq!(a.scroll, 25);
    a.viewport = (80, 24);
    key(&mut a, KeyCode::PageDown);
    assert_eq!(a.scroll, 38);
    for focus in [0, 3] {
        a.focus = focus;
        key(&mut a, KeyCode::PageDown);
        key(&mut a, KeyCode::PageUp);
        assert_eq!(a.scroll, 38);
    }
    assert_eq!(a.item, 4);
    assert_eq!(a.playback.position, 17.0);
    assert!(a.effects.is_empty() && a.player_effects.is_empty());
}
#[test]
fn archive_page_moves_the_whole_window_and_filtered_selection_without_opening() {
    for (size, page) in [((80, 24), 7), ((120, 36), 9), ((160, 48), 15)] {
        let mut a = App {
            focus: 1,
            viewport: size,
            current: Some("opened".into()),
            ..Default::default()
        };
        a.sessions = (0..70)
            .map(|n| Session {
                name: format!("s{n:02}"),
                title: Some(format!("Встреча {n:02}")),
                cooked: true,
                ..Default::default()
            })
            .collect();
        assert_eq!(crate::render::list_metrics(&a).0, page);
        key(&mut a, KeyCode::PageDown);
        assert_eq!((a.selected, a.list_scroll), (page, page));
        key(&mut a, KeyCode::Down);
        assert_eq!((a.selected, a.list_scroll), (page + 1, page));
        key(&mut a, KeyCode::PageUp);
        assert_eq!((a.selected, a.list_scroll), (1, 0));
        key(&mut a, KeyCode::Char('/'));
        a.paste("Встреча 1");
        let filtered = crate::render::list_metrics(&a).0;
        assert_eq!(filtered, if size.1 == 24 { page } else { page - 1 });
        key(&mut a, KeyCode::PageDown);
        assert_eq!(
            a.visible_sessions()[a.selected].name,
            format!("s{}", 10 + filtered.min(9))
        );
        assert_eq!(a.current.as_deref(), Some("opened"));
        assert!(a.effects.is_empty() && a.player_effects.is_empty());
        a.paste(" нет совпадений");
        key(&mut a, KeyCode::PageDown);
        key(&mut a, KeyCode::PageUp);
        assert_eq!((a.selected, a.list_scroll), (0, 0));
    }
}

#[test]
fn single_line_lists_page_by_rows_and_stop_at_their_bounds() {
    let mut a = App {
        view: View::Queue,
        focus: 1,
        viewport: (120, 36),
        ..Default::default()
    };
    a.queue = (0..80)
        .map(|n| QueueItem {
            session: format!("q{n}"),
            ..Default::default()
        })
        .collect();
    assert_eq!(crate::render::list_metrics(&a).0, 18);
    key(&mut a, KeyCode::PageDown);
    assert_eq!((a.selected, a.list_scroll), (18, 18));
    key(&mut a, KeyCode::End);
    assert_eq!((a.selected, a.list_scroll), (79, 62));
    key(&mut a, KeyCode::PageDown);
    assert_eq!((a.selected, a.list_scroll), (79, 62));
    key(&mut a, KeyCode::Home);
    key(&mut a, KeyCode::PageUp);
    assert_eq!((a.selected, a.list_scroll), (0, 0));
    assert!(a.effects.is_empty() && a.player_effects.is_empty());
}
#[test]
fn journal_paging_stops_follow_and_end_restores_it_with_safe_small_windows() {
    let mut a = text_app((120, 36));
    a.tab = Tab::Progress;
    a.journal.merge(
        serde_json::from_str(include_str!("../../../../../tests/fixtures/journal.json")).unwrap(),
        false,
    );
    a.journal.expanded.insert("r1/op1".into());
    a.journal.follow = true;
    let end = crate::render::journal_metrics(&a).1;
    key(&mut a, KeyCode::PageUp);
    assert_eq!(a.scroll, end.saturating_sub(25));
    assert!(!a.journal.follow && !a.follow_selection);
    key(&mut a, KeyCode::End);
    assert!(a.journal.follow);
    for size in [(0, 0), (35, 10), (80, 12)] {
        a.viewport = size;
        a.focus = 1;
        a.sessions.clear();
        a.selected = 0;
        key(&mut a, KeyCode::PageDown);
        key(&mut a, KeyCode::PageUp);
        assert_eq!(a.selected, 0);
    }
}
