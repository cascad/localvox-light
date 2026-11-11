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
pub(crate) const DEFAULT_LIST_WIDTH: u16 = 27;

pub(crate) fn list_width_bounds(width: u16) -> Option<(u16, u16)> {
    (width >= 95).then(|| (22, width - 40 - if width >= 125 { 28 } else { 0 }))
}
pub(crate) fn effective_list_width(width: u16, requested: u16) -> u16 {
    list_width_bounds(width)
        .map(|(min, max)| requested.clamp(min, max))
        .unwrap_or(width)
}

pub(super) fn workspace_columns(area: Rect, requested: u16) -> Vec<Rect> {
    let left = effective_list_width(area.width, requested);
    Layout::horizontal(if area.width >= 125 {
        vec![
            Constraint::Length(left),
            Constraint::Min(40),
            Constraint::Length(28),
        ]
    } else {
        vec![Constraint::Length(left), Constraint::Min(40)]
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
        let cols = workspace_columns(body, a.list_width);
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn requested_width_fits_all_panes_and_survives_temporary_window_constraints() {
        for width in [95, 120, 124, 125, 160, 240] {
            for requested in [0, 22, 27, 31, 100, u16::MAX] {
                let cols = workspace_columns(Rect::new(0, 0, width, 30), requested);
                assert_eq!(cols.iter().map(|r| r.width).sum::<u16>(), width);
                assert!(cols[0].width >= 22 && cols[1].width >= 40);
                if width >= 125 {
                    assert_eq!(cols[2].width, 28);
                }
            }
            assert_eq!(
                workspace_columns(Rect::new(0, 0, width, 30), DEFAULT_LIST_WIDTH)[0].width,
                27
            );
        }
        let mut a = App {
            list_width: 100,
            viewport: (160, 36),
            ..Default::default()
        };
        for (width, visible) in [(160, 92), (120, 80), (95, 55), (80, 80), (240, 100)] {
            a.viewport.0 = width;
            assert_eq!(effective_list_width(width, a.list_width), visible);
            let _ = document_dimensions(&a);
            assert_eq!(a.list_width, 100);
        }
    }
}
