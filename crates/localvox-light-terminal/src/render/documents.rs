//! Pure document projection with a visible, selectable semantic block.
use super::*;

pub(super) struct Projection {
    pub lines: Vec<Line<'static>>,
    pub selected: Option<(usize, usize)>,
}
fn label(value: &str) -> String {
    safe_text(value).replace(['\n', '\t'], " ")
}
fn block(
    projection: &mut Projection,
    header: String,
    text: &str,
    original: Option<&str>,
    selected: bool,
    active: bool,
    t: Theme,
) {
    let start = projection.lines.len();
    let style = if selected {
        selection_style(t)
    } else {
        Style::default()
    };
    if !header.is_empty() {
        projection.lines.push(Line::from(Span::styled(
            label(&header),
            style.fg(if active { t.command } else { t.dim }),
        )));
    }
    for line in safe_text(text).replace('\t', "    ").split('\n') {
        projection.lines.push(Line::from(Span::styled(
            line.to_owned(),
            style.fg(if line.starts_with('#') {
                t.command
            } else {
                t.ink
            }),
        )));
    }
    if let Some(original) = original {
        for line in safe_text(original).replace('\t', "    ").split('\n') {
            projection.lines.push(Line::from(Span::styled(
                format!("  STT: {line}"),
                style.fg(t.dim),
            )));
        }
    }
    if selected {
        projection.selected = Some((start, projection.lines.len()));
    }
    projection.lines.push(Line::from(""));
}
pub(super) fn project(a: &App, t: Theme, metadata: bool) -> Projection {
    let mut projection = Projection {
        lines: vec![],
        selected: None,
    };
    let focused = a.focus == 2;
    match a.view {
        View::Archive => match a.tab {
            Tab::Readable if a.readable.from_transcript => {
                projection.lines.push(Line::from(Span::styled(
                    "Исходная расшифровка · y копировать целиком",
                    Style::default().fg(t.dim),
                )));
                projection.lines.push(Line::from(""));
                block(
                    &mut projection,
                    String::new(),
                    &a.copy_text(),
                    None,
                    false,
                    false,
                    t,
                );
            }
            Tab::Transcript | Tab::Readable => {
                let lines = if a.tab == Tab::Transcript {
                    &a.transcript
                } else {
                    &a.readable.lines
                };
                for (index, line) in lines.iter().enumerate() {
                    let active = a.playback.session == a.current
                        && !a.playback.idle
                        && a.playback.position >= line.start_sec
                        && a.playback.position < line.end_sec;
                    block(
                        &mut projection,
                        format!(
                            "{} {}–{} · {}{}",
                            if a.ascii { ">" } else { "└" },
                            timecode(line.start_sec),
                            timecode(line.end_sec),
                            line.who,
                            line.source_id
                                .map(|id| format!(" · источник {id}"))
                                .unwrap_or_default()
                        ),
                        &line.text,
                        if a.expanded {
                            line.original.as_deref()
                        } else {
                            None
                        },
                        focused && index == a.item,
                        active,
                        t,
                    );
                }
            }
            Tab::Summary => {
                for (index, value) in a.summary.blocks.iter().enumerate() {
                    let text = if value.kind == "heading" {
                        format!("# {}", value.text)
                    } else if value.kind == "bullet" {
                        format!("{} {}", if a.ascii { "+" } else { "└" }, value.text)
                    } else {
                        value.text.clone()
                    };
                    let sources = if value.sources.is_empty() {
                        String::new()
                    } else {
                        format!(
                            "{}–{} · p слушать источник",
                            timecode(value.sources[0].start_sec),
                            timecode(value.sources.last().unwrap().end_sec)
                        )
                    };
                    block(
                        &mut projection,
                        sources,
                        &text,
                        None,
                        focused && index == a.item,
                        false,
                        t,
                    );
                }
            }
            Tab::Speakers => {
                for (index, p) in a.speakers.iter().enumerate() {
                    block(
                        &mut projection,
                        p.name.clone(),
                        &format!(
                            "{} · {} реплик · {}\nГолоса: {}\nИсточники: {}",
                            p.what,
                            p.lines,
                            timecode(p.speech_sec),
                            p.voices.join(", "),
                            p.sources
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join(", ")
                        ),
                        None,
                        focused && index == a.item,
                        false,
                        t,
                    );
                }
            }
            Tab::Progress => {}
        },
        View::Notes => {
            if let Some(slot) = a.notes.get(a.selected) {
                if let Some(error) = &slot.error {
                    projection.lines.push(Line::from(Span::styled(
                        format!("Не прочитать слот: {}", label(error)),
                        Style::default().fg(t.error),
                    )));
                } else if !slot.readable {
                    projection.lines.push(Line::from(
                        "Этот слот принимает заметки, но не предоставляет чтение.",
                    ));
                } else if slot.notes.is_empty() {
                    projection.lines.push(Line::from("Здесь пока нет заметок."));
                }
                for (index, note) in slot.notes.iter().enumerate() {
                    block(
                        &mut projection,
                        format!(
                            "{} · {}",
                            note.date.as_deref().unwrap_or("дата неизвестна"),
                            note.source
                        ),
                        &note.text,
                        None,
                        focused && index == a.item,
                        false,
                        t,
                    );
                }
            }
        }
        _ => {
            for (index, line) in a.document_text().replace('\t', "    ").lines().enumerate() {
                let failed = index == 0
                    && a.view == View::Ask
                    && a.ask.as_ref().is_some_and(|a| a.status == "failed");
                projection.lines.push(Line::from(Span::styled(
                    if failed {
                        format!("! {line}")
                    } else {
                        line.to_owned()
                    },
                    Style::default().fg(if failed {
                        t.error
                    } else if line.starts_with('#') {
                        t.command
                    } else {
                        t.ink
                    }),
                )));
            }
        }
    }
    if metadata
        && !projection.lines.is_empty()
        && !(a.view == View::Archive && a.tab == Tab::Readable && a.readable.from_transcript)
    {
        let text = a.metadata_text();
        if !text.is_empty() {
            projection.lines.push(Line::from(Span::styled(
                "Сведения о документе",
                Style::default().fg(t.dim),
            )));
            for line in text.replace('\t', "    ").lines() {
                projection.lines.push(Line::from(Span::styled(
                    line.to_owned(),
                    Style::default().fg(t.dim),
                )));
            }
        }
    }
    projection
}
pub(super) fn scroll(
    projection: &Projection,
    width: u16,
    height: u16,
    requested: u16,
    follow_selection: bool,
) -> u16 {
    let count = Paragraph::new(projection.lines.clone())
        .wrap(Wrap { trim: false })
        .line_count(width);
    let max = count.saturating_sub(height as usize);
    let mut offset = (requested as usize).min(max);
    if follow_selection {
        if let Some((from, to)) = projection.selected {
            let start = Paragraph::new(projection.lines[..from].to_vec())
                .wrap(Wrap { trim: false })
                .line_count(width);
            let end = Paragraph::new(projection.lines[..to].to_vec())
                .wrap(Wrap { trim: false })
                .line_count(width);
            if start < offset {
                offset = start;
            } else if end > offset + height as usize {
                offset = if end - start > height as usize {
                    start
                } else {
                    end.saturating_sub(height as usize)
                };
            }
        }
    }
    offset.min(max).min(u16::MAX as usize) as u16
}
