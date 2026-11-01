use super::*;
use crate::diagnostics::Context;

impl App {
    pub fn client_error(&mut self, operation: &str, message: &str) {
        self.console.record_attempt(
            self.diagnostic("Клиент", operation, None),
            message,
            self.wall_ms,
        );
    }
    fn diagnostic(&self, source: &str, operation: &str, session: Option<String>) -> Context {
        let phase = session.as_ref().and_then(|name| {
            self.queue
                .iter()
                .find(|job| &job.session == name)
                .and_then(|job| job.phase.clone())
                .or_else(|| {
                    (self.current.as_ref() == Some(name))
                        .then(|| self.progress.phase.clone())
                        .flatten()
                })
        });
        Context {
            source: source.into(),
            operation: operation.into(),
            session,
            phase,
        }
    }
    pub(super) fn read_diagnostic(&self, resource: Resource) -> Context {
        let (name, document) = match resource {
            Resource::Summary => ("Загрузка сводки", true),
            Resource::Transcript => ("Загрузка реплик", true),
            Resource::Readable => ("Загрузка текста", true),
            Resource::Peaks => ("Загрузка волны", true),
            Resource::Progress => ("Чтение хода обработки", true),
            Resource::Journal => ("Чтение журнала", true),
            Resource::Versions => ("Загрузка версий", true),
            Resource::Sources => ("Загрузка источников", true),
            Resource::Speakers => ("Загрузка участников", true),
            Resource::Sessions => ("Чтение архива", false),
            Resource::Record => ("Состояние записи", false),
            Resource::Jobs | Resource::Queue => ("Чтение очереди", false),
            Resource::Settings => ("Загрузка настроек", false),
            Resource::Search => ("Поиск", false),
            Resource::Chat => ("Вопрос к архиву", false),
            Resource::Ask | Resource::Asks => ("Чтение вопросов", false),
            Resource::Notes => ("Чтение заметок", false),
            _ => ("Запрос к серверу", false),
        };
        self.diagnostic(
            "API",
            name,
            document.then(|| self.current.clone()).flatten(),
        )
    }
    pub(super) fn action_diagnostic(&self, task: &Task) -> Context {
        let (name, session) = match task {
            Task::Recook(s, _) => ("Повтор обработки", Some(s.clone())),
            Task::Retry(s) => ("Продолжение этапа", Some(s.clone())),
            Task::Cancel(s) => ("Отмена обработки", Some(s.clone())),
            Task::DeleteSession(s) => ("Удаление записи", Some(s.clone())),
            Task::Best(s, _) => ("Выбор версии", Some(s.clone())),
            Task::Lang(s, _) => ("Изменение языка", Some(s.clone())),
            Task::Speaker(s, ..) | Task::Source(s, ..) | Task::RenameParticipant(s, ..) => {
                ("Изменение имени", Some(s.clone()))
            }
            Task::Confirm(s, _) => ("Подтверждение документа", Some(s.clone())),
            Task::Ingest(_) => ("Импорт ссылки", None),
            Task::Copy(_) => ("Копирование", self.current.clone()),
            Task::SaveSettings(_) => ("Сохранение настроек", None),
            Task::Start(_) => ("Начало записи", None),
            Task::Stop => ("Остановка записи", self.record.session.clone()),
            Task::CreateAsk(_) | Task::CreateAskFile { .. } => ("Отправка вопроса", None),
            Task::Note(..) => ("Сохранение заметки", None),
            _ => ("Выполнение команды", None),
        };
        self.diagnostic(
            if matches!(task, Task::Copy(_) | Task::Export { .. }) {
                "Клиент"
            } else {
                "API"
            },
            name,
            session,
        )
    }
    pub fn apply_playback(&mut self, state: crate::player::PlaybackState) {
        let context = self.diagnostic("Плеер", "Воспроизведение", state.session.clone());
        if let Some(error) = &state.error {
            self.console.record(context, error, self.wall_ms);
        } else {
            self.console.recover(&context);
        }
        self.playback = state;
    }
    pub(super) fn console_key(&mut self, key: KeyEvent) -> bool {
        let toggle = key.code == KeyCode::F(12)
            || (self.form.is_none() && matches!(key.code, KeyCode::Char('`' | '~' | 'ё' | 'Ё')));
        if toggle {
            self.console.toggle();
            return true;
        }
        if !self.console.open {
            return false;
        }
        match key.code {
            KeyCode::Esc => self.console.toggle(),
            KeyCode::Up | KeyCode::PageUp => {
                self.console.scroll = self
                    .console
                    .scroll
                    .saturating_add(if key.code == KeyCode::Up { 1 } else { 8 });
                self.console.follow_selection = false;
            }
            KeyCode::Down | KeyCode::PageDown => {
                self.console.scroll = self
                    .console
                    .scroll
                    .saturating_sub(if key.code == KeyCode::Down { 1 } else { 8 });
                self.console.follow_selection = false;
            }
            KeyCode::End => {
                self.console.scroll = 0;
                self.console.selected = self.console.entries.len().saturating_sub(1);
                self.console.follow_selection = true;
            }
            KeyCode::Home => {
                self.console.scroll = u16::MAX;
                self.console.follow_selection = false;
            }
            KeyCode::Tab => {
                self.console.selected = self.console.selected.saturating_sub(1);
                self.console.follow_selection = true;
            }
            KeyCode::BackTab => {
                self.console.selected =
                    (self.console.selected + 1).min(self.console.entries.len().saturating_sub(1));
                self.console.follow_selection = true;
            }
            KeyCode::Enter => {
                if let Some(entry) = self.console.entries.get_mut(self.console.selected) {
                    entry.expanded = !entry.expanded;
                }
                self.console.follow_selection = true;
            }
            KeyCode::Char('y' | 'Y' | 'н' | 'Н') => {
                let text = self.console.text();
                if !text.is_empty() {
                    self.action(Task::Copy(text));
                }
            }
            _ => {}
        }
        true
    }
}
