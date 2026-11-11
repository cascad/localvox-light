use super::*;

pub(super) fn player_bar(f: &mut Frame, a: &App, area: Rect, t: Theme) {
    let block = panel(
        if a.focus == 3 {
            "Аудио · ←/→ перемотка · F6 к документу"
        } else {
            "Аудио · F6: стрелки плееру"
        },
        a.focus == 3,
        t,
    )
    .borders(Borders::ALL);
    let inner = block.inner(area);
    f.render_widget(block, area);
    let keys = if a.focus == 3 {
        "←/→ ±5с · Home/End · g время · Space ▶/‖"
    } else if area.width < 95 {
        "Space ▶/‖ · p фрагмент · g время · Alt+←/→ ±15с"
    } else {
        "Space ▶/‖ · p фрагмент · g время · Alt+←/→ ±15с · F6: стрелки плееру"
    };
    f.render_widget(
        Paragraph::new(vec![
            player_line(a, t, inner.width),
            Line::from(Span::styled(keys, Style::default().fg(t.dim))),
        ]),
        inner,
    );
}

pub(super) fn player_line(a: &App, t: Theme, width: u16) -> Line<'static> {
    let duration = a.media_duration();
    let duration_text = duration.map(timecode).unwrap_or_else(|| "—".into());
    let position_text = format!(
        "{:>width$}",
        timecode(a.playback.position),
        width = duration_text.len().max(5)
    );
    let label = format!(
        " {} {} / {}  ",
        if a.playback.error.is_some() {
            "!"
        } else if a.playback.idle {
            if a.preferences.ascii {
                "."
            } else {
                "■"
            }
        } else if a.playback.paused {
            if a.preferences.ascii {
                "="
            } else {
                "‖"
            }
        } else {
            if a.preferences.ascii {
                ">"
            } else {
                "▶"
            }
        },
        position_text,
        duration_text
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
            let chars = if a.preferences.ascii {
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
        if let Some(duration) = duration {
            let cells = width.saturating_sub(56).min(36) as usize;
            if cells > 3 {
                let position = ((a.playback.position / duration).clamp(0.0, 1.0)
                    * (cells - 1) as f64) as usize;
                spans.push(Span::styled(
                    format!(
                        "[{}] ",
                        (0..cells)
                            .map(|i| if i == position { '|' } else { '-' })
                            .collect::<String>()
                    ),
                    Style::default().fg(t.dim),
                ));
            }
        }
        let message = if a.errors.contains_key(&Resource::Peaks) {
            "волна: ошибка · ~ консоль"
        } else if !a.last_request.contains_key(&Resource::Peaks) {
            "волна загружается…"
        } else {
            "волны пока нет"
        };
        spans.push(Span::styled(message, Style::default().fg(t.dim)));
    }
    Line::from(spans)
}
