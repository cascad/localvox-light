//! Forms, validation and explicit user actions. No I/O.
use super::*;

impl App {
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
            self.action_diagnostics
                .insert(resource, self.action_diagnostic(&t));
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
            if matches!(t, Task::Retry(..)) {
                self.pending_retry = Some(self.epoch);
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
        self.action_diagnostics.remove(&resource);
        if self.pending_actions.remove(&resource) || self.pending_forms.contains_key(&resource) {
            self.pending_context.remove(&resource);
            if resource == Resource::Action {
                self.settings_pending = None;
                self.pending_document = None;
                self.pending_retry = None;
            }
            if resource == Resource::Control {
                self.pending_record = None;
            }
            self.restore_failed_form(resource);
            self.status = "Очередь запросов занята; введённый текст сохранён".into();
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
            FormKind::LlmServer => {
                let url = localvox_light_client::normalize_llm_server(&get(0))
                    .map_err(|error| error.to_string())?;
                let mut servers = self.llm_servers();
                if !servers.contains(&url) {
                    if servers.len() >= 32 {
                        return Err("Не больше 32 серверов".into());
                    }
                    servers.push(url.clone());
                }
                self.settings_edits.insert(
                    "LOCALVOX_LLM_SERVER_URLS".into(),
                    Some(serde_json::to_string(&servers).map_err(|error| error.to_string())?),
                );
                self.settings_edits
                    .insert("LOCALVOX_LLM_BASE_URL".into(), Some(url));
                self.status = "Сервер в черновике · S сохранить настройки".into();
                self.form = None;
                return Ok(());
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
            FormKind::SettingChoice {
                key,
                options,
                selected,
            } => {
                let value = options.get(selected).ok_or("Выберите значение")?.1.clone();
                let saved = self
                    .settings
                    .settings
                    .iter()
                    .find(|setting| setting.key == key)
                    .and_then(|setting| setting.value.clone());
                if value != saved {
                    self.settings_edits.insert(key.clone(), value);
                } else {
                    self.settings_edits.remove(&key);
                }
                self.status = if self.settings_edits.contains_key(&key) {
                    "Изменение в черновике · S сохранить настройки"
                } else {
                    "Значение не изменено"
                }
                .into();
                self.form = None;
                return Ok(());
            }
            FormKind::Seek => {
                let mut parts = [0u32; 3];
                for (i, part) in parts.iter_mut().enumerate() {
                    let raw = get(i);
                    if raw.is_empty() || !raw.bytes().all(|c| c.is_ascii_digit()) {
                        return Err("Введите часы, минуты и секунды цифрами".into());
                    }
                    *part = raw.parse::<u32>().map_err(|_| "Слишком большое значение")?;
                    if *part > if i == 0 { 999 } else { 59 } {
                        return Err("Минуты и секунды: 0–59, часы: 0–999".into());
                    }
                }
                let time = f64::from(parts[0] * 3600 + parts[1] * 60 + parts[2]);
                if self
                    .media_duration()
                    .is_some_and(|duration| time > duration)
                {
                    return Err(format!(
                        "Позиция должна быть не позже {}",
                        timecode(self.media_duration().unwrap())
                    ));
                }
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
