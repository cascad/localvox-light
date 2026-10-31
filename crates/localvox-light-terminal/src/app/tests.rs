use super::*;
#[test]
fn player_focus_seeks_with_boundaries_and_forms_keep_playback_keys_as_text() {
    let mut a = App::default();
    a.current = Some("demo".into());
    a.playback = crate::player::PlaybackState {
        session: a.current.clone(),
        position: 10.0,
        duration: Some(120.0),
        paused: true,
        ..Default::default()
    };
    a.key(KeyEvent::new(KeyCode::F(6), KeyModifiers::NONE));
    assert_eq!(a.focus, 3);
    for (key, modifiers, expected) in [
        (KeyCode::Right, KeyModifiers::NONE, 15.0),
        (KeyCode::Left, KeyModifiers::ALT, 0.0),
        (KeyCode::Home, KeyModifiers::NONE, 0.0),
        (KeyCode::End, KeyModifiers::NONE, 119.99),
    ] {
        a.key(KeyEvent::new(key, modifiers));
        assert!(
            matches!(a.player_effects.last(), Some(crate::player::PlaybackCommand::Seek { session, position }) if session == "demo" && (*position - expected).abs() < 0.001)
        );
    }
    a.key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
    assert_eq!(a.focus, 2);
    a.command(7);
    let before = a.player_effects.len();
    for c in ['g', 'p', ' '] {
        a.key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
    }
    assert_eq!(a.player_effects.len(), before);
    assert_eq!(a.form.as_ref().unwrap().fields[0].value, "gp ");
}

#[test]
fn ordinary_play_after_a_fragment_continues_from_its_endpoint() {
    let mut a = App::default();
    a.current = Some("demo".into());
    a.playback = crate::player::PlaybackState {
        session: a.current.clone(),
        position: 20.8,
        duration: Some(3600.0),
        idle: true,
        paused: true,
        fragment_end: Some(20.8),
        ..Default::default()
    };
    a.command(24);
    assert!(
        matches!(a.player_effects.last(), Some(crate::player::PlaybackCommand::Load { start, end: None, .. }) if *start == 20.8)
    );
}
#[test]
fn stale_session_cannot_replace_the_open_document() {
    let mut a = App::default();
    a.current = Some("B".into());
    a.epoch = 2;
    a.apply(Response {
        id: 1,
        epoch: 1,
        resource: Resource::Summary,
        result: Ok(Data::Summary(Summary {
            blocks: vec![SummaryBlock {
                text: "A".into(),
                ..Default::default()
            }],
            ..Default::default()
        })),
    });
    assert!(a.summary.blocks.is_empty());
}
#[test]
fn paste_is_text_not_commands() {
    let mut a = App::default();
    a.command(7);
    a.paste("Русский\nqri: текст");
    assert!(!a.quit);
    assert!(a.effects.is_empty());
    assert_eq!(a.form.unwrap().fields[0].value, "Русский\nqri: текст");
}
#[test]
fn cancellation_never_deletes() {
    let mut a = App::default();
    a.current = Some("session".into());
    a.command(17);
    a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert!(a.form.is_none());
    assert!(a.effects.is_empty());
}
#[test]
fn record_requires_engine_confirmation() {
    let mut a = App::default();
    a.record.engine = true;
    a.command(1);
    a.submit_form().unwrap();
    assert_eq!(a.pending_record, Some(true));
    assert!(!a.record.recording);
    a.tick(10_001);
    assert_eq!(a.pending_record, None);
    assert!(!a.record.recording);
}
#[test]
fn failed_mutation_restores_the_original_session_draft() {
    let mut a = App::default();
    a.current = Some("A".into());
    a.command(10);
    a.submit_form().unwrap();
    a.current = Some("B".into());
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Action,
        result: Err("synthetic failure".into()),
    });
    assert_eq!(a.form.as_ref().unwrap().session.as_deref(), Some("A"));
    a.effects.clear();
    a.submit_form().unwrap();
    assert!(matches!(&a.effects[0].1,Task::Lang(s,_) if s=="A"));
}
#[test]
fn stop_is_independent_of_a_pending_model_action() {
    let mut a = App::default();
    a.pending_actions.insert(Resource::Action);
    a.command(2);
    assert!(matches!(&a.effects[0], (Resource::Control, Task::Stop)));
}
#[test]
fn ctrl_q_works_in_russian_layout() {
    let mut a = App::default();
    a.key(KeyEvent::new(KeyCode::Char('й'), KeyModifiers::CONTROL));
    assert!(a.quit);
}
#[test]
fn editing_preserves_unicode_at_the_cursor() {
    let mut a = App::default();
    a.command(7);
    a.paste("абв");
    a.key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
    a.key(KeyEvent::new(KeyCode::Char('Я'), KeyModifiers::NONE));
    assert_eq!(a.form.as_ref().unwrap().fields[0].value, "абЯв");
}
#[test]
fn note_deletion_is_scoped_to_the_selected_slot() {
    let mut a = App::default();
    a.notes = vec![
        SlotNotes {
            name: "one".into(),
            notes: vec![StoredNote {
                text: "wrong".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
        SlotNotes {
            name: "two".into(),
            notes: vec![StoredNote {
                text: "right".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    ];
    a.selected = 1;
    a.command(18);
    let f = a.form.unwrap();
    assert!(
        matches!(f.kind,FormKind::Confirm(Task::DeleteNote(slot,n)) if slot=="two"&&n.text=="right")
    );
}
#[test]
fn failed_poll_keeps_document_focus_and_draft() {
    let mut a = App::default();
    a.current = Some("demo".into());
    a.focus = 2;
    a.summary.blocks = vec![SummaryBlock {
        text: "retained".into(),
        ..Default::default()
    }];
    a.command(7);
    a.paste("черновик");
    a.apply(Response {
        id: 1,
        epoch: a.epoch,
        resource: Resource::Summary,
        result: Err("503".into()),
    });
    assert_eq!(a.summary.blocks[0].text, "retained");
    assert_eq!(a.focus, 2);
    assert_eq!(a.form.as_ref().unwrap().fields[0].value, "черновик");
    assert!(a.errors.contains_key(&Resource::Summary));
}
#[test]
fn copy_excludes_original_and_provenance_even_when_expanded() {
    let mut a = App::default();
    a.tab = Tab::Readable;
    a.expanded = true;
    a.readable = ReadableDoc {
        lines: vec![TranscriptLine {
            text: "чистый документ".into(),
            original: Some("служебный STT".into()),
            ..Default::default()
        }],
        provenance: Some(Provenance {
            model: "private-meta".into(),
            ..Default::default()
        }),
        ..Default::default()
    };
    assert_eq!(a.copy_text(), "чистый документ");
    a.command(22);
    assert!(matches!(&a.effects[0].1,Task::Copy(s) if s=="чистый документ"));
}
#[test]
fn catalogue_field_types_validate_without_exposing_secrets_or_saving_untouched_defaults() {
    use localvox_light_client::{DeviceChoice, Setting};
    let mut app = App {
        view: View::Settings,
        focus: 1,
        ..Default::default()
    };
    app.settings.settings = vec![
        Setting {
            key: "LOCALVOX_N".into(),
            kind: "number".into(),
            ..Default::default()
        },
        Setting {
            key: "LOCALVOX_B".into(),
            kind: "bool".into(),
            ..Default::default()
        },
        Setting {
            key: "LOCALVOX_C".into(),
            options: Some(vec![DeviceChoice {
                value: "ollama".into(),
                ..Default::default()
            }]),
            ..Default::default()
        },
        Setting {
            key: "LOCALVOX_S".into(),
            kind: "secret".into(),
            set: true,
            value: Some("must-not-display-server-value".into()),
            ..Default::default()
        },
        Setting {
            key: "LOCALVOX_P".into(),
            kind: "path".into(),
            ..Default::default()
        },
    ];
    for (index, bad, good) in [
        (0, "NaN", "7"),
        (1, "maybe", "off"),
        (2, "unknown-provider", "ollama"),
    ] {
        app.selected = index;
        app.command(20);
        app.paste(bad);
        assert!(app.submit_form().is_err());
        let field = &mut app.form.as_mut().unwrap().fields[0];
        field.value = good.into();
        app.submit_form().unwrap();
    }
    app.selected = 3;
    app.command(20);
    assert!(app.form.as_ref().unwrap().fields[0].secret);
    assert!(app.form.as_ref().unwrap().fields[0].value.is_empty());
    app.paste("new-synthetic-secret");
    app.submit_form().unwrap();
    assert!(!app.document_text().contains("new-synthetic-secret"));
    assert!(!app
        .document_text()
        .contains("must-not-display-server-value"));
    app.selected = 4;
    app.command(20);
    app.submit_form().unwrap();
    assert!(!app.settings_edits.contains_key("LOCALVOX_P"));
    app.command(32);
    app.paste("ДА");
    app.submit_form().unwrap();
    assert!(
        matches!(&app.effects.last().unwrap().1,Task::SaveSettings(changes) if changes.len()==4 && changes["LOCALVOX_N"].as_deref()==Some("7"))
    );
}
#[test]
fn settings_are_staged_and_failure_does_not_lose_changed_keys() {
    let mut a = App::default();
    a.settings.settings = vec![
        Setting {
            key: "ONE".into(),
            kind: "number".into(),
            value: Some("10".into()),
            ..Default::default()
        },
        Setting {
            key: "UNTOUCHED".into(),
            ..Default::default()
        },
    ];
    a.command(20);
    a.form.as_mut().unwrap().fields[0].value = "20".into();
    a.submit_form().unwrap();
    assert!(a.effects.is_empty());
    assert_eq!(a.settings_edits.len(), 1);
    a.command(32);
    a.form.as_mut().unwrap().fields[0].value = "ДА".into();
    a.submit_form().unwrap();
    assert!(
        matches!(&a.effects[0].1,Task::SaveSettings(set) if set.len()==1&&set["ONE"].as_deref()==Some("20"))
    );
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Action,
        result: Err("write failure".into()),
    });
    assert_eq!(a.settings_edits["ONE"].as_deref(), Some("20"));
    a.effects.clear();
    a.submit_form().unwrap();
    a.apply(Response {
        id: 2,
        epoch: 0,
        resource: Resource::Action,
        result: Ok(Data::Reply(Reply {
            need_restart: vec!["ONE".into()],
            ..Default::default()
        })),
    });
    assert!(a.settings_edits.is_empty());
    assert!(a.status.contains("ONE"));
    assert!(a.status.contains("перезапуск"));
}
#[test]
fn empty_search_and_invalid_number_do_not_send_requests() {
    let mut a = App::default();
    a.command(3);
    assert!(a.submit_form().is_err());
    assert!(a.effects.is_empty());
    a.form = None;
    a.settings.settings = vec![Setting {
        key: "NUM".into(),
        kind: "number".into(),
        ..Default::default()
    }];
    a.command(20);
    a.form.as_mut().unwrap().fields[0].value = "NaN".into();
    assert!(a.submit_form().is_err());
    assert!(a.effects.is_empty());
}
#[test]
fn route_proposes_without_sending_or_replacing_a_note() {
    let mut a = App::default();
    a.command(5);
    a.form.as_mut().unwrap().fields[1].value = "мой черновик".into();
    a.command(6);
    assert!(matches!(&a.effects[0],(Resource::Route,Task::Route(text)) if text=="мой черновик"));
    a.effects.clear();
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Route,
        result: Ok(Data::Route(vec![RouteHint {
            slot: "идеи".into(),
            reason: None,
        }])),
    });
    assert_eq!(a.route_hints, vec!["идеи"]);
    assert_eq!(a.form.as_ref().unwrap().fields[1].value, "мой черновик");
    assert!(a.effects.is_empty());
}
#[test]
fn late_mutation_ack_cannot_navigate_back_to_the_previous_session() {
    let mut a = App::default();
    a.current = Some("A".into());
    a.command(10);
    a.submit_form().unwrap();
    a.select_session("B".into());
    a.effects.clear();
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Action,
        result: Ok(Data::Reply(Reply {
            session: Some("A".into()),
            ..Default::default()
        })),
    });
    assert_eq!(a.current.as_deref(), Some("B"));
}
#[test]
fn failed_action_keeps_its_draft_when_another_form_is_open() {
    let mut a = App::default();
    a.command(7);
    a.paste("original material");
    a.submit_form().unwrap();
    a.command(5);
    a.paste("new note");
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Action,
        result: Err("lost acknowledgement".into()),
    });
    assert!(matches!(a.form.as_ref().unwrap().kind, FormKind::Note));
    a.form = None;
    a.command(35);
    assert_eq!(
        a.form.as_ref().unwrap().fields[0].value,
        "original material"
    );
}
#[test]
fn seek_parses_timecodes_and_repeating_a_fragment_sends_it_twice() {
    assert_eq!(parse_timecode("01:02:03.5"), Some(3723.5));
    for input in ["NaN", "-1", "1:70", "abc"] {
        assert_eq!(parse_timecode(input), None);
    }
    let mut a = App::default();
    a.current = Some("demo".into());
    a.tab = Tab::Transcript;
    a.transcript = vec![TranscriptLine {
        start_sec: 1.0,
        end_sec: 2.0,
        ..Default::default()
    }];
    a.command(25);
    a.command(25);
    assert_eq!(a.player_effects.len(), 2);
    a.select_session("other".into());
    assert!(matches!(
        a.player_effects.last(),
        Some(crate::player::PlaybackCommand::Stop)
    ));
}
#[test]
fn switching_questions_rejects_the_previous_answer() {
    let mut a = App::default();
    a.view = View::Ask;
    a.focus = 1;
    a.asks = vec![
        AskRecord {
            id: "A".into(),
            ..Default::default()
        },
        AskRecord {
            id: "B".into(),
            ..Default::default()
        },
    ];
    a.enter();
    let previous = a.epoch;
    a.selected = 1;
    a.focus = 1;
    a.enter();
    a.apply(Response {
        id: 1,
        epoch: previous,
        resource: Resource::Ask,
        result: Ok(Data::Ask(AskRecord {
            id: "A".into(),
            answer: Some("old".into()),
            ..Default::default()
        })),
    });
    assert!(a.ask.is_none());
    a.apply(Response {
        id: 2,
        epoch: a.epoch,
        resource: Resource::Ask,
        result: Ok(Data::Ask(AskRecord {
            id: "B".into(),
            answer: Some("right".into()),
            ..Default::default()
        })),
    });
    assert_eq!(a.ask.as_ref().unwrap().id, "B");
    a.apply(Response {
        id: 3,
        epoch: a.epoch,
        resource: Resource::Asks,
        result: Ok(Data::Asks(vec![
            AskRecord {
                id: "new".into(),
                ..Default::default()
            },
            AskRecord {
                id: "A".into(),
                ..Default::default()
            },
            AskRecord {
                id: "B".into(),
                ..Default::default()
            },
        ])),
    });
    assert_eq!(a.asks[a.selected].id, "B");
}
#[test]
fn deleting_a_session_drops_its_document_and_ignores_an_older_list() {
    let mut a = App::default();
    let entries = vec![
        Session {
            name: "A".into(),
            ..Default::default()
        },
        Session {
            name: "B".into(),
            ..Default::default()
        },
    ];
    a.sessions = entries.clone();
    a.current = Some("A".into());
    a.summary.blocks = vec![SummaryBlock {
        text: "deleted document".into(),
        ..Default::default()
    }];
    a.command(17);
    a.form.as_mut().unwrap().fields[0].value = "ДА".into();
    a.submit_form().unwrap();
    a.apply(Response {
        id: 10,
        epoch: 0,
        resource: Resource::Action,
        result: Ok(Data::Reply(Reply {
            deleted: Some("A".into()),
            ..Default::default()
        })),
    });
    assert_eq!(a.current.as_deref(), Some("B"));
    assert!(a.summary.blocks.is_empty());
    a.apply(Response {
        id: 5,
        epoch: 0,
        resource: Resource::Sessions,
        result: Ok(Data::Sessions(entries)),
    });
    assert_eq!(a.sessions.len(), 1);
    assert_eq!(a.sessions[0].name, "B");
}
#[test]
fn untouched_settings_do_not_create_an_override() {
    let mut a = App::default();
    a.settings.settings = vec![Setting {
        key: "DEFAULT".into(),
        kind: "number".into(),
        ..Default::default()
    }];
    a.command(20);
    a.submit_form().unwrap();
    assert!(a.settings_edits.is_empty());
    assert!(a.effects.is_empty());
}
#[test]
fn accepted_import_opens_its_progress_without_inventing_a_finished_document() {
    let mut a = App::default();
    a.command(0);
    a.paste("https://example.org/video");
    a.submit_form().unwrap();
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Action,
        result: Ok(Data::Reply(Reply {
            session: Some("new-session".into()),
            ..Default::default()
        })),
    });
    assert_eq!(a.current.as_deref(), Some("new-session"));
    assert_eq!(a.tab, Tab::Progress);
    assert!(a.summary.blocks.is_empty());
    assert_eq!(a.focus, 2);
}

#[test]
fn search_and_chat_failures_keep_the_unicode_query_for_editing() {
    for (command, resource) in [(3, Resource::Search), (4, Resource::Chat)] {
        let mut a = App::default();
        a.command(command);
        a.paste("Зачем нужен журнал? 🦀");
        a.submit_form().unwrap();
        a.apply(Response {
            id: 1,
            epoch: a.epoch,
            resource,
            result: Err("503 temporary".into()),
        });
        assert_eq!(
            a.form.as_ref().unwrap().fields[0].value,
            "Зачем нужен журнал? 🦀"
        );
        assert!(!a
            .effects
            .iter()
            .any(|(_, task)| matches!(task, Task::CreateAsk(_))));
        a.form = None;
        a.command(command);
        assert_eq!(
            a.form.as_ref().unwrap().fields[0].value,
            "Зачем нужен журнал? 🦀"
        );
    }
}

#[test]
fn rejected_submission_keeps_its_draft_beside_a_new_form() {
    let mut a = App::default();
    a.command(4);
    a.paste("Долгий вопрос");
    a.submit_form().unwrap();
    a.command(0);
    a.paste("https://example.org/new");
    a.submission_rejected(Resource::Chat);
    assert_eq!(
        a.form.as_ref().unwrap().fields[0].value,
        "https://example.org/new"
    );
    assert_eq!(
        a.failed_drafts.back().unwrap().fields[0].value,
        "Долгий вопрос"
    );
    assert!(!a.pending_forms.contains_key(&Resource::Chat));
}

#[test]
fn settings_filter_edits_the_visible_key_and_discard_is_local_and_explicit() {
    let mut a = App::default();
    a.settings.settings = vec![
        Setting {
            key: "FIRST".into(),
            label: "Первый".into(),
            ..Default::default()
        },
        Setting {
            key: "SECOND".into(),
            label: "Модель".into(),
            kind: "number".into(),
            value: Some("10".into()),
            ..Default::default()
        },
    ];
    a.view = View::Settings;
    a.key(KeyEvent::new(KeyCode::Char('/'), KeyModifiers::NONE));
    a.paste("модель");
    a.submit_form().unwrap();
    assert_eq!(a.list_len(), 1);
    assert_eq!(a.selected_setting().unwrap().key, "SECOND");
    a.command(20);
    a.form.as_mut().unwrap().fields[0].value = "20".into();
    a.submit_form().unwrap();
    assert_eq!(a.settings_edits["SECOND"].as_deref(), Some("20"));
    a.command(37);
    assert!(a.submit_form().is_err());
    a.key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
    assert_eq!(a.settings_edits.len(), 1);
    a.command(37);
    a.paste("ДА");
    a.submit_form().unwrap();
    assert!(a.settings_edits.is_empty() && a.effects.is_empty());
}

#[test]
fn returning_to_an_archive_retries_stale_peaks_but_backs_off_errors() {
    let mut a = App::default();
    a.select_session("A".into());
    let old_epoch = a.epoch;
    a.switch(View::Notes);
    a.switch(View::Archive);
    a.effects.clear();
    a.refresh_view();
    assert!(a
        .effects
        .iter()
        .any(|(resource, _)| *resource == Resource::Peaks));
    a.apply(Response {
        id: 1,
        epoch: old_epoch,
        resource: Resource::Peaks,
        result: Ok(Data::Peaks(Peaks::default())),
    });
    assert!(!a.peaks_loaded);
    a.apply(Response {
        id: 2,
        epoch: a.epoch,
        resource: Resource::Peaks,
        result: Err("unavailable".into()),
    });
    a.effects.clear();
    a.refresh_view();
    assert!(!a.effects.iter().any(|(r, _)| *r == Resource::Peaks));
    a.tick(30_001);
    a.effects.clear();
    a.refresh_view();
    assert!(a.effects.iter().any(|(r, _)| *r == Resource::Peaks));
    a.apply(Response {
        id: 3,
        epoch: a.epoch,
        resource: Resource::Peaks,
        result: Ok(Data::Peaks(Peaks::default())),
    });
    a.effects.clear();
    a.refresh_view();
    assert!(!a.effects.iter().any(|(r, _)| *r == Resource::Peaks));
}

#[test]
fn artifact_confirmation_rejects_reads_started_before_its_ack() {
    let mut a = App::default();
    a.current = Some("A".into());
    let old_epoch = a.epoch;
    a.command(16);
    a.apply(Response {
        id: 10,
        epoch: 0,
        resource: Resource::Action,
        result: Ok(Data::Reply(Reply::default())),
    });
    assert!(a.epoch > old_epoch);
    a.apply(Response {
        id: 11,
        epoch: old_epoch,
        resource: Resource::Summary,
        result: Ok(Data::Summary(Summary {
            blocks: vec![SummaryBlock {
                text: "old doubtful answer".into(),
                ..Default::default()
            }],
            ..Default::default()
        })),
    });
    assert!(a.summary.blocks.is_empty());
}

#[test]
fn a_failed_answer_with_an_empty_answer_field_still_shows_its_error_and_input() {
    let mut a = App::default();
    a.view = View::Ask;
    a.expanded = true;
    a.ask = Some(AskRecord {
        status: "failed".into(),
        answer: Some(String::new()),
        error: Some("provider failed".into()),
        input: Some("исходный материал".into()),
        input_name: Some("meeting.txt".into()),
        input_chars: 18,
        ..Default::default()
    });
    let document = a.document_text();
    assert!(
        document.contains("provider failed")
            && document.contains("исходный материал")
            && document.contains("meeting.txt")
    );
    assert!(a.copy_text().is_empty());
}

#[test]
fn a_recording_ack_is_not_confirmation_and_timeout_does_not_invent_a_stop() {
    let mut a = App::default();
    a.record.engine = true;
    a.command(1);
    a.paste("Длинное название встречи");
    a.submit_form().unwrap();
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Control,
        result: Ok(Data::Reply(Reply::default())),
    });
    assert_eq!(a.pending_record, Some(true));
    assert!(!a.record.recording);
    a.effects.clear();
    a.tick(10_001);
    assert!(a.pending_record.is_none());
    assert!(a.status.contains("не получено"));
    assert!(!a.effects.iter().any(|(_, task)| matches!(task, Task::Stop)));
}

#[test]
fn opening_a_journal_block_pauses_tail_following_for_reading() {
    let mut a = App::default();
    a.tab = Tab::Progress;
    a.focus = 2;
    a.journal.entries = vec![localvox_light_client::JournalEntry {
        id: "log".into(),
        run: "r".into(),
        operation: Some("o".into()),
        text: Some("detail".into()),
        ..Default::default()
    }];
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(!a.journal.follow);
    assert!(a.journal.expanded.contains("r/o"));
}
#[test]
fn ask_document_enter_opens_source_and_returning_reads_the_same_saved_request() {
    let mut a = App::default();
    a.view = View::Ask;
    a.focus = 2;
    a.ask_id = Some("saved".into());
    a.ask = Some(AskRecord {
        id: "saved".into(),
        input: Some("Материал".into()),
        input_name: Some("встреча.txt".into()),
        ..Default::default()
    });
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert!(a.expanded && a.document_text().contains("Материал"));
    assert_eq!(a.document_title(), "встреча.txt");
    a.switch(View::Archive);
    a.effects.clear();
    a.switch(View::Ask);
    assert!(a
        .effects
        .iter()
        .any(|(_, task)| matches!(task,Task::Read(Resource::Ask,id) if id=="saved")));
}

#[test]
fn clipboard_failure_preserves_the_clean_document_and_has_a_visible_reason() {
    let mut a = App::default();
    a.summary.blocks = vec![SummaryBlock {
        text: "Текст документа".into(),
        ..Default::default()
    }];
    a.command(22);
    assert!(matches!(&a.effects[0].1,Task::Copy(text) if text=="Текст документа"));
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Action,
        result: Err("Буфер обмена недоступен".into()),
    });
    assert_eq!(a.copy_text(), "Текст документа");
    assert!(a.status.contains("Буфер обмена недоступен"));
}
#[test]
fn search_pending_empty_and_error_are_distinct_and_enter_selects_the_timecoded_line() {
    let mut a = App::default();
    a.command(3);
    a.paste("архитектура");
    a.submit_form().unwrap();
    assert!(a.document_text().contains("Ищем:") && !a.document_text().contains("не найдено"));
    a.apply(Response {
        id: 1,
        epoch: a.epoch,
        resource: Resource::Search,
        result: Ok(Data::Search(vec![])),
    });
    assert!(a.document_text().contains("Совпадений не найдено"));
    a.apply(Response {
        id: 2,
        epoch: a.epoch,
        resource: Resource::Search,
        result: Err("поиск недоступен".into()),
    });
    assert!(
        a.document_text().contains("Поиск недоступен")
            && !a.document_text().contains("Совпадений не найдено")
    );
    a.form = None;
    a.hits = vec![Hit {
        session: "found".into(),
        start_sec: Some(12.0),
        end_sec: Some(20.0),
        ..Default::default()
    }];
    a.focus = 1;
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(a.current.as_deref(), Some("found"));
    assert!(
        a.player_effects
            .iter()
            .all(|c| !matches!(c, crate::player::PlaybackCommand::Load { .. })),
        "opening a result does not auto-play"
    );
    a.apply(Response {
        id: 3,
        epoch: a.epoch,
        resource: Resource::Transcript,
        result: Ok(Data::Transcript(vec![
            TranscriptLine {
                start_sec: 0.0,
                end_sec: 10.0,
                ..Default::default()
            },
            TranscriptLine {
                start_sec: 10.0,
                end_sec: 20.0,
                ..Default::default()
            },
        ])),
    });
    assert_eq!(a.item, 1);
    assert!(a.follow_selection);
}
#[test]
fn a_pending_route_cannot_be_relabelled_as_a_suggestion_for_new_text() {
    let mut a = App::default();
    a.command(5);
    a.form.as_mut().unwrap().fields[1].value = "Старый текст".into();
    a.command(6);
    a.form.as_mut().unwrap().fields[1].value = "Новая заметка".into();
    a.command(6);
    assert_eq!(a.route_text.as_deref(), Some("Старый текст"));
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Route,
        result: Ok(Data::Route(vec![RouteHint {
            slot: "старое".into(),
            reason: None,
        }])),
    });
    assert!(a.route_hints.is_empty());
    assert_eq!(a.form.as_ref().unwrap().fields[1].value, "Новая заметка");
    assert_eq!(a.effects.len(), 1);
    assert!(!a
        .effects
        .iter()
        .any(|(_, task)| matches!(task, Task::Note(..))));
}
#[test]
fn failed_note_delivery_preserves_its_text_and_explicit_slot() {
    let mut a = App::default();
    a.command(5);
    a.form.as_mut().unwrap().fields[0].value = "идеи".into();
    a.form.as_mut().unwrap().fields[1].value = "Черновик".into();
    a.submit_form().unwrap();
    a.effects.clear();
    a.apply(Response {
        id: 1,
        epoch: 0,
        resource: Resource::Action,
        result: Err("назначение недоступно".into()),
    });
    assert_eq!(a.form.as_ref().unwrap().fields[0].value, "идеи");
    assert_eq!(a.form.as_ref().unwrap().fields[1].value, "Черновик");
    assert!(a.effects.is_empty());
}

#[test]
fn queue_keeps_server_order_and_retries_the_selected_session_with_explicit_scope() {
    let mut a = App::default();
    a.view = View::Queue;
    let queue = vec![
        QueueItem {
            session: "z".into(),
            state: "running".into(),
            ..Default::default()
        },
        QueueItem {
            session: "a".into(),
            state: "running".into(),
            ..Default::default()
        },
        QueueItem {
            session: "b".into(),
            state: "failed".into(),
            stuck: true,
            last_error: Some("нет модели".into()),
            ..Default::default()
        },
    ];
    a.apply(Response {
        id: 1,
        epoch: a.epoch,
        resource: Resource::Queue,
        result: Ok(Data::Queue(queue)),
    });
    assert_eq!(
        a.queue
            .iter()
            .map(|q| q.session.as_str())
            .collect::<Vec<_>>(),
        vec!["z", "a", "b"]
    );
    a.selected = 2;
    a.focus = 1;
    a.key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    assert_eq!(a.current.as_deref(), Some("b"));
    assert_eq!(a.tab, Tab::Progress);
    for (command, scope) in [(13, Scope::Summary), (14, Scope::Text), (15, Scope::All)] {
        a.pending_actions.clear();
        a.effects.clear();
        a.command(command);
        assert!(!a
            .effects
            .iter()
            .any(|(_, task)| matches!(task, Task::Recook(..))));
        a.paste("ДА");
        a.submit_form().unwrap();
        assert!(
            matches!(&a.effects[0].1,Task::Recook(session,actual) if session=="b"&&*actual==scope)
        );
    }
}
