//! Document text, copy projection and source selection.
use super::*;

impl App {
    pub fn document_title(&self) -> String {
        safe_text(&match self.view {
            View::Archive => self.current_title(),
            View::Ask => self
                .ask
                .as_ref()
                .map(|a| a.input_name.as_deref().unwrap_or(&a.id).to_owned())
                .unwrap_or_else(|| "Спросить".into()),
            View::Notes => self
                .notes
                .get(self.selected)
                .map(|s| s.name.clone())
                .unwrap_or_else(|| "Заметки".into()),
            View::Queue => self
                .queue
                .get(self.selected)
                .map(|q| q.title.clone())
                .unwrap_or_else(|| "Очередь".into()),
            View::Settings => self
                .selected_setting()
                .map(|s| s.label.clone())
                .unwrap_or_else(|| "Настройки".into()),
            View::Search => "Поиск по архиву".into(),
            View::Chat => "Чат с архивом".into(),
        })
    }
    pub fn document_text(&self) -> String {
        let text = match self.view {
            View::Archive => match self.tab {
                Tab::Summary => self
                    .summary
                    .blocks
                    .iter()
                    .map(|b| {
                        format!(
                            "{}{}",
                            if b.kind == "heading" {
                                "# "
                            } else if b.kind == "bullet" {
                                "• "
                            } else {
                                ""
                            },
                            b.text
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                Tab::Transcript => self
                    .transcript
                    .iter()
                    .map(|l| format!("{}  {}\n{}", timecode(l.start_sec), l.who, l.text))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                Tab::Readable if self.readable.from_transcript => self.copy_text(),
                Tab::Readable => self
                    .readable
                    .lines
                    .iter()
                    .map(|l| {
                        if self.expanded {
                            format!(
                                "{}: {}{}",
                                l.who,
                                l.text,
                                l.original
                                    .as_ref()
                                    .map(|s| format!("\n  STT: {s}"))
                                    .unwrap_or_default()
                            )
                        } else {
                            format!("{}: {}", l.who, l.text)
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                Tab::Speakers => self
                    .speakers
                    .iter()
                    .map(|p| {
                        format!(
                            "{} · {} · {} реплик\nГолоса: {}\nИсточники: {}",
                            p.name,
                            p.what,
                            p.lines,
                            p.voices.join(", "),
                            p.sources.iter().map(ToString::to_string).collect::<Vec<_>>().join(", ")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                Tab::Progress => self
                    .progress
                    .stages
                    .iter()
                    .map(|s| {
                        format!(
                            "{} {}  {}{}\n  {}",
                            state_symbol(&s.state, self.ascii),
                            s.stage,
                            s.state,
                            match (s.done, s.total) {
                                (Some(d), Some(t)) => format!(" {d}/{t}"),
                                _ => String::new(),
                            },
                            s.note.as_deref().unwrap_or("")
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n\n"),
            },
            View::Notes => self
                .notes
                .get(self.selected)
                .map(|s| {
                    if !s.readable {
                        format!(
                            "Чтение недоступно: {}",
                            s.error.as_deref().unwrap_or("ошибка")
                        )
                    } else {
                        s.notes
                            .iter()
                            .map(|n| format!("{}\n{}", n.date.as_deref().unwrap_or(""), n.text))
                            .collect::<Vec<_>>()
                            .join("\n\n")
                    }
                })
                .unwrap_or_default(),
            View::Queue => self
                .queue
                .get(self.selected)
                .map(|q| {
                    let mut text = format!(
                        "{}\n{} · {}\n{}\n{}",
                        q.title,
                        q.session,
                        q.state,
                        if q.last_error.is_some() {"Причина отказа · ~ консоль"} else {""},
                        if q.stuck { "Попытки исчерпаны. Выберите явный повтор через палитру.".into() } else { q.position.map(|n|format!("Место в очереди: {n}")).unwrap_or_default() }
                    );
                    text.push_str(&format!("\nЭтап: {} · попыток: {}", q.phase.as_deref().unwrap_or("неизвестен"), q.attempts));
                    if let Some(plan) = &q.retry {
                        text.push_str(&format!("\n{}", plan.message));
                        if let Some(at) = &plan.retry_at { text.push_str(&format!("\nПовтор не раньше: {at}")); }
                        else if plan.state != "failed" { text.push_str("\nСрок повтора неизвестен."); }
                    } else if q.state == "failed" && !q.stuck { text.push_str("\nПлан автоматического повтора не подтверждён старым API."); }
                    text
                })
                .unwrap_or_default(),
            View::Ask => self
                .ask
                .as_ref()
                .map(|a| {
                    let mut parts = vec![format!("{} · {} · {}\n{} · {} символов · {}\n{}", a.provider, a.model.as_deref().unwrap_or("модель не указана"), a.status, a.input_name.as_deref().unwrap_or(&a.id), a.input_chars, a.created_at, a.prompt)];
                    if let Some(answer) = a.answer.as_deref().filter(|s| !s.is_empty()) { parts.push(answer.into()); }
                    if a.error.as_deref().is_some_and(|s| !s.is_empty()) { parts.push("Ответ не получен · ~ консоль".into()); }
                    if matches!(a.status.as_str(), "pending" | "running") { parts.push("Ожидаем ответ…".into()); }
                    if let Some(cost) = a.cost_usd { parts.push(format!("Стоимость: ${cost:.4}")); }
                    if self.expanded { parts.push(format!("Исходный материал:\n{}", a.input.as_deref().unwrap_or("Материал недоступен"))); }
                    else { parts.push("Enter — исходный материал".into()); }
                    parts.join("\n\n")
                })
                .unwrap_or_else(|| "n — новый вопрос; : — действия".into()),
            View::Settings => self
                .selected_setting()
                .map(|s| {
                    format!(
                        "{}\n{}\n\n{}\nПрименение: {}\nЗначение: {}\n\n{}\nСохранение через : → сохранить настройки.\n{}",
                        s.key,
                        s.label,
                        s.hint,
                        s.applies,
                        if s.kind == "secret" {
                            if s.set {
                                "задано".into()
                            } else {
                                "не задано".into()
                            }
                        } else {
                            self.setting_value(s, true)
                        },
                        self.settings_edits.get(&s.key).map(|value|format!("Несохранённый черновик: {}",if s.kind=="secret"{if value.is_some(){"скрыто".into()}else{"сброс к умолчанию".into()}}else{value.clone().unwrap_or_else(||"сброс к умолчанию".into())})).unwrap_or_else(||"Enter изменить · : сохранить изменённые ключи".into()),
                        if s.group == "После распознавания" {
                            "Транскрибация обязательна. Затем — существующая сводка, если включена. Во вкладке «Текст» доступна та же расшифровка без дополнительного прохода модели. Очистка запускается вручную.\nПереключатель сводки действует без restart для новых автоматических заданий. Текущий LLM-этап сохраняет результат. Старые задания и явные ручные операции сохраняют свои параметры."
                        } else { "restart — после перезапуска демона; capture — переоткрывает устройства." }
                    )
                })
                .unwrap_or_default(),
            View::Search => {
                let mut parts = vec![];
                if self.pending_forms.contains_key(&Resource::Search) { parts.push(format!("Ищем: {} · {}",self.search_query,self.search_mode)); }
                if !self.search_results_query.is_empty() { parts.push(format!("Последние результаты: {}",self.search_results_query)); }
                if let Some(hit) = self.hits.get(self.selected) {
                    parts.push(format!("{} · {} · {}\n{}{}\n\n{}",hit.session,hit.kind,hit.start_sec.map(timecode).unwrap_or_default(),hit.why.as_deref().unwrap_or(""),hit.score.map(|score|format!(" · близость {score:.3}")).unwrap_or_default(),hit.snippet));
                } else if self.errors.contains_key(&Resource::Search) { parts.push("Поиск недоступен. Причина — в консоли (~).".into()); }
                else if !self.search_results_query.is_empty() { parts.push("Совпадений не найдено.".into()); }
                else if parts.is_empty() { parts.push("/ — поиск по архиву".into()); }
                parts.join("\n\n")
            },
            View::Chat => {
                let mut parts = vec![];
                if self.pending_forms.contains_key(&Resource::Chat) { parts.push(format!("Ответ готовится: {}",self.chat_query)); }
                if !self.chat_answer_query.is_empty() { parts.push(format!("Вопрос: {}",self.chat_answer_query)); }
                if !self.answer.text.is_empty() {
                    parts.push(self.answer.text.clone());
                    parts.push(format!("Проверка источников: {}",if self.answer.verified {"пройдена"} else {"не подтверждена"}));
                }
                if !self.answer.ungrounded.is_empty() { parts.push(format!("Без подтверждения источниками:\n{}", self.answer.ungrounded.join("\n"))); }
                for s in &self.answer.sources { parts.push(format!("[{}] {} · {} · {}\n{}", s.n, s.session, s.kind, s.start_sec.map(timecode).unwrap_or_default(), s.text)); }
                parts.join("\n\n")
            },
        };
        safe_text(&text)
    }
    pub fn copy_text(&self) -> String {
        safe_text(&match self.view {
            View::Archive => match self.tab {
                Tab::Summary => self
                    .summary
                    .blocks
                    .iter()
                    .map(|b| b.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                Tab::Transcript => self
                    .transcript
                    .iter()
                    .map(|l| format!("{}: {}", l.who, l.text))
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                Tab::Readable if self.readable.from_transcript => self
                    .readable
                    .lines
                    .iter()
                    .map(|line| line.text.trim())
                    .filter(|text| !text.is_empty())
                    .collect::<Vec<_>>()
                    .join(" "),
                Tab::Readable => self
                    .readable
                    .lines
                    .iter()
                    .map(|l| l.text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n\n"),
                _ => String::new(),
            },
            View::Ask => self
                .ask
                .as_ref()
                .and_then(|a| a.answer.clone())
                .unwrap_or_default(),
            View::Notes => self
                .notes
                .get(self.selected)
                .and_then(|s| s.notes.get(self.item))
                .map(|n| n.text.clone())
                .unwrap_or_default(),
            View::Chat => self.answer.text.clone(),
            View::Search => self
                .hits
                .get(self.selected)
                .map(|h| h.snippet.clone())
                .unwrap_or_default(),
            _ => String::new(),
        })
    }
    pub fn selected_reply_text(&self) -> String {
        if self.view == View::Archive && self.tab == Tab::Transcript {
            self.transcript
                .get(self.item)
                .map(|line| safe_text(&line.text))
                .unwrap_or_default()
        } else {
            String::new()
        }
    }
    pub fn metadata_text(&self) -> String {
        if self.view != View::Archive {
            return String::new();
        }
        let mut lines = Vec::new();
        if let Some(session) = self
            .sessions
            .iter()
            .find(|s| Some(&s.name) == self.current.as_ref())
        {
            lines.push(format!("ID: {}", session.name));
            if let Some(lang) = &session.lang {
                lines.push(format!("Язык: {lang}"));
            }
            if let Some(source) = &session.source_url {
                lines.push(format!("Источник: {source}"));
            }
        }
        for v in &self.versions {
            lines.push(format!(
                "{} v{:03} · {} · {} · {} реплик",
                if v.best {
                    "Рабочая"
                } else {
                    "Версия"
                },
                v.id,
                v.model,
                v.created_at,
                v.lines
            ));
        }
        let provenance = match self.tab {
            Tab::Summary => self.summary.provenance.as_ref(),
            Tab::Readable => self.readable.provenance.as_ref(),
            _ => None,
        };
        if let Some(p) = provenance {
            lines.push(format!(
                "Модель: {}\nШаблон: {}\nСварено: {}\nГлоссарий: {}",
                p.model, p.template, p.at, p.glossary
            ));
            if let Some(doubts) = &p.doubts {
                lines.push(format!("Замечания к содержанию: {doubts}"));
            }
        }
        if self.tab == Tab::Readable {
            lines.push(format!(
                "Пропущено строк: {}; отклонено правок: {}",
                self.readable.omitted, self.readable.rejected
            ));
        }
        for source in &self.sources {
            lines.push(format!(
                "Источник {}: {} · {}",
                source.source_id, source.name, source.what
            ));
        }
        safe_text(&lines.join("\n"))
    }
    pub fn fragment(&self) -> Option<(String, f64, Option<f64>)> {
        match self.view {
            View::Search => self.hits.get(self.selected).and_then(|h| {
                h.start_sec
                    .map(|start| (h.session.clone(), start, h.end_sec))
            }),
            View::Chat => self
                .answer
                .sources
                .get(self.selected)
                .and_then(|s| s.start_sec.map(|start| (s.session.clone(), start, None))),
            View::Archive => self.current.as_ref().and_then(|session| match self.tab {
                Tab::Transcript => self
                    .transcript
                    .get(self.item)
                    .map(|l| (session.clone(), l.start_sec, Some(l.end_sec))),
                Tab::Readable if self.readable.from_transcript => {
                    let first = self.readable.lines.first()?;
                    let (start, end) = self
                        .readable
                        .lines
                        .iter()
                        .fold((first.start_sec, first.end_sec), |(start, end), line| {
                            (start.min(line.start_sec), end.max(line.end_sec))
                        });
                    Some((session.clone(), start, Some(end)))
                }
                Tab::Readable => self
                    .readable
                    .lines
                    .get(self.item)
                    .map(|l| (session.clone(), l.start_sec, Some(l.end_sec))),
                Tab::Summary => self
                    .summary
                    .blocks
                    .get(self.item)
                    .filter(|b| !b.sources.is_empty())
                    .map(|b| {
                        (
                            session.clone(),
                            b.sources[0].start_sec,
                            b.sources.last().map(|s| s.end_sec),
                        )
                    }),
                _ => None,
            }),
            _ => None,
        }
    }
}
