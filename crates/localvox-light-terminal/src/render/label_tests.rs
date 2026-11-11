use super::*;
use localvox_light_client::{Session, Setting, TranscriptLine};
use ratatui::{
    backend::TestBackend,
    buffer::{Buffer, Cell},
    Terminal,
};

fn position(buffer: &Buffer, text: &str, min_x: u16) -> (u16, u16) {
    let chars: Vec<_> = text.chars().map(|c| c.to_string()).collect();
    for y in 0..buffer.area.height {
        for x in min_x..buffer.area.width.saturating_sub(chars.len() as u16) + 1 {
            if chars
                .iter()
                .enumerate()
                .all(|(i, c)| buffer[(x + i as u16, y)].symbol() == c)
            {
                return (x, y);
            }
        }
    }
    panic!("word {text:?} must be visible");
}
fn word<'a>(buffer: &'a Buffer, text: &str, min_x: u16) -> &'a Cell {
    &buffer[position(buffer, text, min_x)]
}
fn screen(app: &App) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
    terminal.draw(|f| draw(f, app)).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn rgb_labels_and_values_keep_readable_contrast_on_background_and_selection() {
    fn luminance(color: Color) -> f64 {
        let Color::Rgb(r, g, b) = color else {
            panic!("RGB theme expected");
        };
        [r, g, b]
            .into_iter()
            .zip([0.2126, 0.7152, 0.0722])
            .map(|(channel, weight)| {
                let c = f64::from(channel) / 255.0;
                (if c <= 0.04045 {
                    c / 12.92
                } else {
                    ((c + 0.055) / 1.055).powf(2.4)
                }) * weight
            })
            .sum()
    }
    for light in [false, true] {
        let t = Theme::for_app(&App {
            light,
            ..Default::default()
        });
        assert_ne!(t.label, t.ink);
        assert_ne!(t.label, t.value);
        for foreground in [t.label, t.ink, t.value] {
            for background in [t.bg, t.selection] {
                let (a, b) = (luminance(foreground), luminance(background));
                let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                assert!(
                    ratio >= 4.5,
                    "light={light}: {foreground:?}/{background:?} = {ratio}"
                );
            }
        }
    }
}

#[test]
fn rendered_captions_values_and_hints_have_distinct_roles_in_all_color_modes() {
    for mode in 0..4 {
        let mut app = App {
            view: View::Settings,
            focus: 2,
            viewport: (120, 36),
            light: mode == 1,
            color16: mode == 2,
            monochrome: mode == 3,
            ..Default::default()
        };
        app.settings.settings = vec![Setting {
            key: "LOCALVOX_LLM_MODEL".into(),
            group: "LLM".into(),
            label: "Модель".into(),
            kind: "text".into(),
            value: Some("QwenFixture".into()),
            default_value: Some("PresetFixture".into()),
            hint: "Пояснение настройки".into(),
            ..Default::default()
        }];
        let t = Theme::for_app(&app);
        let buffer = screen(&app);
        assert_eq!(word(&buffer, "Разделы", 0).fg, t.label);
        assert_eq!(word(&buffer, "значение", 26).fg, t.label);
        assert_eq!(word(&buffer, "Модель", 26).fg, t.label);
        assert_eq!(word(&buffer, "QwenFixture", 26).fg, t.value);
        assert_eq!(word(&buffer, "Пояснение настройки", 26).fg, t.dim);
        assert!(word(&buffer, "Модель", 26)
            .modifier
            .contains(Modifier::BOLD));
        if app.monochrome {
            assert!(word(&buffer, "значение", 26)
                .modifier
                .contains(Modifier::DIM));
            assert!(buffer
                .content
                .iter()
                .all(|c| c.fg == Color::Reset && c.bg == Color::Reset));
        }

        app.command(crate::app::Action::Search);
        app.paste("Данные запроса");
        let labels: Vec<_> = app
            .form
            .as_ref()
            .unwrap()
            .fields
            .iter()
            .map(|f| f.label.clone())
            .collect();
        let buffer = screen(&app);
        assert_eq!(word(&buffer, &labels[0], 0).fg, t.label);
        assert_eq!(word(&buffer, &labels[1], 0).fg, t.label);
        assert_eq!(word(&buffer, "Данные запроса", 0).fg, t.value);
        let (x, y) = position(&buffer, &labels[1], 0);
        let value = &buffer[(x + labels[1].chars().count() as u16 + 2, y)];
        assert_eq!(value.symbol(), "l");
        assert_eq!(value.fg, t.ink);
        if app.monochrome {
            assert!(word(&buffer, &labels[1], 0)
                .modifier
                .contains(Modifier::DIM));
            assert!(!value.modifier.contains(Modifier::DIM));
        }
        if app.color16 {
            assert!(buffer
                .content
                .iter()
                .all(|c| !matches!(c.fg, Color::Rgb(..)) && !matches!(c.bg, Color::Rgb(..))));
        }
    }
}

#[test]
fn document_title_and_user_text_remain_primary_data_without_changing_copy() {
    let mut app = App {
        current: Some("fixture".into()),
        tab: Tab::Readable,
        focus: 2,
        viewport: (120, 36),
        ..Default::default()
    };
    app.sessions = vec![Session {
        name: "fixture".into(),
        title: Some("Реальная запись".into()),
        ..Default::default()
    }];
    app.readable.from_transcript = true;
    app.readable.lines = vec![TranscriptLine {
        text: "Пользовательский текст".into(),
        ..Default::default()
    }];
    let t = Theme::for_app(&app);
    let copied = app.copy_text();
    let buffer = screen(&app);
    assert_eq!(word(&buffer, "Реальная запись", 26).fg, t.ink);
    assert_eq!(word(&buffer, "Пользовательский текст", 26).fg, t.ink);
    assert_eq!(app.copy_text(), copied);
    assert_eq!(copied, "Пользовательский текст");
}
