use super::*;

impl App {
    pub(super) fn resize_list(&mut self, delta: i16) {
        let Some((min, max)) = crate::render::list_width_bounds(self.viewport.0) else {
            self.status = "Ширина списка меняется в окне от 95 колонок".into();
            return;
        };
        let old = crate::render::effective_list_width(self.viewport.0, self.list_width);
        let next = old.saturating_add_signed(delta).clamp(min, max);
        if next != old {
            self.set_list_width(next);
        }
        self.status = format!("Ширина списка: {next} · Ctrl+←/→ · стандартная — в F2");
    }

    pub(super) fn reset_list_width(&mut self) {
        self.set_list_width(crate::render::DEFAULT_LIST_WIDTH);
        self.status = "Ширина списка: 27 · стандартная".into();
    }

    fn set_list_width(&mut self, width: u16) {
        let anchor = crate::render::journal_anchor(self);
        self.list_width = width;
        crate::render::restore_journal_anchor(self, anchor);
    }
}

#[cfg(test)]
mod tests;
