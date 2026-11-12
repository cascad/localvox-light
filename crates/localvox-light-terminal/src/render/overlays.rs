use super::*;

pub(super) fn draw(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    if a.palette {
        let offset = a.palette_index.saturating_sub(8);
        let entries = COMMANDS
            .iter()
            .enumerate()
            .skip(offset)
            .take(16)
            .map(|(i, (_, label))| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {:02} ", i + 1), Style::default().fg(t.dim)),
                    Span::styled((*label).to_owned(), Style::default().fg(t.command)),
                ]))
            })
            .collect::<Vec<_>>();
        let rect = modal(area, 70, 20);
        f.render_widget(Clear, rect);
        let mut state = ListState::default().with_selected(Some(a.palette_index - offset));
        f.render_stateful_widget(
            List::new(entries)
                .block(
                    borders(a.preferences.ascii)
                        .title(" Действия · Enter / Esc ")
                        .title_style(t.focus_style())
                        .border_style(Style::default().fg(t.focus))
                        .style(Style::default().bg(t.bg).fg(t.ink)),
                )
                .highlight_style(
                    Style::default()
                        .bg(t.selection)
                        .add_modifier(Modifier::REVERSED),
                ),
            rect,
            &mut state,
        );
    }
    if a.help {
        let rect = modal(area, 78, 26);
        f.render_widget(Clear, rect);
        f.render_widget(Paragraph::new("УПРАВЛЕНИЕ\nSpace play/pause; p fragment; g/п time; Alt+Left/Right +/-15s\nF6 — стрелки плееру; повтор — к документу\n\nTab / Shift+Tab — фокус панели\nEsc — окно → детали → список → меню\n↑↓ — выбрать строку; Enter — открыть\nМеню разделов: l/Enter — выбрать\nСписок: j/k ↑↓; l/Enter открыть; / поиск; Ctrl+←/→ ширина\nВ поиске буквы и / — текст; Esc — сброс и выход\n1–5 — вкладки; y — блок/реплика; Ctrl+Y — всё; 3 → y — весь текст\n/ или точка в RU — поиск в выбранной панели; : / F2 — действия; n — создать; i — URL → Enter\nF8 — остановить запись; F9 — начать запись\nX — отменить обработку; R — продолжить этап\nPgUp / PgDn, Home / End — чтение документа\n← / → или h/l — свернуть / раскрыть подробности\n\nВ форме: Tab — поле; Enter — новая строка\nCtrl+Enter — отправить; Esc — отменить\nURL, настройка, время: Enter — подтвердить\nВставка остаётся текстом, команды не исполняются.\nI — сведения в «Ход»; A / S — все настройки / сохранить\n~ / ё — консоль ошибок; F12 — также из формы\n\nF10 / Ctrl+Q — выход; qq/йй за 0.5 с — вне ввода.\nДругая клавиша или вставка отменяет первое q.\nДемон и запись продолжают работать.\n\nЛюбая клавиша — закрыть справку").block(borders(a.preferences.ascii).title(" Помощь ").title_style(t.focus_style()).border_style(Style::default().fg(t.focus)).style(Style::default().bg(t.bg).fg(t.ink))).wrap(Wrap{trim:false}),rect);
    }
    if let Some(form) = &a.form {
        let desired = match &form.kind {
            crate::app::FormKind::SettingChoice { options, .. } => {
                (options.len() as u16 + 5).clamp(7, 24)
            }
            crate::app::FormKind::Seek
            | crate::app::FormKind::Import
            | crate::app::FormKind::LlmServer
            | crate::app::FormKind::Setting(_) => 9,
            crate::app::FormKind::Shutdown => 10,
            _ => 30,
        };
        let rect = modal(
            area,
            if matches!(form.kind, crate::app::FormKind::Seek) {
                54
            } else {
                88
            },
            area.height.saturating_sub(4).min(desired),
        );
        f.render_widget(Clear, rect);
        let block = borders(a.preferences.ascii)
            .title(format!(" {} ", safe_label(&form.title)))
            .title_style(t.focus_style())
            .border_style(Style::default().fg(t.focus))
            .style(Style::default().bg(t.bg).fg(t.ink));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
        if matches!(form.kind, crate::app::FormKind::Shutdown) {
            let mut lines = vec![
                Line::from(Span::styled("Запись закроется.", t.label_style())),
                Line::from(Span::styled(
                    "Очередь продолжится при следующем запуске.",
                    t.label_style(),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "Ctrl+S / Ctrl+Enter — завершить",
                    Style::default().fg(t.command).add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled("Esc отменить", Style::default().fg(t.dim))),
            ];
            if let Some(error) = &form.error {
                lines.push(Line::from(Span::styled(
                    error.as_str(),
                    Style::default().fg(t.error),
                )));
            }
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
            render_console(f, a, t);
            return;
        }
        if let crate::app::FormKind::SettingChoice {
            options, selected, ..
        } = &form.kind
        {
            let choices = options
                .iter()
                .map(|(label, _)| ListItem::new(label.clone()))
                .collect::<Vec<_>>();
            let mut state = ListState::default().with_selected(Some(*selected));
            f.render_stateful_widget(
                List::new(choices)
                    .highlight_style(selection_style(t))
                    .highlight_symbol("> "),
                inner,
                &mut state,
            );
            if inner.height > 2 {
                f.render_widget(
                    Paragraph::new("↑↓ выбрать · Enter в черновик · Esc отменить")
                        .style(Style::default().fg(t.dim)),
                    Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
                );
            }
            render_console(f, a, t);
            return;
        }
        if matches!(form.kind, crate::app::FormKind::Seek) {
            let mut values = vec![];
            for (i, field) in form.fields.iter().enumerate() {
                if i > 0 {
                    values.push(Span::styled(" : ", Style::default().fg(t.dim)));
                }
                values.push(Span::styled(
                    format!(
                        "[ {} ]",
                        if field.value.is_empty() {
                            "__".into()
                        } else {
                            format!("{:0>2}", field.value)
                        }
                    ),
                    if i == form.active {
                        Style::default()
                            .fg(t.value)
                            .bg(t.selection)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(t.dim)
                    },
                ));
            }
            let range = a
                .media_duration()
                .map(|duration| format!("Длина записи {}", timecode(duration)))
                .unwrap_or_else(|| "Длительность пока неизвестна".into());
            let lines = vec![
                Line::from(Span::styled(" Часы      Минуты   Секунды", t.label_style())),
                Line::from(values),
                Line::from(Span::styled(
                    " 0–999     0–59     0–59",
                    Style::default().fg(t.dim),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    form.error.as_deref().unwrap_or(&range),
                    Style::default().fg(if form.error.is_some() { t.error } else { t.dim }),
                )),
                Line::from(Span::styled(
                    "Цифры / ↑↓ изменить · Tab поле",
                    Style::default().fg(t.command),
                )),
                Line::from(Span::styled(
                    "Enter перейти · Esc отменить",
                    Style::default().fg(t.dim),
                )),
            ];
            f.render_widget(Paragraph::new(lines).wrap(Wrap { trim: false }), inner);
            render_console(f, a, t);
            return;
        }
        let mut constraints = form
            .fields
            .iter()
            .enumerate()
            .map(|(i, _)| {
                if i == form.active {
                    Constraint::Min(5)
                } else {
                    Constraint::Length(2)
                }
            })
            .collect::<Vec<_>>();
        constraints.push(Constraint::Length(3));
        let fields = Layout::default()
            .direction(Direction::Vertical)
            .constraints(constraints)
            .split(inner);
        for (i, field) in form.fields.iter().enumerate() {
            if i == form.active {
                let rows = Layout::default()
                    .direction(Direction::Vertical)
                    .constraints([Constraint::Length(1), Constraint::Min(1)])
                    .split(fields[i]);
                f.render_widget(
                    Paragraph::new(format!("> {}", safe_label(&field.label)))
                        .style(t.heading_style()),
                    rows[0],
                );
                let mut editor = field.editor.clone();
                editor.set_style(Style::default().fg(t.value).bg(t.bg));
                editor.set_cursor_line_style(Style::default().fg(t.value).bg(t.bg));
                editor.set_cursor_style(Style::default().fg(t.bg).bg(t.value));
                f.render_widget(&editor, rows[1]);
            } else {
                let value = if field.secret {
                    "••••".into()
                } else {
                    safe_label(&field.value)
                        .lines()
                        .next()
                        .unwrap_or("")
                        .to_owned()
                };
                f.render_widget(
                    Paragraph::new(Line::from(vec![
                        Span::styled(format!("{}: ", safe_label(&field.label)), t.label_style()),
                        Span::styled(value, Style::default().fg(t.ink)),
                    ])),
                    fields[i],
                );
            }
        }
        let hint = if matches!(form.kind, crate::app::FormKind::Note) {
            format!("Ctrl+R предложить слот · {}", a.route_hints.join(", "))
        } else {
            String::new()
        };
        let footer = vec![
            Line::from(Span::styled(
                form.error.as_deref().unwrap_or(&hint),
                Style::default().fg(if form.error.is_some() { t.error } else { t.dim }),
            )),
            Line::from(Span::styled(
                if matches!(form.kind, crate::app::FormKind::Seek) {
                    "↑↓ число · Tab поле · Enter перейти · Esc отменить"
                } else if matches!(form.kind, crate::app::FormKind::Import) {
                    "Enter импортировать · Esc отменить"
                } else if matches!(
                    form.kind,
                    crate::app::FormKind::Setting(_) | crate::app::FormKind::LlmServer
                ) {
                    "Enter в черновик · Esc отменить"
                } else {
                    "Tab поле · Ctrl+Enter / Ctrl+S отправить · Esc отменить"
                },
                Style::default().fg(t.dim),
            )),
        ];
        f.render_widget(
            Paragraph::new(footer).wrap(Wrap { trim: false }),
            fields[fields.len() - 1],
        );
    }
    render_console(f, a, t);
    if a.quit_pending() && area.height > 0 {
        f.render_widget(
            Paragraph::new(" q ещё раз — выйти · до 0.5 с · другая клавиша — отмена")
                .style(Style::default().fg(t.command).bg(t.bg)),
            Rect::new(area.x, area.bottom() - 1, area.width, 1),
        );
    }
}
