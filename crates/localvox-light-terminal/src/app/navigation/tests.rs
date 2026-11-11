use super::*;

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn document() -> App {
    App {
        current: Some("meeting".into()),
        tab: Tab::Transcript,
        focus: 2,
        item: 1,
        transcript: vec![
            TranscriptLine {
                who: "Первый".into(),
                text: "Начало".into(),
                ..Default::default()
            },
            TranscriptLine {
                who: "Второй".into(),
                text: "Выбранная реплика 🦀\nПродолжение".into(),
                start_sec: 12.0,
                end_sec: 24.0,
                original: Some("служебный оригинал".into()),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

fn copied(app: &App) -> Option<&str> {
    app.effects.iter().find_map(|(_, task)| match task {
        Task::Copy(text) => Some(text.as_str()),
        _ => None,
    })
}

fn summary(kind: &str) -> App {
    let mut app = document();
    app.tab = Tab::Summary;
    app.summary = Summary {
        blocks: vec![
            SummaryBlock {
                kind: "heading".into(),
                text: "Вся сводка".into(),
                ..Default::default()
            },
            SummaryBlock {
                kind: kind.into(),
                text: "Выбранный блок 🦀\nВторая строка".into(),
                sources: vec![Citation {
                    line: 42,
                    start_sec: 12.0,
                    end_sec: 24.0,
                }],
            },
            SummaryBlock {
                text: "Последний блок".into(),
                ..Default::default()
            },
        ],
        provenance: Some(Provenance {
            model: "private-meta".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    app
}

#[test]
fn summary_y_copies_the_selected_block_for_every_kind_while_control_y_keeps_all_blocks() {
    for kind in ["heading", "bullet", "paragraph"] {
        for c in ['y', 'н'] {
            let mut app = summary(kind);
            app.expanded = true;
            app.scroll = 100;
            key(&mut app, KeyCode::Char(c));
            assert_eq!(copied(&app), Some("Выбранный блок 🦀\nВторая строка"));
            assert_eq!((app.item, app.scroll), (1, 100));
            assert!(app.player_effects.is_empty());

            let mut app = summary(kind);
            app.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
            assert_eq!(
                copied(&app),
                Some("Вся сводка\n\nВыбранный блок 🦀\nВторая строка\n\nПоследний блок")
            );
        }
        let mut app = summary(kind);
        app.command(Action::CopyDocument);
        assert_eq!(copied(&app), Some(app.copy_text().as_str()));
    }
    let mut app = summary("bullet");
    app.palette = true;
    app.palette_index = COMMANDS
        .iter()
        .position(|(a, _)| *a == Action::CopySummaryBlock)
        .unwrap();
    key(&mut app, KeyCode::Enter);
    assert_eq!(copied(&app), Some("Выбранный блок 🦀\nВторая строка"));
    assert!(!app.palette);

    let mut app = summary("paragraph");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Char('y'));
    assert_eq!(copied(&app), Some("Последний блок"));
}

#[test]
fn missing_summary_block_does_not_copy_other_text_and_clipboard_error_preserves_selection() {
    for item in [1, 100] {
        let mut app = summary("paragraph");
        app.item = item;
        app.summary.blocks[1].text = " \n ".into();
        key(&mut app, KeyCode::Char('y'));
        assert!(copied(&app).is_none());
        assert!(app.status.contains("Нет выбранного блока сводки"));
    }
    let mut app = summary("paragraph");
    app.summary.blocks.clear();
    key(&mut app, KeyCode::Char('y'));
    assert!(copied(&app).is_none());
    app.tab = Tab::Transcript;
    app.command(Action::CopySummaryBlock);
    assert!(copied(&app).is_none());

    let mut app = summary("paragraph");
    key(&mut app, KeyCode::Char('y'));
    app.apply(Response {
        id: 1,
        epoch: app.epoch,
        resource: Resource::Action,
        result: Err("clipboard unavailable".into()),
    });
    assert_eq!(app.item, 1);
    assert_eq!(
        app.selected_summary_text(),
        "Выбранный блок 🦀\nВторая строка"
    );
    assert!(app.console.unread() > 0 && app.player_effects.is_empty());

    let mut app = summary("paragraph");
    app.command(Action::NewAsk);
    key(&mut app, KeyCode::Char('y'));
    key(&mut app, KeyCode::Char('н'));
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "yн");
    assert!(copied(&app).is_none());
}

#[test]
fn reply_shortcut_and_palette_copy_selected_text_while_control_y_keeps_whole_document() {
    for c in ['y', 'н'] {
        let mut app = document();
        key(&mut app, KeyCode::Char(c));
        assert_eq!(copied(&app), Some("Выбранная реплика 🦀\nПродолжение"));
        assert_eq!((app.item, app.focus), (1, 2));
        assert!(app.player_effects.is_empty());

        let mut app = document();
        let full = app.copy_text();
        app.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL));
        assert_eq!(copied(&app), Some(full.as_str()));
        assert!(full.contains("Первый: Начало") && full.contains("Второй:"));
    }
    let mut app = document();
    app.palette = true;
    app.palette_index = COMMANDS
        .iter()
        .position(|(a, _)| *a == Action::CopyReply)
        .unwrap();
    key(&mut app, KeyCode::Enter);
    assert_eq!(copied(&app), Some("Выбранная реплика 🦀\nПродолжение"));
    assert!(!app.palette);
}

#[test]
fn missing_or_empty_reply_never_falls_back_and_clipboard_failure_keeps_document() {
    for item in [1, 200] {
        let mut app = document();
        app.item = item;
        app.transcript[1].text.clear();
        key(&mut app, KeyCode::Char('y'));
        assert!(copied(&app).is_none());
        assert!(app.status.contains("Нет выбранной реплики"));
    }
    let mut app = document();
    key(&mut app, KeyCode::Char('y'));
    app.apply(Response {
        id: 1,
        epoch: app.epoch,
        resource: Resource::Action,
        result: Err("clipboard unavailable".into()),
    });
    assert_eq!(app.item, 1);
    assert_eq!(
        app.selected_reply_text(),
        "Выбранная реплика 🦀\nПродолжение"
    );
    assert!(app.console.unread() > 0);
    assert!(app.player_effects.is_empty());
}

#[test]
fn copy_shortcut_stays_literal_in_forms_and_filter_and_console_retains_its_own_copy() {
    let mut app = document();
    app.command(Action::NewAsk);
    key(&mut app, KeyCode::Char('y'));
    key(&mut app, KeyCode::Char('н'));
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "yн");
    assert!(copied(&app).is_none());
    key(&mut app, KeyCode::Esc);
    app.focus = 1;
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Char('y'));
    assert_eq!(app.archive_filter, "y");
    assert!(copied(&app).is_none());
    key(&mut app, KeyCode::Esc);
    app.console.open = true;
    key(&mut app, KeyCode::Char('y'));
    assert_ne!(copied(&app), Some(app.transcript[1].text.as_str()));
}

#[test]
fn escape_closes_one_level_in_every_section_without_touching_audio_or_selection() {
    for (view, _) in VIEWS {
        let mut app = document();
        app.view = view;
        app.selected = 4;
        app.list_scroll = 2;
        app.scroll = 7;
        app.playback.position = 42.0;
        app.playback.paused = false;
        app.focus = 3;
        for focus in [2, 1, 0, 0] {
            key(&mut app, KeyCode::Esc);
            assert_eq!(app.focus, focus);
        }
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, 0);
        assert_eq!((app.selected, app.list_scroll, app.scroll), (4, 2, 7));
        assert_eq!(app.playback.position, 42.0);
        assert!(!app.playback.paused);
        assert!(app.effects.is_empty() && app.player_effects.is_empty());
    }
}

#[test]
fn escape_closes_overlays_before_details_and_retains_idle_filter_at_menu() {
    for overlay in [0, 1, 2, 3] {
        let mut app = document();
        match overlay {
            0 => app.command(Action::NewAsk),
            1 => app.palette = true,
            2 => app.help = true,
            _ => app.console.open = true,
        }
        key(&mut app, KeyCode::Esc);
        assert!(app.form.is_none() && !app.palette && !app.help && !app.console.open);
        assert_eq!(app.focus, 2);
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.focus, 1);
    }
    let mut app = document();
    app.sessions = vec![Session {
        name: "meeting".into(),
        title: Some("Тест".into()),
        ..Default::default()
    }];
    app.archive_filter = "Тест".into();
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus, 1);
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus, 0);
    assert_eq!(app.archive_filter, "Тест");
    app.focus = 1;
    key(&mut app, KeyCode::Char('/'));
    key(&mut app, KeyCode::Esc);
    assert!(app.archive_filter.is_empty() && !app.archive_filter_editing);
    assert_eq!(app.focus, 1);
}

#[test]
fn notes_enter_opens_details_and_escape_returns_to_same_slot() {
    let mut app = App {
        view: View::Notes,
        focus: 1,
        ..Default::default()
    };
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, 1);
    app.notes = vec![
        SlotNotes::default(),
        SlotNotes {
            name: "Идеи".into(),
            ..Default::default()
        },
    ];
    app.selected = 1;
    key(&mut app, KeyCode::Enter);
    assert_eq!(app.focus, 2);
    key(&mut app, KeyCode::Esc);
    assert_eq!((app.focus, app.selected), (1, 1));
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus, 0);
}

fn search() -> App {
    App {
        view: View::Search,
        focus: 1,
        selected: 1,
        search_query: "слова".into(),
        hits: vec![
            Hit::default(),
            Hit {
                session: "meeting".into(),
                kind: "transcript".into(),
                snippet: "точное совпадение".into(),
                start_sec: Some(12.0),
                end_sec: Some(24.0),
                ..Default::default()
            },
        ],
        ..Default::default()
    }
}

#[test]
fn search_back_restores_exact_hit_after_reorder_and_rejects_late_requests_without_stopping_audio() {
    let mut app = search();
    app.command(Action::Search);
    app.submit_form().unwrap();
    app.selected = 1;
    let stale_epoch = app.epoch;
    assert!(app.pending_forms.contains_key(&Resource::Search));
    key(&mut app, KeyCode::Enter);
    assert_eq!((app.view, app.focus), (View::Archive, 2));
    assert_eq!(app.document_target, Some(12.0));
    assert!(!app.pending_forms.contains_key(&Resource::Search));
    assert!(!app
        .effects
        .iter()
        .any(|(_, t)| matches!(t, Task::Search(..))));
    app.apply(Response {
        id: 1,
        epoch: stale_epoch,
        resource: Resource::Search,
        result: Ok(Data::Search(vec![])),
    });
    assert_eq!(app.hits.len(), 2);
    let stale_document = app.epoch;
    app.hits.swap(0, 1);
    app.hits[0].score = Some(0.9);
    app.player_effects.clear();
    app.effects.clear();
    app.playback.position = 15.0;
    app.playback.paused = false;
    key(&mut app, KeyCode::Esc);
    assert_eq!((app.view, app.focus, app.selected), (View::Search, 1, 0));
    assert_eq!(app.search_query, "слова");
    assert_eq!(app.playback.position, 15.0);
    assert!(!app.playback.paused);
    assert!(app.effects.is_empty() && app.player_effects.is_empty());
    app.apply(Response {
        id: 2,
        epoch: stale_document,
        resource: Resource::Transcript,
        result: Ok(Data::Transcript(vec![TranscriptLine::default()])),
    });
    assert!(app.transcript.is_empty());
    key(&mut app, KeyCode::Esc);
    assert_eq!(app.focus, 0);
}

#[test]
fn parent_list_handles_removed_rows_and_explicit_switch_discards_parent() {
    for queue in [false, true] {
        let mut app = search();
        if queue {
            app.view = View::Queue;
            app.queue = vec![
                QueueItem {
                    session: "other".into(),
                    ..Default::default()
                },
                QueueItem {
                    session: "meeting".into(),
                    ..Default::default()
                },
            ];
        }
        app.list_scroll = usize::MAX;
        key(&mut app, KeyCode::Enter);
        if queue {
            app.queue.swap(0, 1);
        } else {
            app.hits.swap(0, 1);
        }
        app.player_effects.clear();
        key(&mut app, KeyCode::Esc);
        assert_eq!(app.selected, 0);
        assert!(app.list_scroll < 2 && app.player_effects.is_empty());
        key(&mut app, KeyCode::Enter);
        if queue {
            app.queue.clear();
        } else {
            app.hits.clear();
        }
        key(&mut app, KeyCode::Esc);
        assert_eq!((app.focus, app.selected, app.list_scroll), (1, 0, 0));

        let mut app = search();
        key(&mut app, KeyCode::Enter);
        app.switch(View::Notes);
        app.focus = 2;
        key(&mut app, KeyCode::Esc);
        assert_eq!((app.view, app.focus), (View::Notes, 1));
        assert!(app.detail_parent.is_none());
    }
}

#[test]
fn returning_to_queue_preserves_visible_page_and_selection() {
    let mut app = App {
        view: View::Queue,
        focus: 1,
        selected: 24,
        list_scroll: 20,
        queue: (0..60)
            .map(|i| QueueItem {
                session: format!("record-{i}"),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    let offset = crate::render::list_metrics(&app).1;
    key(&mut app, KeyCode::Enter);
    key(&mut app, KeyCode::Esc);
    assert_eq!(
        (app.view, app.selected, app.list_scroll),
        (View::Queue, 24, offset)
    );
    key(&mut app, KeyCode::Enter);
    app.select_session("explicit-other".into());
    key(&mut app, KeyCode::Esc);
    assert_eq!((app.view, app.focus), (View::Archive, 1));
}
