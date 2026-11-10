use super::*;

#[test]
fn growing_partial_keeps_anchor_scroll_copy_and_playback_and_rejects_stale_or_late_prefix() {
    let mut app = App {
        current: Some("live".into()),
        tab: Tab::Transcript,
        focus: 2,
        ..Default::default()
    };
    let info = |revision| PartialInfo {
        generation: "g1".into(),
        revision,
        snapshot: Some(format!("s{revision}")),
        audio_end_sec: revision as f64 * 20.0,
        text_end_sec: revision as f64 * 20.0,
        ..Default::default()
    };
    let lines = |count| {
        (0..count)
            .map(|i| TranscriptLine {
                id: Some(format!("g1/0/{i}")),
                source_id: Some(0),
                start_sec: i as f64 * 10.0,
                end_sec: (i + 1) as f64 * 10.0,
                text: format!("реплика {i}"),
                ..Default::default()
            })
            .collect::<Vec<_>>()
    };
    response(
        &mut app,
        1,
        Resource::Transcript,
        Ok(Data::TranscriptSnapshot(TranscriptDoc {
            lines: lines(2),
            partial: Some(info(1)),
        })),
    );
    app.item = 1;
    app.scroll = 7;
    app.follow_selection = false;
    app.playback = crate::player::PlaybackState {
        session: Some("live".into()),
        snapshot: Some("s1".into()),
        position: 6.0,
        duration: Some(20.0),
        paused: false,
        idle: false,
        ..Default::default()
    };
    app.effects.clear();
    app.player_effects.clear();
    response(
        &mut app,
        2,
        Resource::Transcript,
        Ok(Data::TranscriptSnapshot(TranscriptDoc {
            lines: lines(3),
            partial: Some(info(2)),
        })),
    );
    assert_eq!((app.item, app.scroll, app.playback.position), (1, 7, 6.0));
    assert!(!app.playback.paused);
    assert!(app.player_effects.is_empty());
    response(
        &mut app,
        3,
        Resource::Transcript,
        Ok(Data::TranscriptSnapshot(TranscriptDoc {
            lines: lines(1),
            partial: Some(info(1)),
        })),
    );
    assert_eq!(app.transcript.len(), 3);
    let mut foreign = info(3);
    foreign.generation = "obsolete generation".into();
    response(
        &mut app,
        4,
        Resource::Transcript,
        Ok(Data::TranscriptSnapshot(TranscriptDoc {
            lines: vec![],
            partial: Some(foreign),
        })),
    );
    assert_eq!(app.transcript.len(), 3);
    response(
        &mut app,
        5,
        Resource::Readable,
        Ok(Data::Readable(ReadableDoc {
            from_transcript: true,
            lines: lines(3),
            partial: Some(info(2)),
            ..Default::default()
        })),
    );
    app.tab = Tab::Readable;
    key(&mut app, KeyCode::Char('y'));
    assert!(app
        .effects
        .iter()
        .any(|(_, task)| matches!(task,Task::Copy(text) if text=="реплика 0 реплика 1 реплика 2")));
    response(
        &mut app,
        6,
        Resource::Action,
        Ok(Data::Reply(Reply {
            msg: Some("Документ скопирован".into()),
            ..Default::default()
        })),
    );
    app.sessions = vec![Session {
        name: "live".into(),
        recording: true,
        job: Some("waiting_input".into()),
        ..Default::default()
    }];
    key(&mut app, KeyCode::Char('X'));
    assert!(app
        .effects
        .iter()
        .any(|(_, task)| matches!(task,Task::Cancel(name) if name=="live")));
    assert!(!app
        .effects
        .iter()
        .any(|(_, task)| matches!(task, Task::Stop)));
    response(
        &mut app,
        6,
        Resource::Transcript,
        Ok(Data::TranscriptSnapshot(TranscriptDoc {
            lines: lines(3),
            partial: None,
        })),
    );
    response(
        &mut app,
        7,
        Resource::Transcript,
        Ok(Data::TranscriptSnapshot(TranscriptDoc {
            lines: lines(2),
            partial: Some(info(2)),
        })),
    );
    assert!(app.document_final);
    assert_eq!(app.transcript.len(), 3);
}

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
}

#[test]
fn visible_presets_do_not_create_overrides_and_llm_servers_are_staged() {
    let mut app = App::default();
    app.view = View::Settings;
    app.settings.settings = vec![
        Setting {
            key: "LOCALVOX_LLM_MODEL".into(),
            default_value: Some("qwen3.5:9b".into()),
            ..Default::default()
        },
        Setting {
            key: "LOCALVOX_LLM_BASE_URL".into(),
            default_value: Some("http://localhost:11434/v1".into()),
            ..Default::default()
        },
        Setting {
            key: "LOCALVOX_LLM_SERVER_URLS".into(),
            kind: "servers".into(),
            default_value: Some(r#"["http://localhost:11434/v1"]"#.into()),
            ..Default::default()
        },
    ];
    assert_eq!(app.visible_settings().len(), 2);
    assert_eq!(
        app.setting_value(app.selected_setting().unwrap(), true),
        "qwen3.5:9b"
    );
    app.command(Action::EditSetting);
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "qwen3.5:9b");
    key(&mut app, KeyCode::Enter);
    assert!(app.settings_edits.is_empty());
    app.command(Action::AddLlmServer);
    app.paste("https://models.example/v1/");
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.llm_servers(),
        vec!["http://localhost:11434/v1", "https://models.example/v1"]
    );
    assert!(app.effects.is_empty());
    app.command(Action::AddLlmServer);
    app.paste("https://user:password@host/v1");
    key(&mut app, KeyCode::Enter);
    assert!(app.form.as_ref().unwrap().error.is_some());
    assert_eq!(app.llm_servers().len(), 2);
}

#[test]
fn adding_a_server_from_palette_waits_for_the_server_list_before_editing() {
    let mut app = App::default();
    app.command(Action::AddLlmServer);
    assert_eq!(app.view, View::Settings);
    assert!(app.form.is_none() && app.llm_server_pending);
    response(
        &mut app,
        1,
        Resource::Settings,
        Ok(Data::Settings(SettingsDoc {
            settings: vec![Setting {
                key: "LOCALVOX_LLM_SERVER_URLS".into(),
                kind: "servers".into(),
                value: Some(r#"["https://saved.example/v1"]"#.into()),
                ..Default::default()
            }],
            ..Default::default()
        })),
    );
    assert!(matches!(
        app.form.as_ref().unwrap().kind,
        FormKind::LlmServer
    ));
    assert_eq!(app.llm_servers(), vec!["https://saved.example/v1"]);
}

#[test]
fn f6_assigns_arrows_to_audio_then_returns_them_to_document_controls() {
    let mut app = App::default();
    app.current = Some("demo".into());
    app.focus = 2;
    app.playback = crate::player::PlaybackState {
        session: app.current.clone(),
        position: 20.,
        duration: Some(120.),
        ..Default::default()
    };
    key(&mut app, KeyCode::F(6));
    assert_eq!(app.focus, 3);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.player_effects.len(), 1);
    assert_eq!(app.tab, Tab::Summary);
    key(&mut app, KeyCode::F(6));
    assert_eq!(app.focus, 2);
    key(&mut app, KeyCode::Right);
    assert_eq!(app.tab, Tab::Summary);
    assert!(app.expanded);
    assert_eq!(app.player_effects.len(), 1);
}
fn response(app: &mut App, id: u64, resource: Resource, result: Result<Data, String>) {
    app.apply(Response {
        id,
        epoch: app.epoch,
        resource,
        result,
    });
}

#[test]
fn progress_with_unreadable_queue_logs_the_cause_until_recovery() {
    let mut app = App::default();
    app.current = Some("session".into());
    let failed = Progress {
        queue_error: Some("permission denied".into()),
        ..Default::default()
    };
    response(
        &mut app,
        1,
        Resource::Progress,
        Ok(Data::Progress(failed.clone())),
    );
    assert_eq!(app.console.unread(), 1);
    app.console.toggle();
    app.console.toggle();
    response(
        &mut app,
        2,
        Resource::Progress,
        Ok(Data::Progress(failed.clone())),
    );
    assert_eq!(app.console.unread(), 0);
    assert_eq!(app.console.entries.len(), 1);
    assert_eq!(
        app.console.entries[0].context.session.as_deref(),
        Some("session")
    );
    response(
        &mut app,
        3,
        Resource::Progress,
        Ok(Data::Progress(Progress::default())),
    );
    response(&mut app, 4, Resource::Progress, Ok(Data::Progress(failed)));
    assert_eq!(app.console.unread(), 1);
    assert_eq!(app.console.entries.len(), 2);
}

#[test]
fn opening_and_accepting_a_legacy_setting_keeps_its_exact_value() {
    for value in ["true", "0", "legacy-value"] {
        let mut app = App::default();
        app.view = View::Settings;
        app.settings.settings = vec![Setting {
            key: "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY".into(),
            kind: "bool".into(),
            value: Some(value.into()),
            ..Default::default()
        }];
        app.command(Action::EditSetting);
        key(&mut app, KeyCode::Enter);
        assert!(
            app.settings_edits.is_empty(),
            "unintended change for {value}"
        );
        assert!(app.effects.is_empty());
    }
}

#[test]
fn waveform_retries_an_early_empty_result_and_invalidates_when_audio_appears() {
    let mut app = App::default();
    app.current = Some("import".into());
    app.sessions.push(Session {
        name: "import".into(),
        duration_sec: 0.0,
        ..Default::default()
    });
    response(
        &mut app,
        1,
        Resource::Peaks,
        Ok(Data::Peaks(Peaks::default())),
    );
    assert!(!app.peaks_loaded);
    app.effects.clear();
    app.refresh_view();
    assert!(!app.effects.iter().any(|(r, _)| *r == Resource::Peaks));
    response(
        &mut app,
        2,
        Resource::Sessions,
        Ok(Data::Sessions(vec![Session {
            name: "import".into(),
            duration_sec: 53.6,
            ..Default::default()
        }])),
    );
    app.effects.clear();
    app.refresh_view();
    assert!(app.effects.iter().any(|(r, _)| *r == Resource::Peaks));
    response(
        &mut app,
        3,
        Resource::Peaks,
        Ok(Data::Peaks(Peaks {
            snapshot: None,
            duration_sec: 53.6,
            peaks: vec![0.2; 160],
        })),
    );
    assert!(app.peaks_loaded && app.peaks.peaks.len() == 160);
    response(
        &mut app,
        4,
        Resource::Peaks,
        Err("temporary failure".into()),
    );
    let unchanged = app.sessions.clone();
    response(
        &mut app,
        5,
        Resource::Sessions,
        Ok(Data::Sessions(unchanged)),
    );
    assert!(
        app.peaks_retry_at > app.now_ms,
        "unchanged metadata must preserve backoff"
    );
}
#[test]
fn metadata_is_explicit_and_control_y_copies_the_entire_plain_text() {
    let mut app = App::default();
    app.select_session("one".into());
    app.tab = Tab::Readable;
    let full = (0..500)
        .map(|n| format!("Фрагмент {n}"))
        .collect::<Vec<_>>()
        .join(" ");
    app.readable = ReadableDoc {
        from_transcript: true,
        lines: vec![TranscriptLine {
            text: full.clone(),
            ..Default::default()
        }],
        ..Default::default()
    };
    app.effects.clear();
    app.key(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
    assert!(matches!(&app.effects[0].1,Task::Copy(text) if text==&full));
    key(&mut app, KeyCode::Char('I'));
    assert_eq!(app.tab, Tab::Progress);
    assert!(app.metadata_open);
    app.select_session("two".into());
    assert!(!app.metadata_open);
}
#[test]
fn time_picker_rejects_text_and_out_of_range_positions_without_dispatching() {
    let mut app = App::default();
    app.current = Some("audio".into());
    app.peaks.duration_sec = 90.0;
    app.command(Action::Seek);
    assert_eq!(app.form.as_ref().unwrap().fields.len(), 3);
    key(&mut app, KeyCode::Char('q'));
    assert!(!app.quit);
    assert_eq!(app.form.as_ref().unwrap().fields[0].value, "0");
    app.paste("99");
    assert!(app.form.as_ref().unwrap().error.is_some());
    app.paste("1");
    key(&mut app, KeyCode::Tab);
    app.paste("59");
    key(&mut app, KeyCode::Enter);
    assert!(app.player_effects.is_empty());
    assert!(app.form.as_ref().unwrap().error.is_some());
    app.paste("12");
    key(&mut app, KeyCode::Enter);
    assert!(app.form.is_none());
    assert!(
        matches!(app.player_effects.last(),Some(crate::player::PlaybackCommand::Seek{position,..}) if *position==72.0)
    );
}
#[test]
fn settings_choose_values_without_turning_hints_into_defaults() {
    let mut app = App::default();
    app.view = View::Settings;
    app.settings.settings = vec![
        Setting {
            key: "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY".into(),
            kind: "bool".into(),
            hint: "Описание, не значение".into(),
            ..Default::default()
        },
        Setting {
            key: "LOCALVOX_API_TOKEN".into(),
            kind: "secret".into(),
            value: Some("never-show".into()),
            set: true,
            ..Default::default()
        },
    ];
    assert_eq!(app.visible_settings().len(), 1);
    app.command(Action::EditSetting);
    assert!(matches!(
        app.form.as_ref().unwrap().kind,
        FormKind::SettingChoice { .. }
    ));
    app.paste("maybe");
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Down);
    key(&mut app, KeyCode::Enter);
    assert_eq!(
        app.settings_edits["LOCALVOX_LIGHT_AUTOCOOK_SUMMARY"].as_deref(),
        Some("off")
    );
    assert!(app.effects.is_empty());
    key(&mut app, KeyCode::Char('A'));
    assert_eq!(app.visible_settings().len(), 2);
    app.selected = 1;
    assert_eq!(
        app.setting_value(app.selected_setting().unwrap(), true),
        "задано · скрыто"
    );
}
#[test]
fn console_folds_repeated_polling_errors_acknowledges_and_preserves_an_open_form() {
    let mut app = App::default();
    app.current = Some("session".into());
    app.queue = vec![QueueItem {
        session: "session".into(),
        phase: Some("summary".into()),
        ..Default::default()
    }];
    response(&mut app, 1, Resource::Summary, Err("offline".into()));
    assert_eq!(app.console.unread(), 1);
    app.command(Action::ImportUrl);
    key(&mut app, KeyCode::F(12));
    assert!(app.console.open);
    assert_eq!(app.console.unread(), 0);
    assert!(app.form.is_some());
    key(&mut app, KeyCode::F(12));
    response(&mut app, 2, Resource::Summary, Err("offline".into()));
    assert_eq!(app.console.entries.len(), 1);
    assert_eq!(app.console.entries[0].repeats, 2);
    assert_eq!(app.console.unread(), 0);
    assert_eq!(
        app.console.entries[0].context.phase.as_deref(),
        Some("summary")
    );
    key(&mut app, KeyCode::F(12));
    key(&mut app, KeyCode::Enter);
    assert!(app.console.entries[0].expanded);
    key(&mut app, KeyCode::Esc);
    assert!(matches!(app.form.as_ref().unwrap().kind, FormKind::Import));
    response(
        &mut app,
        3,
        Resource::Summary,
        Ok(Data::Summary(Summary::default())),
    );
    response(&mut app, 4, Resource::Summary, Err("offline".into()));
    assert_eq!(app.console.unread(), 1);
}
#[test]
fn late_action_failure_keeps_the_original_session_and_operation_in_the_console() {
    let mut app = App::default();
    app.view = View::Queue;
    app.queue = vec![QueueItem {
        session: "original".into(),
        state: "running".into(),
        phase: Some("summary".into()),
        ..Default::default()
    }];
    app.command(Action::CancelProcessing);
    let epoch = app.epoch;
    app.select_session("other".into());
    app.apply(Response {
        id: 1,
        epoch,
        resource: Resource::Action,
        result: Err("not acknowledged".into()),
    });
    let entry = app.console.entries.back().unwrap();
    assert_eq!(entry.context.session.as_deref(), Some("original"));
    assert_eq!(entry.context.phase.as_deref(), Some("summary"));
    assert_eq!(entry.context.operation, "Отмена обработки");
    assert!(!app.status.contains("not acknowledged"));
}
#[test]
fn import_shortcut_pastes_one_url_without_running_or_replaying_it() {
    let mut app = App::default();
    key(&mut app, KeyCode::Char('i'));
    app.paste("https://www.youtube.com/shorts/GKpN_NksATI");
    assert!(app.effects.is_empty());
    key(&mut app, KeyCode::Enter);
    assert!(matches!(&app.effects[0].1,Task::Ingest(url) if url.ends_with("GKpN_NksATI")));
    assert!(app.form.is_none());
    assert_eq!(app.effects.len(), 1);
}

#[test]
fn model_failure_is_logged_with_the_ask_stage_and_not_dumped_into_the_document() {
    let mut app = App::default();
    app.ask_id = Some("question".into());
    app.view = View::Ask;
    response(
        &mut app,
        1,
        Resource::Ask,
        Ok(Data::Ask(AskRecord {
            id: "question".into(),
            status: "failed".into(),
            error: Some("provider failed".into()),
            ..Default::default()
        })),
    );
    assert!(app.console.text().contains("provider failed"));
    assert_eq!(app.console.entries[0].context.phase.as_deref(), Some("ask"));
    assert!(!app.document_text().contains("provider failed"));
    assert!(app.document_text().contains("Ответ не получен"));
}
