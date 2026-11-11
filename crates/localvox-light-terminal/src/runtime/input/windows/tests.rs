use super::*;

fn feed(decoder: &mut Decoder, text: &str) {
    for unit in text.encode_utf16() {
        decoder.unit(unit);
    }
}

#[test]
fn fragmented_paste_preserves_unicode_and_never_emits_commands_or_early_text() {
    let mut decoder = Decoder::default();
    // Initial Esc is inherently ambiguous with the key. Once CSI is begun,
    // arbitrary idle polls must not flush partial begin/end delimiters.
    feed(&mut decoder, "\x1b[");
    decoder.idle();
    let payload = "https://example.org/q?i=Русский🦀\r\nq\x11\t\x1b[21~";
    for unit in format!("200~{payload}\x1b[201").encode_utf16() {
        decoder.unit(unit);
        if unit != 0x1b {
            decoder.idle();
        }
        assert!(decoder.events.is_empty(), "partial paste must remain text");
    }
    feed(&mut decoder, "~");
    assert_eq!(
        decoder.events.pop_front(),
        Some(Event::Paste(payload.into()))
    );
    assert!(decoder.events.is_empty());
    feed(&mut decoder, "\x1b[21~");
    assert_eq!(
        decoder.events.pop_front(),
        Some(Event::Key(KeyEvent::new(
            KeyCode::F(10),
            KeyModifiers::NONE
        )))
    );
}

#[test]
fn vt_keys_and_modifiers_keep_the_existing_contract() {
    for (sequence, code, modifiers) in [
        ("\x1b[A", KeyCode::Up, KeyModifiers::NONE),
        ("\x1b[B", KeyCode::Down, KeyModifiers::NONE),
        ("\x1b[5~", KeyCode::PageUp, KeyModifiers::NONE),
        ("\x1b[6~", KeyCode::PageDown, KeyModifiers::NONE),
        ("\x1b[H", KeyCode::Home, KeyModifiers::NONE),
        ("\x1b[F", KeyCode::End, KeyModifiers::NONE),
        ("\x1b[17~", KeyCode::F(6), KeyModifiers::NONE),
        ("\x1b[19~", KeyCode::F(8), KeyModifiers::NONE),
        ("\x1b[20~", KeyCode::F(9), KeyModifiers::NONE),
        ("\x1b[21~", KeyCode::F(10), KeyModifiers::NONE),
        ("\x1b[1;3D", KeyCode::Left, KeyModifiers::ALT),
        ("\x1b[1;5C", KeyCode::Right, KeyModifiers::CONTROL),
        ("\x1b[Z", KeyCode::BackTab, KeyModifiers::SHIFT),
        ("\x11", KeyCode::Char('q'), KeyModifiers::CONTROL),
        ("\x1b[13;5u", KeyCode::Enter, KeyModifiers::CONTROL),
        ("\x1b[27;5;13~", KeyCode::Enter, KeyModifiers::CONTROL),
        ("\x1b[9;2u", KeyCode::BackTab, KeyModifiers::SHIFT),
        ("\r", KeyCode::Enter, KeyModifiers::NONE),
        ("\t", KeyCode::Tab, KeyModifiers::NONE),
        ("\x7f", KeyCode::Backspace, KeyModifiers::NONE),
        ("ж", KeyCode::Char('ж'), KeyModifiers::NONE),
        ("\x1b", KeyCode::Esc, KeyModifiers::NONE),
    ] {
        let mut decoder = Decoder::default();
        feed(&mut decoder, sequence);
        decoder.idle();
        assert_eq!(
            decoder.events.pop_front(),
            Some(Event::Key(KeyEvent::new(code, modifiers))),
            "{sequence:?}"
        );
        assert!(decoder.events.is_empty(), "{sequence:?}");
    }
}

#[test]
fn native_record_surrogates_repeat_modifiers_and_resize_survive_batch_boundaries() {
    fn key(unit: u16, down: bool, vk: u16, repeat: u16) -> INPUT_RECORD {
        INPUT_RECORD {
            EventType: KEY_EVENT as u16,
            Event: INPUT_RECORD_0 {
                KeyEvent: KEY_EVENT_RECORD {
                    bKeyDown: down.into(),
                    wRepeatCount: repeat,
                    wVirtualKeyCode: vk,
                    uChar: KEY_EVENT_RECORD_0 { UnicodeChar: unit },
                    dwControlKeyState: LEFT_ALT_PRESSED | SHIFT_PRESSED,
                    ..Default::default()
                },
            },
        }
    }
    let mut decoder = Decoder::default();
    decoder.records(&[key(0xd83e, true, 0, 1), key(0xd83e, false, 0, 1)]);
    assert!(decoder.events.is_empty());
    decoder.records(&[key(0xdd80, true, 0, 1), key(0xdd80, false, 0, 1)]);
    assert_eq!(
        decoder.events.pop_front(),
        Some(Event::Key(KeyEvent::new(
            KeyCode::Char('🦀'),
            KeyModifiers::NONE
        )))
    );
    decoder.records(&[key(0, true, 37, 2)]);
    for _ in 0..2 {
        assert_eq!(
            decoder.events.pop_front(),
            Some(Event::Key(KeyEvent::new(
                KeyCode::Left,
                KeyModifiers::ALT | KeyModifiers::SHIFT
            )))
        );
    }
    let mut enter = key(13, true, 13, 1);
    unsafe {
        enter.Event.KeyEvent.dwControlKeyState = LEFT_CTRL_PRESSED;
    }
    decoder.records(&[enter]);
    assert_eq!(
        decoder.events.pop_front(),
        Some(Event::Key(KeyEvent::new(
            KeyCode::Enter,
            KeyModifiers::CONTROL
        )))
    );
    let mut tab = key(9, true, 9, 1);
    unsafe {
        tab.Event.KeyEvent.dwControlKeyState = SHIFT_PRESSED;
    }
    decoder.records(&[tab]);
    assert_eq!(
        decoder.events.pop_front(),
        Some(Event::Key(KeyEvent::new(
            KeyCode::BackTab,
            KeyModifiers::SHIFT
        )))
    );
    decoder.records(&[INPUT_RECORD {
        EventType: WINDOW_BUFFER_SIZE_EVENT as u16,
        Event: INPUT_RECORD_0 {
            WindowBufferSizeEvent: WINDOW_BUFFER_SIZE_RECORD {
                dwSize: COORD { X: 120, Y: 36 },
            },
        },
    }]);
    assert_eq!(decoder.events.pop_front(), Some(Event::Resize(120, 36)));
}
