//! Archive activity is a projection of server snapshots, never a scheduler.
use super::{App, Color, Line, Resource, Span, Style, Theme, View};
use crate::app::timecode;
use localvox_light_client::{safe_label, Session};

const FRAME_MS: u64 = 100;
const SNAKE: [&str; 10] = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];
const ASCII: [&str; 4] = ["|", "/", "-", "\\"];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum State {
    Running,
    Queued,
    Waiting,
    Cancelling,
    Cancelled,
    Failed,
    Retry,
    Blocked,
    Recording,
    Ready,
    Idle,
    Unknown,
    Unconfirmed,
}
struct Activity<'a> {
    state: State,
    phase: Option<&'a str>,
    position: Option<usize>,
}
fn describe<'a>(a: &'a App, session: &Session) -> Activity<'a> {
    let queue = a.queue.iter().find(|job| {
        job.session == session.name && Some(job.state.as_str()) == session.job.as_deref()
    });
    let state =
        if a.errors.contains_key(&Resource::Sessions) || a.errors.contains_key(&Resource::Queue) {
            State::Unconfirmed
        } else {
            match session.job.as_deref() {
                Some("running") => State::Running,
                Some("pending") => State::Queued,
                Some("waiting_input") => State::Waiting,
                Some("cancelling") => State::Cancelling,
                Some("cancelled") => State::Cancelled,
                Some("failed" | "invalid") => {
                    match queue
                        .and_then(|job| job.retry.as_ref())
                        .map(|r| r.state.as_str())
                    {
                        Some("retry_wait") => State::Retry,
                        Some("blocked" | "stalled") => State::Blocked,
                        _ => State::Failed,
                    }
                }
                None | Some("done" | "ready") if session.recording => State::Recording,
                Some("done" | "ready") => State::Ready,
                None if session.cooked => State::Ready,
                None => State::Idle,
                _ => State::Unknown,
            }
        };
    Activity {
        state,
        phase: queue.and_then(|job| job.phase.as_deref()),
        position: queue.and_then(|job| job.position),
    }
}
impl Activity<'_> {
    fn symbol(&self, ascii: bool, now_ms: u64) -> &'static str {
        match self.state {
            State::Running => {
                let frames: &[&str] = if ascii { &ASCII } else { &SNAKE };
                frames[((now_ms / FRAME_MS) % frames.len() as u64) as usize]
            }
            state if ascii => match state {
                State::Queued => ">",
                State::Waiting => "~",
                State::Cancelling => "-",
                State::Cancelled => "x",
                State::Failed => "!",
                State::Retry => "r",
                State::Recording => "*",
                State::Ready => "+",
                State::Idle => ".",
                _ => "?",
            },
            State::Queued => "◷",
            State::Waiting => "◦",
            State::Cancelling => "◌",
            State::Cancelled => "■",
            State::Failed => "!",
            State::Retry => "↻",
            State::Recording => "●",
            State::Ready => "✓",
            State::Idle => "·",
            _ => "?",
        }
    }
    fn color(&self, t: Theme) -> Color {
        match self.state {
            State::Running => t.command,
            State::Queued | State::Retry | State::Cancelling => t.busy,
            State::Failed | State::Recording => t.error,
            State::Ready => t.ok,
            State::Waiting => t.argument,
            _ => t.dim,
        }
    }
    fn label(&self) -> String {
        match self.state {
            State::Running => self.phase.map(phase).unwrap_or_else(|| "обработка".into()),
            State::Queued => self
                .position
                .map_or_else(|| "в очереди".into(), |n| format!("в очереди #{n}")),
            State::Waiting => "ждём кусок".into(),
            State::Cancelling => "остановка".into(),
            State::Cancelled => "отменено".into(),
            State::Failed => "ошибка".into(),
            State::Retry => "ждём повтор".into(),
            State::Blocked => "повтор не подтверждён".into(),
            State::Recording => "запись".into(),
            State::Ready => "готово".into(),
            State::Idle => "нет расшифровки".into(),
            State::Unknown => "статус неизвестен".into(),
            State::Unconfirmed => "нет обновления".into(),
        }
    }
}
fn phase(value: &str) -> String {
    match value {
        "prepare" => "подготовка".into(),
        "transcribe" => "STT".into(),
        "text" => "текст".into(),
        "summary" => "сводка".into(),
        "index" => "индекс".into(),
        other => safe_label(other),
    }
}

pub(super) fn lines(a: &App, session: &Session, t: Theme) -> Vec<Line<'static>> {
    let activity = describe(a, session);
    let color = activity.color(t);
    let title = Line::from(vec![
        Span::styled(
            activity.symbol(a.preferences.ascii, a.now_ms),
            Style::default().fg(color),
        ),
        Span::raw(" "),
        Span::styled(safe_label(session.title()), Style::default().fg(t.ink)),
    ]);
    let mut detail = vec![Span::raw("  ")];
    if matches!(activity.state, State::Ready | State::Idle) {
        let date = session
            .started_at
            .as_deref()
            .and_then(|date| chrono::DateTime::parse_from_rfc3339(date).ok())
            .map(|date| date.format("%d.%m.%Y").to_string())
            .unwrap_or_else(|| "дата неизвестна".into());
        detail.push(Span::styled(date, Style::default().fg(t.dim)));
    } else {
        detail.push(Span::styled(activity.label(), Style::default().fg(color)));
        if activity.state == State::Queued {
            if let Some(value) = activity.phase {
                detail.push(Span::styled(" · ", Style::default().fg(t.dim)));
                detail.push(Span::styled(phase(value), Style::default().fg(t.argument)));
            }
        }
        if session.recording && !matches!(activity.state, State::Recording | State::Unconfirmed) {
            detail.push(Span::styled(" · запись", Style::default().fg(t.error)));
        }
        if activity.state == State::Unconfirmed {
            let date = session
                .started_at
                .as_deref()
                .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
                .map(|value| value.format("%d.%m.%Y").to_string());
            if let Some(date) = date {
                detail.push(Span::styled(
                    format!(" · {date}"),
                    Style::default().fg(t.dim),
                ));
            }
        }
    }
    detail.push(Span::styled(
        format!(" · {}", timecode(session.duration_sec)),
        Style::default().fg(t.dim),
    ));
    if activity.state == State::Idle {
        detail.push(Span::styled(" · нет STT", Style::default().fg(t.dim)));
    }
    vec![title, Line::from(detail)]
}

pub(crate) fn animation_frame(a: &App) -> Option<u64> {
    let (width, height) = a.viewport;
    if a.view != View::Archive
        || width < 35
        || height < 10
        || (width < 95 && a.focus != 1)
        || a.help
        || a.palette
        || a.form.is_some()
        || a.console.open
        || a.errors.contains_key(&Resource::Sessions)
        || a.errors.contains_key(&Resource::Queue)
    {
        return None;
    }
    a.visible_sessions()
        .iter()
        .any(|session| session.job.as_deref() == Some("running"))
        .then_some(a.now_ms / FRAME_MS)
}

#[cfg(test)]
mod tests;
