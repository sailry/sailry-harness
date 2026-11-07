use super::*;

impl Vt {
    pub fn encode_input(&mut self, input: &Input) -> Result<Vec<u8>, Fault> {
        match input {
            Input::Focus { focused } => {
                if check(
                    self.terminal.mode(Mode::FOCUS_EVENT),
                    "read Ghostty focus reporting",
                )? {
                    Ok(if *focused {
                        b"\x1b[I".to_vec()
                    } else {
                        b"\x1b[O".to_vec()
                    })
                } else {
                    Ok(Vec::new())
                }
            }
            Input::Text { text } => Ok(text.as_bytes().to_vec()),
            Input::Key { event } => self.encode_key(event),
            Input::Paste { text } => self.encode_paste(text),
            Input::Mouse { event } => self.encode_mouse(event),
        }
    }

    fn encode_key(&mut self, event: &KeyEvent) -> Result<Vec<u8>, Fault> {
        self.key_encoder.set_options_from_terminal(&self.terminal);
        // Protocol modifiers are semantic Alt, including input from remote clients.
        // Ghostty otherwise resets macOS Option handling when loading VT options.
        self.key_encoder
            .set_macos_option_as_alt(libghostty_vt::key::OptionAsAlt::True);

        let (key, character) = ghostty_key(event.key)?;
        self.key_event
            .set_action(match event.action {
                Action::Press => GhosttyKeyAction::Press,
                Action::Repeat => GhosttyKeyAction::Repeat,
                Action::Release => GhosttyKeyAction::Release,
            })
            .set_key(key)
            .set_mods(ghostty_modifiers(event.modifiers))
            .set_utf8(event.utf8.clone().or_else(|| {
                (!matches!(event.action, Action::Release))
                    .then(|| character.map(|character| character.to_string()))
                    .flatten()
            }));
        let unshifted = if event.unshifted_codepoint.is_some() {
            event.unshifted_character().ok_or_else(|| {
                fail("validated terminal key has an invalid unshifted codepoint".to_owned())
            })?
        } else {
            '\0'
        };
        self.key_event.set_unshifted_codepoint(unshifted);

        let mut output = Vec::new();
        check(
            self.key_encoder.encode_to_vec(&self.key_event, &mut output),
            "encode Ghostty key event",
        )?;
        Ok(output)
    }

    fn encode_paste(&self, text: &str) -> Result<Vec<u8>, Fault> {
        let bracketed = check(
            self.terminal.mode(Mode::BRACKETED_PASTE),
            "read Ghostty bracketed paste mode",
        )?;
        let capacity = text
            .len()
            .checked_add(12)
            .ok_or_else(|| fail("Ghostty paste output size overflowed".to_owned()))?;
        let mut data = text.as_bytes().to_vec();
        let mut output = vec![0_u8; capacity];
        let written = match paste::encode(&mut data, bracketed, &mut output) {
            Ok(written) => written,
            Err(GhosttyError::OutOfSpace { required }) => {
                data.clear();
                data.extend_from_slice(text.as_bytes());
                output.resize(required, 0);
                check(
                    paste::encode(&mut data, bracketed, &mut output),
                    "encode Ghostty paste",
                )?
            }
            Err(error) => return Err(ghostty_error("encode Ghostty paste", error)),
        };
        output.truncate(written);
        Ok(output)
    }
}

fn ghostty_key(key: Key) -> Result<(GhosttyKey, Option<char>), Fault> {
    let mapped = match key {
        Key::Named { key } => (named_key(key), None),
        Key::Character { .. } => {
            let character = key.character().ok_or_else(|| {
                fail("validated terminal key has an invalid character".to_owned())
            })?;
            (ghostty_character_key(character), Some(character))
        }
        Key::Escape => (GhosttyKey::Escape, None),
        Key::Enter => (GhosttyKey::Enter, None),
        Key::Tab => (GhosttyKey::Tab, None),
        Key::Backspace => (GhosttyKey::Backspace, None),
        Key::Insert => (GhosttyKey::Insert, None),
        Key::Delete => (GhosttyKey::Delete, None),
        Key::Home => (GhosttyKey::Home, None),
        Key::End => (GhosttyKey::End, None),
        Key::PageUp => (GhosttyKey::PageUp, None),
        Key::PageDown => (GhosttyKey::PageDown, None),
        Key::ArrowUp => (GhosttyKey::ArrowUp, None),
        Key::ArrowDown => (GhosttyKey::ArrowDown, None),
        Key::ArrowLeft => (GhosttyKey::ArrowLeft, None),
        Key::ArrowRight => (GhosttyKey::ArrowRight, None),
        Key::Function { .. } => {
            let index = key.function_index().ok_or_else(|| {
                fail("validated terminal function key is outside the supported range".to_owned())
            })?;
            let key = GHOSTTY_FUNCTION_KEYS.get(index).copied().ok_or_else(|| {
                fail("validated terminal function key has no Ghostty mapping".to_owned())
            })?;
            (key, None)
        }
    };
    Ok(mapped)
}

fn ghostty_character_key(character: char) -> GhosttyKey {
    match character {
        '`' | '~' => GhosttyKey::Backquote,
        '\\' | '|' => GhosttyKey::Backslash,
        '[' | '{' => GhosttyKey::BracketLeft,
        ']' | '}' => GhosttyKey::BracketRight,
        ',' | '<' => GhosttyKey::Comma,
        '0' | ')' => GhosttyKey::Digit0,
        '1' | '!' => GhosttyKey::Digit1,
        '2' | '@' => GhosttyKey::Digit2,
        '3' | '#' => GhosttyKey::Digit3,
        '4' | '$' => GhosttyKey::Digit4,
        '5' | '%' => GhosttyKey::Digit5,
        '6' | '^' => GhosttyKey::Digit6,
        '7' | '&' => GhosttyKey::Digit7,
        '8' | '*' => GhosttyKey::Digit8,
        '9' | '(' => GhosttyKey::Digit9,
        '=' | '+' => GhosttyKey::Equal,
        'a' | 'A' => GhosttyKey::A,
        'b' | 'B' => GhosttyKey::B,
        'c' | 'C' => GhosttyKey::C,
        'd' | 'D' => GhosttyKey::D,
        'e' | 'E' => GhosttyKey::E,
        'f' | 'F' => GhosttyKey::F,
        'g' | 'G' => GhosttyKey::G,
        'h' | 'H' => GhosttyKey::H,
        'i' | 'I' => GhosttyKey::I,
        'j' | 'J' => GhosttyKey::J,
        'k' | 'K' => GhosttyKey::K,
        'l' | 'L' => GhosttyKey::L,
        'm' | 'M' => GhosttyKey::M,
        'n' | 'N' => GhosttyKey::N,
        'o' | 'O' => GhosttyKey::O,
        'p' | 'P' => GhosttyKey::P,
        'q' | 'Q' => GhosttyKey::Q,
        'r' | 'R' => GhosttyKey::R,
        's' | 'S' => GhosttyKey::S,
        't' | 'T' => GhosttyKey::T,
        'u' | 'U' => GhosttyKey::U,
        'v' | 'V' => GhosttyKey::V,
        'w' | 'W' => GhosttyKey::W,
        'x' | 'X' => GhosttyKey::X,
        'y' | 'Y' => GhosttyKey::Y,
        'z' | 'Z' => GhosttyKey::Z,
        '-' | '_' => GhosttyKey::Minus,
        '.' | '>' => GhosttyKey::Period,
        '\'' | '"' => GhosttyKey::Quote,
        ';' | ':' => GhosttyKey::Semicolon,
        '/' | '?' => GhosttyKey::Slash,
        ' ' => GhosttyKey::Space,
        _ => GhosttyKey::Unidentified,
    }
}

const GHOSTTY_FUNCTION_KEYS: [GhosttyKey; FUNCTION_KEY_COUNT] = [
    GhosttyKey::F1,
    GhosttyKey::F2,
    GhosttyKey::F3,
    GhosttyKey::F4,
    GhosttyKey::F5,
    GhosttyKey::F6,
    GhosttyKey::F7,
    GhosttyKey::F8,
    GhosttyKey::F9,
    GhosttyKey::F10,
    GhosttyKey::F11,
    GhosttyKey::F12,
    GhosttyKey::F13,
    GhosttyKey::F14,
    GhosttyKey::F15,
    GhosttyKey::F16,
    GhosttyKey::F17,
    GhosttyKey::F18,
    GhosttyKey::F19,
    GhosttyKey::F20,
    GhosttyKey::F21,
    GhosttyKey::F22,
    GhosttyKey::F23,
    GhosttyKey::F24,
    GhosttyKey::F25,
];

pub(super) fn ghostty_modifiers(modifiers: Modifiers) -> GhosttyKeyModifiers {
    let mut mapped = GhosttyKeyModifiers::empty();
    if modifiers.shift {
        mapped.insert(GhosttyKeyModifiers::SHIFT);
    }
    if modifiers.control {
        mapped.insert(GhosttyKeyModifiers::CTRL);
    }
    if modifiers.alt {
        mapped.insert(GhosttyKeyModifiers::ALT);
    }
    if modifiers.super_key {
        mapped.insert(GhosttyKeyModifiers::SUPER);
    }
    if modifiers.caps_lock {
        mapped.insert(GhosttyKeyModifiers::CAPS_LOCK);
    }
    if modifiers.num_lock {
        mapped.insert(GhosttyKeyModifiers::NUM_LOCK);
    }
    mapped
}

fn named_key(key: sailry_protocol::terminal::NamedKey) -> GhosttyKey {
    use sailry_protocol::terminal::NamedKey;
    match key {
        NamedKey::AltLeft => GhosttyKey::AltLeft,
        NamedKey::AltRight => GhosttyKey::AltRight,
        NamedKey::ControlLeft => GhosttyKey::ControlLeft,
        NamedKey::ControlRight => GhosttyKey::ControlRight,
        NamedKey::MetaLeft => GhosttyKey::MetaLeft,
        NamedKey::MetaRight => GhosttyKey::MetaRight,
        NamedKey::ShiftLeft => GhosttyKey::ShiftLeft,
        NamedKey::ShiftRight => GhosttyKey::ShiftRight,
        NamedKey::CapsLock => GhosttyKey::CapsLock,
        NamedKey::NumLock => GhosttyKey::NumLock,
        NamedKey::Fn => GhosttyKey::Fn,
        NamedKey::PrintScreen => GhosttyKey::PrintScreen,
        NamedKey::ScrollLock => GhosttyKey::ScrollLock,
        NamedKey::Pause => GhosttyKey::Pause,
        NamedKey::ContextMenu => GhosttyKey::ContextMenu,
        NamedKey::Numpad0 => GhosttyKey::Numpad0,
        NamedKey::Numpad1 => GhosttyKey::Numpad1,
        NamedKey::Numpad2 => GhosttyKey::Numpad2,
        NamedKey::Numpad3 => GhosttyKey::Numpad3,
        NamedKey::Numpad4 => GhosttyKey::Numpad4,
        NamedKey::Numpad5 => GhosttyKey::Numpad5,
        NamedKey::Numpad6 => GhosttyKey::Numpad6,
        NamedKey::Numpad7 => GhosttyKey::Numpad7,
        NamedKey::Numpad8 => GhosttyKey::Numpad8,
        NamedKey::Numpad9 => GhosttyKey::Numpad9,
        NamedKey::NumpadAdd => GhosttyKey::NumpadAdd,
        NamedKey::NumpadSubtract => GhosttyKey::NumpadSubtract,
        NamedKey::NumpadMultiply => GhosttyKey::NumpadMultiply,
        NamedKey::NumpadDivide => GhosttyKey::NumpadDivide,
        NamedKey::NumpadEnter => GhosttyKey::NumpadEnter,
        NamedKey::NumpadEqual => GhosttyKey::NumpadEqual,
        NamedKey::NumpadDecimal => GhosttyKey::NumpadDecimal,
        NamedKey::NumpadComma => GhosttyKey::NumpadComma,
        NamedKey::NumpadClear => GhosttyKey::NumpadClear,
    }
}
