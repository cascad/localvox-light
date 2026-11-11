//! One owner of terminal input. Emulator names/configuration do not select a reader.
pub(super) trait InputSource {
    fn next_event(
        &mut self,
        wait: std::time::Duration,
    ) -> anyhow::Result<Option<crossterm::event::Event>>;
}

pub(super) fn source() -> anyhow::Result<Box<dyn InputSource>> {
    Ok(Box::new(Input::new()?))
}

impl InputSource for Input {
    fn next_event(
        &mut self,
        wait: std::time::Duration,
    ) -> anyhow::Result<Option<crossterm::event::Event>> {
        self.read(wait)
    }
}
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::Input;

#[cfg(not(windows))]
pub(super) struct Input;
#[cfg(not(windows))]
impl Input {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }
    pub fn read(
        &mut self,
        wait: std::time::Duration,
    ) -> anyhow::Result<Option<crossterm::event::Event>> {
        Ok(if crossterm::event::poll(wait)? {
            Some(crossterm::event::read()?)
        } else {
            None
        })
    }
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
    struct RecordedInput(std::collections::VecDeque<Event>);
    impl InputSource for RecordedInput {
        fn next_event(&mut self, _: std::time::Duration) -> anyhow::Result<Option<Event>> {
            Ok(self.0.pop_front())
        }
    }
    #[test]
    fn an_injected_event_source_uses_the_same_commands_and_inert_paste_path() {
        let mut source: Box<dyn InputSource> = Box::new(RecordedInput(
            [
                Event::Key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)),
                Event::Key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE)),
                Event::Key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE)),
                Event::Paste("https://example.test/audio".into()),
            ]
            .into(),
        ));
        let mut app = crate::app::App::default();
        while let Some(event) = source.next_event(std::time::Duration::ZERO).unwrap() {
            super::super::dispatch(&mut app, event, || {
                panic!("protocol events must not read the clipboard")
            });
        }
        assert_eq!(app.preferences.list_width, 31);
        assert!(!app.palette);
        assert_eq!(
            app.form.as_ref().unwrap().fields[0].value,
            "https://example.test/audio"
        );
        assert!(app.effects.is_empty());
    }
}
