//! One explicit parent list for records opened from search or queue; no history stack.
use super::*;

pub enum DetailParent {
    Queue { session: String, offset: usize },
    Search { hit: Hit, offset: usize },
}

impl App {
    pub(super) fn open_list_record(&mut self, session: String, tab: Tab, time: Option<f64>) {
        let parent = match self.view {
            View::Queue => Some(DetailParent::Queue {
                session: session.clone(),
                offset: self.list_scroll,
            }),
            View::Search => self
                .hits
                .get(self.selected)
                .cloned()
                .map(|hit| DetailParent::Search {
                    hit,
                    offset: self.list_scroll,
                }),
            _ => None,
        };
        if self.view == View::Search {
            self.pending_forms.remove(&Resource::Search);
            self.effects
                .retain(|(resource, _)| *resource != Resource::Search);
        }
        self.tab = tab;
        self.select_session(session);
        self.epoch += 1;
        self.document_target = time;
        self.detail_parent = parent;
        self.focus = 2;
    }

    pub(super) fn back(&mut self) {
        match self.focus {
            3 => self.focus = 2,
            2 => {
                if let Some(parent) = self
                    .detail_parent
                    .take()
                    .filter(|_| self.view == View::Archive)
                {
                    let (view, selected, offset) = match parent {
                        DetailParent::Queue { session, offset } => (
                            View::Queue,
                            self.queue
                                .iter()
                                .position(|item| item.session == session)
                                .unwrap_or(0),
                            offset,
                        ),
                        DetailParent::Search { hit, offset } => (
                            View::Search,
                            self.hits
                                .iter()
                                .position(|item| {
                                    item.session == hit.session
                                        && item.kind == hit.kind
                                        && item.start_sec == hit.start_sec
                                        && item.end_sec == hit.end_sec
                                        && item.snippet == hit.snippet
                                })
                                .unwrap_or(0),
                            offset,
                        ),
                    };
                    self.view = view;
                    self.nav = VIEWS.iter().position(|(v, _)| *v == view).unwrap_or(0);
                    self.selected = selected;
                    self.list_scroll = offset;
                    self.item = 0;
                    self.scroll = 0;
                    self.epoch += 1;
                    self.clear_document_errors();
                    self.focus = 1;
                    self.list_scroll = crate::render::list_metrics(self).1;
                    self.refresh_view();
                } else {
                    self.focus = 1;
                }
            }
            1 => self.focus = 0,
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
