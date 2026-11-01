//! Deterministic workspace state. I/O results carry the selection epoch.
mod documents;
mod forms;
use crate::io::{Data, Resource, Response, Task};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use localvox_light_client::{safe_text, types::*, Scope};
use std::collections::{BTreeMap, HashMap, HashSet};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Archive,
    Notes,
    Queue,
    Ask,
    Settings,
    Search,
    Chat,
}
pub const VIEWS: [(View, &str); 7] = [
    (View::Archive, "Архив"),
    (View::Notes, "Заметки"),
    (View::Queue, "Очередь"),
    (View::Ask, "Спросить"),
    (View::Settings, "Настройки"),
    (View::Search, "Поиск"),
    (View::Chat, "Чат с архивом"),
];
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Summary,
    Transcript,
    Readable,
    Speakers,
    Progress,
}
pub const TABS: [(Tab, &str); 5] = [
    (Tab::Summary, "Сводка"),
    (Tab::Transcript, "Реплики"),
    (Tab::Readable, "Текст"),
    (Tab::Speakers, "Участники"),
    (Tab::Progress, "Ход"),
];
#[derive(Clone)]
pub struct Field {
    pub label: String,
    pub value: String,
    pub initial: String,
    pub secret: bool,
    pub editor: tui_textarea::TextArea<'static>,
}
#[derive(Clone)]
pub struct Form {
    pub title: String,
    pub kind: FormKind,
    pub fields: Vec<Field>,
    pub active: usize,
    pub error: Option<String>,
    pub session: Option<String>,
}
#[derive(Clone)]
pub enum FormKind {
    Import,
    Start,
    Search,
    Chat,
    Note,
    Ask,
    AskFile,
    Lang,
    Best,
    Participant(Participant),
    Source,
    Export(String),
    Confirm(Task),
    Setting(String),
    Connection,
    JournalFilter,
    SettingsFilter,
    DiscardSettings,
    Seek,
}

pub struct App {
    pub view: View,
    pub tab: Tab,
    pub focus: usize,
    pub nav: usize,
    pub selected: usize,
    pub item: usize,
    pub epoch: u64,
    pub sessions: Vec<Session>,
    pub current: Option<String>,
    pub record: RecordState,
    pub jobs: Jobs,
    pub queue: Vec<QueueItem>,
    pub summary: Summary,
    pub transcript: Vec<TranscriptLine>,
    pub readable: ReadableDoc,
    pub versions: Vec<Version>,
    pub speakers: Vec<Participant>,
    pub sources: Vec<SourceName>,
    pub languages: Vec<String>,
    pub progress: Progress,
    pub journal: crate::journal::Journal,
    pub notes: Vec<SlotNotes>,
    pub asks: Vec<AskRecord>,
    pub ask: Option<AskRecord>,
    pub ask_id: Option<String>,
    pub settings: SettingsDoc,
    pub settings_edits: BTreeMap<String, Option<String>>,
    pub settings_pending: Option<BTreeMap<String, Option<String>>>,
    pub settings_filter: String,
    pub search_query: String,
    pub search_mode: String,
    pub chat_query: String,
    pub search_results_query: String,
    pub chat_answer_query: String,
    pub document_target: Option<f64>,
    pub connection_request: Option<(String, Option<String>)>,
    pub address: String,
    pub route_hints: Vec<String>,
    pub route_text: Option<String>,
    pub hits: Vec<Hit>,
    pub answer: Answer,
    pub form: Option<Form>,
    pub pending_forms: HashMap<Resource, Form>,
    pub pending_actions: HashSet<Resource>,
    pub pending_context: HashMap<Resource, u64>,
    pub pending_document: Option<u64>,
    pub failed_drafts: std::collections::VecDeque<Form>,
    pub palette: bool,
    pub help: bool,
    pub palette_index: usize,
    pub scroll: u16,
    pub status: String,
    pub errors: HashMap<Resource, String>,
    pub quit: bool,
    pub ascii: bool,
    pub monochrome: bool,
    pub color16: bool,
    pub light: bool,
    pub expanded: bool,
    pub follow_selection: bool,
    pub pending_record: Option<bool>,
    pub pending_record_at: u64,
    pub now_ms: u64,
    pub wall_ms: i64,
    pub viewport: (u16, u16),
    pub playback: crate::player::PlaybackState,
    pub player_effects: Vec<crate::player::PlaybackCommand>,
    pub peaks: Peaks,
    pub peaks_loaded: bool,
    pub peaks_retry_at: u64,
    pub last_request: HashMap<Resource, u64>,
    pub effects: Vec<(Resource, Task)>,
}
impl Default for App {
    fn default() -> Self {
        Self {
            view: View::Archive,
            tab: Tab::Summary,
            focus: 1,
            nav: 0,
            selected: 0,
            item: 0,
            epoch: 1,
            sessions: vec![],
            current: None,
            record: RecordState::default(),
            jobs: Jobs::default(),
            queue: vec![],
            summary: Summary::default(),
            transcript: vec![],
            readable: ReadableDoc::default(),
            versions: vec![],
            speakers: vec![],
            sources: vec![],
            languages: vec![],
            progress: Progress::default(),
            journal: crate::journal::Journal::new(),
            notes: vec![],
            asks: vec![],
            ask: None,
            ask_id: None,
            settings: SettingsDoc::default(),
            settings_edits: BTreeMap::new(),
            settings_pending: None,
            settings_filter: String::new(),
            search_query: String::new(),
            search_mode: "lexical".into(),
            chat_query: String::new(),
            search_results_query: String::new(),
            chat_answer_query: String::new(),
            document_target: None,
            connection_request: None,
            address: String::new(),
            route_hints: vec![],
            route_text: None,
            hits: vec![],
            answer: Answer::default(),
            form: None,
            pending_forms: HashMap::new(),
            pending_actions: HashSet::new(),
            pending_context: HashMap::new(),
            pending_document: None,
            failed_drafts: std::collections::VecDeque::new(),
            palette: false,
            help: false,
            palette_index: 0,
            scroll: 0,
            status: "Подключение…".into(),
            errors: HashMap::new(),
            quit: false,
            ascii: false,
            monochrome: false,
            color16: false,
            light: false,
            expanded: false,
            follow_selection: true,
            pending_record: None,
            pending_record_at: 0,
            now_ms: 0,
            wall_ms: 0,
            viewport: (120, 36),
            playback: crate::player::PlaybackState {
                paused: true,
                idle: true,
                ..Default::default()
            },
            player_effects: vec![],
            peaks: Peaks::default(),
            peaks_loaded: false,
            peaks_retry_at: 0,
            last_request: HashMap::new(),
            effects: vec![],
        }
    }
}

impl App {
    pub fn refresh(&mut self) {
        self.read(Resource::Sessions, String::new());
        self.read(Resource::Record, String::new());
        self.read(Resource::Jobs, String::new());
        self.refresh_view();
    }
    fn read(&mut self, r: Resource, s: String) {
        self.effects.push((r, Task::Read(r, s)));
    }
    pub fn refresh_view(&mut self) {
        match self.view {
            View::Archive => {
                if let Some(s) = self.current.clone() {
                    if !self.peaks_loaded && self.now_ms >= self.peaks_retry_at {
                        self.read(Resource::Peaks, s.clone());
                    }
                    let r = match self.tab {
                        Tab::Summary => Resource::Summary,
                        Tab::Transcript => Resource::Transcript,
                        Tab::Readable => Resource::Readable,
                        Tab::Speakers => Resource::Speakers,
                        Tab::Progress => Resource::Progress,
                    };
                    self.read(r, s.clone());
                    if self.tab == Tab::Progress {
                        self.effects.push((
                            Resource::Journal,
                            Task::Journal(
                                s.clone(),
                                (!self.journal.after.is_empty())
                                    .then(|| self.journal.after.clone()),
                                None,
                            ),
                        ));
                    }
                    self.read(Resource::Versions, s.clone());
                    self.read(Resource::Sources, s);
                }
            }
            View::Notes => self.read(Resource::Notes, String::new()),
            View::Queue => self.read(Resource::Queue, String::new()),
            View::Ask => {
                self.read(Resource::Asks, String::new());
                if let Some(id) = &self.ask_id {
                    self.read(Resource::Ask, id.clone());
                }
            }
            View::Settings => self.read(Resource::Settings, String::new()),
            _ => {}
        }
    }
    pub fn switch(&mut self, v: View) {
        self.view = v;
        self.nav = VIEWS.iter().position(|(view, _)| *view == v).unwrap_or(0);
        self.epoch += 1;
        self.item = 0;
        self.selected = 0;
        self.scroll = 0;
        self.errors.clear();
        self.refresh_view();
    }
    pub fn select_session(&mut self, name: String) {
        if self.current.as_ref() != Some(&name) {
            self.player_effects
                .push(crate::player::PlaybackCommand::Stop);
            self.playback = crate::player::PlaybackState {
                paused: true,
                idle: true,
                ..Default::default()
            };
            self.peaks = Peaks::default();
            self.peaks_loaded = false;
            self.peaks_retry_at = 0;
            self.epoch += 1;
            self.current = Some(name.clone());
            self.document_target = None;
            self.summary = Summary::default();
            self.transcript.clear();
            self.readable = ReadableDoc::default();
            self.speakers.clear();
            self.versions.clear();
            self.sources.clear();
            self.progress = Progress::default();
            self.journal = crate::journal::Journal::new();
            self.scroll = 0;
            self.item = 0;
            self.errors.clear();
        }
        self.view = View::Archive;
        self.nav = 0;
        self.selected = self
            .sessions
            .iter()
            .position(|s| s.name == name)
            .unwrap_or(0);
        self.refresh_view();
    }
    pub fn apply(&mut self, r: Response) {
        let global = matches!(
            r.resource,
            Resource::Sessions
                | Resource::Record
                | Resource::Jobs
                | Resource::Action
                | Resource::Control
                | Resource::Route
        );
        if !global && r.epoch != self.epoch {
            return;
        }
        if self
            .last_request
            .get(&r.resource)
            .is_some_and(|id| *id >= r.id)
        {
            return;
        }
        self.last_request.insert(r.resource, r.id);
        let data = match r.result {
            Ok(d) => {
                self.errors.remove(&r.resource);
                d
            }
            Err(e) => {
                if r.resource == Resource::Peaks {
                    self.peaks_retry_at = self.now_ms.saturating_add(30_000);
                }
                self.pending_actions.remove(&r.resource);
                self.pending_context.remove(&r.resource);
                if r.resource == Resource::Action {
                    self.settings_pending = None;
                    if self.pending_document.take() == Some(self.epoch) {
                        // Multi-label rename may have partially succeeded. Keep
                        // the current document until an authoritative read arrives.
                        self.epoch += 1;
                        self.refresh_view();
                    }
                }
                if r.resource == Resource::Control {
                    self.pending_record = None;
                }
                self.restore_failed_form(r.resource);
                self.errors.insert(r.resource, safe_text(&e));
                self.status = format!("Ошибка: {}", safe_text(&e));
                return;
            }
        };
        let action_context = self.pending_context.remove(&r.resource);
        self.pending_actions.remove(&r.resource);
        let completed_form = self.pending_forms.remove(&r.resource);
        if self.status == "Подключение…" {
            self.status = "Данные обновлены".into();
        }
        let response_id = r.id;
        match data {
            Data::Sessions(s) => {
                let selected = self.sessions.get(self.selected).map(|s| s.name.clone());
                self.sessions = s;
                if self.view == View::Archive {
                    self.selected = selected
                        .and_then(|name| self.sessions.iter().position(|s| s.name == name))
                        .unwrap_or_else(|| {
                            self.selected.min(self.sessions.len().saturating_sub(1))
                        });
                }
                if self.current.is_none() && self.view == View::Archive {
                    if let Some(s) = self.sessions.first() {
                        self.select_session(s.name.clone());
                    }
                }
            }
            Data::Record(r) => {
                if self.pending_record == Some(r.recording) {
                    self.pending_record = None;
                }
                self.record = r;
            }
            Data::Jobs(j) => self.jobs = j,
            Data::Queue(q) => {
                let selected = self.queue.get(self.selected).map(|q| q.session.clone());
                self.queue = q;
                if self.view == View::Queue {
                    self.selected = selected
                        .and_then(|name| self.queue.iter().position(|q| q.session == name))
                        .unwrap_or_else(|| self.selected.min(self.queue.len().saturating_sub(1)));
                }
            }
            Data::Summary(s) => self.summary = s,
            Data::Transcript(t) => {
                self.transcript = t;
                if let Some(time) = self.document_target.take() {
                    self.item = self
                        .transcript
                        .iter()
                        .position(|line| line.end_sec > time)
                        .unwrap_or_else(|| self.transcript.len().saturating_sub(1));
                    self.scroll = 0;
                    self.follow_selection = true;
                }
            }
            Data::Readable(t) => self.readable = t,
            Data::Versions(v) => self.versions = v,
            Data::Speakers(s) => self.speakers = s,
            Data::Sources(s) => self.sources = s,
            Data::Languages(languages) => {
                self.languages = languages;
                if let Some(form) = self
                    .form
                    .as_mut()
                    .filter(|f| matches!(f.kind, FormKind::Lang))
                {
                    form.fields[0].label =
                        format!("Код языка · доступно: {} · auto", self.languages.join(", "));
                }
            }
            Data::Progress(p) => {
                let anchor = crate::render::journal_anchor(self);
                self.progress = p;
                crate::render::restore_journal_anchor(self, anchor);
            }
            Data::Peaks(p) => {
                self.peaks = p;
                self.peaks_loaded = true;
            }
            Data::Journal(page, prepend) => {
                let selected = self
                    .journal
                    .visible_groups()
                    .get(self.item)
                    .map(|g| g.id.clone());
                let anchor = crate::render::journal_anchor(self);
                self.journal.merge(page, prepend);
                if let Some(id) = selected {
                    if let Some(index) = self
                        .journal
                        .visible_groups()
                        .iter()
                        .position(|g| g.id == id)
                    {
                        self.item = index;
                    }
                }
                crate::render::restore_journal_anchor(self, anchor);
            }
            Data::Notes(n) => {
                let selected = self.notes.get(self.selected).map(|s| s.name.clone());
                let note = self
                    .notes
                    .get(self.selected)
                    .and_then(|s| s.notes.get(self.item))
                    .map(|n| (n.source.clone(), n.raw.clone()));
                self.notes = n;
                if self.view == View::Notes {
                    self.selected = selected
                        .and_then(|name| self.notes.iter().position(|s| s.name == name))
                        .unwrap_or_else(|| self.selected.min(self.notes.len().saturating_sub(1)));
                    self.item = note
                        .and_then(|(source, raw)| {
                            self.notes.get(self.selected).and_then(|s| {
                                s.notes
                                    .iter()
                                    .position(|n| n.source == source && n.raw == raw)
                            })
                        })
                        .unwrap_or_else(|| self.item.min(self.selection_len().saturating_sub(1)));
                }
            }
            Data::Asks(a) => {
                let selected = self.asks.get(self.selected).map(|a| a.id.clone());
                self.asks = a;
                if self.view == View::Ask {
                    self.selected = selected
                        .and_then(|id| self.asks.iter().position(|a| a.id == id))
                        .unwrap_or_else(|| self.selected.min(self.asks.len().saturating_sub(1)));
                }
            }
            Data::Ask(a) => {
                if r.resource == Resource::Action {
                    if action_context == Some(self.epoch) {
                        self.ask_id = Some(a.id.clone());
                        self.ask = Some(a);
                        self.switch(View::Ask);
                        self.focus = 2;
                    }
                } else if self.ask_id.as_ref() == Some(&a.id) {
                    self.ask = Some(a);
                }
            }
            Data::Settings(s) => {
                let key = self.selected_setting().map(|s| s.key.clone());
                self.settings = s;
                self.selected = key
                    .and_then(|key| self.visible_settings().iter().position(|s| s.key == key))
                    .unwrap_or_else(|| {
                        self.selected
                            .min(self.visible_settings().len().saturating_sub(1))
                    });
            }
            Data::Search(h) => {
                self.hits = h;
                self.search_results_query = self.search_query.clone();
            }
            Data::Chat(a) => {
                self.answer = a;
                self.chat_answer_query = self.chat_query.clone();
            }
            Data::Route(v) => {
                if self.form.as_ref().is_some_and(|f| {
                    matches!(f.kind, FormKind::Note)
                        && f.fields.get(1).map(|v| &v.value) == self.route_text.as_ref()
                }) {
                    self.route_hints = v
                        .iter()
                        .take(10)
                        .map(|hint| {
                            safe_text(
                                &hint
                                    .reason
                                    .as_ref()
                                    .map(|reason| format!("{}: {reason}", hint.slot))
                                    .unwrap_or_else(|| hint.slot.clone()),
                            )
                        })
                        .collect();
                    self.status = "Слоты предложены; выберите назначение в форме".into();
                }
            }
            Data::Reply(r) => {
                let changes_document = self.pending_document.take() == Some(self.epoch);
                if changes_document && action_context == Some(self.epoch) {
                    self.epoch += 1;
                    self.summary = Summary::default();
                    self.transcript.clear();
                    self.readable = ReadableDoc::default();
                    self.versions.clear();
                    self.speakers.clear();
                    self.sources.clear();
                }
                if let Some(Form {
                    kind: FormKind::Confirm(task),
                    ..
                }) = completed_form.as_ref()
                {
                    match task {
                        Task::DeleteSession(target) if r.deleted.as_ref() == Some(target) => {
                            self.last_request.insert(Resource::Sessions, response_id);
                            self.sessions.retain(|s| &s.name != target);
                            if self.current.as_ref() == Some(target) {
                                self.current = None;
                                self.summary = Summary::default();
                                self.transcript.clear();
                                self.readable = ReadableDoc::default();
                                self.speakers.clear();
                                self.versions.clear();
                                self.sources.clear();
                                self.progress = Progress::default();
                                self.journal = crate::journal::Journal::new();
                                self.peaks = Peaks::default();
                                self.epoch += 1;
                                self.player_effects
                                    .push(crate::player::PlaybackCommand::Stop);
                                if self.view == View::Archive {
                                    if let Some(next) = self.sessions.first() {
                                        self.select_session(next.name.clone());
                                    }
                                }
                            }
                        }
                        Task::DeleteAsk(target) if r.deleted.as_ref() == Some(target) => {
                            self.last_request.insert(Resource::Asks, response_id);
                            self.asks.retain(|a| &a.id != target);
                            if self.ask_id.as_ref() == Some(target) {
                                self.ask = None;
                                self.ask_id = None;
                                self.epoch += 1;
                            }
                        }
                        Task::DeleteNote(slot, note) if r.deleted.as_ref() == Some(&note.raw) => {
                            self.last_request.insert(Resource::Notes, response_id);
                            if let Some(slot) = self.notes.iter_mut().find(|s| &s.name == slot) {
                                if let Some(index) = slot
                                    .notes
                                    .iter()
                                    .position(|n| n.raw == note.raw && n.source == note.source)
                                {
                                    slot.notes.remove(index);
                                }
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(saved) = self.settings_pending.take() {
                    for (key, value) in saved {
                        if self.settings_edits.get(&key) == Some(&value) {
                            self.settings_edits.remove(&key);
                        }
                    }
                }
                self.status = r.msg.or(r.dest).unwrap_or_else(|| "Запрос принят".into());
                if !r.need_restart.is_empty() {
                    self.status = format!(
                        "Сохранено; нужен перезапуск демона: {}",
                        r.need_restart.join(", ")
                    );
                }
                if let Some(s) = r.session {
                    if !s.is_empty() && action_context == Some(self.epoch) {
                        if completed_form
                            .as_ref()
                            .is_some_and(|f| matches!(f.kind, FormKind::Import))
                        {
                            self.tab = Tab::Progress;
                            self.focus = 2;
                        }
                        self.select_session(s);
                    }
                }
                self.refresh();
            }
        }
    }
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if self.pending_record.is_some() && now_ms.saturating_sub(self.pending_record_at) > 10_000 {
            self.pending_record = None;
            self.status = "Подтверждение записи не получено — проверьте состояние движка".into();
        }
    }
    pub fn current_title(&self) -> String {
        self.sessions
            .iter()
            .find(|s| Some(&s.name) == self.current.as_ref())
            .map(|s| safe_text(s.title()))
            .unwrap_or_else(|| {
                self.current
                    .as_deref()
                    .map(safe_text)
                    .unwrap_or_else(|| "Выберите запись".into())
            })
    }
    pub fn paste(&mut self, text: &str) {
        if let Some(f) = &mut self.form {
            let field = &mut f.fields[f.active];
            let remaining = 400_000usize.saturating_sub(field.value.chars().count());
            let paste: String = text
                .chars()
                .filter(|c| !c.is_control() || matches!(*c, '\n' | '\t'))
                .take(remaining)
                .collect();
            field.editor.insert_str(&paste);
            field.value = field.editor.lines().join("\n");
        }
        self.discard_stale_route_hints();
    }
    fn discard_stale_route_hints(&mut self) {
        if self.form.as_ref().is_some_and(|form| {
            matches!(form.kind, FormKind::Note)
                && form.fields.get(1).map(|f| &f.value) != self.route_text.as_ref()
        }) {
            self.route_hints.clear();
        }
    }
    pub fn key(&mut self, key: KeyEvent) {
        if (key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('q' | 'Q' | 'й' | 'Й')))
            || key.code == KeyCode::Char('\u{11}')
            || (key.code == KeyCode::F(10) && key.modifiers.is_empty())
        {
            self.quit = true;
            return;
        }
        if self.form.is_some() {
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(key.code, KeyCode::Char('r' | 'R' | 'к' | 'К'))
                && self
                    .form
                    .as_ref()
                    .is_some_and(|f| matches!(f.kind, FormKind::Note))
            {
                self.command(6);
                return;
            }
            if key.code == KeyCode::Esc {
                self.form = None;
                return;
            }
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && matches!(
                    key.code,
                    KeyCode::Enter | KeyCode::Char('s' | 'S' | 'ы' | 'Ы')
                )
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
                    self.palette_index = (self.palette_index + 1) % self.commands().len()
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    self.palette_index =
                        (self.palette_index + self.commands().len() - 1) % self.commands().len()
                }
                KeyCode::Enter => self.command(self.palette_index),
                _ => {}
            }
            return;
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
            KeyCode::Char('q' | 'Q' | 'й' | 'Й')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.quit = true
            }
            KeyCode::Char('y' | 'Y' | 'н' | 'Н')
                if !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.command(22)
            }
            KeyCode::Char('g') => self.command(26),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char(':') => self.palette = true,
            KeyCode::Char('/') => {
                self.command(if self.view == View::Archive && self.tab == Tab::Progress {
                    34
                } else if self.view == View::Settings {
                    36
                } else {
                    3
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
            KeyCode::Char(' ') => self.command(24),
            KeyCode::Char('p') => self.command(25),
            KeyCode::Left if key.modifiers.contains(KeyModifiers::ALT) => self.nudge(-15.0),
            KeyCode::Right if key.modifiers.contains(KeyModifiers::ALT) => self.nudge(15.0),
            KeyCode::Char('o') if self.view == View::Archive && self.tab == Tab::Progress => {
                if let (Some(s), Some(before)) = (self.current.clone(), self.journal.before.clone())
                {
                    self.effects
                        .push((Resource::Journal, Task::Journal(s, None, Some(before))));
                }
            }
            KeyCode::Char('i') => self.command(0),
            KeyCode::Char('n') => self.command(match self.view {
                View::Notes => 5,
                View::Ask => 7,
                _ => 0,
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
                    self.selected = self
                        .selected
                        .saturating_add(1)
                        .min(self.list_len().saturating_sub(1));
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
                    self.selected = self.selected.saturating_sub(1);
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
            KeyCode::PageDown => {
                self.scroll = self.effective_scroll();
                if self.is_journal() {
                    self.journal.follow = false;
                }
                self.follow_selection = false;
                self.scroll = self.scroll.saturating_add(10);
            }
            KeyCode::PageUp => {
                self.scroll = self.effective_scroll();
                if self.is_journal() {
                    self.journal.follow = false;
                }
                self.follow_selection = false;
                self.scroll = self.scroll.saturating_sub(10);
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
                    .saturating_sub(self.viewport.1.saturating_sub(8));
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
            KeyCode::Enter => self.enter(),
            KeyCode::Esc => {
                self.focus = 0;
            }
            _ => {}
        }
    }
    pub fn list_len(&self) -> usize {
        match self.view {
            View::Archive => self.sessions.len(),
            View::Queue => self.queue.len(),
            View::Ask => self.asks.len(),
            View::Notes => self.notes.len(),
            View::Search => self.hits.len(),
            View::Settings => self.visible_settings().len(),
            View::Chat => self.answer.sources.len(),
        }
    }
    pub fn is_journal(&self) -> bool {
        self.view == View::Archive && self.tab == Tab::Progress
    }
    pub fn visible_settings(&self) -> Vec<&Setting> {
        let filter = self.settings_filter.to_lowercase();
        self.settings
            .settings
            .iter()
            .filter(|s| {
                filter.is_empty()
                    || format!("{} {} {} {}", s.key, s.group, s.label, s.hint)
                        .to_lowercase()
                        .contains(&filter)
            })
            .collect()
    }
    pub fn selected_setting(&self) -> Option<&Setting> {
        self.visible_settings().get(self.selected).copied()
    }
    fn effective_scroll(&self) -> u16 {
        if self.is_journal() {
            crate::render::journal_metrics(self).1
        } else {
            crate::render::document_metrics(self).1
        }
    }
    fn nudge(&mut self, delta: f64) {
        let position = if self.playback.session == self.current {
            self.playback.position
        } else {
            0.0
        };
        self.seek_to(position + delta);
    }
    fn media_duration(&self) -> Option<f64> {
        let duration = if self.playback.session == self.current {
            self.playback.duration
        } else {
            None
        };
        duration
            .or_else(|| (self.peaks.duration_sec > 0.0).then_some(self.peaks.duration_sec))
            .filter(|d| d.is_finite() && *d > 0.0)
    }
    fn seek_to(&mut self, position: f64) {
        if let Some(session) = self.current.clone() {
            let position = position.max(0.0);
            let position = self
                .media_duration()
                .map_or(position, |d| position.min((d - 0.01).max(0.0)));
            self.player_effects
                .push(crate::player::PlaybackCommand::Seek { session, position });
        }
    }
    pub fn selection_len(&self) -> usize {
        match self.view {
            View::Archive if self.tab == Tab::Progress => self.journal.visible_groups().len(),
            View::Archive if self.tab == Tab::Speakers => self.speakers.len(),
            View::Archive if self.tab == Tab::Transcript => self.transcript.len(),
            View::Archive if self.tab == Tab::Readable && self.readable.from_transcript => {
                usize::from(!self.readable.lines.is_empty())
            }
            View::Archive if self.tab == Tab::Readable => self.readable.lines.len(),
            View::Archive if self.tab == Tab::Summary => self.summary.blocks.len(),
            View::Notes => self
                .notes
                .get(self.selected)
                .map(|s| s.notes.len())
                .unwrap_or(0),
            _ => self.body_len(),
        }
    }
    pub fn body_len(&self) -> usize {
        if self.view == View::Archive && self.tab == Tab::Progress {
            return self.progress.stages.len() * 2
                + self
                    .journal
                    .groups()
                    .iter()
                    .map(|g| {
                        4 + if self.journal.expanded.contains(&g.id) {
                            g.hidden_lines()
                        } else {
                            0
                        }
                    })
                    .sum::<usize>()
                + 3;
        }
        self.document_text().lines().count().max(1)
    }
    fn enter(&mut self) {
        if self.focus == 0 {
            self.selected = 0;
            self.switch(VIEWS[self.nav].0);
            self.focus = 1;
            return;
        }
        match self.view {
            View::Archive if self.focus == 2 && self.tab == Tab::Progress => {
                self.scroll = self.effective_scroll();
                self.journal.follow = false;
                self.follow_selection = true;
                self.journal.toggle(self.item)
            }
            View::Archive if self.focus == 1 => {
                if let Some(s) = self.sessions.get(self.selected) {
                    self.select_session(s.name.clone());
                    self.focus = 2;
                }
            }
            View::Queue => {
                if let Some(i) = self.queue.get(self.selected) {
                    let s = i.session.clone();
                    self.tab = Tab::Progress;
                    self.select_session(s);
                    self.focus = 2;
                }
            }
            View::Ask if self.focus == 1 => {
                if let Some(i) = self.asks.get(self.selected) {
                    let id = i.id.clone();
                    if self.ask_id.as_ref() != Some(&id) {
                        self.epoch += 1;
                        self.ask = None;
                        self.ask_id = Some(id.clone());
                    }
                    self.read(Resource::Ask, id);
                    self.focus = 2;
                }
            }
            View::Search => {
                if let Some(h) = self.hits.get(self.selected) {
                    let (session, time) = (h.session.clone(), h.start_sec);
                    self.tab = Tab::Transcript;
                    self.select_session(session);
                    self.document_target = time;
                    self.focus = 2;
                }
            }
            View::Settings => {
                self.item = self.selected;
                self.command(20);
            }
            _ => self.expanded = !self.expanded,
        }
    }
}

pub fn parse_timecode(value: &str) -> Option<f64> {
    let fields = value.trim().split(':').collect::<Vec<_>>();
    if fields.is_empty() || fields.len() > 3 {
        return None;
    }
    let mut seconds = 0.0;
    for (index, field) in fields.iter().enumerate() {
        let value = field.parse::<f64>().ok()?;
        if !value.is_finite() || value < 0.0 || (index > 0 && value >= 60.0) {
            return None;
        }
        seconds = seconds * 60.0 + value;
    }
    Some(seconds)
}
pub fn timecode(sec: f64) -> String {
    let s = if sec.is_finite() {
        sec.max(0.0) as u64
    } else {
        0
    };
    format!("{:02}:{:02}", s / 60, s % 60)
}
pub fn state_symbol(state: &str, ascii: bool) -> &'static str {
    match state {
        "done" | "ready" => {
            if ascii {
                "+"
            } else {
                "✓"
            }
        }
        "running" => {
            if ascii {
                "*"
            } else {
                "●"
            }
        }
        "failed" | "invalid" => "!",
        "skipped" => "-",
        _ => {
            if ascii {
                "."
            } else {
                "·"
            }
        }
    }
}

#[cfg(test)]
mod tests;
