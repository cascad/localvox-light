//! One owner of terminal input. Emulator names/configuration do not select a reader.
#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub(super) use windows::Input;

#[cfg(not(windows))]
pub(super) struct Input;
#[cfg(not(windows))]
impl Input {
    pub fn new() -> anyhow::Result<Self> {
        Ok(Self)
    }
    pub fn read(
        &mut self,
        wait: std::time::Duration,
    ) -> anyhow::Result<Option<crossterm::event::Event>> {
        Ok(if crossterm::event::poll(wait)? {
            Some(crossterm::event::read()?)
        } else {
            None
        })
    }
}
