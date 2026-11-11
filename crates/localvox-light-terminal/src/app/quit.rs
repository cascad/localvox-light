//! A short, cancellable keyboard sequence on the application's monotonic clock.
use super::*;
use crossterm::event::KeyEventKind;

const CONFIRM_MS: u64 = 500;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Context {
    view: View,
    tab: Tab,
    focus: usize,
    nav: usize,
    epoch: u64,
    help: bool,
    palette: bool,
    console: bool,
    input: bool,
    viewport: (u16, u16),
}
#[derive(Clone, Copy)]
pub struct QuitSequence {
    pressed_at: u64,
    context: Context,
}
impl App {
    fn quit_context(&self) -> Context {
        Context {
            view: self.view,
            tab: self.tab,
            focus: self.focus,
            nav: self.nav,
            epoch: self.epoch,
            help: self.help,
            palette: self.palette,
            console: self.console.open,
            input: self.form.is_some() || self.archive_filter_input(),
            viewport: self.viewport,
        }
    }
    pub fn quit_pending(&self) -> bool {
        self.quit_sequence.is_some_and(|sequence| {
            sequence.context == self.quit_context()
                && self
                    .now_ms
                    .checked_sub(sequence.pressed_at)
                    .is_some_and(|elapsed| elapsed <= CONFIRM_MS)
        })
    }
    pub fn cancel_quit_confirmation(&mut self) {
        self.quit_sequence = None;
    }
    pub(super) fn quit_key(&mut self, key: KeyEvent) -> bool {
        let q = matches!(key.code, KeyCode::Char('q' | 'Q' | 'й' | 'Й'));
        if (q && key.modifiers.contains(KeyModifiers::CONTROL))
            || key.code == KeyCode::Char('\u{11}')
            || (key.code == KeyCode::F(10) && key.modifiers.is_empty())
        {
            self.cancel_quit_confirmation();
            self.quit = true;
            return true;
        }
        if q && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
            && self.form.is_none()
            && !self.archive_filter_input()
        {
            if key.kind == KeyEventKind::Press {
                if self.quit_pending() {
                    self.cancel_quit_confirmation();
                    self.quit = true;
                } else {
                    self.quit_sequence = Some(QuitSequence {
                        pressed_at: self.now_ms,
                        context: self.quit_context(),
                    });
                }
            }
            return true;
        }
        self.cancel_quit_confirmation();
        false
    }
}

#[cfg(test)]
mod tests;
