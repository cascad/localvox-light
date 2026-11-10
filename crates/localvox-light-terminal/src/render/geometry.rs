//! The renderer and keyboard navigation measure the same panes and blocks.
use super::*;

pub(super) fn workspace_rows(area: Rect) -> [Rect; 4] {
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Min(1),
        Constraint::Length(2),
    ])
    .split(area);
    [rows[0], rows[1], rows[2], rows[3]]
}
pub(super) fn workspace_columns(area: Rect) -> Vec<Rect> {
    Layout::horizontal(if area.width >= 125 {
        vec![
            Constraint::Length(27),
            Constraint::Min(40),
            Constraint::Length(28),
        ]
    } else {
        vec![Constraint::Length(27), Constraint::Min(40)]
    })
    .split(area)
    .to_vec()
}
pub(super) fn left_rows(area: Rect) -> [Rect; 2] {
    let rows = Layout::vertical([Constraint::Length(9), Constraint::Min(2)]).split(area);
    [rows[0], rows[1]]
}
pub(super) fn document_rows(inner: Rect) -> [Rect; 2] {
    let rows = Layout::vertical([Constraint::Length(2), Constraint::Min(1)]).split(inner);
    [rows[0], rows[1]]
}
fn panes(a: &App) -> (Rect, Rect) {
    let (width, height) = a.viewport;
    let body = workspace_rows(Rect::new(0, 0, width, height))[2];
    if width < 95 {
        (body, body)
    } else {
        let cols = workspace_columns(body);
        (left_rows(cols[0])[1], cols[1])
    }
}
pub(crate) fn document_dimensions(a: &App) -> (u16, u16) {
    let area = panes(a).1;
    let inner = panel("", false, Theme::for_app(a)).inner(area);
    let body = document_rows(inner)[1];
    (body.width, body.height)
}
/// Number of whole entries on a page, then the clamped visible entry offset.
pub(crate) fn list_metrics(a: &App) -> (usize, usize) {
    let area = panes(a).0;
    let height = list_block(a, Theme::for_app(a), "").inner(area).height as usize;
    let page = (height / if a.view == View::Archive { 2 } else { 1 }).max(1);
    let len = a.list_len();
    let selected = a.selected.min(len.saturating_sub(1));
    let max = len.saturating_sub(page);
    let mut offset = a.list_scroll.min(max);
    if selected < offset {
        offset = selected;
    } else if selected >= offset + page {
        offset = selected + 1 - page;
    }
    (page, offset.min(max))
}
