use super::*;

pub(super) fn render_console(f: &mut Frame, a: &App, t: Theme) {
    if !a.console.open {
        return;
    }
    let area = f.area();
    let height = (area.height / 2).clamp(6, 18).min(area.height);
    let rect = Rect::new(area.x, area.bottom() - height, area.width, height);
    f.render_widget(Clear, rect);
    let block = borders(a.ascii)
        .title(" Консоль ошибок · просмотрено ")
        .title_style(t.focus_style())
        .border_style(Style::default().fg(t.focus))
        .style(Style::default().fg(t.ink).bg(t.bg));
    let inner = block.inner(rect);
    f.render_widget(block, rect);
    let mut lines = vec![];
    let mut selected = 0;
    for (index, entry) in a.console.entries.iter().enumerate() {
        if index == a.console.selected {
            selected = lines.len();
        }
        let clock = chrono::DateTime::from_timestamp_millis(entry.at_ms)
            .map(|at| at.format("%H:%M:%SZ").to_string())
            .unwrap_or_else(|| "—".into());
        lines.push(Line::from(vec![
            Span::styled(
                format!(
                    "{} {clock} ",
                    if index == a.console.selected {
                        ">"
                    } else {
                        " "
                    }
                ),
                Style::default().fg(t.command),
            ),
            Span::styled(
                format!("{} · {}", entry.context.source, entry.context.operation),
                Style::default().fg(t.error),
            ),
        ]));
        lines.push(Line::from(Span::styled(
            format!(
                "  сессия {} · фаза {}",
                entry.context.session.as_deref().unwrap_or("—"),
                entry.context.phase.as_deref().unwrap_or("неизвестна")
            ),
            Style::default().fg(t.dim),
        )));
        for line in entry.text.lines() {
            lines.push(Line::from(Span::styled(
                format!("  {} {line}", if a.ascii { "+" } else { "└" }),
                Style::default().fg(t.ink),
            )));
        }
        if entry.repeats > 1 {
            let last = entry
                .repeat_times
                .back()
                .and_then(|at| {
                    chrono::DateTime::from_timestamp_millis(*at)
                        .map(|at| at.format("%H:%M:%SZ").to_string())
                })
                .unwrap_or_else(|| "—".into());
            lines.push(Line::from(Span::styled(
                format!(
                    "  {} {}{} {} · последний {last} · Enter",
                    if a.ascii { "+" } else { "└" },
                    if entry.expanded { "-" } else { "+" },
                    entry.repeats - 1,
                    repeat_word(entry.repeats - 1)
                ),
                Style::default().fg(t.argument),
            )));
            if entry.expanded {
                for at in &entry.repeat_times {
                    let clock = chrono::DateTime::from_timestamp_millis(*at)
                        .map(|at| at.format("%H:%M:%SZ").to_string())
                        .unwrap_or_else(|| "—".into());
                    lines.push(Line::from(Span::styled(
                        format!("    {clock} · тот же отказ"),
                        Style::default().fg(t.dim),
                    )));
                }
                if entry.repeats - 1 > entry.repeat_times.len() as u32 {
                    lines.push(Line::from("    Показаны последние 32 повторения"));
                }
            }
        }
        lines.push(Line::from(""));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "Ошибок пока нет",
            Style::default().fg(t.dim),
        )));
    }
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    let paragraph = Paragraph::new(lines.clone()).wrap(Wrap { trim: false });
    let max = paragraph
        .line_count(body.width)
        .saturating_sub(body.height as usize);
    let mut offset = max.saturating_sub(a.console.scroll as usize);
    if a.console.follow_selection {
        let chosen = Paragraph::new(lines[..selected].to_vec())
            .wrap(Wrap { trim: false })
            .line_count(body.width);
        if chosen < offset {
            offset = chosen;
        } else if chosen >= offset + body.height as usize {
            offset = chosen.saturating_sub(1).min(max);
        }
    }
    f.render_widget(
        paragraph.scroll((offset.min(u16::MAX as usize) as u16, 0)),
        body,
    );
    if inner.height > 0 {
        f.render_widget(
            Paragraph::new(if a.record.recording || a.pending_record.is_some() {
                "F8 стоп · ~ / F12 · ↑↓ читать · Tab ошибка · Enter повторы · y копия"
            } else {
                "~ / F12 закрыть · ↑↓ читать · Tab событие · Enter повторы · y копия"
            })
            .style(Style::default().fg(t.dim)),
            Rect::new(inner.x, inner.bottom() - 1, inner.width, 1),
        );
    }
}

fn repeat_word(n: u32) -> &'static str {
    if (11..=14).contains(&(n % 100)) {
        "повторов"
    } else {
        match n % 10 {
            1 => "повтор",
            2..=4 => "повтора",
            _ => "повторов",
        }
    }
}
