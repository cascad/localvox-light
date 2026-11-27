use super::*;

/// Keep complete shortcuts visible on narrow terminals.
pub(super) fn hints<'a>(width: u16, t: Theme, groups: &[&'a str]) -> Line<'a> {
    let mut spans = Vec::new();
    let mut used = 0;
    for &group in groups.iter().filter(|g| !g.is_empty()) {
        let gap = if spans.is_empty() { " " } else { "   " };
        let size = gap.len() + Span::raw(group).width();
        if used + size > usize::from(width) {
            continue;
        }
        used += size;
        let (key, label) = group.split_once(' ').unwrap_or((group, ""));
        spans.push(Span::raw(gap));
        spans.push(Span::styled(key, Style::default().fg(t.command)));
        if !label.is_empty() {
            spans.push(Span::styled(
                format!(" {label}"),
                Style::default().fg(t.dim),
            ));
        }
    }
    Line::from(spans)
}

pub(super) fn draw(f: &mut Frame, a: &App, area: Rect, t: Theme, record_key: &str) {
    let list = a.view == View::Archive && a.focus == 1;
    let editing = list && a.archive_filter_editing;
    let context = match a.focus {
        0 => hints(area.width, t, &["j/k ↑↓ выбор", "Enter/l раздел"]),
        1 if editing => hints(area.width, t, &["↑↓ выбор", "Enter открыть", "Esc сброс"]),
        1 if list => hints(
            area.width,
            t,
            &[
                "j/k ↑↓ выбор",
                "/ поиск",
                "Enter/l открыть",
                "Esc меню",
                "i YouTube / URL",
            ],
        ),
        1 => hints(
            area.width,
            t,
            &["j/k ↑↓ выбор", "Enter/l открыть", "Esc меню"],
        ),
        3 => hints(area.width, t, &["Esc документ"]),
        _ => {
            let copy = match (a.view, a.tab) {
                (View::Archive, Tab::Summary) => "y блок",
                (View::Archive, Tab::Transcript) => "y реплика",
                _ => "y копия",
            };
            hints(
                area.width,
                t,
                &[
                    copy,
                    "Ctrl+Y всё",
                    "Esc список",
                    if a.view == View::Archive {
                        "i YouTube / URL"
                    } else {
                        ""
                    },
                ],
            )
        }
    };
    let globals = hints(
        area.width,
        t,
        &[
            record_key,
            "F10 выход",
            if editing {
                "F2 действия"
            } else {
                "F2/: действия"
            },
            "Tab фокус",
            if a.view == View::Archive {
                "1–5 вкладки"
            } else {
                ""
            },
        ],
    );
    let mut status = safe_label(&a.status);
    if let Some(v) = &a.record.voice {
        let voice = if let Some(c) = &v.capturing {
            format!("диктую → {}: {}", c.slot, c.text)
        } else if let Some(last) = &v.last {
            if last.error.is_some() {
                format!("сбой заметки → {} · ~ консоль", last.slot)
            } else {
                format!("сохранено → {}: {}", last.slot, last.text)
            }
        } else {
            format!("голос: {}", v.detail)
        };
        if !status.is_empty() {
            status.push_str("   ");
        }
        status.push_str(&safe_label(&voice).replace(['\n', '\t'], " "));
    }
    f.render_widget(
        Paragraph::new(vec![
            Line::styled(format!(" {status}"), Style::default().fg(t.dim)),
            context,
            globals,
        ]),
        area,
    );
}
