use super::*;
use crate::io::Response;
use localvox_light_client::{QueueItem, Setting};
use ratatui::{backend::TestBackend, Terminal};

fn screen(case: &str, width: u16, height: u16, light: bool) {
    let mut app = App {
        viewport: (width, height),
        focus: 2,
        light,
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
            ascii,
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
