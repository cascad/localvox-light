use super::*;

impl App {
    pub(super) fn move_list(&mut self, down: bool, step: usize) {
        self.list_scroll = crate::render::list_metrics(self).1;
        self.selected = if down {
            self.selected
                .saturating_add(step)
                .min(self.list_len().saturating_sub(1))
        } else {
            self.selected.saturating_sub(step)
        };
        self.list_scroll = crate::render::list_metrics(self).1;
    }
    pub(super) fn page(&mut self, down: bool) {
        match self.focus {
            1 => {
                let (page, offset) = crate::render::list_metrics(self);
                self.selected = if down {
                    self.selected
                        .saturating_add(page)
                        .min(self.list_len().saturating_sub(1))
                } else {
                    self.selected.saturating_sub(page)
                };
                self.list_scroll = if down {
                    offset.saturating_add(page)
                } else {
                    offset.saturating_sub(page)
                }
                .min(self.list_len().saturating_sub(page));
                self.list_scroll = crate::render::list_metrics(self).1;
            }
            2 => {
                let (count, offset) = if self.is_journal() {
                    crate::render::journal_metrics(self)
                } else {
                    crate::render::document_metrics(self)
                };
                let page = crate::render::document_dimensions(self).1.max(1);
                self.scroll = if down {
                    offset.saturating_add(page)
                } else {
                    offset.saturating_sub(page)
                }
                .min(count.saturating_sub(page));
                self.follow_selection = false;
                if self.is_journal() {
                    self.journal.follow = false;
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests;
