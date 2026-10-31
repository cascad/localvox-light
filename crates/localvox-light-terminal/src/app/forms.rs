//! Forms, validation and explicit user actions. No I/O.
use super::*;

impl App {
    pub fn commands(&self) -> Vec<&'static str> {
        vec![
            "Импорт URL",
            "Начать запись",
            "Остановить запись",
            "Поиск",
            "Вопрос к архиву",
            "Новая заметка",
            "Предложить слот",
            "Новый вопрос",
            "Вопрос из файла",
            "Выбрать версию",
            "Изменить язык",
            "Имя участника",
            "Имя источника",
            "Повторить сводку",
            "Повторить текст",
            "Повторить всё",
            "Подтвердить документ",
            "Удалить запись",
            "Удалить заметку",
            "Удалить вопрос",
            "Изменить настройку",
            "Сбросить настройку",
            "Копировать документ",
            "Сохранить документ",
            "Play / pause",
            "Слушать фрагмент",
            "Перейти ко времени",
            "Светлая / тёмная тема",
            "ASCII / Unicode",
            "Без цвета / цвет",
            "Обновить",
            "Подключение и токен",
            "Сохранить изменённые настройки",
            "16 цветов / true color",
            "Фильтр журнала",
            "Восстановить черновик после ошибки",
            "Фильтр настроек",
            "Отменить черновик настроек",
        ]
    }
    pub(super) fn open(&mut self, title: &str, kind: FormKind, fields: &[(&str, &str)]) {
        self.form = Some(Form {
            title: title.into(),
            kind,
            fields: fields
                .iter()
                .map(|(label, value)| Field {
                    label: (*label).into(),
                    value: (*value).into(),
                    initial: (*value).into(),
                    secret: false,
                    editor: {
                        let mut editor = tui_textarea::TextArea::new(
                            value.split('\n').map(str::to_owned).collect(),
                        );
                        editor.set_max_histories(32);
                        editor
                    },
                })
                .collect(),
            active: 0,
            error: None,
            session: self.current.clone(),
        });
    }
    pub(super) fn confirm(&mut self, title: &str, task: Task) {
        self.open(
            title,
            FormKind::Confirm(task),
            &[("Введите ДА для подтверждения", "")],
        );
    }
    pub(super) fn action(&mut self, t: Task) {
        let resource = crate::io::action_resource(&t);
        if self.pending_actions.insert(resource) {
            if matches!(
                t,
                Task::Best(..)
                    | Task::Lang(..)
                    | Task::Speaker(..)
                    | Task::RenameParticipant(..)
                    | Task::Source(..)
                    | Task::Recook(..)
                    | Task::Confirm(..)
            ) {
                self.pending_document = Some(self.epoch);
            }
            self.pending_context.insert(resource, self.epoch);
            self.effects.push((resource, t));
        } else {
            self.status = "Дождитесь результата предыдущего действия этого типа".into();
        }
    }
    pub(super) fn restore_failed_form(&mut self, resource: Resource) {
        if let Some(draft) = self.pending_forms.remove(&resource) {
            if self.form.is_none() {
                self.form = Some(draft);
            } else {
                if self.failed_drafts.len() >= 8 {
                    self.failed_drafts.pop_front();
                }
                self.failed_drafts.push_back(draft);
            }
        }
    }
    pub fn submission_rejected(&mut self, resource: Resource) {
        if self.pending_actions.remove(&resource) || self.pending_forms.contains_key(&resource) {
            self.pending_context.remove(&resource);
            if resource == Resource::Action {
                self.settings_pending = None;
                self.pending_document = None;
            }
            if resource == Resource::Control {
                self.pending_record = None;
            }
            self.restore_failed_form(resource);
            self.status = "Очередь запросов занята; введённый текст сохранён".into();
        }
    }
    pub fn command(&mut self, index: usize) {
        self.palette = false;
        let s = self.current.clone().unwrap_or_default();
        match index {
            0 => self.open("Импорт", FormKind::Import, &[("URL", "")]),
            1 => self.open("Начать запись", FormKind::Start, &[("Название", "")]),
            2 => {
                self.pending_record = Some(false);
                self.pending_record_at = self.now_ms;
                self.action(Task::Stop);
            }
            3 => {
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
            4 => {
                let query = self.chat_query.clone();
                self.open("Вопрос к архиву", FormKind::Chat, &[("Вопрос", &query)]);
            }
            5 => {
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
            6 => {
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
                        self.command(5);
                    }
                    self.status = "Сначала напишите заметку; Ctrl+R предложит слот".into();
                }
            }
            7 | 8 => self.open(
                "Спросить",
                if index == 8 {
                    FormKind::AskFile
                } else {
                    FormKind::Ask
                },
                &[
                    (
                        if index == 8 {
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
            9 if !s.is_empty() => {
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
            10 if !s.is_empty() => {
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
            11 => {
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
            12 if !s.is_empty() => self.open(
                &format!("Имя источника · {}", self.current_title()),
                FormKind::Source,
                &[("ID источника", "0"), ("Имя (пустое — сброс)", "")],
            ),
            13..=15 if !s.is_empty() => self.confirm(
                &format!("Повторить: {}", self.current_title()),
                Task::Recook(
                    s,
                    match index {
                        13 => Scope::Summary,
                        14 => Scope::Text,
                        _ => Scope::All,
                    },
                ),
            ),
            16 if !s.is_empty()
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
            17 if !s.is_empty() => self.confirm(
                &format!("Удалить навсегда: {}", self.current_title()),
                Task::DeleteSession(s),
            ),
            18 => {
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
            19 => {
                if let Some(a) = &self.ask {
                    self.confirm(
                        &format!("Удалить вопрос {}", a.id),
                        Task::DeleteAsk(a.id.clone()),
                    );
                }
            }
            20 => {
                if let Some(s) = self.selected_setting() {
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
                            .unwrap_or_default()
                    };
                    let title = format!("{} · {}", s.label, s.key);
                    let hint = format!(
                        "Новое значение · {}{}",
                        s.hint,
                        s.options
                            .as_ref()
                            .map(|o| format!(
                                " · {}",
                                o.iter()
                                    .map(|v| format!("{}={}", v.value, v.label))
                                    .collect::<Vec<_>>()
                                    .join("; ")
                            ))
                            .unwrap_or_default()
                    );
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
            21 => {
                if let Some(s) = self.selected_setting() {
                    self.settings_edits.insert(s.key.clone(), None);
                    self.status =
                        "Сброс добавлен в черновик настроек; сохраните изменения через палитру"
                            .into();
                }
            }
            22 => {
                let text = self.copy_text();
                if text.trim().is_empty() {
                    self.status = "Нет документа для копирования".into();
                } else {
                    self.action(Task::Copy(text));
                }
            }
            23 => {
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
            24 => {
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
            25 => {
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
            26 => self.open(
                "Перейти ко времени",
                FormKind::Seek,
                &[("Секунды или mm:ss", &timecode(self.playback.position))],
            ),
            27 => self.light = !self.light,
            28 => self.ascii = !self.ascii,
            29 => self.monochrome = !self.monochrome,
            30 => self.refresh(),
            31 => {
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
            32 => {
                if self.settings_edits.is_empty() {
                    self.status = "Нет изменённых настроек".into();
                } else {
                    self.confirm(
                        &format!("Сохранить {} изменённых ключей", self.settings_edits.len()),
                        Task::SaveSettings(self.settings_edits.clone()),
                    );
                }
            }
            33 => self.color16 = !self.color16,
            34 => {
                let filter = self.journal.filter.clone();
                self.open(
                    "Фильтр журнала",
                    FormKind::JournalFilter,
                    &[("Строка (пусто — все события)", &filter)],
                );
            }
            35 => {
                if self.form.is_none() {
                    self.form = self.failed_drafts.pop_back();
                } else {
                    self.status = "Закройте текущую форму перед восстановлением черновика".into();
                }
            }
            36 => {
                let filter = self.settings_filter.clone();
                self.open(
                    "Фильтр настроек",
                    FormKind::SettingsFilter,
                    &[("Ключ, группа или описание", &filter)],
                );
            }
            37 if !self.settings_edits.is_empty() => self.open(
                "Отменить несохранённые настройки",
                FormKind::DiscardSettings,
                &[("Введите ДА для отмены черновика", "")],
            ),
            _ => self.status = "Сначала выберите запись или элемент".into(),
        }
    }
    pub fn submit_form(&mut self) -> Result<(), String> {
        let Some(f) = self.form.clone() else {
            return Ok(());
        };
        let retained = f.clone();
        let resource = if matches!(f.kind, FormKind::Start) {
            Resource::Control
        } else {
            Resource::Action
        };
        if matches!(
            f.kind,
            FormKind::Import
                | FormKind::Start
                | FormKind::Note
                | FormKind::Ask
                | FormKind::AskFile
                | FormKind::Best
                | FormKind::Lang
                | FormKind::Participant(_)
                | FormKind::Source
                | FormKind::Export(_)
                | FormKind::Confirm(_)
        ) && self.pending_actions.contains(&resource)
        {
            return Err("Дождитесь результата предыдущего действия".into());
        }
        let get = |i: usize| f.fields.get(i).map(|f| f.value.clone()).unwrap_or_default();
        let s = f.session.clone().unwrap_or_default();
        let task = match f.kind {
            FormKind::Import => {
                let url = get(0);
                if !(url.starts_with("https://") || url.starts_with("http://")) {
                    return Err("Нужна HTTP(S) ссылка".into());
                }
                Task::Ingest(url)
            }
            FormKind::Start => {
                if !self.record.engine {
                    return Err("Движок записи недоступен".into());
                }
                self.pending_record = Some(true);
                self.pending_record_at = self.now_ms;
                Task::Start(get(0))
            }
            FormKind::Search => {
                if get(0).trim().is_empty() {
                    return Err("Введите запрос".into());
                }
                if !matches!(get(1).as_str(), "lexical" | "semantic" | "hybrid") {
                    return Err("Режим: lexical, semantic или hybrid".into());
                }
                self.search_query = get(0);
                self.search_mode = get(1);
                self.switch(View::Search);
                self.pending_forms.insert(Resource::Search, retained);
                self.effects.push((
                    Resource::Search,
                    Task::Search(
                        get(0),
                        if get(1) == "lexical" {
                            String::new()
                        } else {
                            get(1)
                        },
                    ),
                ));
                self.form = None;
                return Ok(());
            }
            FormKind::Chat => {
                if get(0).trim().is_empty() {
                    return Err("Введите вопрос".into());
                }
                self.chat_query = get(0);
                self.switch(View::Chat);
                self.pending_forms.insert(Resource::Chat, retained);
                self.effects.push((Resource::Chat, Task::Chat(get(0))));
                self.form = None;
                return Ok(());
            }
            FormKind::Note => {
                if get(1).trim().is_empty() {
                    return Err("Заметка пуста".into());
                }
                Task::Note(get(1), get(0))
            }
            FormKind::Ask => {
                if !matches!(get(1).as_str(), "ollama" | "claude") {
                    return Err("Выберите ollama или claude".into());
                }
                if get(0).trim().is_empty() || get(0).chars().count() > 400_000 {
                    return Err("Нужно от 1 до 400000 символов материала".into());
                }
                Task::CreateAsk(AskRequest {
                    text: get(0),
                    provider: get(1),
                    prompt: get(2),
                    input_name: Some(get(3)),
                })
            }
            FormKind::AskFile => {
                if !matches!(get(1).as_str(), "ollama" | "claude") {
                    return Err("Выберите провайдера явно".into());
                }
                Task::CreateAskFile {
                    path: get(0),
                    provider: get(1),
                    prompt: get(2),
                    input_name: get(3),
                }
            }
            FormKind::Best => Task::Best(s, get(0).parse().map_err(|_| "ID должен быть числом")?),
            FormKind::Lang => Task::Lang(s, get(0)),
            FormKind::Participant(p) => {
                if p.voices.is_empty() && p.sources.is_empty() {
                    return Err("У участника нет доступных меток голоса или источников".into());
                }
                if get(0).trim().is_empty() && !p.voices.is_empty() {
                    return Err("Укажите имя участника".into());
                }
                Task::RenameParticipant(s, p, get(0))
            }
            FormKind::Export(text) => {
                if get(0).trim().is_empty() {
                    return Err("Укажите новый путь к файлу".into());
                }
                Task::Export { path: get(0), text }
            }
            FormKind::Source => Task::Source(
                s,
                get(0).parse().map_err(|_| "ID должен быть числом")?,
                get(1),
            ),
            FormKind::Confirm(t) => {
                if get(0) != "ДА" {
                    return Err("Введите ДА или Esc для отмены".into());
                }
                t
            }
            FormKind::Setting(key) => {
                let value = get(0);
                if f.fields.first().is_some_and(|field| field.initial == value) {
                    self.form = None;
                    return Ok(());
                }
                if let Some(setting) = self.settings.settings.iter().find(|s| s.key == key) {
                    if !value.is_empty() {
                        if setting.kind == "bool"
                            && !matches!(
                                value.as_str(),
                                "true" | "false" | "on" | "off" | "1" | "0"
                            )
                        {
                            return Err("Введите on/off или true/false".into());
                        }
                        if setting.kind == "number"
                            && !value.parse::<f64>().is_ok_and(f64::is_finite)
                        {
                            return Err("Введите конечное число".into());
                        }
                        if let Some(options) = &setting.options {
                            if !options.iter().any(|o| o.value == value) {
                                return Err("Выберите одно из значений каталога".into());
                            }
                        }
                    }
                }
                self.settings_edits
                    .insert(key, (!value.is_empty()).then_some(value));
                self.form = None;
                self.status = "Изменение в черновике; сохраните настройки через палитру".into();
                return Ok(());
            }
            FormKind::Connection => {
                if !self.settings_edits.is_empty() {
                    return Err("Сначала сохраните черновик настроек текущего сервера".into());
                }
                if !matches!(get(2).as_str(), "true" | "false") {
                    return Err("Очистить токен: true или false".into());
                }
                let token = if get(2) == "true" {
                    Some(String::new())
                } else if get(1).is_empty() {
                    None
                } else {
                    Some(get(1))
                };
                localvox_light_client::Client::new(&get(0), token.clone().unwrap_or_default())
                    .map_err(|e| e.to_string())?;
                self.connection_request = Some((get(0), token));
                self.form = None;
                return Ok(());
            }
            FormKind::JournalFilter => {
                self.journal.filter = get(0);
                self.journal.follow = false;
                self.item = 0;
                self.scroll = 0;
                self.form = None;
                return Ok(());
            }
            FormKind::SettingsFilter => {
                self.settings_filter = get(0);
                self.selected = 0;
                self.item = 0;
                self.scroll = 0;
                self.form = None;
                return Ok(());
            }
            FormKind::DiscardSettings => {
                if get(0) != "ДА" {
                    return Err("Введите ДА или Esc для отмены".into());
                }
                if self.settings_pending.is_some() {
                    return Err("Дождитесь результата сохранения настроек".into());
                }
                self.settings_edits.clear();
                self.form = None;
                self.status = "Несохранённые изменения отменены".into();
                return Ok(());
            }
            FormKind::Seek => {
                let time = parse_timecode(&get(0)).ok_or("Нужны секунды или mm:ss")?;
                if f.session != self.current || self.current.is_none() {
                    return Err("Выберите запись заново".into());
                }
                self.seek_to(time);
                self.form = None;
                return Ok(());
            }
        };
        if let Task::SaveSettings(set) = &task {
            self.settings_pending = Some(set.clone());
        }
        self.pending_forms.insert(resource, retained);
        self.action(task);
        self.form = None;
        Ok(())
    }
}
