use super::*;

impl App {
    pub(super) fn constrained_form_key(&mut self, key: KeyEvent) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if matches!(form.kind, FormKind::Import | FormKind::Setting(_))
            && key.code == KeyCode::Enter
        {
            if let Err(error) = self.submit_form() {
                if let Some(form) = &mut self.form {
                    form.error = Some(error);
                }
            }
            return true;
        }
        if let FormKind::SettingChoice {
            options, selected, ..
        } = &mut form.kind
        {
            match key.code {
                KeyCode::Esc => self.form = None,
                KeyCode::Up | KeyCode::BackTab => *selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Tab => {
                    *selected = (*selected + 1).min(options.len().saturating_sub(1))
                }
                KeyCode::Home => *selected = 0,
                KeyCode::End => *selected = options.len().saturating_sub(1),
                KeyCode::Enter => {
                    if let Err(e) = self.submit_form() {
                        if let Some(form) = &mut self.form {
                            form.error = Some(e);
                        }
                    }
                }
                _ => {}
            }
            return true;
        }
        if !matches!(form.kind, FormKind::Seek) {
            return false;
        }
        match key.code {
            KeyCode::Esc => self.form = None,
            KeyCode::Tab => form.active = (form.active + 1) % 3,
            KeyCode::BackTab => form.active = (form.active + 2) % 3,
            KeyCode::Enter => {
                if let Err(e) = self.submit_form() {
                    if let Some(form) = &mut self.form {
                        form.error = Some(e);
                    }
                }
            }
            _ => {
                let limit = if form.active == 0 { 999u32 } else { 59 };
                let field = &mut form.fields[form.active];
                match key.code {
                    KeyCode::Up | KeyCode::Down => {
                        let value = field.value.parse::<u32>().unwrap_or(0);
                        field.value = if key.code == KeyCode::Up {
                            value.saturating_add(1).min(limit)
                        } else {
                            value.saturating_sub(1)
                        }
                        .to_string();
                        field.initial.clear();
                    }
                    KeyCode::Char(c)
                        if c.is_ascii_digit()
                            && !key
                                .modifiers
                                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                    {
                        if field.value == field.initial {
                            field.value.clear();
                            field.initial.clear();
                        }
                        let value = format!("{}{c}", field.value);
                        if value.len() <= if form.active == 0 { 3 } else { 2 }
                            && value.parse::<u32>().is_ok_and(|v| v <= limit)
                        {
                            field.value = value;
                        }
                    }
                    KeyCode::Backspace | KeyCode::Delete => {
                        field.value.clear();
                        field.initial.clear();
                    }
                    _ => {}
                }
                field.editor = tui_textarea::TextArea::new(vec![field.value.clone()]);
                form.error = None;
            }
        }
        true
    }
    pub(super) fn constrained_paste(&mut self, text: &str) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if matches!(form.kind, FormKind::SettingChoice { .. }) {
            return true;
        }
        if !matches!(form.kind, FormKind::Seek) {
            return false;
        }
        let text = text.trim();
        let limit = if form.active == 0 { 999u32 } else { 59 };
        if !text.is_empty()
            && text.bytes().all(|c| c.is_ascii_digit())
            && text.len() <= if form.active == 0 { 3 } else { 2 }
            && text.parse::<u32>().is_ok_and(|v| v <= limit)
        {
            let field = &mut form.fields[form.active];
            field.value = text.into();
            field.initial.clear();
            field.editor = tui_textarea::TextArea::new(vec![field.value.clone()]);
            form.error = None;
        } else {
            form.error = Some("Введите число в допустимом диапазоне".into());
        }
        true
    }
    pub fn setting_value(&self, setting: &Setting, draft: bool) -> String {
        let value = if draft {
            self.settings_edits
                .get(&setting.key)
                .cloned()
                .unwrap_or_else(|| setting.value.clone())
        } else {
            setting.value.clone()
        };
        if setting.kind == "secret" {
            return if draft && self.settings_edits.contains_key(&setting.key) {
                if value.is_some() {
                    "задано · скрыто"
                } else {
                    "не задано"
                }
            } else if setting.set {
                "задано · скрыто"
            } else {
                "не задано"
            }
            .into();
        }
        let Some(value) = value else {
            return "По умолчанию".into();
        };
        if value.is_empty() {
            return "По умолчанию".into();
        }
        if setting.kind == "bool" {
            return match value.to_lowercase().as_str() {
                "0" | "off" | "false" | "no" => "Выключено".into(),
                "1" | "on" | "true" | "yes" => "Включено".into(),
                _ => value,
            };
        }
        setting
            .options
            .as_ref()
            .and_then(|options| options.iter().find(|choice| choice.value == value))
            .map(|choice| choice.label.clone())
            .unwrap_or(value)
    }
}
