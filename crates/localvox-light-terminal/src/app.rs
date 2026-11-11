//! Deterministic workspace state. I/O results carry the selection epoch.
mod archive_list;
mod commands;
mod diagnostics;
mod documents;
mod forms;
mod input;
mod navigation;
mod paging;
mod pane_width;
mod preferences;
mod responses;
pub use navigation::DetailParent;
pub use preferences::UiPreferences;
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
    pub preferences: UiPreferences,
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
            preferences: UiPreferences::default(),
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
        self.preferences
            .keymap
            .translate(key)
            .unwrap_or_else(|| archive_list::command_key(key))
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
