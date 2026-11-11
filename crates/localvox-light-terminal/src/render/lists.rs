use super::*;

pub(super) fn nav(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    let rows = VIEWS
        .iter()
        .enumerate()
        .map(|(i, (v, label))| {
            ListItem::new(format!(" {} {label}", if *v == a.view { ">" } else { " " })).style(
                if i == a.nav {
                    Style::default().fg(t.ink)
                } else {
                    t.label_style()
                },
            )
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(a.nav));
    f.render_stateful_widget(
        List::new(rows)
            .block(panel("Разделы", a.focus == 0, t))
            .highlight_style(selection_style(t)),
        area,
        &mut state,
    );
}

pub(super) fn list_block(a: &App, t: Theme, title: &str) -> Block<'static> {
    let mut block = panel(title, a.focus == 1, t);
    if a.view == View::Archive && (a.archive_filter_editing || !a.archive_filter.is_empty()) {
        block = block.title_bottom(Line::from(vec![
            Span::styled(" / ", Style::default().fg(t.command)),
            Span::styled(safe_label(&a.archive_filter), Style::default().fg(t.value)),
            Span::styled(
                if a.archive_filter_editing {
                    if a.preferences.ascii {
                        "_"
                    } else {
                        "▏"
                    }
                } else {
                    ""
                },
                Style::default().fg(t.command),
            ),
            Span::styled(
                if a.archive_filter_editing {
                    " · Esc сброс "
                } else {
                    " · / править "
                },
                Style::default().fg(t.dim),
            ),
        ]));
    }
    block
}

pub(super) fn items(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    let archive = a.visible_sessions();
    let entries: Vec<String> = match a.view {
        View::Archive => archive.iter().map(|s| safe_label(s.title())).collect(),
        View::Queue => {
            a.queue
                .iter()
                .map(|q| {
                    format!(
                        "{} {}",
                        if q.state == "cancelling" {
                            "~ остановка"
                        } else if q.state == "waiting_input" {
                            "~ фрагменты"
                        } else if q.state == "cancelled" {
                            "x отменено"
                        } else if q
                            .retry
                            .as_ref()
                            .is_some_and(|plan| plan.state == "retry_wait")
                        {
                            "~ повтор"
                        } else if q.retry.as_ref().is_some_and(|plan| matches!(
                            plan.state.as_str(),
                            "blocked" | "stalled"
                        )) {
                            "? повтор"
                        } else if q.stuck {
                            "! остановлено"
                        } else {
                            state_symbol(&q.state, a.preferences.ascii)
                        },
                        safe_label(&q.title)
                    )
                })
                .collect()
        }
        View::Notes => a
            .notes
            .iter()
            .map(|s| format!("{} ({})", safe_label(&s.name), s.notes.len()))
            .collect(),
        View::Ask => a
            .asks
            .iter()
            .map(|s| {
                format!(
                    "{} {}",
                    state_symbol(&s.status, a.preferences.ascii),
                    safe_label(s.input_name.as_deref().unwrap_or(&s.id))
                )
            })
            .collect(),
        View::Settings => a
            .visible_settings()
            .into_iter()
            .map(|s| {
                format!(
                    "{} {}",
                    if a.settings_edits.contains_key(&s.key) {
                        "*"
                    } else {
                        " "
                    },
                    safe_label(&s.label)
                )
            })
            .collect(),
        View::Search => a.hits.iter().map(|h| safe_label(&h.snippet)).collect(),
        View::Chat => a
            .answer
            .sources
            .iter()
            .map(|s| format!("[{}] {}", s.n, safe_label(&s.session)))
            .collect(),
    };
    let resource = match a.view {
        View::Archive => Some(Resource::Sessions),
        View::Notes => Some(Resource::Notes),
        View::Queue => Some(Resource::Queue),
        View::Ask => Some(Resource::Asks),
        View::Settings => Some(Resource::Settings),
        _ => None,
    };
    let error = resource.and_then(|resource| a.errors.get(&resource));
    let title = if error.is_some() {
        "Список · ошибка обновления".to_string()
    } else if a.view == View::Archive && (a.archive_filter_editing || !a.archive_filter.is_empty())
    {
        format!("Список · {}/{}", archive.len(), a.sessions.len())
    } else if a.view == View::Archive {
        "Список · / поиск".to_string()
    } else {
        "Список · Enter открыть".to_string()
    };
    let block = list_block(a, t, &title);
    if entries.is_empty() {
        let message: String = if error.is_some() {
            "Список недоступен · ~ консоль · r повторить".into()
        } else if a.view == View::Archive && !a.archive_filter.is_empty() {
            if a.archive_filter_editing {
                "Нет совпадений\nEsc — сбросить фильтр".into()
            } else {
                "Нет совпадений\n/ — изменить фильтр".into()
            }
        } else if resource.is_some_and(|resource| !a.last_request.contains_key(&resource)) {
            "Загрузка…".into()
        } else {
            "Пока пусто".into()
        };
        f.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(if error.is_some() { t.error } else { t.dim }))
                .wrap(Wrap { trim: false })
                .block(block),
            area,
        );
        return;
    }
    let mut state = ListState::default().with_selected(Some(a.selected));
    *state.offset_mut() = list_metrics(a).1;
    let items = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let lines = if a.view == View::Archive {
                activity::lines(a, archive[index], t)
            } else {
                vec![Line::from(entry)]
            };
            ListItem::new(lines)
        })
        .collect::<Vec<_>>();
    f.render_stateful_widget(
        List::new(items)
            .block(block)
            .highlight_style(selection_style(t))
            .highlight_symbol("> "),
        area,
        &mut state,
    );
}
