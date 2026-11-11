use super::*;

impl App {
    pub fn llm_servers(&self) -> Vec<String> {
        let setting = self
            .settings
            .settings
            .iter()
            .find(|s| s.key == "LOCALVOX_LLM_SERVER_URLS");
        let raw = self
            .settings_edits
            .get("LOCALVOX_LLM_SERVER_URLS")
            .cloned()
            .unwrap_or_else(|| setting.and_then(|s| s.value.clone()))
            .or_else(|| setting.and_then(|s| s.default_value.clone()));
        raw.and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_else(|| {
                self.settings
                    .settings
                    .iter()
                    .find(|s| s.key == "LOCALVOX_LLM_BASE_URL")
                    .and_then(|s| s.options.as_ref())
                    .map(|options| options.iter().map(|option| option.value.clone()).collect())
                    .unwrap_or_default()
            })
    }
    pub(super) fn constrained_form_key(&mut self, key: KeyEvent) -> bool {
        let Some(form) = &mut self.form else {
            return false;
        };
        if matches!(
            form.kind,
            FormKind::Import | FormKind::Setting(_) | FormKind::LlmServer
        ) && key.code == KeyCode::Enter
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
        let Some(value) = value.or_else(|| setting.default_value.clone()) else {
            return "Предустановка".into();
        };
        if value.is_empty() {
            return if setting.kind == "path" {
                "Автопоиск"
            } else {
                "Автоматически"
            }
            .into();
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

impl App {
    pub fn key(&mut self, mut key: KeyEvent) {
        if key.kind == crossterm::event::KeyEventKind::Release {
            return;
        }
        if self.view != View::Archive || self.focus != 1 {
            self.archive_filter_editing = false;
        }
        let shortcut = self.shortcut_key(key);
        if self.quit_key(shortcut) {
            return;
        }
        if self.archive_filter_key(key) {
            return;
        }
        if self.form.is_none() {
            key = shortcut;
        }
        if self.console_key(key) {
            return;
        }
        if self.constrained_form_key(key) {
            return;
        }
        if self.form.is_none() && self.view == View::Settings {
            if key.code == KeyCode::Char('N') {
                self.command(Action::AddLlmServer);
                return;
            }
            if key.code == KeyCode::Char('A') {
                self.command(Action::ToggleAdvancedSettings);
                return;
            }
            if key.code == KeyCode::Char('S') {
                self.command(Action::SaveSettings);
                return;
            }
        }
        if self.form.is_none() && self.view == View::Archive && key.code == KeyCode::Char('I') {
            self.command(Action::DocumentInfo);
            return;
        }
        if self.form.is_none()
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('y' | 'Y' | 'н' | 'Н'))
        {
            self.command(Action::CopyDocument);
            return;
        }
        if self.form.is_some() {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(shortcut.code, KeyCode::Char('r' | 'R'))
                && self
                    .form
                    .as_ref()
                    .is_some_and(|f| matches!(f.kind, FormKind::Note))
            {
                self.command(Action::SuggestSlot);
                return;
            }
            if key.code == KeyCode::Esc {
                self.form = None;
                return;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(shortcut.code, KeyCode::Enter | KeyCode::Char('s' | 'S'))
            {
                if let Err(e) = self.submit_form() {
                    if let Some(f) = &mut self.form {
                        f.error = Some(e);
                    }
                }
                return;
            }
            if let Some(f) = &mut self.form {
                match key.code {
                    KeyCode::Tab => f.active = (f.active + 1) % f.fields.len(),
                    KeyCode::BackTab => f.active = (f.active + f.fields.len() - 1) % f.fields.len(),
                    _ => {
                        let field = &mut f.fields[f.active];
                        let may_insert = matches!(key.code, KeyCode::Char(_) | KeyCode::Enter)
                            && !key.modifiers.contains(KeyModifiers::CONTROL);
                        if !may_insert || field.value.chars().count() < 400_000 {
                            if field.editor.input(key) {
                                field.value = field.editor.lines().join("\n");
                            }
                        }
                    }
                }
            }
            self.discard_stale_route_hints();
            return;
        }
        if self.help {
            self.help = false;
            return;
        }
        if self.palette {
            match key.code {
                KeyCode::Esc => self.palette = false,
                KeyCode::Down | KeyCode::Char('j') => {
                    self.palette_index = (self.palette_index + 1) % COMMANDS.len()
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.palette_index = (self.palette_index + COMMANDS.len() - 1) % COMMANDS.len()
                }
                KeyCode::Enter => self.command(COMMANDS[self.palette_index].0),
                _ => {}
            }
            return;
        }
        if self.focus <= 1 && key.modifiers == KeyModifiers::CONTROL {
            let action = match key.code {
                KeyCode::Left => Some(Action::NarrowList),
                KeyCode::Right => Some(Action::WidenList),
                _ => None,
            };
            if let Some(action) = action {
                if !self.archive_filter_editing {
                    self.command(action);
                }
                return;
            }
        }
        if self.view == View::Archive && self.focus == 2 && key.modifiers.is_empty() {
            key.code = match key.code {
                KeyCode::Char('h') => KeyCode::Left,
                KeyCode::Char('l') => KeyCode::Right,
                other => other,
            };
        }
        match key.code {
            KeyCode::F(6) => self.focus = if self.focus == 3 { 2 } else { 3 },
            KeyCode::Left if self.focus == 3 => {
                self.nudge(if key.modifiers.contains(KeyModifiers::ALT) {
                    -15.0
                } else {
                    -5.0
                })
            }
            KeyCode::Right if self.focus == 3 => {
                self.nudge(if key.modifiers.contains(KeyModifiers::ALT) {
                    15.0
                } else {
                    5.0
                })
            }
            KeyCode::Home if self.focus == 3 => self.seek_to(0.0),
            KeyCode::End if self.focus == 3 => {
                if let Some(duration) = self.media_duration() {
                    self.seek_to(duration);
                }
            }
            KeyCode::Char('y' | 'Y' | 'н' | 'Н')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.command(match (self.view, self.tab) {
                    (View::Archive, Tab::Summary) => Action::CopySummaryBlock,
                    (View::Archive, Tab::Transcript) => Action::CopyReply,
                    _ => Action::CopyDocument,
                })
            }
            KeyCode::Char('g' | 'G') => self.command(Action::Seek),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char(':') | KeyCode::F(2) => self.palette = true,
            KeyCode::Char('/') => {
                self.command(if self.view == View::Archive && self.tab == Tab::Progress {
                    Action::FilterJournal
                } else if self.view == View::Settings {
                    Action::FilterSettings
                } else {
                    Action::Search
                })
            }
            KeyCode::Tab => {
                self.focus = if self.focus == 3 {
                    2
                } else {
                    (self.focus + 1) % 3
                }
            }
            KeyCode::BackTab => {
                self.focus = if self.focus == 3 {
                    2
                } else {
                    (self.focus + 2) % 3
                }
            }
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('R') => self.command(Action::RetryFailed),
            KeyCode::Char('X') => self.command(Action::CancelProcessing),
            KeyCode::Char(' ') => self.command(Action::TogglePlayback),
            KeyCode::Char('p') => self.command(Action::PlayFragment),
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => self.nudge(-15.0),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => self.nudge(15.0),
            KeyCode::Char('o') if self.view == View::Archive && self.tab == Tab::Progress => {
                if let (Some(s), Some(before)) = (self.current.clone(), self.journal.before.clone())
                {
                    self.effects
                        .push((Resource::Journal, Task::Journal(s, None, Some(before))));
                }
            }
            KeyCode::Char('i') => self.command(Action::ImportUrl),
            KeyCode::Char('n') => self.command(match self.view {
                View::Notes => Action::NewNote,
                View::Ask => Action::NewAsk,
                _ => Action::ImportUrl,
            }),
            KeyCode::Char('1'..='5') => {
                if let KeyCode::Char(c) = key.code {
                    self.tab = TABS[c as usize - '1' as usize].0;
                    self.scroll = 0;
                    self.item = 0;
                    self.refresh_view();
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if self.focus == 0 {
                    self.nav = (self.nav + 1) % VIEWS.len();
                } else if self.focus == 1 {
                    self.move_list(true, 1);
                } else {
                    self.scroll = self.effective_scroll();
                    if self.is_journal() {
                        if self.journal.follow {
                            self.item = self.selection_len().saturating_sub(1);
                        }
                        self.journal.follow = false;
                    }
                    self.follow_selection = true;
                    self.item = self
                        .item
                        .saturating_add(1)
                        .min(self.selection_len().saturating_sub(1));
                    self.scroll = self.scroll.saturating_add(1);
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if self.focus == 0 {
                    self.nav = (self.nav + VIEWS.len() - 1) % VIEWS.len();
                } else if self.focus == 1 {
                    self.move_list(false, 1);
                } else {
                    self.scroll = self.effective_scroll();
                    if self.is_journal() {
                        if self.journal.follow {
                            self.item = self.selection_len().saturating_sub(1);
                        }
                        self.journal.follow = false;
                    }
                    self.follow_selection = true;
                    self.item = self.item.saturating_sub(1);
                    self.scroll = self.scroll.saturating_sub(1);
                }
            }
            KeyCode::PageDown => self.page(true),
            KeyCode::PageUp => self.page(false),
            KeyCode::Home if self.focus == 1 => {
                self.selected = 0;
                self.list_scroll = 0;
            }
            KeyCode::End if self.focus == 1 => {
                self.selected = self.list_len().saturating_sub(1);
                self.list_scroll = self
                    .list_len()
                    .saturating_sub(crate::render::list_metrics(self).0);
            }
            KeyCode::Home => {
                self.scroll = 0;
                self.item = 0;
                if self.is_journal() {
                    self.journal.follow = false;
                }
                self.follow_selection = false;
            }
            KeyCode::End => {
                self.scroll = crate::render::document_metrics(self)
                    .0
                    .saturating_sub(crate::render::document_dimensions(self).1);
                self.follow_selection = false;
                if self.is_journal() {
                    self.journal.follow = true;
                    self.journal.unseen = 0;
                }
                if self.is_journal() && self.journal.tail_gap {
                    let expanded = std::mem::take(&mut self.journal.expanded);
                    let filter = std::mem::take(&mut self.journal.filter);
                    self.journal = crate::journal::Journal::new();
                    self.journal.expanded = expanded;
                    self.journal.filter = filter;
                    self.refresh_view();
                }
            }
            KeyCode::Left => {
                self.expanded = false;
                if self.is_journal() {
                    self.scroll = self.effective_scroll();
                    self.journal.follow = false;
                    self.follow_selection = true;
                    if let Some(g) = self.journal.visible_groups().get(self.item) {
                        self.journal.expanded.remove(&g.id);
                    }
                }
            }
            KeyCode::Right => {
                self.expanded = true;
                if self.is_journal() {
                    self.scroll = self.effective_scroll();
                    self.journal.follow = false;
                    self.follow_selection = true;
                    if let Some(g) = self.journal.visible_groups().get(self.item) {
                        self.journal.expanded.insert(g.id.clone());
                    }
                }
            }
            KeyCode::Char('l')
                if key.modifiers.is_empty()
                    && (self.focus == 0 || (self.view == View::Archive && self.focus == 1)) =>
            {
                self.enter()
            }
            KeyCode::Enter => self.enter(),
            KeyCode::Esc => self.back(),
            _ => {}
        }
    }
}
