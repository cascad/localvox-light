//! Deterministic workspace state. I/O results carry the selection epoch.
mod archive_list;
mod commands;
mod diagnostics;
mod documents;
mod forms;
mod input;
mod navigation;
mod paging;
pub use navigation::DetailParent;
mod quit;
use crate::io::{Data, Resource, Response, Task};
pub use commands::{Action, COMMANDS};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use localvox_light_client::{safe_text, types::*, Scope};
pub use quit::QuitSequence;
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
    LlmServer,
    SettingChoice {
        key: String,
        options: Vec<(String, Option<String>)>,
        selected: usize,
    },
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
    pub list_scroll: usize,
    pub item: usize,
    pub epoch: u64,
    pub sessions: Vec<Session>,
    pub archive_filter: String,
    pub archive_filter_editing: bool,
    pub current: Option<String>,
    pub record: RecordState,
    pub jobs: Jobs,
    pub queue: Vec<QueueItem>,
    pub summary: Summary,
    pub transcript: Vec<TranscriptLine>,
    pub partial: Option<PartialInfo>,
    pub document_final: bool,
    pub readable: ReadableDoc,
    pub versions: Vec<Version>,
    pub speakers: Vec<Participant>,
    pub sources: Vec<SourceName>,
    pub languages: Vec<String>,
    pub progress: Progress,
    pub journal: crate::journal::Journal,
    pub console: crate::diagnostics::Console,
    pub metadata_open: bool,
    pub settings_advanced: bool,
    pub llm_server_pending: bool,
    pub action_diagnostics: HashMap<Resource, crate::diagnostics::Context>,
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
    pub pending_retry: Option<u64>,
    pub failed_drafts: std::collections::VecDeque<Form>,
    pub palette: bool,
    pub help: bool,
    pub palette_index: usize,
    pub scroll: u16,
    pub status: String,
    pub errors: HashMap<Resource, String>,
    pub quit: bool,
    pub quit_sequence: Option<QuitSequence>,
    pub detail_parent: Option<DetailParent>,
    pub keymap: crate::keymap::Keymap,
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
            list_scroll: 0,
            item: 0,
            epoch: 1,
            sessions: vec![],
            archive_filter: String::new(),
            archive_filter_editing: false,
            current: None,
            record: RecordState::default(),
            jobs: Jobs::default(),
            queue: vec![],
            summary: Summary::default(),
            transcript: vec![],
            readable: ReadableDoc::default(),
            partial: None,
            document_final: false,
            versions: vec![],
            speakers: vec![],
            sources: vec![],
            languages: vec![],
            progress: Progress::default(),
            journal: crate::journal::Journal::new(),
            console: crate::diagnostics::Console::default(),
            metadata_open: false,
            settings_advanced: false,
            llm_server_pending: false,
            action_diagnostics: HashMap::new(),
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
            pending_retry: None,
            failed_drafts: std::collections::VecDeque::new(),
            palette: false,
            help: false,
            palette_index: 0,
            scroll: 0,
            status: "Подключение…".into(),
            errors: HashMap::new(),
            quit: false,
            quit_sequence: None,
            detail_parent: None,
            keymap: crate::keymap::Keymap::default(),
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
    fn accept_document(&mut self, partial: Option<&PartialInfo>) -> bool {
        if let Some(next) = partial {
            if self.document_final || self.partial.as_ref().is_some_and(|old| !next.follows(old)) {
                return false;
            }
            if self.partial.as_ref().and_then(|p| p.snapshot.as_ref()) != next.snapshot.as_ref()
                && (self.playback.session != self.current || self.playback.idle)
            {
                self.peaks_loaded = false;
            }
        } else {
            self.document_final = true;
        }
        self.partial = partial.cloned();
        true
    }
    fn update_transcript(&mut self, lines: Vec<TranscriptLine>) {
        let anchor = self
            .transcript
            .get(self.item)
            .map(|l| (l.id.clone(), l.start_sec, l.source_id));
        self.transcript = lines;
        if let Some(time) = self.document_target.take() {
            self.item = self
                .transcript
                .iter()
                .position(|l| l.end_sec > time)
                .unwrap_or_else(|| self.transcript.len().saturating_sub(1));
            self.scroll = 0;
            self.follow_selection = true;
        } else if let Some((id, time, source)) = anchor {
            self.item = self
                .transcript
                .iter()
                .position(|l| id.is_some() && l.id == id)
                .or_else(|| {
                    self.transcript.iter().position(|l| {
                        l.source_id == source && l.start_sec <= time && l.end_sec > time
                    })
                })
                .unwrap_or_else(|| self.item.min(self.transcript.len().saturating_sub(1)));
        }
    }
    pub fn refresh(&mut self) {
        self.read(Resource::Sessions, String::new());
        self.read(Resource::Record, String::new());
        self.read(Resource::Jobs, String::new());
        self.read(Resource::Queue, String::new());
        self.refresh_view();
    }
    fn read(&mut self, r: Resource, s: String) {
        let task = if r == Resource::Peaks {
            let snapshot = if self.playback.session == self.current && !self.playback.idle {
                self.playback.snapshot.clone()
            } else {
                self.partial.as_ref().and_then(|p| p.snapshot.clone())
            };
            Task::Peaks(s, snapshot)
        } else {
            Task::Read(r, s)
        };
        self.effects.push((r, task));
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
        self.detail_parent = None;
        self.llm_server_pending = false;
        self.view = v;
        self.nav = VIEWS.iter().position(|(view, _)| *view == v).unwrap_or(0);
        self.epoch += 1;
        self.item = 0;
        self.selected = 0;
        self.list_scroll = 0;
        self.scroll = 0;
        self.clear_document_errors();
        self.refresh_view();
    }
    pub fn select_session(&mut self, name: String) {
        self.detail_parent = None;
        self.archive_filter_editing = false;
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
            self.metadata_open = false;
            self.document_target = None;
            self.summary = Summary::default();
            self.transcript.clear();
            self.partial = None;
            self.document_final = false;
            self.readable = ReadableDoc::default();
            self.speakers.clear();
            self.versions.clear();
            self.sources.clear();
            self.progress = Progress::default();
            self.journal = crate::journal::Journal::new();
            self.scroll = 0;
            self.item = 0;
            self.clear_document_errors();
        }
        self.view = View::Archive;
        self.nav = 0;
        if !self.visible_sessions().iter().any(|s| s.name == name) {
            self.archive_filter.clear();
        }
        self.selected = self
            .visible_sessions()
            .iter()
            .position(|s| s.name == name)
            .unwrap_or(0);
        self.list_scroll = crate::render::list_metrics(self).1;
        self.refresh_view();
    }
    pub fn apply(&mut self, r: Response) {
        let global = matches!(
            r.resource,
            Resource::Sessions
                | Resource::Record
                | Resource::Jobs
                | Resource::Queue
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
                let context = self
                    .action_diagnostics
                    .remove(&r.resource)
                    .unwrap_or_else(|| self.read_diagnostic(r.resource));
                self.console.recover_operation(&context);
                self.errors.remove(&r.resource);
                d
            }
            Err(e) => {
                let context = self
                    .action_diagnostics
                    .remove(&r.resource)
                    .unwrap_or_else(|| self.read_diagnostic(r.resource));
                if matches!(
                    r.resource,
                    Resource::Action | Resource::Control | Resource::Route
                ) {
                    self.console.record_attempt(context, &e, self.wall_ms);
                } else {
                    self.console.record(context, &e, self.wall_ms);
                }
                if r.resource == Resource::Peaks {
                    self.peaks_retry_at = self.now_ms.saturating_add(30_000);
                }
                self.pending_actions.remove(&r.resource);
                self.pending_context.remove(&r.resource);
                if r.resource == Resource::Action {
                    self.settings_pending = None;
                    self.pending_retry = None;
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
                // Read errors live with their resource and disappear on recovery. A background
                // poll must neither persist an old read failure nor erase a command outcome.
                if matches!(
                    r.resource,
                    Resource::Action | Resource::Control | Resource::Route
                ) {
                    self.status = "Действие не выполнено · ~ консоль ошибок".into();
                }
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
        let response_resource = r.resource;
        match data {
            Data::Sessions(s) => {
                if let Some(current) = &self.current {
                    if let Some(updated) = s.iter().find(|session| &session.name == current) {
                        if self
                            .sessions
                            .iter()
                            .find(|session| &session.name == current)
                            .is_some_and(|previous| {
                                (previous.duration_sec - updated.duration_sec).abs() > 0.5
                            })
                        {
                            self.peaks_loaded = false;
                            self.peaks_retry_at = 0;
                        }
                    }
                }
                let selected = self
                    .visible_sessions()
                    .get(self.selected)
                    .map(|s| s.name.clone());
                self.sessions = s;
                if self.view == View::Archive {
                    self.selected = selected
                        .and_then(|name| {
                            self.visible_sessions().iter().position(|s| s.name == name)
                        })
                        .unwrap_or_else(|| {
                            self.selected
                                .min(self.visible_sessions().len().saturating_sub(1))
                        });
                }
                if self.current.is_none() && self.view == View::Archive {
                    if let Some(s) = self.visible_sessions().first() {
                        self.select_session(s.name.clone());
                    }
                }
            }
            Data::Record(r) => {
                if let Some(last) = r.voice.as_ref().and_then(|voice| voice.last.as_ref()) {
                    let context = crate::diagnostics::Context {
                        source: "Голосовой модуль".into(),
                        operation: format!("Обработка заметки в слот {}", last.slot),
                        session: None,
                        phase: Some("voice".into()),
                    };
                    if let Some(error) = &last.error {
                        self.console.record(context, error, self.wall_ms);
                    } else {
                        self.console.recover_operation(&context);
                    }
                }
                if self.pending_record == Some(r.recording) {
                    self.pending_record = None;
                }
                self.record = r;
            }
            Data::Jobs(j) => self.jobs = j,
            Data::Queue(q) => {
                for old in &self.queue {
                    if old.state == "failed"
                        && !q.iter().any(|job| {
                            job.session == old.session
                                && job.state == "failed"
                                && job.attempts == old.attempts
                                && job.last_error == old.last_error
                        })
                    {
                        self.console.recover(&crate::diagnostics::Context {
                            source: "Пайплайн".into(),
                            operation: "Выполнение этапа".into(),
                            session: Some(old.session.clone()),
                            phase: old.phase.clone(),
                        });
                    }
                }
                for job in q.iter().filter(|job| job.state == "failed") {
                    if let Some(error) = &job.last_error {
                        self.console.record(
                            crate::diagnostics::Context {
                                source: "Пайплайн".into(),
                                operation: "Выполнение этапа".into(),
                                session: Some(job.session.clone()),
                                phase: job.phase.clone(),
                            },
                            error,
                            self.wall_ms,
                        );
                    }
                }
                let selected = self.queue.get(self.selected).map(|q| q.session.clone());
                self.queue = q;
                if self.view == View::Queue {
                    self.selected = selected
                        .and_then(|name| self.queue.iter().position(|q| q.session == name))
                        .unwrap_or_else(|| self.selected.min(self.queue.len().saturating_sub(1)));
                }
            }
            Data::Summary(s) => self.summary = s,
            Data::TranscriptSnapshot(t) => {
                if self.accept_document(t.partial.as_ref()) {
                    self.update_transcript(t.lines);
                }
            }
            Data::Transcript(t) => {
                self.update_transcript(t);
            }
            Data::Readable(t) => {
                if self.accept_document(t.partial.as_ref()) {
                    self.readable = t;
                }
            }
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
                let queue_context = crate::diagnostics::Context {
                    source: "Пайплайн".into(),
                    operation: "Чтение очереди".into(),
                    session: self.current.clone(),
                    phase: None,
                };
                if let Some(error) = &p.queue_error {
                    self.console.record(queue_context, error, self.wall_ms);
                } else {
                    self.console.recover(&queue_context);
                }
                for stage in &p.stages {
                    let context = crate::diagnostics::Context {
                        source: "Пайплайн".into(),
                        operation: "Выполнение этапа".into(),
                        session: self.current.clone(),
                        phase: Some(stage.stage.clone()),
                    };
                    if matches!(stage.state.as_str(), "failed" | "invalid") {
                        if let Some(note) = &stage.note {
                            self.console.record(context, note, self.wall_ms);
                        }
                    } else {
                        self.console.recover(&context);
                    }
                }
                let anchor = crate::render::journal_anchor(self);
                self.progress = p;
                crate::render::restore_journal_anchor(self, anchor);
            }
            Data::Peaks(p) => {
                let expected = if self.playback.session == self.current && !self.playback.idle {
                    self.playback.snapshot.as_ref()
                } else {
                    self.partial.as_ref().and_then(|p| p.snapshot.as_ref())
                };
                if expected.is_some() && p.snapshot.as_ref() != expected {
                    self.peaks_loaded = false;
                    return;
                }
                self.peaks = p;
                self.peaks_loaded = !self.peaks.peaks.is_empty();
                if !self.peaks_loaded {
                    self.peaks_retry_at = self.now_ms.saturating_add(5000);
                }
            }
            Data::Journal(page, prepend) => {
                for event in page
                    .events
                    .iter()
                    .filter(|event| matches!(event.status.as_deref(), Some("failed" | "invalid")))
                {
                    if let Some(text) = &event.text {
                        self.console.record(
                            crate::diagnostics::Context {
                                source: "Журнал".into(),
                                operation: if event.label.is_empty() {
                                    "Ошибка операции".into()
                                } else {
                                    event.label.clone()
                                },
                                session: self.current.clone(),
                                phase: Some(event.stage.clone()),
                            },
                            text,
                            chrono::DateTime::parse_from_rfc3339(&event.at)
                                .map(|at| at.timestamp_millis())
                                .unwrap_or(self.wall_ms),
                        );
                    }
                }
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
                for slot in &n {
                    let context = crate::diagnostics::Context {
                        source: "API".into(),
                        operation: format!("Чтение слота {}", slot.name),
                        session: None,
                        phase: None,
                    };
                    if let Some(error) = &slot.error {
                        self.console.record(context, error, self.wall_ms);
                    } else {
                        self.console.recover_operation(&context);
                    }
                }
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
                let context = crate::diagnostics::Context {
                    source: "LLM".into(),
                    operation: format!("Ответ на вопрос {}", a.id),
                    session: None,
                    phase: Some("ask".into()),
                };
                if let Some(error) = a.error.as_deref().filter(|error| !error.is_empty()) {
                    self.console.record(context, error, self.wall_ms);
                } else {
                    self.console.recover_operation(&context);
                }
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
                if std::mem::take(&mut self.llm_server_pending) {
                    if self.settings.settings.iter().any(|s| s.kind == "servers") {
                        self.command(Action::AddLlmServer);
                    } else {
                        self.status = "Список LLM-серверов требует обновления API".into();
                    }
                }
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
                if response_resource == Resource::Action
                    && self.pending_retry.take() == Some(self.epoch)
                    && action_context == Some(self.epoch)
                {
                    // Fence the previous attempt's reads, keeping its committed documents visible.
                    self.epoch += 1;
                }
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
        if !self.quit_pending() {
            self.cancel_quit_confirmation();
        }
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
        self.cancel_quit_confirmation();
        if self.console.open {
            return;
        }
        if self.archive_filter_input() {
            self.append_archive_filter(text);
            return;
        }
        let url = text.trim();
        if self.form.is_none()
            && (url.starts_with("https://") || url.starts_with("http://"))
            && !url.chars().any(char::is_whitespace)
            && url.len() <= 400_000
        {
            self.palette = false;
            self.open("Импорт", FormKind::Import, &[("URL", url)]);
            return;
        }
        if self.constrained_paste(text) {
            return;
        }
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
    pub fn shortcut_key(&self, key: KeyEvent) -> KeyEvent {
        self.keymap
            .translate(key)
            .unwrap_or_else(|| archive_list::command_key(key))
    }
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
    pub fn list_len(&self) -> usize {
        match self.view {
            View::Archive => self.visible_sessions().len(),
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
                s.kind != "servers"
                    && (self.settings_advanced
                        || !filter.is_empty()
                        || self
                            .settings
                            .settings
                            .iter()
                            .all(|s| !basic_setting(&s.key))
                        || basic_setting(&s.key))
                    && (filter.is_empty()
                        || format!("{} {} {} {}", s.key, s.group, s.label, s.hint)
                            .to_lowercase()
                            .contains(&filter))
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
    pub(crate) fn media_duration(&self) -> Option<f64> {
        let duration = if self.playback.session == self.current {
            self.playback.duration
        } else {
            None
        };
        duration
            .or_else(|| (self.peaks.duration_sec > 0.0).then_some(self.peaks.duration_sec))
            .or_else(|| {
                self.sessions
                    .iter()
                    .find(|session| Some(&session.name) == self.current.as_ref())
                    .map(|session| session.duration_sec)
            })
            .filter(|d| d.is_finite() && *d > 0.0)
    }
    fn seek_to(&mut self, position: f64) {
        if let Some(session) = self.current.clone() {
            let position = position.max(0.0);
            let position = self
                .partial
                .as_ref()
                .map(|p| p.audio_end_sec)
                .or_else(|| self.media_duration())
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
                if let Some(s) = self.visible_sessions().get(self.selected) {
                    self.select_session(s.name.clone());
                    self.focus = 2;
                }
            }
            View::Queue => {
                if let Some(i) = self.queue.get(self.selected) {
                    let s = i.session.clone();
                    self.open_list_record(s, Tab::Progress, None);
                }
            }
            View::Notes if self.focus == 1 => {
                if self.notes.get(self.selected).is_some() {
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
                    self.open_list_record(session, Tab::Transcript, time);
                }
            }
            View::Settings => {
                self.item = self.selected;
                self.command(Action::EditSetting);
            }
            _ => self.expanded = !self.expanded,
        }
    }
}

fn basic_setting(key: &str) -> bool {
    matches!(
        key,
        "LOCALVOX_LIGHT_MIC"
            | "LOCALVOX_LIGHT_LOOPBACK_DEVICE"
            | "LOCALVOX_LIGHT_AUTOCOOK_SUMMARY"
            | "LOCALVOX_LLM_BASE_URL"
            | "LOCALVOX_LLM_MODEL"
            | "LOCALVOX_SUMMARY_PROVIDER"
    )
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
        "cancelling" => {
            if ascii {
                "~"
            } else {
                "◌"
            }
        }
        "cancelled" => {
            if ascii {
                "x"
            } else {
                "■"
            }
        }
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
#[cfg(test)]
mod ux_tests;
