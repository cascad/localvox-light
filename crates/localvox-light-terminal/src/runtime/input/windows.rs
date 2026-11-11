//! Windows console UTF-16 -> standard VT -> application events.
//! ReadConsoleInputW preserves resize and Unicode without changing the codepage.
use anyhow::{bail, Result};
use crossterm::event::{Event, KeyCode, KeyEvent, KeyModifiers};
use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};
use termwiz::input::{InputEvent, InputParser, KeyCode as VtKey, Modifiers};
use windows::Win32::{
    Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT},
    System::{Console::*, Threading::WaitForSingleObject},
};

pub(crate) struct Input {
    handle: HANDLE,
    original: CONSOLE_MODE,
    decoder: Decoder,
}
impl Input {
    pub fn new() -> Result<Self> {
        unsafe {
            let handle = GetStdHandle(STD_INPUT_HANDLE)?;
            let mut original = CONSOLE_MODE::default();
            GetConsoleMode(handle, &mut original)?;
            SetConsoleMode(
                handle,
                original | ENABLE_VIRTUAL_TERMINAL_INPUT | ENABLE_WINDOW_INPUT,
            )?;
            Ok(Self {
                handle,
                original,
                decoder: Decoder::default(),
            })
        }
    }
    pub fn read(&mut self, wait: Duration) -> Result<Option<Event>> {
        let deadline = Instant::now() + wait;
        loop {
            if let Some(event) = self.decoder.events.pop_front() {
                return Ok(Some(event));
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            match unsafe {
                WaitForSingleObject(
                    self.handle,
                    remaining.as_millis().min(u32::MAX as u128) as u32,
                )
            } {
                WAIT_OBJECT_0 => {
                    let mut records = [INPUT_RECORD::default(); 256];
                    let mut count = 0;
                    unsafe {
                        ReadConsoleInputW(self.handle, &mut records, &mut count)?;
                    }
                    self.decoder.records(&records[..count as usize]);
                }
                WAIT_TIMEOUT => {
                    self.decoder.idle();
                    return Ok(self.decoder.events.pop_front());
                }
                _ => bail!(
                    "Не удалось прочитать терминал: {}",
                    std::io::Error::last_os_error()
                ),
            }
            if Instant::now() >= deadline {
                return Ok(self.decoder.events.pop_front());
            }
        }
    }
}
impl Drop for Input {
    fn drop(&mut self) {
        // Captured after the outer raw-mode guard; that guard restores the original next.
        let _ = unsafe { SetConsoleMode(self.handle, self.original) };
    }
}

#[derive(Default)]
struct Decoder {
    parser: InputParser,
    high_surrogate: Option<u16>,
    escape_at_end: bool,
    events: VecDeque<Event>,
}
impl Decoder {
    fn text(&mut self, scalar: char) {
        let mut bytes = [0; 4];
        self.escape_at_end = scalar == '\x1b';
        self.parser.parse(
            scalar.encode_utf8(&mut bytes).as_bytes(),
            |event| {
                if let Some(event) = convert(event) {
                    self.events.push_back(event);
                }
            },
            true,
        );
    }
    fn unit(&mut self, unit: u16) {
        if let Some(high) = self.high_surrogate.take() {
            if (0xdc00..=0xdfff).contains(&unit) {
                self.text(
                    char::from_u32(0x10000 + ((high as u32 - 0xd800) << 10) + unit as u32 - 0xdc00)
                        .unwrap(),
                );
                return;
            }
            self.text(char::REPLACEMENT_CHARACTER);
        }
        if (0xd800..=0xdbff).contains(&unit) {
            self.high_surrogate = Some(unit);
        } else {
            self.text(char::from_u32(unit as u32).unwrap_or(char::REPLACEMENT_CHARACTER));
        }
    }
    fn idle(&mut self) {
        // Only the standard lone-Esc ambiguity is flushed. A fragmented CSI/paste
        // prefix and an open paste remain buffered regardless of how long they take.
        if self.escape_at_end {
            self.escape_at_end = false;
            self.parser.parse(
                &[],
                |event| {
                    if let Some(event) = convert(event) {
                        self.events.push_back(event);
                    }
                },
                false,
            );
        }
    }
    fn records(&mut self, records: &[INPUT_RECORD]) {
        for record in records {
            match record.EventType as u32 {
                KEY_EVENT => {
                    let key = unsafe { record.Event.KeyEvent };
                    if !key.bKeyDown.as_bool() {
                        continue;
                    }
                    let unit = unsafe { key.uChar.UnicodeChar };
                    let modifiers = native_modifiers(key.dwControlKeyState);
                    // Native console hosts can retain Ctrl+Enter/Shift+Tab as
                    // structured keys rather than VT sequences. Preserve both forms.
                    let native_control = if key.wVirtualKeyCode == 13
                        && modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                    {
                        Some(KeyCode::Enter)
                    } else if key.wVirtualKeyCode == 9 && modifiers.contains(KeyModifiers::SHIFT) {
                        Some(KeyCode::BackTab)
                    } else {
                        None
                    };
                    if let Some(code) = native_control {
                        self.idle();
                        for _ in 0..key.wRepeatCount.max(1) {
                            self.events
                                .push_back(Event::Key(KeyEvent::new(code, modifiers)));
                        }
                        continue;
                    }
                    if unit != 0 {
                        for _ in 0..key.wRepeatCount.max(1) {
                            self.unit(unit);
                        }
                    } else if let Some(code) = native_key(key.wVirtualKeyCode) {
                        self.idle();
                        for _ in 0..key.wRepeatCount.max(1) {
                            self.events
                                .push_back(Event::Key(KeyEvent::new(code, modifiers)));
                        }
                    }
                }
                WINDOW_BUFFER_SIZE_EVENT => {
                    let size = unsafe { record.Event.WindowBufferSizeEvent }.dwSize;
                    self.events
                        .push_back(Event::Resize(size.X.max(0) as u16, size.Y.max(0) as u16));
                }
                _ => {}
            }
        }
    }
}
fn native_modifiers(state: u32) -> KeyModifiers {
    let mut modifiers = KeyModifiers::empty();
    if state & SHIFT_PRESSED != 0 {
        modifiers |= KeyModifiers::SHIFT;
    }
    if state & (LEFT_ALT_PRESSED | RIGHT_ALT_PRESSED) != 0 {
        modifiers |= KeyModifiers::ALT;
    }
    if state & (LEFT_CTRL_PRESSED | RIGHT_CTRL_PRESSED) != 0 {
        modifiers |= KeyModifiers::CONTROL;
    }
    modifiers
}
fn native_key(vk: u16) -> Option<KeyCode> {
    Some(match vk {
        8 => KeyCode::Backspace,
        9 => KeyCode::Tab,
        13 => KeyCode::Enter,
        27 => KeyCode::Esc,
        33 => KeyCode::PageUp,
        34 => KeyCode::PageDown,
        35 => KeyCode::End,
        36 => KeyCode::Home,
        37 => KeyCode::Left,
        38 => KeyCode::Up,
        39 => KeyCode::Right,
        40 => KeyCode::Down,
        45 => KeyCode::Insert,
        46 => KeyCode::Delete,
        112..=135 => KeyCode::F((vk - 111) as u8),
        _ => return None,
    })
}
fn convert(event: InputEvent) -> Option<Event> {
    Some(match event {
        InputEvent::Paste(text) => Event::Paste(text),
        InputEvent::Key(key) => {
            let mut modifiers = KeyModifiers::empty();
            if key.modifiers.contains(Modifiers::SHIFT) {
                modifiers |= KeyModifiers::SHIFT;
            }
            if key.modifiers.contains(Modifiers::ALT) {
                modifiers |= KeyModifiers::ALT;
            }
            if key.modifiers.contains(Modifiers::CTRL) {
                modifiers |= KeyModifiers::CONTROL;
            }
            if key.modifiers.contains(Modifiers::SUPER) {
                modifiers |= KeyModifiers::SUPER;
            }
            let code = match key.key {
                VtKey::Char('\r' | '\n') => KeyCode::Enter,
                VtKey::Char('\t') if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
                VtKey::Char('\t') => KeyCode::Tab,
                VtKey::Char('\x1b') => KeyCode::Esc,
                VtKey::Char('\x08' | '\x7f') => KeyCode::Backspace,
                VtKey::Char(c) => KeyCode::Char(c),
                VtKey::Enter => KeyCode::Enter,
                VtKey::Tab if modifiers.contains(KeyModifiers::SHIFT) => KeyCode::BackTab,
                VtKey::Tab => KeyCode::Tab,
                VtKey::Escape => KeyCode::Esc,
                VtKey::Backspace => KeyCode::Backspace,
                VtKey::Delete => KeyCode::Delete,
                VtKey::Insert => KeyCode::Insert,
                VtKey::LeftArrow => KeyCode::Left,
                VtKey::RightArrow => KeyCode::Right,
                VtKey::UpArrow => KeyCode::Up,
                VtKey::DownArrow => KeyCode::Down,
                VtKey::Home => KeyCode::Home,
                VtKey::End => KeyCode::End,
                VtKey::PageUp => KeyCode::PageUp,
                VtKey::PageDown => KeyCode::PageDown,
                VtKey::Function(n) => KeyCode::F(n),
                _ => return None,
            };
            Event::Key(KeyEvent::new(code, modifiers))
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests;
