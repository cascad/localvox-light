use super::*;

fn key(app: &mut App, code: KeyCode) {
    app.key(KeyEvent::new(code, KeyModifiers::NONE));
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
