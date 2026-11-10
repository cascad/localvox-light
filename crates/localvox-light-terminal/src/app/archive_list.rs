use super::*;

impl App {
    // Changing documents does not recover a failed global poll.
    pub(super) fn clear_document_errors(&mut self) {
        self.errors.retain(|resource, _| {
            matches!(
                resource,
                Resource::Sessions | Resource::Queue | Resource::Record | Resource::Jobs
            )
        });
    }
    pub fn visible_sessions(&self) -> Vec<&Session> {
        let query = self.archive_filter.to_lowercase();
        self.sessions
            .iter()
            .filter(|session| query.is_empty() || session.title().to_lowercase().contains(&query))
            .collect()
    }
    pub(super) fn set_archive_filter(&mut self, query: String) {
        let selected = self
            .visible_sessions()
            .get(self.selected)
            .map(|s| s.name.clone());
        self.archive_filter = query;
        self.selected = selected
            .and_then(|id| self.visible_sessions().iter().position(|s| s.name == id))
            .unwrap_or(0);
    }
    pub(super) fn archive_filter_input(&self) -> bool {
        self.view == View::Archive
            && self.focus == 1
            && self.form.is_none()
            && !self.help
            && !self.palette
            && !self.console.open
    }
    pub(super) fn append_archive_filter(&mut self, text: &str) {
        let left = 256usize.saturating_sub(self.archive_filter.chars().count());
        let added: String = text
            .chars()
            .filter(|c| !c.is_control())
            .take(left)
            .collect();
        self.set_archive_filter(format!("{}{added}", self.archive_filter));
    }
    pub(super) fn archive_filter_key(&mut self, key: KeyEvent) -> bool {
        if !self.archive_filter_input() {
            return false;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            if matches!(key.code, KeyCode::Char('u' | 'U' | 'г' | 'Г')) {
                self.set_archive_filter(String::new());
                return true;
            }
            return false;
        }
        if key.modifiers.contains(KeyModifiers::ALT) {
            return false;
        }
        match key.code {
            KeyCode::Char(c) if !c.is_control() && !matches!(c, ':' | '?') => {
                self.append_archive_filter(&c.to_string());
            }
            KeyCode::Backspace => {
                let mut query = self.archive_filter.clone();
                query.pop();
                self.set_archive_filter(query);
            }
            KeyCode::Esc => self.set_archive_filter(String::new()),
            _ => return false,
        }
        true
    }
}

/// KeyEvent exposes text, not a portable physical scan code. Normalize command
/// positions for standard RU/EN only, after text-input contexts have consumed it.
pub(super) fn command_key(mut key: KeyEvent) -> KeyEvent {
    if let KeyCode::Char(c) = key.code {
        const RU: &str = "йцукенгшщзхъфывапролджэячсмитьбюё";
        const EN: &str = "qwertyuiop[]asdfghjkl;'zxcvbnm,.`";
        let lower = c.to_lowercase().next().unwrap_or(c);
        let mapped = RU
            .chars()
            .zip(EN.chars())
            .find(|(r, _)| *r == lower)
            .map(|(_, e)| e);
        if let Some(mut value) = mapped {
            if c.is_uppercase() {
                value = match value {
                    '[' => '{',
                    ']' => '}',
                    ';' => ':',
                    '\'' => '"',
                    ',' => '<',
                    '.' => '>',
                    '`' => '~',
                    other => other.to_ascii_uppercase(),
                };
            }
            key.code = KeyCode::Char(value);
        } else if c == '.' {
            key.code = KeyCode::Char('/');
        } else if c == ',' && key.modifiers.contains(KeyModifiers::SHIFT) {
            key.code = KeyCode::Char('?');
        }
    }
    key
}

#[cfg(test)]
mod tests;
