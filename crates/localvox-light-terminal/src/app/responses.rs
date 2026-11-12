//! Accept API responses only for the current request epoch and document revision.
use super::*;

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
    pub fn apply(&mut self, r: Response) {
        self.apply_response(r);
        self.update_shutdown_status();
    }
    fn apply_response(&mut self, r: Response) {
        let global = matches!(
            r.resource,
            Resource::Sessions
                | Resource::Record
                | Resource::Jobs
                | Resource::Queue
                | Resource::Action
                | Resource::Control
                | Resource::Shutdown
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
                    Resource::Action | Resource::Control | Resource::Route | Resource::Shutdown
                ) {
                    self.console.record_attempt(context, &e, self.wall_ms);
                } else {
                    self.console.record(context, &e, self.wall_ms);
                }
                if r.resource == Resource::Peaks {
                    self.peaks_retry_at = self.now_ms.saturating_add(30_000);
                }
                self.pending_actions.remove(&r.resource);
                if r.resource == Resource::Shutdown {
                    self.shutdown_started = None;
                }
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
                    Resource::Action | Resource::Control | Resource::Route | Resource::Shutdown
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
            Data::Shutdown => {
                self.shutdown_started = None;
                self.quit = true;
            }
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
}
