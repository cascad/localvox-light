//! Client preferences outlive connection data; reconnect moves them as one value.
use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UiPreferences {
    pub ascii: bool,
    pub monochrome: bool,
    pub color16: bool,
    pub light: bool,
    pub keymap: crate::keymap::Keymap,
    pub list_width: u16,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnect_preserves_all_preferences_but_never_replays_mutations_or_old_documents() {
        let preferences = UiPreferences {
            ascii: true,
            light: true,
            monochrome: true,
            color16: true,
            list_width: 53,
            keymap: crate::keymap::Keymap::from_json(br#"{"version":1,"keys":{"x":"l"}}"#).unwrap(),
        };
        let mut app = App {
            preferences: preferences.clone(),
            current: Some("old-server".into()),
            epoch: 50,
            document_final: true,
            pending_document: Some(5),
            ..Default::default()
        };
        app.effects.push((Resource::Action, Task::Stop));
        app.player_effects
            .push(crate::player::PlaybackCommand::Toggle);
        app.playback.position = 42.0;
        app.client_error("Подключение", "synthetic old error");
        let entries = app.console.entries.len();
        app.reconnected("http://127.0.0.1:1234".into());
        assert_eq!(app.preferences, preferences);
        assert_eq!(
            app.shortcut_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
                .code,
            KeyCode::Char('l')
        );
        assert!(app.current.is_none() && !app.document_final && app.pending_document.is_none());
        assert_eq!(app.console.entries.len(), entries);
        assert_eq!(app.address, "http://127.0.0.1:1234");
        assert_eq!(app.playback.position, 0.0);
        assert!(app.player_effects.is_empty());
        assert!(!app
            .effects
            .iter()
            .any(|(_, task)| matches!(task, Task::Stop)));
        assert!(app
            .effects
            .iter()
            .any(|(resource, _)| *resource == Resource::Sessions));
    }
}
impl Default for UiPreferences {
    fn default() -> Self {
        Self {
            ascii: false,
            monochrome: false,
            color16: false,
            light: false,
            keymap: crate::keymap::Keymap::default(),
            list_width: crate::render::DEFAULT_LIST_WIDTH,
        }
    }
}

impl App {
    /// Only session-independent client state crosses a connection boundary.
    /// Pending mutations, epochs, documents and playback start fresh.
    pub fn reconnected(&mut self, address: String) {
        self.console
            .recover_operation(&crate::diagnostics::Context {
                source: "Клиент".into(),
                operation: "Подключение".into(),
                ..Default::default()
            });
        let console = std::mem::take(&mut self.console);
        let preferences = self.preferences.clone();
        *self = Self {
            console,
            preferences,
            address,
            ..Default::default()
        };
        self.status = "Подключено к API LocalVox".into();
        self.refresh();
    }
}
