//! Stateless terminal presentation. All user/server text is inert display data.
mod documents;
use crate::app::{state_symbol, timecode, App, Tab, View, TABS, VIEWS};
use crate::io::Resource;
use localvox_light_client::{safe_label, safe_text};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: Color,
    pub ink: Color,
    pub dim: Color,
    pub line: Color,
    pub command: Color,
    pub argument: Color,
    pub value: Color,
    pub ok: Color,
    pub busy: Color,
    pub error: Color,
    pub selection: Color,
    pub ascii: bool,
}
impl Theme {
    pub fn for_app(a: &App) -> Self {
        if a.monochrome {
            return Self {
                bg: Color::Reset,
                ink: Color::Reset,
                dim: Color::Reset,
                line: Color::Reset,
                command: Color::Reset,
                argument: Color::Reset,
                value: Color::Reset,
                ok: Color::Reset,
                busy: Color::Reset,
                error: Color::Reset,
                selection: Color::Reset,
                ascii: a.ascii,
            };
        }
        if a.color16 {
            return Self {
                bg: Color::Reset,
                ink: Color::Reset,
                dim: Color::DarkGray,
                line: Color::DarkGray,
                command: Color::Cyan,
                argument: Color::Magenta,
                value: Color::Yellow,
                ok: Color::Green,
                busy: Color::Yellow,
                error: Color::Red,
                selection: Color::Blue,
                ascii: a.ascii,
            };
        }
        if a.light {
            return Self {
                bg: Color::Rgb(246, 245, 241),
                ink: Color::Rgb(39, 43, 48),
                dim: Color::Rgb(94, 101, 108),
                line: Color::Rgb(207, 207, 199),
                command: Color::Rgb(42, 94, 133),
                argument: Color::Rgb(113, 70, 144),
                value: Color::Rgb(120, 93, 36),
                ok: Color::Rgb(35, 112, 72),
                busy: Color::Rgb(137, 96, 26),
                error: Color::Rgb(171, 54, 54),
                selection: Color::Rgb(225, 230, 229),
                ascii: a.ascii,
            };
        }
        Self {
            bg: Color::Rgb(23, 25, 29),
            ink: Color::Rgb(216, 220, 227),
            dim: Color::Rgb(156, 164, 175),
            line: Color::Rgb(52, 57, 66),
            command: Color::Rgb(133, 180, 220),
            argument: Color::Rgb(194, 165, 216),
            value: Color::Rgb(214, 194, 139),
            ok: Color::Rgb(127, 189, 154),
            busy: Color::Rgb(216, 183, 119),
            error: Color::Rgb(227, 141, 133),
            selection: Color::Rgb(42, 48, 57),
            ascii: a.ascii,
        }
    }
}
fn borders<'a>(ascii: bool) -> Block<'a> {
    let block = Block::bordered();
    if ascii {
        block.border_set(ratatui::symbols::border::Set {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "|",
            vertical_right: "|",
            horizontal_top: "-",
            horizontal_bottom: "-",
        })
    } else {
        block
    }
}
fn selection_style(t: Theme) -> Style {
    let style = Style::default().bg(t.selection);
    if t.selection == Color::Reset {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    }
}
fn panel<'a>(title: &str, focused: bool, t: Theme) -> Block<'a> {
    borders(t.ascii)
        .borders(Borders::TOP | Borders::LEFT)
        .title(format!(
            " {}{} ",
            if focused && t.command == Color::Reset {
                "> "
            } else {
                ""
            },
            safe_label(title)
        ))
        .border_style(Style::default().fg(if focused { t.command } else { t.line }))
}
pub fn draw(f: &mut Frame, a: &App) {
    let t = Theme::for_app(a);
    let area = f.area();
    f.render_widget(
        Block::default().style(Style::default().fg(t.ink).bg(t.bg)),
        area,
    );
    if area.width < 35 || area.height < 10 {
        f.render_widget(
            Paragraph::new(
                "LocalVox\nОкно слишком мало\nF8 — остановить запись\nq / F10 — закрыть клиент",
            )
            .wrap(Wrap { trim: false }),
            area,
        );
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(2),
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);
    let record = if a.pending_record.is_some() {
        "ожидаем подтверждение"
    } else if a.record.recording {
        "ИДЁТ ЗАПИСЬ"
    } else if a.record.engine {
        "готов к записи"
    } else {
        "движок недоступен"
    };
    let rec_color = if a.record.recording {
        t.error
    } else if a.pending_record.is_some() {
        t.busy
    } else {
        t.dim
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled(
                    " localvox ",
                    Style::default().fg(t.ink).add_modifier(Modifier::BOLD),
                ),
                Span::styled(" / технический журнал   ", Style::default().fg(t.dim)),
                Span::styled(
                    format!("{} {record}", if a.ascii { "*" } else { "●" }),
                    Style::default().fg(rec_color),
                ),
                Span::styled(
                    format!("   очередь {} · ошибки {}", a.jobs.pending, a.jobs.failed),
                    Style::default().fg(t.dim),
                ),
            ]),
            Line::from(Span::styled(
                format!(
                    " pre-roll {}   {}",
                    timecode(a.record.preroll_sec),
                    a.record
                        .voice
                        .as_ref()
                        .map(|v| {
                            let detail = if let Some(capturing) = &v.capturing {
                                format!("диктую → {}: {}", capturing.slot, capturing.text)
                            } else if let Some(last) = &v.last {
                                if let Some(error) = &last.error {
                                    format!("ошибка заметки → {}: {error}", last.slot)
                                } else {
                                    format!("сохранено → {}: {}", last.slot, last.text)
                                }
                            } else {
                                format!("голос: {}", v.detail)
                            };
                            safe_label(&detail).replace(['\n', '\t'], " ")
                        })
                        .unwrap_or_default()
                ),
                Style::default().fg(t.dim),
            )),
        ]),
        rows[0],
    );
    player_bar(f, a, rows[1], t);
    let wide = area.width >= 125;
    let compact = area.width < 95;
    if compact {
        let title = format!("{} · Tab переключить панель", VIEWS[a.nav].1);
        match a.focus {
            0 => nav(f, a, rows[2], t),
            1 => items(f, a, rows[2], t),
            _ => document(f, a, rows[2], t, &title),
        };
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints(if wide {
                vec![
                    Constraint::Length(27),
                    Constraint::Min(40),
                    Constraint::Length(28),
                ]
            } else {
                vec![Constraint::Length(27), Constraint::Min(40)]
            })
            .split(rows[2]);
        let left = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Length(9), Constraint::Min(2)])
            .split(cols[0]);
        nav(f, a, left[0], t);
        items(f, a, left[1], t);
        document(f, a, cols[1], t, &a.document_title());
        if wide {
            context(f, a, cols[2], t);
        }
    }
    let error = a.errors.values().next();
    let status = error
        .map(|e| format!("Данные не обновляются: {e}"))
        .unwrap_or_else(|| a.status.clone());
    let keys = if area.width < 95 {
        Line::from(" F8 stop  q/F10 выход  y копия  Tab фокус  : действия  ?")
    } else {
        Line::from(vec![
            Span::styled(" Tab", Style::default().fg(t.command)),
            Span::raw(" фокус   "),
            Span::styled("1–5", Style::default().fg(t.argument)),
            Span::raw(" вкладка   "),
            Span::styled(":", Style::default().fg(t.command)),
            Span::raw(" действия   y копия   F8 stop   F9 rec   ?   q / F10 выход"),
        ])
    };
    f.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(" {}", safe_label(&status)),
                Style::default().fg(if error.is_some() { t.error } else { t.dim }),
            )),
            keys,
        ]),
        rows[3],
    );
    if a.palette {
        let commands = a.commands();
        let offset = a.palette_index.saturating_sub(8);
        let entries = commands
            .iter()
            .enumerate()
            .skip(offset)
            .take(16)
            .map(|(i, c)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!(" {:02} ", i + 1), Style::default().fg(t.dim)),
                    Span::styled((*c).to_owned(), Style::default().fg(t.command)),
                ]))
            })
            .collect::<Vec<_>>();
        let rect = modal(area, 70, 20);
        f.render_widget(Clear, rect);
        let mut state = ListState::default().with_selected(Some(a.palette_index - offset));
        f.render_stateful_widget(
            List::new(entries)
                .block(
                    borders(a.ascii)
                        .title(" Действия · Enter / Esc ")
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
        f.render_widget(Paragraph::new("НОВОЕ ТЕРМИНАЛЬНОЕ ПРОСТРАНСТВО\nSpace play/pause; p fragment; g time; Alt+Left/Right +/-15s\nF6 player focus: Left/Right +/-5s, Home/End\n\nTab / Shift+Tab — фокус панели\n↑↓ / j k — выбрать строку; Enter — открыть\n1–5 — вкладка документа; y — копировать всё\n/ — поиск; : — все действия; n — создать; i — импорт URL\nF8 — остановить запись; F9 — начать запись\nPgUp / PgDn, Home / End — чтение документа\n← / → — свернуть / раскрыть подробности\n\nВ форме: Tab — поле; Enter — новая строка\nCtrl+Enter — отправить; Esc — отменить\nВставка остаётся текстом, команды не исполняются.\n\nq / F10 / Ctrl+Q закрывают только клиент.\nДемон и запись продолжают работать.\n\nЛюбая клавиша — закрыть справку").block(borders(a.ascii).title(" Помощь ").style(Style::default().bg(t.bg).fg(t.ink))).wrap(Wrap{trim:false}),rect);
    }
    if let Some(form) = &a.form {
        let rect = modal(area, 88, area.height.saturating_sub(4).min(30));
        f.render_widget(Clear, rect);
        let block = borders(a.ascii)
            .title(format!(" {} ", safe_label(&form.title)))
            .style(Style::default().bg(t.bg).fg(t.ink));
        let inner = block.inner(rect);
        f.render_widget(block, rect);
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
                        .style(Style::default().fg(t.command)),
                    rows[0],
                );
                f.render_widget(&field.editor, rows[1]);
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
                    Paragraph::new(format!("{}: {}", safe_label(&field.label), value))
                        .style(Style::default().fg(t.dim)),
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
                "Tab поле · Ctrl+Enter / Ctrl+S отправить · Esc отменить",
                Style::default().fg(t.dim),
            )),
        ];
        f.render_widget(
            Paragraph::new(footer).wrap(Wrap { trim: false }),
            fields[fields.len() - 1],
        );
    }
}
fn player_bar(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    let block = panel("Аудио · F6 фокус", a.focus == 3, t).borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let keys = if a.focus == 3 {
        "←/→ ±5с · Home/End · g время · Space пауза · F6 назад"
    } else if area.width < 95 {
        "Space play/pause · p фрагмент · g время · Alt+←/→ ±15с"
    } else {
        "Space play/pause · p фрагмент · g время · Alt+←/→ ±15с · F6 перемотка"
    };
    f.render_widget(
        Paragraph::new(vec![
            player_line(a, t, inner.width),
            Line::from(Span::styled(keys, Style::default().fg(t.dim))),
        ]),
        inner,
    );
}
fn player_line(a: &App, t: Theme, width: u16) -> Line<'static> {
    if let Some(error) = &a.playback.error {
        return Line::from(Span::styled(
            format!(" player ! {}", safe_label(error)),
            Style::default().fg(t.error),
        ));
    }
    let duration = a
        .playback
        .duration
        .or_else(|| (a.peaks.duration_sec > 0.0).then_some(a.peaks.duration_sec));
    let label = format!(
        " {} {} / {}  ",
        if a.playback.idle {
            "готов"
        } else if a.playback.paused {
            "пауза"
        } else {
            "играет"
        },
        timecode(a.playback.position),
        duration.map(timecode).unwrap_or_else(|| "—".into())
    );
    let buckets = width.saturating_sub(30).min(100) as usize;
    let mut spans = vec![Span::styled(label, Style::default().fg(t.command))];
    if !a.peaks.peaks.is_empty() && buckets > 0 {
        for i in 0..buckets {
            let from = i * a.peaks.peaks.len() / buckets;
            let to = ((i + 1) * a.peaks.peaks.len() / buckets)
                .max(from + 1)
                .min(a.peaks.peaks.len());
            let peak = a.peaks.peaks[from.min(a.peaks.peaks.len() - 1)..to]
                .iter()
                .copied()
                .filter(|v| v.is_finite())
                .fold(0.0, f64::max)
                .clamp(0.0, 1.0);
            let chars = if a.ascii {
                [" ", ".", ":", "*", "#"]
            } else {
                ["▁", "▂", "▄", "▆", "█"]
            };
            let symbol = chars[(peak * 4.0).round() as usize];
            let active = duration
                .is_some_and(|d| d > 0.0 && (i as f64 / buckets as f64) < a.playback.position / d);
            spans.push(Span::styled(
                symbol,
                Style::default().fg(if active { t.command } else { t.dim }),
            ));
        }
    }
    if a.peaks.peaks.is_empty() {
        spans.push(Span::styled("волна недоступна", Style::default().fg(t.dim)));
    }
    Line::from(spans)
}
fn modal(a: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(a.width.saturating_sub(2));
    let h = h.min(a.height.saturating_sub(2));
    Rect::new(a.x + (a.width - w) / 2, a.y + (a.height - h) / 2, w, h)
}
fn nav(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    let rows = VIEWS
        .iter()
        .enumerate()
        .map(|(i, (v, label))| {
            ListItem::new(format!(" {} {label}", if *v == a.view { ">" } else { " " }))
                .style(Style::default().fg(if i == a.nav { t.ink } else { t.dim }))
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
fn items(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    let entries: Vec<String> = match a.view {
        View::Archive => a
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "{} {}",
                    if s.recording {
                        state_symbol("running", a.ascii)
                    } else {
                        state_symbol(
                            s.job
                                .as_deref()
                                .unwrap_or(if s.cooked { "done" } else { "waiting" }),
                            a.ascii,
                        )
                    },
                    safe_label(s.title())
                )
            })
            .collect(),
        View::Queue => a
            .queue
            .iter()
            .map(|q| {
                format!(
                    "{} {}",
                    if q.stuck {
                        "! остановлено"
                    } else {
                        state_symbol(&q.state, a.ascii)
                    },
                    safe_label(&q.title)
                )
            })
            .collect(),
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
                    state_symbol(&s.status, a.ascii),
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
        "Список · ошибка обновления"
    } else {
        "Список · Enter открыть"
    };
    if entries.is_empty() {
        let message = if let Some(error) = error {
            format!("Ошибка: {}\nr — повторить", safe_text(error))
        } else if resource.is_some_and(|resource| !a.last_request.contains_key(&resource)) {
            "Загрузка…".into()
        } else {
            "Пока пусто".into()
        };
        f.render_widget(
            Paragraph::new(message)
                .style(Style::default().fg(if error.is_some() { t.error } else { t.dim }))
                .wrap(Wrap { trim: false })
                .block(panel(title, a.focus == 1, t)),
            area,
        );
        return;
    }
    let mut state = ListState::default().with_selected(Some(a.selected));
    let items = entries
        .into_iter()
        .enumerate()
        .map(|(index, entry)| {
            let mut lines = vec![Line::from(entry)];
            if a.view == View::Archive {
                let session = &a.sessions[index];
                let date = session
                    .started_at
                    .as_deref()
                    .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
                    .map(|date| date.format("%d.%m.%Y").to_string())
                    .unwrap_or_else(|| "дата неизвестна".into());
                lines.push(Line::from(Span::styled(
                    format!("  {date} · {}", timecode(session.duration_sec)),
                    Style::default().fg(t.dim),
                )));
            }
            ListItem::new(lines)
        })
        .collect::<Vec<_>>();
    f.render_stateful_widget(
        List::new(items)
            .block(panel(title, a.focus == 1, t))
            .highlight_style(selection_style(t).fg(t.ink))
            .highlight_symbol("> "),
        area,
        &mut state,
    );
}
fn document(f: &mut Frame, a: &App, area: Rect, t: Theme, title: &str) {
    let block = panel(title, a.focus == 2, t);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(2), Constraint::Min(1)])
        .split(inner);
    let tabs = if a.view == View::Archive {
        Line::from(
            TABS.iter()
                .enumerate()
                .map(|(i, (tab, label))| {
                    Span::styled(
                        format!(" {} {label} ", i + 1),
                        Style::default()
                            .fg(if *tab == a.tab { t.command } else { t.dim })
                            .add_modifier(if *tab == a.tab {
                                Modifier::BOLD
                            } else {
                                Modifier::empty()
                            }),
                    )
                })
                .collect::<Vec<_>>(),
        )
    } else {
        Line::from(Span::styled(VIEWS[a.nav].1, Style::default().fg(t.command)))
    };
    f.render_widget(Paragraph::new(tabs), rows[0]);
    if a.view == View::Archive && a.tab == Tab::Progress {
        let lines = journal_lines(a, t);
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        let max = paragraph
            .line_count(rows[1].width)
            .saturating_sub(rows[1].height as usize)
            .min(u16::MAX as usize) as u16;
        let scroll = journal_scroll(a, rows[1].width, rows[1].height, max);
        f.render_widget(paragraph.scroll((scroll, 0)), rows[1]);
        return;
    }
    let projection = documents::project(a, t, true);
    if projection.lines.is_empty() {
        f.render_widget(Paragraph::new("Результат пока недоступен.\nДругие вкладки можно читать независимо.\n\n: — действия · r — обновить").style(Style::default().fg(t.dim)).wrap(Wrap{trim:false}),rows[1]);
        return;
    }
    let scroll = documents::scroll(
        &projection,
        rows[1].width,
        rows[1].height,
        a.scroll,
        a.follow_selection,
    );
    f.render_widget(
        Paragraph::new(projection.lines)
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        rows[1],
    );
}
pub fn document_metrics(a: &App) -> (u16, u16) {
    let (width, height) = a.viewport;
    let body_width = width.saturating_sub(if width >= 125 {
        56
    } else if width >= 95 {
        28
    } else {
        1
    });
    let body_height = height.saturating_sub(8);
    let projection = documents::project(a, Theme::for_app(a), true);
    let count = Paragraph::new(projection.lines.clone())
        .wrap(Wrap { trim: false })
        .line_count(body_width)
        .min(u16::MAX as usize) as u16;
    (
        count,
        documents::scroll(
            &projection,
            body_width,
            body_height,
            a.scroll,
            a.follow_selection,
        ),
    )
}
pub fn journal_metrics(a: &App) -> (u16, u16) {
    let (width, height) = a.viewport;
    let body_width = width.saturating_sub(if width >= 125 {
        56
    } else if width >= 95 {
        28
    } else {
        1
    });
    let body_height = height.saturating_sub(8);
    let count = Paragraph::new(journal_lines(a, Theme::for_app(a)))
        .wrap(Wrap { trim: false })
        .line_count(body_width)
        .min(u16::MAX as usize) as u16;
    let max = count.saturating_sub(body_height);
    (count, journal_scroll(a, body_width, body_height, max))
}
type JournalRows = Vec<(String, usize)>;
struct JournalProjection {
    lines: Vec<Line<'static>>,
    markers: JournalRows,
    history_start: usize,
}
fn journal_lines(a: &App, t: Theme) -> Vec<Line<'static>> {
    journal_projection(a, t).lines
}
fn journal_projection(a: &App, t: Theme) -> JournalProjection {
    let uncertain = a.errors.contains_key(&Resource::Progress);
    let clock_ms = if uncertain {
        a.progress
            .at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.timestamp_millis())
            .unwrap_or(0)
    } else {
        a.wall_ms
    };
    let mut markers = Vec::new();
    let branch = if a.ascii { "+" } else { "├" };
    let end = if a.ascii { "+" } else { "└" };
    let mut lines = vec![Line::from(Span::styled(
        "Этапы / проверенные результаты",
        Style::default().fg(t.dim),
    ))];
    if let Some(error) = &a.progress.queue_error {
        lines.push(Line::from(Span::styled(
            format!("Очередь недоступна: {}", safe_label(error)),
            Style::default().fg(t.error),
        )));
    }
    if let Some(timing) = &a.progress.timing {
        let duration = |v: Option<f64>| {
            v.filter(|s| s.is_finite() && *s >= 0.0)
                .map(timecode)
                .unwrap_or_else(|| "—".into())
        };
        lines.push(Line::from(Span::styled(
            format!(
                "Ожидание старта: {} · после старта: {} · всего: {}",
                duration(timing.queue_sec),
                duration(timing.processing_sec),
                duration(timing.total_sec)
            ),
            Style::default().fg(t.dim),
        )));
    }
    if let Some(execution) = &a.progress.execution {
        let elapsed = execution
            .since
            .as_deref()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            .filter(|at| clock_ms >= at.timestamp_millis())
            .map(|at| timecode(((clock_ms - at.timestamp_millis()) as f64) / 1000.0))
            .unwrap_or_else(|| "время неизвестно".into());
        lines.push(Line::from(Span::styled(
            format!(
                "{}{}",
                if uncertain {
                    "Текущее выполнение не подтверждено; последний полученный статус".into()
                } else {
                    safe_label(&execution.message)
                },
                if ["running", "queued", "retry_wait", "blocked", "stalled"]
                    .contains(&execution.state.as_str())
                {
                    format!(" · {elapsed}")
                } else {
                    String::new()
                }
            ),
            Style::default().fg(
                if uncertain
                    || ["blocked", "stalled", "failed", "unavailable", "attention"]
                        .contains(&execution.state.as_str())
                {
                    t.error
                } else {
                    t.busy
                },
            ),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "Далее: {}",
                if execution.next.is_empty() {
                    if ["untracked", "unavailable", "attention"].contains(&execution.state.as_str())
                    {
                        "план не подтверждён".into()
                    } else {
                        "следующих этапов нет".into()
                    }
                } else {
                    execution.next.join(" → ")
                }
            ),
            Style::default().fg(t.dim),
        )));
        if let Some(at) = &execution.retry_at {
            lines.push(Line::from(Span::styled(
                format!("Повтор не раньше: {}", safe_label(at)),
                Style::default().fg(t.dim),
            )));
        }
    }
    let mut stages = [
        "download",
        "extract",
        "transcribe",
        "refine",
        "cleanup",
        "summary",
    ]
    .iter()
    .filter(|name| {
        a.progress.source.is_some()
            || !matches!(**name, "download" | "extract")
            || a.progress.stages.iter().any(|s| s.stage == **name)
    })
    .map(|name| {
        a.progress
            .stages
            .iter()
            .find(|s| s.stage == *name)
            .cloned()
            .unwrap_or_else(|| localvox_light_client::StageStatus {
                stage: (*name).into(),
                state: "waiting".into(),
                ..Default::default()
            })
    })
    .collect::<Vec<_>>();
    stages.extend(
        a.progress
            .stages
            .iter()
            .filter(|s| {
                ![
                    "download",
                    "extract",
                    "transcribe",
                    "refine",
                    "cleanup",
                    "summary",
                ]
                .contains(&s.stage.as_str())
            })
            .cloned(),
    );
    for stage in &stages {
        let color = match stage.state.as_str() {
            "failed" | "invalid" => t.error,
            "done" => t.ok,
            "running" => t.busy,
            _ => t.dim,
        };
        let counter = match (stage.done, stage.total) {
            (Some(d), Some(total)) => format!("  {d}/{total}"),
            _ => String::new(),
        };
        let elapsed = match (stage.started_at.as_deref(), stage.ended_at.as_deref()) {
            (Some(start), end) => chrono::DateTime::parse_from_rfc3339(start)
                .ok()
                .map(|start| {
                    let end = end
                        .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                        .map(|e| e.timestamp_millis())
                        .or_else(|| (stage.state == "running" && clock_ms > 0).then_some(clock_ms));
                    end.filter(|end| *end >= start.timestamp_millis())
                        .map(|end| timecode(((end - start.timestamp_millis()) as f64) / 1000.0))
                        .unwrap_or_else(|| "—".into())
                })
                .unwrap_or_else(|| "—".into()),
            _ => {
                if ["done", "failed", "invalid"].contains(&stage.state.as_str()) {
                    "длительность неизвестна".into()
                } else {
                    "—".into()
                }
            }
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("{branch} {} ", state_symbol(&stage.state, a.ascii)),
                Style::default().fg(color),
            ),
            Span::styled(safe_label(&stage.stage), Style::default().fg(t.command)),
            Span::styled(
                format!("  {}{counter}  {elapsed}", safe_label(&stage.state)),
                Style::default().fg(color),
            ),
        ]));
        if let Some(note) = &stage.note {
            lines.push(Line::from(Span::styled(
                format!("    {}", safe_label(note)),
                Style::default().fg(if matches!(stage.state.as_str(), "failed" | "invalid") {
                    t.error
                } else {
                    t.dim
                }),
            )));
        }
        if stage.state == "running" {
            if let Some(at) = stage
                .updated_at
                .as_deref()
                .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
            {
                if clock_ms >= at.timestamp_millis() {
                    lines.push(Line::from(Span::styled(
                        format!(
                            "    Последний сигнал: {} назад · heartbeat не означает прогресс",
                            timecode((clock_ms - at.timestamp_millis()) as f64 / 1000.0)
                        ),
                        Style::default().fg(t.dim),
                    )));
                }
            }
        }
    }
    lines.push(Line::from(""));
    let history_start = lines.len();
    lines.push(Line::from(Span::styled(
        format!(
            "Журнал · Enter раскрыть · / фильтр · End хвост · o раньше{}",
            if a.journal.unseen > 0 {
                format!(" · новых {}", a.journal.unseen)
            } else {
                String::new()
            }
        ),
        Style::default().fg(t.dim),
    )));
    if a.journal.reset_notice {
        lines.push(Line::from(Span::styled(
            "Файл истории сменился. Показаны события нового журнала.",
            Style::default().fg(t.busy),
        )));
    }
    if a.journal.truncated {
        lines.push(Line::from(Span::styled(
            "Показана часть истории; сворачивание строк — отдельное действие.",
            Style::default().fg(t.busy),
        )));
    }
    if a.journal.tail_gap {
        lines.push(Line::from(Span::styled(
            "Сохранено окно чтения. End загрузит последние события.",
            Style::default().fg(t.busy),
        )));
    }
    if a.journal.dropped_verbose > 0 {
        lines.push(Line::from(Span::styled(
            format!(
                "Пропущено подробностей по лимиту: {}",
                a.journal.dropped_verbose
            ),
            Style::default().fg(t.busy),
        )));
    }
    let groups = a.journal.visible_groups();
    let mut run = None;
    let mut stage = None;
    for (index, g) in groups.iter().enumerate() {
        if run.as_ref() != Some(&g.run) {
            lines.push(Line::from(Span::styled(
                format!(
                    "run {}",
                    if g.run.is_empty() {
                        "исторический · запуск неизвестен".to_owned()
                    } else {
                        safe_label(&g.run)
                    }
                ),
                Style::default().fg(t.dim),
            )));
            run = Some(g.run.clone());
            stage = None;
        }
        if stage.as_ref() != Some(&g.stage) {
            lines.push(Line::from(Span::styled(
                format!("{branch} {}", safe_label(&g.stage)),
                Style::default().fg(t.dim),
            )));
            stage = Some(g.stage.clone());
        }
        let active = !uncertain
            && a.progress.execution.as_ref().is_some_and(|execution| {
                execution.state == "running"
                    && execution
                        .run
                        .as_ref()
                        .zip(execution.operation.as_ref())
                        .is_some_and(|(run, op)| g.id == format!("{run}/{op}"))
            });
        let state = if g.state() == "running" && !active {
            "unconfirmed"
        } else {
            g.state()
        };
        let duration = g
            .elapsed_sec
            .or_else(|| {
                let start = chrono::DateTime::parse_from_rfc3339(g.started_at.as_deref()?)
                    .ok()?
                    .timestamp_millis();
                let end = g
                    .ended_at
                    .as_deref()
                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                    .map(|at| at.timestamp_millis())
                    .or(active.then_some(clock_ms))?;
                (end >= start).then_some((end - start) as f64 / 1000.0)
            })
            .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
            .map(timecode)
            .unwrap_or_else(|| "время неизвестно".into());
        let color = match state {
            "failed" | "invalid" => t.error,
            "running" => t.busy,
            "done" => t.ok,
            _ => t.dim,
        };
        let mut spans = vec![Span::styled(
            format!("  {end} {} ", state_symbol(state, a.ascii)),
            Style::default().fg(color),
        )];
        if let Some(command) = &g.command {
            spans.push(Span::styled(
                safe_label(&command.program),
                Style::default().fg(t.command),
            ));
            for arg in &command.args {
                spans.push(Span::styled(
                    format!(" {}", safe_label(&arg.name)),
                    Style::default().fg(t.argument),
                ));
                if let Some(value) = &arg.value {
                    spans.push(Span::styled(
                        format!(" {}", safe_label(value)),
                        Style::default().fg(t.value),
                    ));
                }
            }
        } else {
            spans.push(Span::styled(
                safe_label(&g.label),
                Style::default().fg(t.command),
            ));
        }
        spans.push(Span::styled(
            format!(
                " · {} · {} {}",
                if g.ended {
                    "операция завершилась"
                } else {
                    if state == "unconfirmed" {
                        "завершение не зафиксировано"
                    } else {
                        state
                    }
                },
                duration,
                g.at.get(11..19).unwrap_or("")
            ),
            Style::default().fg(color),
        ));
        let mut line = Line::from(spans);
        if a.focus == 2 && index == a.item {
            line = line.style(selection_style(t));
        }
        markers.push((g.id.clone(), lines.len()));
        lines.push(line);
        for e in &g.details {
            if matches!(e.status.as_deref(), Some("failed" | "invalid")) {
                lines.push(Line::from(Span::styled(
                    format!("      ! {}", safe_label(e.text.as_deref().unwrap_or(""))),
                    Style::default().fg(t.error),
                )));
            }
        }
        let open = a.journal.expanded.contains(&g.id);
        let count = g.hidden_lines();
        if count > 0 {
            lines.push(Line::from(Span::styled(
                if open {
                    "      − свернуть".into()
                } else {
                    format!("      + {count} lines")
                },
                Style::default().fg(t.dim),
            )));
        }
        if open {
            for e in &g.details {
                if !matches!(e.status.as_deref(), Some("failed" | "invalid")) {
                    for text in safe_text(e.text.as_deref().unwrap_or("")).split('\n') {
                        lines.push(Line::from(Span::styled(
                            format!("      {} {text}", e.at.get(11..19).unwrap_or("")),
                            Style::default().fg(t.dim),
                        )));
                    }
                }
            }
        }
    }
    if groups.is_empty() {
        lines.push(Line::from(Span::styled(
            "Подробных событий пока нет.",
            Style::default().fg(t.dim),
        )));
    }
    JournalProjection {
        lines,
        markers,
        history_start,
    }
}
fn journal_dimensions(a: &App) -> (u16, u16) {
    let (width, height) = a.viewport;
    (
        width.saturating_sub(if width >= 125 {
            56
        } else if width >= 95 {
            28
        } else {
            1
        }),
        height.saturating_sub(8),
    )
}
fn wrapped_offsets(lines: &[Line], width: u16) -> Vec<usize> {
    let mut offsets = Vec::with_capacity(lines.len() + 1);
    offsets.push(0);
    for line in lines {
        let count = Paragraph::new(vec![line.clone()])
            .wrap(Wrap { trim: false })
            .line_count(width);
        offsets.push(offsets.last().copied().unwrap_or(0) + count);
    }
    offsets
}
fn journal_scroll(a: &App, width: u16, height: u16, max: u16) -> u16 {
    if a.journal.follow {
        return max;
    }
    let mut offset = a.scroll.min(max) as usize;
    if a.follow_selection {
        let projection = journal_projection(a, Theme::for_app(a));
        if let Some((_, logical)) = projection.markers.get(a.item) {
            let offsets = wrapped_offsets(&projection.lines, width);
            let start = offsets[*logical];
            let end = offsets[logical + 1];
            if start < offset {
                offset = start;
            } else if end > offset + height as usize {
                offset = end.saturating_sub(height as usize);
            }
        }
    }
    offset.min(max as usize) as u16
}
pub struct JournalAnchor {
    group: String,
    relative: i64,
}
pub fn journal_anchor(a: &App) -> Option<JournalAnchor> {
    if a.journal.follow || !a.is_journal() {
        return None;
    }
    let (width, height) = journal_dimensions(a);
    let projection = journal_projection(a, Theme::for_app(a));
    let scroll = journal_metrics(a).1 as usize;
    let heights = wrapped_offsets(&projection.lines, width);
    if scroll < heights[projection.history_start] {
        return None;
    }
    let offsets = projection
        .markers
        .into_iter()
        .map(|(id, row)| (id, heights[row]))
        .collect::<Vec<_>>();
    let selected = if a.focus == 2 {
        offsets
            .get(a.item)
            .filter(|(_, at)| *at >= scroll && *at < scroll + height as usize)
    } else {
        None
    };
    let (group, at) = selected
        .or_else(|| offsets.iter().rev().find(|(_, at)| *at <= scroll))
        .or_else(|| offsets.first())?;
    Some(JournalAnchor {
        group: group.clone(),
        relative: scroll as i64 - *at as i64,
    })
}
pub fn restore_journal_anchor(a: &mut App, anchor: Option<JournalAnchor>) {
    let Some(anchor) = anchor else {
        return;
    };
    let (width, _) = journal_dimensions(a);
    let projection = journal_projection(a, Theme::for_app(a));
    if let Some((_, row)) = projection
        .markers
        .iter()
        .find(|(id, _)| *id == anchor.group)
    {
        a.scroll = (wrapped_offsets(&projection.lines, width)[*row] as i64 + anchor.relative)
            .clamp(0, u16::MAX as i64) as u16;
    }
}
fn context(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    let mut lines = vec![Line::from(Span::styled(
        "Рабочая версия",
        Style::default().fg(t.dim),
    ))];
    for v in &a.versions {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{} v{:03} ", if v.best { "*" } else { " " }, v.id),
                Style::default().fg(if v.best { t.ok } else { t.dim }),
            ),
            Span::styled(safe_label(&v.model), Style::default().fg(t.value)),
        ]));
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Источники",
        Style::default().fg(t.dim),
    )));
    for s in &a.sources {
        lines.push(Line::from(format!(
            "{} {}",
            s.source_id,
            safe_label(&s.name)
        )));
    }
    if let Some(p) = if a.tab == Tab::Summary {
        a.summary.provenance.as_ref()
    } else {
        a.readable.provenance.as_ref()
    } {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            safe_label(&p.model),
            Style::default().fg(t.value),
        )));
        lines.push(Line::from(safe_label(&p.template)));
        if let Some(d) = &p.doubts {
            lines.push(Line::from(Span::styled(
                safe_label(d),
                Style::default().fg(t.busy),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        ": все действия",
        Style::default().fg(t.command),
    )));
    f.render_widget(
        Paragraph::new(lines)
            .block(panel("Контекст", false, t))
            .wrap(Wrap { trim: false }),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn waiting_and_running_pipeline_keep_their_reason_next_step_and_clock_visible() {
        let mut app = App {
            tab: Tab::Progress,
            focus: 2,
            viewport: (120, 36),
            ..Default::default()
        };
        app.wall_ms = chrono::DateTime::parse_from_rfc3339("2026-10-31T07:00:25Z")
            .unwrap()
            .timestamp_millis();
        app.progress.execution = Some(localvox_light_client::Execution {
            state: "queued".into(),
            message: "Ожидает свободного обработчика".into(),
            since: Some("2026-10-31T07:00:20Z".into()),
            next: vec!["summary".into()],
            ..Default::default()
        });
        app.progress.stages = vec![
            localvox_light_client::StageStatus {
                stage: "transcribe".into(),
                state: "done".into(),
                started_at: Some("2026-10-31T07:00:01Z".into()),
                ended_at: Some("2026-10-31T07:00:05Z".into()),
                ..Default::default()
            },
            localvox_light_client::StageStatus {
                stage: "refine".into(),
                state: "skipped".into(),
                ..Default::default()
            },
            localvox_light_client::StageStatus {
                stage: "cleanup".into(),
                state: "skipped".into(),
                ..Default::default()
            },
            localvox_light_client::StageStatus {
                stage: "summary".into(),
                state: "queued".into(),
                ..Default::default()
            },
        ];
        let text = |app: &App| {
            journal_lines(app, Theme::for_app(app))
                .iter()
                .map(|line| {
                    line.spans
                        .iter()
                        .map(|span| span.content.as_ref())
                        .collect::<String>()
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        assert!(text(&app).contains("Ожидает свободного обработчика · 00:05"));
        assert!(text(&app).contains("Далее: summary"));
        app.wall_ms += 2000;
        assert!(text(&app).contains("00:07"));
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        insta::assert_debug_snapshot!("pipeline_waiting", terminal.backend().buffer());
        app.progress.at = Some("2026-10-31T07:00:27Z".into());
        app.progress.execution.as_mut().unwrap().state = "running".into();
        app.progress.execution.as_mut().unwrap().message = "Этап выполняется".into();
        app.progress.execution.as_mut().unwrap().since = Some("2026-10-31T07:00:25Z".into());
        app.progress.stages[3].state = "running".into();
        app.progress.stages[3].started_at = Some("2026-10-31T07:00:25Z".into());
        app.progress.stages[3].updated_at = Some("2026-10-31T07:00:26Z".into());
        assert!(text(&app).contains("Этап выполняется · 00:02"));
        app.journal.merge(
            serde_json::from_str(include_str!("../../../tests/fixtures/journal.json")).unwrap(),
            false,
        );
        app.progress.execution.as_mut().unwrap().run = Some("r1".into());
        app.progress.execution.as_mut().unwrap().operation = Some("op1".into());
        assert!(!text(&app).contains("завершение не зафиксировано"));
        app.progress.execution.as_mut().unwrap().operation = Some("retry-op2".into());
        assert!(text(&app).contains("завершение не зафиксировано"));
        app.progress.execution.as_mut().unwrap().operation = Some("op1".into());
        app.errors.insert(Resource::Progress, "offline".into());
        assert!(text(&app).contains("Текущее выполнение не подтверждено"));
        assert!(text(&app).contains("завершение не зафиксировано"));
        let frozen = text(&app);
        app.wall_ms += 60_000;
        assert_eq!(
            text(&app),
            frozen,
            "an unavailable status must not advance clocks"
        );
        app.errors.remove(&Resource::Progress);
        app.progress.execution.as_mut().unwrap().state = "done".into();
        app.progress.execution.as_mut().unwrap().message =
            "Все запрошенные результаты готовы".into();
        app.progress.stages[3].state = "done".into();
        app.progress.stages[3].ended_at = Some("2026-10-31T07:00:30Z".into());
        let finished = text(&app);
        assert!(finished.contains("summary  done  00:05"));
        app.wall_ms += 60_000;
        assert_eq!(
            text(&app),
            finished,
            "completed durations must remain stable"
        );
    }

    #[test]
    fn text_without_cleanup_is_readable_and_copies_only_transcript_words() {
        let mut app = App {
            tab: Tab::Readable,
            focus: 2,
            ..Default::default()
        };
        app.readable.from_transcript = true;
        app.readable
            .lines
            .push(localvox_light_client::TranscriptLine {
                text: "Первичная расшифровка".into(),
                who: "Анна".into(),
                start_sec: 2.0,
                end_sec: 3.0,
                ..Default::default()
            });
        app.readable
            .lines
            .push(localvox_light_client::TranscriptLine {
                text: "Продолжение разговора.".into(),
                who: "Борис".into(),
                start_sec: 3600.0,
                end_sec: 3602.0,
                ..Default::default()
            });
        let projection = documents::project(&app, Theme::for_app(&app), false);
        let text = projection
            .lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Исходная расшифровка") && text.contains("Первичная расшифровка"));
        assert_eq!(
            app.copy_text(),
            "Первичная расшифровка Продолжение разговора."
        );
        assert!(text.contains(&app.copy_text()));
        assert!(!text.contains("Анна") && !text.contains("Борис") && !text.contains("00:02"));
        assert!(projection.selected.is_none());
    }

    fn plain_text_snapshot(width: u16, height: u16) {
        let mut app = App {
            tab: Tab::Readable,
            focus: 2,
            viewport: (width, height),
            ..Default::default()
        };
        app.readable.from_transcript = true;
        app.readable.lines = (0..20)
            .map(|i| localvox_light_client::TranscriptLine {
                text: format!("Реплика {i}: сохраняем все слова и читаем документ целиком."),
                who: "Скрытый говорящий".into(),
                start_sec: i as f64,
                end_sec: i as f64 + 1.0,
                ..Default::default()
            })
            .collect();
        let projection = documents::project(&app, Theme::for_app(&app), true);
        let count = Paragraph::new(projection.lines.clone())
            .wrap(Wrap { trim: false })
            .line_count(width - 1);
        let offset = documents::scroll(&projection, width - 1, 6, u16::MAX, true);
        assert!(offset > 0 && offset as usize <= count.saturating_sub(6));
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        insta::assert_debug_snapshot!(
            format!("plain_text_{width}_{height}"),
            terminal.backend().buffer()
        );
    }

    #[test]
    fn snapshot_plain_text_wide() {
        plain_text_snapshot(120, 36);
    }

    #[test]
    fn snapshot_plain_text_narrow() {
        plain_text_snapshot(64, 24);
    }

    fn player_snapshot(width: u16, height: u16) {
        let mut app = App {
            focus: 3,
            current: Some("demo".into()),
            viewport: (width, height),
            ..Default::default()
        };
        app.playback = crate::player::PlaybackState {
            session: app.current.clone(),
            position: 3900.0,
            duration: Some(7200.0),
            paused: true,
            ..Default::default()
        };
        app.peaks.duration_sec = 7200.0;
        app.peaks.peaks = (0..300).map(|i| ((i * 7) % 11) as f64 / 11.0).collect();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        let buffer = terminal.backend().buffer();
        let plain = (0..height)
            .map(|y| {
                (0..width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");
        assert!(plain.contains("пауза") && plain.contains("Home/End") && plain.contains("F8"));
        insta::assert_debug_snapshot!(format!("player_{width}_{height}"), buffer);
    }

    #[test]
    fn snapshot_player_wide() {
        player_snapshot(160, 48);
    }
    #[test]
    fn snapshot_player_narrow() {
        player_snapshot(80, 24);
    }
    #[test]
    fn snapshot_player_low() {
        player_snapshot(64, 12);
    }

    fn workspace_snapshot(width: u16, height: u16, light: bool) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/workspace.json")).unwrap();
        let mut app = App::default();
        app.light = light;
        app.focus = 2;
        app.status = "Данные обновлены".into();
        app.jobs = serde_json::from_value(fixture["jobs"].clone()).unwrap();
        app.sessions = serde_json::from_value(fixture["sessions"].clone()).unwrap();
        app.current = Some(app.sessions[0].name.clone());
        app.record = serde_json::from_value(fixture["record"].clone()).unwrap();
        app.summary = serde_json::from_value(fixture["summary"].clone()).unwrap();
        app.versions = serde_json::from_value(fixture["versions"].clone()).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        insta::assert_debug_snapshot!(
            format!(
                "workspace_{width}_{height}_{}",
                if light { "light" } else { "dark" }
            ),
            terminal.backend().buffer()
        );
    }
    #[test]
    fn snapshot_wide_dark() {
        workspace_snapshot(160, 48, false);
    }
    #[test]
    fn snapshot_wide_light() {
        workspace_snapshot(160, 48, true);
    }
    #[test]
    fn snapshot_medium_dark() {
        workspace_snapshot(120, 36, false);
    }
    #[test]
    fn snapshot_medium_light() {
        workspace_snapshot(120, 36, true);
    }
    #[test]
    fn snapshot_narrow_dark() {
        workspace_snapshot(80, 24, false);
    }
    #[test]
    fn snapshot_narrow_light() {
        workspace_snapshot(80, 24, true);
    }
    fn journal_snapshot(
        width: u16,
        height: u16,
        light: bool,
        ascii: bool,
        mono: bool,
        open: bool,
        failed: bool,
    ) {
        let mut app = App {
            tab: Tab::Progress,
            focus: 2,
            light,
            ascii,
            monochrome: mono,
            color16: ascii,
            viewport: (width, height),
            ..Default::default()
        };
        app.wall_ms = chrono::DateTime::parse_from_rfc3339("2026-10-31T07:00:25Z")
            .unwrap()
            .timestamp_millis();
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../../tests/fixtures/workspace.json")).unwrap();
        app.record = serde_json::from_value(fixture["record"].clone()).unwrap();
        app.progress = serde_json::from_value(fixture["progress"].clone()).unwrap();
        app.journal.merge(
            serde_json::from_str(include_str!("../../../tests/fixtures/journal.json")).unwrap(),
            false,
        );
        app.journal.follow = false;
        app.follow_selection = false;
        if open {
            app.journal.expanded.insert("r1/op1".into());
            let projection = journal_projection(&app, Theme::for_app(&app));
            app.scroll = wrapped_offsets(&projection.lines, width.saturating_sub(1))
                [projection.markers[0].1] as u16;
        }
        if failed {
            app.journal
                .entries
                .push(localvox_light_client::JournalEntry {
                    id: "failure".into(),
                    run: "r1".into(),
                    operation: Some("op1".into()),
                    stage: "summary".into(),
                    kind: "log".into(),
                    status: Some("failed".into()),
                    text: Some("Сводка не прошла проверку; расшифровка доступна".into()),
                    ..Default::default()
                });
        }
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        insta::assert_debug_snapshot!(format!("journal_{width}_{height}_light{light}_ascii{ascii}_mono{mono}_open{open}_failed{failed}"),terminal.backend().buffer());
    }
    #[test]
    fn snapshot_journal_collapsed_dark() {
        journal_snapshot(160, 48, false, false, false, false, false);
    }
    #[test]
    fn snapshot_journal_collapsed_light() {
        journal_snapshot(160, 48, true, false, false, false, false);
    }
    #[test]
    fn snapshot_journal_expanded_ascii16() {
        journal_snapshot(80, 24, false, true, false, true, false);
    }
    #[test]
    fn snapshot_journal_error_monochrome() {
        journal_snapshot(120, 36, false, true, true, false, true);
    }
    #[test]
    fn snapshot_ask_failed_source() {
        let mut app = App {
            view: View::Ask,
            nav: 3,
            focus: 2,
            expanded: true,
            ..Default::default()
        };
        app.ask = Some(localvox_light_client::AskRecord {
            id: "ask-1".into(),
            status: "failed".into(),
            provider: "ollama".into(),
            input_name: Some("встреча.txt".into()),
            input_chars: 36,
            prompt: "Выдели решения".into(),
            error: Some("Сервис модели недоступен; материал сохранён".into()),
            input: Some("Синтетический материал.\nВторая строка.".into()),
            created_at: "2026-10-31T07:00:00Z".into(),
            ..Default::default()
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();
        insta::assert_debug_snapshot!("ask_failed_source", terminal.backend().buffer());
    }
    #[test]
    fn a_new_detail_above_the_reading_block_keeps_its_anchor() {
        let mut app = App {
            tab: Tab::Progress,
            focus: 2,
            follow_selection: false,
            viewport: (80, 24),
            ..Default::default()
        };
        app.journal.entries = (0..35)
            .map(|i| localvox_light_client::JournalEntry {
                id: i.to_string(),
                run: "run".into(),
                operation: Some("A".into()),
                stage: "summary".into(),
                kind: "log".into(),
                text: Some(format!("line {i}")),
                ..Default::default()
            })
            .collect();
        app.journal
            .entries
            .push(localvox_light_client::JournalEntry {
                id: "B".into(),
                run: "run".into(),
                operation: Some("B".into()),
                stage: "summary".into(),
                kind: "operation_start".into(),
                ..Default::default()
            });
        app.journal.expanded.insert("run/A".into());
        app.journal.follow = false;
        app.item = 1;
        let projection = journal_projection(&app, Theme::for_app(&app));
        let row = wrapped_offsets(&projection.lines, 79)[projection.markers[1].1];
        app.scroll = row.saturating_sub(3) as u16;
        app.scroll = journal_metrics(&app).1;
        let anchor = journal_anchor(&app);
        let previous = app.scroll;
        app.journal.merge(
            localvox_light_client::JournalPage {
                events: vec![localvox_light_client::JournalEntry {
                    id: "new".into(),
                    run: "run".into(),
                    operation: Some("A".into()),
                    stage: "summary".into(),
                    kind: "log".into(),
                    text: Some("late detail".into()),
                    ..Default::default()
                }],
                ..Default::default()
            },
            false,
        );
        restore_journal_anchor(&mut app, anchor);
        let projection = journal_projection(&app, Theme::for_app(&app));
        let new_row = wrapped_offsets(&projection.lines, 79)[projection.markers[1].1];
        assert_eq!(new_row - app.scroll as usize, row - previous as usize);
        assert_eq!(app.item, 1);
    }
    #[test]
    fn selecting_a_long_transcript_block_brings_it_into_view() {
        let mut app = App {
            tab: Tab::Transcript,
            focus: 2,
            viewport: (80, 24),
            ..Default::default()
        };
        app.transcript = (0..20)
            .map(|i| localvox_light_client::TranscriptLine {
                who: format!("Участник {i}"),
                text: "длинная реплика ".repeat(25),
                start_sec: i as f64,
                end_sec: i as f64 + 1.0,
                ..Default::default()
            })
            .collect();
        app.item = 15;
        let projection = documents::project(&app, Theme::for_app(&app), true);
        let offset = documents::scroll(&projection, 79, 16, 0, true);
        let (from, _) = projection.selected.unwrap();
        let start = wrapped_offsets(&projection.lines, 79)[from];
        assert!(start >= offset as usize && start < (offset as usize + 16));
    }
    #[test]
    fn layout_keeps_stop_and_client_exit_at_all_sizes() {
        for (w, h) in [(160, 48), (120, 36), (80, 24), (30, 8)] {
            let mut term = Terminal::new(TestBackend::new(w, h)).unwrap();
            let a = App::default();
            term.draw(|f| draw(f, &a)).unwrap();
            let buffer = term.backend().buffer();
            let text = buffer
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("F8"), "{w}x{h}");
            assert!(text.contains("F10"), "{w}x{h}");
            if w >= 35 {
                assert!(buffer
                    .content
                    .iter()
                    .any(|c| c.fg == Theme::for_app(&a).command));
            }
        }
    }
    #[test]
    fn hostile_document_cannot_write_escape_controls() {
        let mut a = App::default();
        a.summary.blocks.push(localvox_light_client::SummaryBlock {
            text: "\x1b]52;bad\x07".into(),
            ..Default::default()
        });
        let mut term = Terminal::new(TestBackend::new(120, 36)).unwrap();
        term.draw(|f| draw(f, &a)).unwrap();
        assert!(term
            .backend()
            .buffer()
            .content
            .iter()
            .all(|c| !c.symbol().contains(['\x1b', '\x07'])));
    }
    #[test]
    fn snapshot_representation_detects_symbols_styles_and_geometry() {
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| draw(f, &App::default())).unwrap();
        let original = terminal.backend().buffer().clone();
        let contract = format!("{original:#?}");
        let mut symbol = original.clone();
        symbol[(1, 0)].set_symbol("X");
        assert_ne!(contract, format!("{symbol:#?}"));
        let mut color = original.clone();
        color[(1, 0)].set_fg(Color::Magenta);
        assert_ne!(contract, format!("{color:#?}"));
        let mut geometry = original;
        geometry.resize(Rect::new(0, 0, 79, 24));
        assert_ne!(contract, format!("{geometry:#?}"));
    }
    #[test]
    fn ascii_borders_and_monochrome_do_not_require_unicode_or_rgb() {
        for mono in [false, true] {
            let app = App {
                ascii: true,
                color16: true,
                monochrome: mono,
                ..Default::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(120, 36)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            assert!(terminal.backend().buffer().content.iter().all(|c| !c
                .symbol()
                .chars()
                .any(|c| ('\u{2500}'..='\u{257f}').contains(&c))));
            assert!(terminal
                .backend()
                .buffer()
                .content
                .iter()
                .all(|c| !matches!(c.fg, Color::Rgb(..)) && !matches!(c.bg, Color::Rgb(..))));
        }
    }
    #[test]
    fn recording_voice_delivery_and_open_forms_keep_stop_visible_with_long_titles() {
        use localvox_light_client::{Capturing, Session, VoiceStatus, Written};
        for (width, height) in [(160, 48), (120, 36), (80, 24)] {
            let mut app = App::default();
            app.record.engine = true;
            app.record.recording = true;
            app.sessions = vec![Session {
                name: "id".into(),
                title: Some("Длинная встреча 🦀 ".repeat(50)),
                ..Default::default()
            }];
            app.current = Some("id".into());
            app.record.voice = Some(VoiceStatus {
                capturing: Some(Capturing {
                    slot: "идеи".into(),
                    text: "текст заметки".into(),
                }),
                ..Default::default()
            });
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| draw(f, &app)).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("ИДЁТ ЗАПИСЬ") && text.contains("диктую → идеи"));
            app.record.voice = Some(VoiceStatus {
                last: Some(Written {
                    slot: "идеи".into(),
                    error: Some("нет доступа".into()),
                    ..Default::default()
                }),
                ..Default::default()
            });
            terminal.draw(|f| draw(f, &app)).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("ошибка заметки → идеи"));
            app.command(5);
            app.paste("Черновик при изменении размера");
            terminal.draw(|f| draw(f, &app)).unwrap();
            let text = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect::<String>();
            assert!(text.contains("F8") && text.contains("F10"));
            assert!(app.form.as_ref().unwrap().fields[0]
                .value
                .contains("Черновик"));
        }
    }
    #[test]
    fn archive_list_distinguishes_loading_empty_error_and_keeps_dated_unicode_rows() {
        use localvox_light_client::Session;
        for (width, height) in [(160, 48), (120, 36), (80, 24)] {
            let mut app = App {
                focus: 1,
                ..Default::default()
            };
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            let mut list_text = |app: &App| {
                terminal
                    .draw(|frame| items(frame, app, frame.area(), Theme::for_app(app)))
                    .unwrap();
                terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .map(|c| c.symbol())
                    .collect::<String>()
            };
            assert!(list_text(&app).contains("Загрузка"));
            app.last_request.insert(Resource::Sessions, 1);
            assert!(list_text(&app).contains("Пока пусто"));
            app.errors.insert(Resource::Sessions, "HTTP 503".into());
            assert!(list_text(&app).contains("Ошибка: HTTP 503"));
            app.sessions.push(Session {
                name: "stable-id".into(),
                title: Some("Встреча 🦀 ".repeat(80)),
                started_at: Some("2026-10-31T10:30:00+03:00".into()),
                duration_sec: 3661.0,
                ..Default::default()
            });
            let text = list_text(&app);
            assert!(text.contains("Встреча 🦀"));
            assert!(text.contains("31.10.2026 · 61:01"));
            assert!(text.contains("ошибка обновления"));
            assert_eq!(app.sessions[0].name, "stable-id");
        }
    }
    #[test]
    fn stage_heartbeat_does_not_change_counters_or_the_independent_transcript() {
        let mut app = App::default();
        app.tab = Tab::Progress;
        app.wall_ms = 1_000_000;
        app.progress.stages = vec![localvox_light_client::StageStatus {
            stage: "summary".into(),
            state: "running".into(),
            done: Some(2),
            total: Some(10),
            updated_at: Some("1970-01-01T00:16:35Z".into()),
            ..Default::default()
        }];
        app.transcript = vec![localvox_light_client::TranscriptLine {
            text: "Готовый STT".into(),
            ..Default::default()
        }];
        let text = |app: &App| {
            journal_lines(app, Theme::for_app(app))
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let initial = text(&app);
        assert!(initial.contains("2/10") && initial.contains("00:05 назад"));
        app.progress.stages[0].updated_at = Some("1970-01-01T00:16:39Z".into());
        let later = text(&app);
        assert!(later.contains("2/10") && later.contains("00:01 назад"));
        app.progress.stages[0].state = "failed".into();
        app.progress.stages[0].note = Some("Ошибка сводки".into());
        assert!(text(&app).contains("Ошибка сводки"));
        app.tab = Tab::Transcript;
        assert!(app.copy_text().contains("Готовый STT"));
    }
    #[test]
    fn unreadable_and_failed_note_slots_do_not_look_empty() {
        use localvox_light_client::SlotNotes;
        for (slot, expected) in [
            (
                SlotNotes {
                    readable: true,
                    ..Default::default()
                },
                "Здесь пока нет заметок",
            ),
            (
                SlotNotes {
                    readable: false,
                    ..Default::default()
                },
                "не предоставляет чтение",
            ),
            (
                SlotNotes {
                    readable: false,
                    error: Some("назначение недоступно".into()),
                    ..Default::default()
                },
                "Не прочитать слот",
            ),
        ] {
            let mut app = App::default();
            app.view = View::Notes;
            app.notes = vec![slot];
            let text = documents::project(&app, Theme::for_app(&app), false)
                .lines
                .iter()
                .map(|l| l.to_string())
                .collect::<Vec<_>>()
                .join("\n");
            assert!(text.contains(expected));
        }
    }
}
