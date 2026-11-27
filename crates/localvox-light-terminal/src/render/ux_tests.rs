use super::playback::player_line;
use super::*;

#[test]
fn record_state_is_in_the_header_and_shortcuts_are_grouped_by_scope() {
    for width in [35, 80, 140] {
        let mut app = App::default();
        app.record.engine = true;
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        for (recording, pending, unknown, expected, shortcut) in [
            (false, None, false, "готов к записи", Some("F9")),
            (false, Some(true), false, "запускаем запись", Some("F8")),
            (true, None, false, "ИДЁТ ЗАПИСЬ", Some("F8")),
            (true, Some(false), false, "останавливаем запись", None),
            (false, None, true, "состояние записи", None),
        ] {
            app.record.recording = recording;
            app.pending_record = pending;
            if unknown {
                app.errors.insert(Resource::Record, "offline".into());
            }
            for focus in [1, 3] {
                app.focus = focus;
                terminal.draw(|f| draw(f, &app)).unwrap();
                let rows: Vec<String> = terminal
                    .backend()
                    .buffer()
                    .content
                    .chunks(usize::from(width))
                    .map(|row| row.iter().map(|cell| cell.symbol()).collect())
                    .collect();
                assert!(rows[0].contains(expected));
                assert!(rows[1].chars().all(|c| "┌─┐".contains(c)), "{}", rows[1]);
                let screen = rows.join("\n");
                assert_eq!(screen.matches("F6").count(), 1);
                assert!(rows[3].contains(if focus == 3 {
                    "F6 к документу"
                } else {
                    "F6 стрелки плееру"
                }));
                for key in ["F8", "F9"] {
                    assert_eq!(
                        screen.matches(key).count(),
                        usize::from(shortcut == Some(key))
                    );
                    assert_eq!(rows[23].contains(key), shortcut == Some(key));
                }
                if focus == 1 && width >= 80 {
                    assert!(rows[22].contains("j/k") && rows[22].contains("/ поиск"));
                    assert!(rows[23].contains("F2/: действия"));
                }
            }
        }
    }
}

#[test]
fn palette_shows_the_visible_range_and_both_scroll_boundaries_on_short_screens() {
    for height in [12, 36] {
        let mut app = App::default();
        app.viewport = (100, height);
        app.palette = true;
        for selected in [0, COMMANDS.len() / 2, COMMANDS.len() - 1] {
            app.palette_index = selected;
            let mut terminal = Terminal::new(TestBackend::new(100, height)).unwrap();
            terminal.draw(|frame| draw(frame, &app)).unwrap();
            let buffer = terminal.backend().buffer();
            let text = buffer
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            let (count, offset) = app.palette_window(height);
            assert!(text.contains(COMMANDS[selected].1));
            assert!(text.contains(&format!(
                "{}–{} из {}",
                offset + 1,
                offset + count,
                COMMANDS.len()
            )));
            assert_eq!(text.contains("ещё ↓"), offset + count < COMMANDS.len());
            assert_eq!(text.contains("↑ ещё"), offset > 0);
            if selected + 1 == COMMANDS.len() {
                assert!(text.contains("конец"));
            }
        }
    }
}

#[test]
fn skipped_summary_has_its_reason_and_the_full_recook_command_in_the_document() {
    let mut app = App::default();
    app.current = Some("short".into());
    app.focus = 2;
    app.summary.available = Some(false);
    app.summary.status = Some("skipped".into());
    app.summary.reason = Some("Модель не создала содержательной сводки по короткому тексту".into());
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal.draw(|frame| draw(frame, &app)).unwrap();
    let text = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(text.contains("Сводка пропущена") && text.contains("Модель не создала"));
    assert!(text.contains("r — переварить запись с аудио"));
    assert!(!text.contains("ещё не создана"));
}
use crate::io::Response;
use localvox_light_client::{QueueItem, Setting};
use ratatui::{backend::TestBackend, Terminal};

fn screen(case: &str, width: u16, height: u16, light: bool) {
    let mut app = App {
        viewport: (width, height),
        focus: 2,
        preferences: crate::app::UiPreferences {
            light,
            ..Default::default()
        },
        ..Default::default()
    };
    let fixture: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/fixtures/workspace.json")).unwrap();
    app.sessions = serde_json::from_value(fixture["sessions"].clone()).unwrap();
    app.current = Some(app.sessions[0].name.clone());
    app.versions = serde_json::from_value(fixture["versions"].clone()).unwrap();
    app.progress = serde_json::from_value(fixture["progress"].clone()).unwrap();
    app.wall_ms = chrono::DateTime::parse_from_rfc3339("2026-11-01T20:40:00+03:00")
        .unwrap()
        .timestamp_millis();
    match case {
        "partial" => {
            app.tab = Tab::Transcript;
            app.transcript =
                serde_json::from_value(fixture["transcript"]["lines"].clone()).unwrap();
            app.partial = Some(localvox_light_client::PartialInfo {
                generation: "fixture".into(),
                revision: 2,
                snapshot: Some("snapshot2".into()),
                sealed_end_sec: 600.0,
                stt_end_sec: 585.0,
                text_end_sec: 570.0,
                audio_end_sec: 585.0,
            });
        }
        "preset" => {
            app.view = View::Settings;
            app.nav = 4;
            app.settings.settings = vec![Setting {
                key: "LOCALVOX_LLM_MODEL".into(),
                group: "LLM".into(),
                label: "Модель".into(),
                kind: "text".into(),
                default_value: Some("qwen3.5:9b".into()),
                applies: "restart".into(),
                hint: "Модель обработки текста и сводки".into(),
                ..Default::default()
            }];
        }
        "settings" | "choice" => {
            app.view = View::Settings;
            app.nav = 4;
            app.settings.settings=vec![
                Setting{key:"LOCALVOX_LIGHT_AUTOCOOK_SUMMARY".into(),group:"После распознавания".into(),label:"Сводка · summary".into(),kind:"bool".into(),value:Some("off".into()),hint:"По умолчанию включена. Распознавание обязательно; очистители запускаются вручную.".into(),applies:"live".into(),set:true,..Default::default()},
                Setting{key:"LOCALVOX_SUMMARY_PROVIDER".into(),label:"Сводку делает".into(),value:Some("ollama".into()),..Default::default()},
                Setting{key:"LOCALVOX_LLM_MODEL".into(),label:"Модель".into(),value:Some("qwen3.5:9b".into()),..Default::default()},
                Setting{key:"LOCALVOX_API_TOKEN".into(),kind:"secret".into(),set:true,..Default::default()},
            ];
            if case == "choice" {
                app.command(crate::app::Action::EditSetting);
            }
        }
        "console" | "console-expanded" | "notification" => {
            app.queue = vec![QueueItem {
                session: app.current.clone().unwrap(),
                phase: Some("summary".into()),
                state: "running".into(),
                ..Default::default()
            }];
            for id in 1..=13 {
                app.wall_ms += 3000;
                app.apply(Response {
                    id,
                    epoch: app.epoch,
                    resource: Resource::Summary,
                    result: Err("HTTP 503: локальный провайдер временно недоступен".into()),
                });
            }
            if case != "notification" {
                app.console.toggle();
                app.console.entries[0].expanded = case == "console-expanded";
                assert_eq!(app.console.unread(), 0);
            }
        }
        "seek" => {
            app.peaks.duration_sec = 53.6;
            app.command(crate::app::Action::Seek);
        }
        "missing-summary" => {
            app.summary.available = Some(false);
        }
        "metadata" => {
            app.tab = Tab::Progress;
            app.metadata_open = true;
        }
        _ => unreachable!(),
    }
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw(f, &app)).unwrap();
    let buffer = terminal.backend().buffer();
    let text = (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    if case == "settings" {
        assert!(
            text.contains("Выключено")
                && text.contains("пояснение")
                && !text.contains("LOCALVOX_API_TOKEN")
        );
    }
    if case == "console" {
        assert!(text.contains("+12 повторов") && text.contains("фаза summary"));
    }
    if case == "notification" {
        assert!(text.lines().next().unwrap().contains("! 1"));
        assert!(!text.contains("HTTP 503"));
    }
    if case == "missing-summary" {
        assert!(text.contains("Сводка ещё не создана") && !text.contains("HTTP 400"));
    }
    insta::assert_debug_snapshot!(format!("ux_{case}_{width}_{height}_light{light}"), buffer);
}
#[test]
fn settings_dark() {
    screen("settings", 120, 36, false);
}
#[test]
fn partial_document() {
    screen("partial", 120, 36, false);
}
#[test]
fn settings_light() {
    screen("settings", 80, 24, true);
}
#[test]
fn settings_preset() {
    screen("preset", 80, 24, false);
}
#[test]
fn setting_choice() {
    screen("choice", 80, 24, false);
}
#[test]
fn console_folded() {
    screen("console", 80, 24, false);
}
#[test]
fn console_expanded() {
    screen("console-expanded", 120, 36, true);
}
#[test]
fn seek_numeric() {
    screen("seek", 80, 24, true);
}
#[test]
fn missing_summary() {
    screen("missing-summary", 120, 36, false);
}
#[test]
fn metadata_expanded() {
    screen("metadata", 120, 36, false);
}

#[test]
fn notification_visible() {
    screen("notification", 80, 24, false);
}

#[test]
fn play_pause_keeps_the_waveform_in_the_same_columns() {
    for ascii in [false, true] {
        let mut app = App {
            preferences: crate::app::UiPreferences {
                ascii,
                ..Default::default()
            },
            current: Some("demo".into()),
            ..Default::default()
        };
        app.playback = crate::player::PlaybackState {
            session: app.current.clone(),
            duration: Some(120.),
            position: 15.,
            ..Default::default()
        };
        let text = |a: &App| {
            player_line(a, Theme::for_app(a), 80)
                .spans
                .iter()
                .map(|s| s.content.as_ref())
                .collect::<String>()
        };
        let playing = text(&app);
        app.playback.paused = true;
        let paused = text(&app);
        assert!(playing.contains('['));
        assert_eq!(
            playing.chars().position(|c| c == '['),
            paused.chars().position(|c| c == '[')
        );
        assert!(!playing.contains("играет") && !paused.contains("пауза"));
        app.playback.duration = Some(7200.);
        app.playback.position = 5999.;
        let before = text(&app);
        app.playback.position = 6000.;
        assert_eq!(
            before.chars().position(|c| c == '['),
            text(&app).chars().position(|c| c == '[')
        );
    }
}

#[test]
fn no_selected_audio_does_not_claim_to_be_loading_a_waveform() {
    let app = App::default();
    let text: String = player_line(&app, Theme::for_app(&app), 120)
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    assert!(text.contains("Выберите запись для прослушивания"), "{text}");
    assert!(!text.contains("загружается") && !text.contains("00:00"));
    assert!(app.effects.is_empty() && app.player_effects.is_empty());
}

#[test]
fn selected_audio_still_distinguishes_pending_failed_and_empty_waveforms() {
    let mut app = App {
        current: Some("demo".into()),
        ..Default::default()
    };
    let text = |a: &App| -> String {
        player_line(a, Theme::for_app(a), 120)
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect()
    };
    assert!(text(&app).contains("волна загружается"));
    app.errors
        .insert(Resource::Peaks, "synthetic read error".into());
    assert!(text(&app).contains("волна: ошибка"));
    app.errors.remove(&Resource::Peaks);
    app.last_request.insert(Resource::Peaks, 1);
    assert!(text(&app).contains("волны пока нет"));
}
