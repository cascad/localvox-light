//! Named user actions and the ordered palette. No I/O.
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    ImportUrl,
    StartRecording,
    StopRecording,
    Search,
    Chat,
    NewNote,
    SuggestSlot,
    NewAsk,
    AskFromFile,
    SelectVersion,
    ChangeLanguage,
    RenameParticipant,
    RenameSource,
    RecookSummary,
    RecookText,
    RecookAll,
    ConfirmDocument,
    DeleteSession,
    DeleteNote,
    DeleteAsk,
    EditSetting,
    ResetSetting,
    CopyDocument,
    CopyReply,
    SaveDocument,
    TogglePlayback,
    PlayFragment,
    Seek,
    ToggleTheme,
    ToggleAscii,
    ToggleMonochrome,
    Refresh,
    Connection,
    AddLlmServer,
    SaveSettings,
    ToggleColorDepth,
    FilterJournal,
    RestoreDraft,
    FilterSettings,
    DiscardSettings,
    RetryFailed,
    CancelProcessing,
    DocumentInfo,
    ToggleAdvancedSettings,
    ErrorConsole,
}

/// Only this list defines palette order; shortcuts dispatch by Action.
pub const COMMANDS: &[(Action, &str)] = &[
    (Action::ImportUrl, "Импорт URL"),
    (Action::StartRecording, "Начать запись"),
    (Action::StopRecording, "Остановить запись"),
    (Action::Search, "Поиск"),
    (Action::Chat, "Вопрос к архиву"),
    (Action::NewNote, "Новая заметка"),
    (Action::SuggestSlot, "Предложить слот"),
    (Action::NewAsk, "Новый вопрос"),
    (Action::AskFromFile, "Вопрос из файла"),
    (Action::SelectVersion, "Выбрать версию"),
    (Action::ChangeLanguage, "Изменить язык"),
    (Action::RenameParticipant, "Имя участника"),
    (Action::RenameSource, "Имя источника"),
    (Action::RecookSummary, "Повторить сводку"),
    (Action::RecookText, "Повторить текст"),
    (Action::RecookAll, "Повторить всё"),
    (Action::ConfirmDocument, "Подтвердить документ"),
    (Action::DeleteSession, "Удалить запись"),
    (Action::DeleteNote, "Удалить заметку"),
    (Action::DeleteAsk, "Удалить вопрос"),
    (Action::EditSetting, "Изменить настройку"),
    (Action::ResetSetting, "Сбросить настройку"),
    (Action::CopyDocument, "Копировать документ"),
    (Action::CopyReply, "Копировать выбранную реплику"),
    (Action::SaveDocument, "Сохранить документ"),
    (Action::TogglePlayback, "Play / pause"),
    (Action::PlayFragment, "Слушать фрагмент"),
    (Action::Seek, "Перейти ко времени"),
    (Action::ToggleTheme, "Светлая / тёмная тема"),
    (Action::ToggleAscii, "ASCII / Unicode"),
    (Action::ToggleMonochrome, "Без цвета / цвет"),
    (Action::Refresh, "Обновить"),
    (Action::Connection, "Подключение и токен"),
    (Action::AddLlmServer, "Добавить LLM-сервер"),
    (Action::SaveSettings, "Сохранить изменённые настройки"),
    (Action::ToggleColorDepth, "16 цветов / true color"),
    (Action::FilterJournal, "Фильтр журнала"),
    (Action::RestoreDraft, "Восстановить черновик после ошибки"),
    (Action::FilterSettings, "Фильтр настроек"),
    (Action::DiscardSettings, "Отменить черновик настроек"),
    (Action::RetryFailed, "Повторить или продолжить этап"),
    (Action::CancelProcessing, "Отменить обработку"),
    (Action::DocumentInfo, "Сведения о записи"),
    (Action::ToggleAdvancedSettings, "Основные / все настройки"),
    (Action::ErrorConsole, "Консоль ошибок"),
];

impl App {
    pub fn command(&mut self, action: Action) {
        self.cancel_quit_confirmation();
        self.palette = false;
        let s = self.current.clone().unwrap_or_default();
        match action {
            Action::AddLlmServer => {
                if self.view != View::Settings
                    || !self.settings.settings.iter().any(|s| s.kind == "servers")
                {
                    self.switch(View::Settings);
                    self.llm_server_pending = true;
                    return;
                }
                self.open(
                    "Добавить LLM-сервер",
                    FormKind::LlmServer,
                    &[("HTTP(S) адрес", "")],
                );
            }
            Action::DocumentInfo => {
                if self.view != View::Archive {
                    self.switch(View::Archive);
                }
                self.tab = Tab::Progress;
                self.metadata_open = !self.metadata_open;
                self.scroll = 0;
                self.refresh_view();
            }
            Action::ToggleAdvancedSettings => {
                self.settings_advanced = !self.settings_advanced;
                self.selected = 0;
                self.scroll = 0;
            }
            Action::ErrorConsole => self.console.toggle(),
            Action::CancelProcessing => {
                let target = if self.view == View::Queue {
                    self.queue
                        .get(self.selected)
                        .filter(|job| {
                            matches!(
                                job.state.as_str(),
                                "pending" | "running" | "failed" | "waiting_input"
                            )
                        })
                        .map(|job| job.session.clone())
                } else if self.view == View::Archive {
                    self.current
                        .as_ref()
                        .and_then(|current| {
                            self.sessions
                                .iter()
                                .find(|session| &session.name == current)
                        })
                        .filter(|session| {
                            matches!(
                                session.job.as_deref(),
                                Some("pending" | "running" | "failed" | "waiting_input")
                            )
                        })
                        .map(|session| session.name.clone())
                } else {
                    None
                };
                if let Some(session) = target {
                    self.action(Task::Cancel(session));
                } else {
                    self.status = "Выберите ожидающую или выполняющуюся обработку".into();
                }
            }
            Action::RetryFailed => {
                let target = if self.view == View::Queue {
                    self.queue
                        .get(self.selected)
                        .filter(|job| matches!(job.state.as_str(), "failed" | "cancelled"))
                        .map(|job| job.session.clone())
                } else if self.view == View::Archive {
                    self.current.clone()
                } else {
                    None
                };
                if let Some(session) = target {
                    self.action(Task::Retry(session));
                } else {
                    self.status = "Выберите остановленное задание в очереди или запись".into();
                }
            }
            Action::ImportUrl => self.open("Импорт", FormKind::Import, &[("URL", "")]),
            Action::StartRecording => {
                self.open("Начать запись", FormKind::Start, &[("Название", "")])
            }
            Action::StopRecording => {
                self.pending_record = Some(false);
                self.pending_record_at = self.now_ms;
                self.action(Task::Stop);
            }
            Action::Search => {
                let query = self.search_query.clone();
                let mode = self.search_mode.clone();
                self.open(
                    "Поиск",
                    FormKind::Search,
                    &[
                        ("Запрос", &query),
                        ("Режим: lexical / semantic / hybrid", &mode),
                    ],
                );
            }
            Action::Chat => {
                let query = self.chat_query.clone();
                self.open("Вопрос к архиву", FormKind::Chat, &[("Вопрос", &query)]);
            }
            Action::NewNote => {
                self.route_hints.clear();
                self.route_text = None;
                let slot = self
                    .notes
                    .iter()
                    .find(|s| s.default)
                    .or_else(|| self.notes.first())
                    .map(|s| s.name.clone())
                    .unwrap_or_default();
                self.open(
                    "Новая заметка",
                    FormKind::Note,
                    &[("Слот (пусто — по умолчанию)", &slot), ("Текст", "")],
                );
            }
            Action::SuggestSlot => {
                if self.pending_actions.contains(&Resource::Route) {
                    self.status = "Дождитесь предыдущего предложения слотов".into();
                    return;
                }
                let text = self
                    .form
                    .as_ref()
                    .filter(|f| matches!(f.kind, FormKind::Note))
                    .and_then(|f| f.fields.get(1))
                    .map(|f| f.value.clone());
                if let Some(text) = text.filter(|s| !s.trim().is_empty()) {
                    self.route_text = Some(text.clone());
                    self.action(Task::Route(text));
                } else {
                    if self.form.is_none() {
                        self.command(Action::NewNote);
                    }
                    self.status = "Сначала напишите заметку; Ctrl+R предложит слот".into();
                }
            }
            Action::NewAsk | Action::AskFromFile => self.open(
                "Спросить",
                if action == Action::AskFromFile {
                    FormKind::AskFile
                } else {
                    FormKind::Ask
                },
                &[
                    (
                        if action == Action::AskFromFile {
                            "Путь к текстовому файлу"
                        } else {
                            "Материал"
                        },
                        "",
                    ),
                    ("Provider: ollama / claude", "ollama"),
                    ("Инструкция", ""),
                    ("Имя материала", ""),
                ],
            ),
            Action::SelectVersion if !s.is_empty() => {
                let ids = self
                    .versions
                    .iter()
                    .map(|v| format!("{}{} {}", v.id, if v.best { "*" } else { "" }, v.model))
                    .collect::<Vec<_>>()
                    .join(" · ");
                let current = self
                    .versions
                    .iter()
                    .find(|v| v.best)
                    .map(|v| v.id.to_string())
                    .unwrap_or_default();
                self.open(
                    &format!("Рабочая версия · {}", self.current_title()),
                    FormKind::Best,
                    &[(&format!("ID версии · {ids}"), &current)],
                );
            }
            Action::ChangeLanguage if !s.is_empty() => {
                self.read(Resource::Languages, String::new());
                let lang = self
                    .sessions
                    .iter()
                    .find(|session| session.name == s)
                    .and_then(|session| session.lang.clone())
                    .unwrap_or_else(|| "auto".into());
                self.open(
                    &format!("Язык · полный повтор STT · {}", self.current_title()),
                    FormKind::Lang,
                    &[("Код языка или auto", &lang)],
                );
            }
            Action::RenameParticipant => {
                if let Some(participant) = self.speakers.get(self.item).cloned() {
                    let name = participant.name.clone();
                    self.open(
                        &format!("Локальное имя · {}", self.current_title()),
                        FormKind::Participant(participant),
                        &[("Имя (для всех его голосов и источников)", &name)],
                    );
                    return;
                }
                if !s.is_empty() {
                    self.tab = Tab::Speakers;
                    self.item = 0;
                    self.focus = 2;
                    self.refresh_view();
                }
                self.status =
                    "Выберите участника в списке; имя действует только в этой записи".into();
            }
            Action::RenameSource if !s.is_empty() => self.open(
                &format!("Имя источника · {}", self.current_title()),
                FormKind::Source,
                &[("ID источника", "0"), ("Имя (пустое — сброс)", "")],
            ),
            Action::RecookSummary | Action::RecookText | Action::RecookAll if !s.is_empty() => self
                .confirm(
                    &format!("Повторить: {}", self.current_title()),
                    Task::Recook(
                        s,
                        match action {
                            Action::RecookSummary => Scope::Summary,
                            Action::RecookText => Scope::Text,
                            _ => Scope::All,
                        },
                    ),
                ),
            Action::ConfirmDocument
                if !s.is_empty()
                    && self.view == View::Archive
                    && matches!(self.tab, Tab::Summary | Tab::Transcript | Tab::Readable) =>
            {
                self.action(Task::Confirm(
                    s,
                    if self.tab == Tab::Summary {
                        "summary"
                    } else {
                        "processed"
                    }
                    .into(),
                ))
            }
            Action::DeleteSession if !s.is_empty() => self.confirm(
                &format!("Удалить навсегда: {}", self.current_title()),
                Task::DeleteSession(s),
            ),
            Action::DeleteNote => {
                let note = self.notes.get(self.selected).and_then(|slot| {
                    slot.notes
                        .get(self.item)
                        .map(|note| (slot.name.clone(), note.clone()))
                });
                if let Some((slot, n)) = note {
                    self.confirm(
                        &format!("Удалить заметку: {}", safe_text(&n.text)),
                        Task::DeleteNote(slot, n),
                    );
                }
            }
            Action::DeleteAsk => {
                if let Some(a) = &self.ask {
                    self.confirm(
                        &format!("Удалить вопрос {}", a.id),
                        Task::DeleteAsk(a.id.clone()),
                    );
                }
            }
            Action::EditSetting => {
                if let Some(s) = self.selected_setting() {
                    if s.kind == "bool"
                        || s.key == "LOCALVOX_LLM_BASE_URL"
                        || s.options
                            .as_ref()
                            .is_some_and(|options| !options.is_empty())
                    {
                        let preset = s
                            .default_value
                            .as_deref()
                            .filter(|v| !v.is_empty())
                            .unwrap_or("автоматически");
                        let mut options = vec![(format!("Предустановка · {preset}"), None)];
                        if s.key == "LOCALVOX_LLM_BASE_URL" {
                            options.extend(
                                self.llm_servers()
                                    .into_iter()
                                    .map(|url| (url.clone(), Some(url))),
                            );
                        } else if s.kind == "bool" {
                            options.extend([
                                ("Включено".into(), Some("on".into())),
                                ("Выключено".into(), Some("off".into())),
                            ]);
                        } else if let Some(choices) = &s.options {
                            options.extend(
                                choices.iter().map(|choice| {
                                    (choice.label.clone(), Some(choice.value.clone()))
                                }),
                            );
                        }
                        let value = self
                            .settings_edits
                            .get(&s.key)
                            .cloned()
                            .unwrap_or_else(|| s.value.clone());
                        let selected = options
                            .iter()
                            .position(|(_, v)| v == &value)
                            .unwrap_or_else(|| {
                                // Preserve legacy bool spellings and unavailable device/provider
                                // values until the user explicitly selects a replacement.
                                options.push((
                                    format!("Текущее: {}", self.setting_value(s, true)),
                                    value.clone(),
                                ));
                                options.len() - 1
                            });
                        let title = s.label.clone();
                        let key = s.key.clone();
                        self.open(
                            &title,
                            FormKind::SettingChoice {
                                key,
                                options,
                                selected,
                            },
                            &[],
                        );
                        return;
                    }
                    let key = s.key.clone();
                    let value = if s.kind == "secret" {
                        self.settings_edits
                            .get(&s.key)
                            .cloned()
                            .flatten()
                            .unwrap_or_default()
                    } else {
                        self.settings_edits
                            .get(&s.key)
                            .cloned()
                            .unwrap_or_else(|| s.value.clone())
                            .or_else(|| s.default_value.clone())
                            .unwrap_or_default()
                    };
                    let title = s.label.clone();
                    let hint = "Новое значение";
                    self.open(&title, FormKind::Setting(key.clone()), &[(&hint, &value)]);
                    if let Some(f) = &mut self.form {
                        f.fields[0].secret = self
                            .settings
                            .settings
                            .iter()
                            .find(|s| s.key == key)
                            .is_some_and(|s| s.kind == "secret");
                        if f.fields[0].secret {
                            f.fields[0].editor.set_mask_char('•');
                        }
                    }
                }
            }
            Action::ResetSetting => {
                if let Some(s) = self.selected_setting() {
                    self.settings_edits.insert(s.key.clone(), None);
                    self.status =
                        "Сброс добавлен в черновик настроек; сохраните изменения через палитру"
                            .into();
                }
            }
            Action::CopyDocument => {
                let text = self.copy_text();
                if text.trim().is_empty() {
                    self.status = "Нет документа для копирования".into();
                } else {
                    self.action(Task::Copy(text));
                }
            }
            Action::CopyReply => {
                let text = self.selected_reply_text();
                if text.trim().is_empty() {
                    self.status = "Нет выбранной реплики для копирования".into();
                } else {
                    self.action(Task::Copy(text));
                }
            }
            Action::SaveDocument => {
                let text = self.copy_text();
                if text.trim().is_empty() {
                    self.status = "Нет документа для сохранения".into();
                } else {
                    self.open(
                        "Сохранить копию документа",
                        FormKind::Export(text),
                        &[("Новый путь к файлу UTF-8", "")],
                    );
                }
            }
            Action::TogglePlayback => {
                if let Some(session) = self.current.clone() {
                    if self.playback.session.as_ref() == Some(&session) && !self.playback.idle {
                        self.player_effects
                            .push(crate::player::PlaybackCommand::Toggle);
                    } else {
                        self.player_effects
                            .push(crate::player::PlaybackCommand::Load {
                                session,
                                start: if self.playback.session == self.current
                                    && !self
                                        .playback
                                        .duration
                                        .is_some_and(|d| self.playback.position >= d - 0.05)
                                {
                                    self.playback.position
                                } else {
                                    0.0
                                },
                                end: None,
                            });
                    }
                }
            }
            Action::PlayFragment => {
                if let Some((session, start, end)) = self.fragment() {
                    if self.current.as_ref() != Some(&session) {
                        self.select_session(session.clone());
                        self.tab = Tab::Transcript;
                        self.refresh_view();
                    }
                    self.player_effects
                        .push(crate::player::PlaybackCommand::Load {
                            session,
                            start,
                            end,
                        });
                } else {
                    self.status =
                        "Выберите реплику, утверждение сводки или источник поиска/чата".into();
                }
            }
            Action::Seek => {
                let seconds = self.playback.position.max(0.0) as u64;
                self.open(
                    "Перейти ко времени",
                    FormKind::Seek,
                    &[
                        ("Часы · 0–999", &(seconds / 3600).to_string()),
                        ("Минуты · 0–59", &((seconds / 60) % 60).to_string()),
                        ("Секунды · 0–59", &(seconds % 60).to_string()),
                    ],
                );
                if let Some(form) = &mut self.form {
                    form.active = 1;
                }
            }
            Action::ToggleTheme => self.light = !self.light,
            Action::ToggleAscii => self.ascii = !self.ascii,
            Action::ToggleMonochrome => self.monochrome = !self.monochrome,
            Action::Refresh => self.refresh(),
            Action::Connection => {
                let address = self.address.clone();
                self.open(
                    "Подключение к демону",
                    FormKind::Connection,
                    &[
                        ("HTTP(S) адрес", &address),
                        ("Новый токен (пусто — оставить)", ""),
                        ("Очистить токен: true / false", "false"),
                    ],
                );
                if let Some(f) = &mut self.form {
                    f.fields[1].secret = true;
                    f.fields[1].editor.set_mask_char('•');
                }
            }
            Action::SaveSettings => {
                if self.settings_edits.is_empty() {
                    self.status = "Нет изменённых настроек".into();
                } else {
                    self.confirm(
                        &format!("Сохранить {} изменённых ключей", self.settings_edits.len()),
                        Task::SaveSettings(self.settings_edits.clone()),
                    );
                }
            }
            Action::ToggleColorDepth => self.color16 = !self.color16,
            Action::FilterJournal => {
                let filter = self.journal.filter.clone();
                self.open(
                    "Фильтр журнала",
                    FormKind::JournalFilter,
                    &[("Строка (пусто — все события)", &filter)],
                );
            }
            Action::RestoreDraft => {
                if self.form.is_none() {
                    self.form = self.failed_drafts.pop_back();
                } else {
                    self.status = "Закройте текущую форму перед восстановлением черновика".into();
                }
            }
            Action::FilterSettings => {
                let filter = self.settings_filter.clone();
                self.open(
                    "Фильтр настроек",
                    FormKind::SettingsFilter,
                    &[("Ключ, группа или описание", &filter)],
                );
            }
            Action::DiscardSettings if !self.settings_edits.is_empty() => self.open(
                "Отменить несохранённые настройки",
                FormKind::DiscardSettings,
                &[("Введите ДА для отмены черновика", "")],
            ),
            Action::SelectVersion
            | Action::ChangeLanguage
            | Action::RenameSource
            | Action::RecookSummary
            | Action::RecookText
            | Action::RecookAll
            | Action::ConfirmDocument
            | Action::DeleteSession
            | Action::DiscardSettings => self.status = "Сначала выберите запись или элемент".into(),
        }
    }
}
