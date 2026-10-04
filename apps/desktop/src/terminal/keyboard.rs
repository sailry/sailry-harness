use super::protocol::{self, Action, Input, Key, KeyEvent};
use gpui_kit::{KeyDownEvent, Keystroke};
use std::collections::BTreeMap;
mod native;

#[derive(Default)]
pub(super) struct State {
    pending: Option<(String, KeyEvent)>,
    pressed: BTreeMap<String, KeyEvent>,
}

impl State {
    pub fn press(&mut self, event: &KeyDownEvent, composing: bool) -> Option<KeyEvent> {
        self.pending = None;
        if composing || event.prefer_character_input {
            return None;
        }
        let original = &event.keystroke;
        let native = native::current(false);
        let stroke = native::normalize(original, native.as_ref());
        let mut key = semantic(&stroke).or_else(|| {
            native
                .as_ref()
                .and_then(|event| event.key)
                .map(|key| KeyEvent {
                    key: Key::Named { key },
                    action: Action::Press,
                    modifiers: native
                        .as_ref()
                        .map(|event| event.modifiers)
                        .unwrap_or_default(),
                    utf8: None,
                    unshifted_codepoint: None,
                })
        })?;
        native::apply(&mut key, native.as_ref());
        let id = native::identity(original, native.as_ref());
        key.action = if event.is_held {
            Action::Repeat
        } else {
            Action::Press
        };
        let text = key.key.character().is_some()
            && !stroke.modifiers.control
            && !stroke.modifiers.alt
            && !stroke.modifiers.platform;
        if text {
            // Let the platform resolve text/IME first, retaining the physical event
            // so Kitty still receives press/repeat instead of untyped UTF-8 bytes.
            self.pending = Some((id, key));
            return None;
        }
        self.pressed.insert(id, key.clone());
        Some(key)
    }

    pub fn commit(&mut self, text: &str) -> Option<Input> {
        let pending = self.pending.take();
        if text.is_empty() {
            return None;
        }
        if text.chars().any(char::is_control) {
            return Some(Input::Paste { text: text.into() });
        }
        if let Some((id, mut event)) = pending
            && text.len() <= protocol::MAX_KEY_UTF8_BYTES
        {
            event.utf8 = Some(text.into());
            self.pressed.insert(id, event.clone());
            return Some(Input::Key { event });
        }
        Some(Input::Text { text: text.into() })
    }

    pub fn composing(&mut self) {
        self.pending = None;
    }

    pub fn release(&mut self, original: &Keystroke) -> Option<KeyEvent> {
        let native = native::current(true);
        let stroke = native::normalize(original, native.as_ref());
        let id = native::identity(original, native.as_ref());
        if self.pending.as_ref().is_some_and(|(key, _)| key == &id) {
            self.pending = None;
        }
        let mut event = self.pressed.remove(&id)?;
        event.action = Action::Release;
        // Keep the pressed key identity, but modifiers can change before key-up.
        event.modifiers = semantic(&stroke)
            .map(|event| event.modifiers)
            .or_else(|| native.as_ref().map(|event| event.modifiers))?;
        event.modifiers.caps_lock = native.as_ref().is_some_and(|event| event.caps_lock);
        Some(event)
    }

    pub fn modifier(&mut self) -> Option<KeyEvent> {
        self.modifier_event(native::modifier()?)
    }

    fn modifier_event(&mut self, native: native::Native) -> Option<KeyEvent> {
        let key = native.key?;
        let held = match key {
            protocol::NamedKey::ShiftLeft | protocol::NamedKey::ShiftRight => {
                native.modifiers.shift
            }
            protocol::NamedKey::ControlLeft | protocol::NamedKey::ControlRight => {
                native.modifiers.control
            }
            protocol::NamedKey::AltLeft | protocol::NamedKey::AltRight => native.modifiers.alt,
            protocol::NamedKey::MetaLeft | protocol::NamedKey::MetaRight => {
                native.modifiers.super_key
            }
            protocol::NamedKey::CapsLock => native.caps_lock,
            _ => true,
        };
        let id = format!("native:{}", native.code);
        // Focus changes clear tracked presses. A subsequent native release must
        // not become a new press; tracking still distinguishes left/right keys
        // while the other side keeps the aggregate modifier flag set.
        let action = if self.pressed.remove(&id).is_some() || !held {
            Action::Release
        } else {
            Action::Press
        };
        let mut event = KeyEvent {
            key: Key::Named { key },
            action,
            modifiers: Default::default(),
            utf8: None,
            unshifted_codepoint: None,
        };
        native::apply(&mut event, Some(&native));
        event.modifiers = native.modifiers;
        if action == Action::Press {
            self.pressed.insert(id, event.clone());
        }
        Some(event)
    }

    pub fn release_all(&mut self) -> Vec<KeyEvent> {
        self.pending = None;
        std::mem::take(&mut self.pressed)
            .into_values()
            .map(|mut event| {
                event.action = Action::Release;
                event
            })
            .collect()
    }
}

fn semantic(stroke: &Keystroke) -> Option<KeyEvent> {
    let key = match stroke.key.as_str() {
        "enter" | "return" => Key::Enter,
        "escape" => Key::Escape,
        "tab" => Key::Tab,
        "backspace" => Key::Backspace,
        "delete" => Key::Delete,
        "insert" => Key::Insert,
        "home" => Key::Home,
        "end" => Key::End,
        "pageup" => Key::PageUp,
        "pagedown" => Key::PageDown,
        "up" => Key::ArrowUp,
        "down" => Key::ArrowDown,
        "left" => Key::ArrowLeft,
        "right" => Key::ArrowRight,
        "space" => Key::Character {
            codepoint: ' ' as u32,
        },
        key if key.starts_with('f')
            && key[1..]
                .parse::<u8>()
                .is_ok_and(|number| (1..=protocol::FUNCTION_KEY_COUNT as u8).contains(&number)) =>
        {
            Key::Function {
                number: key[1..].parse().ok()?,
            }
        }
        key => {
            let mut characters = key.chars();
            let character = characters.next()?;
            if characters.next().is_some() || character.is_control() {
                return None;
            }
            Key::Character {
                codepoint: character as u32,
            }
        }
    };
    let mut event = KeyEvent {
        key,
        action: Action::Press,
        modifiers: protocol::Modifiers {
            shift: stroke.modifiers.shift,
            control: stroke.modifiers.control,
            alt: stroke.modifiers.alt,
            super_key: stroke.modifiers.platform,
            ..Default::default()
        },
        utf8: stroke.key_char.clone().filter(|text| {
            !stroke.modifiers.control
                && !stroke.modifiers.alt
                && !stroke.modifiers.platform
                && !text.is_empty()
                && text.chars().all(|character| {
                    !character.is_control() && !matches!(character as u32, 0xF700..=0xF8FF)
                })
        }),
        unshifted_codepoint: key.character().map(|character| character as u32),
    };
    if cfg!(target_os = "macos") {
        natural_editing(&mut event);
    }
    Some(event)
}

fn natural_editing(event: &mut KeyEvent) {
    // Restore the old terminal's macOS natural editing bindings (67ae9fa0),
    // also used by Ghostty's app defaults. See sailry-code-terminal.md.
    let modifiers = event.modifiers;
    if modifiers.shift || modifiers.control {
        return;
    }
    let character = match (event.key, modifiers.super_key, modifiers.alt) {
        (Key::ArrowLeft, true, false) => 'a',
        (Key::ArrowRight, true, false) => 'e',
        (Key::Backspace, true, false) => 'u',
        (Key::ArrowLeft, false, true) => 'b',
        (Key::ArrowRight, false, true) => 'f',
        _ => return,
    };
    event.key = Key::Character {
        codepoint: character as u32,
    };
    event.modifiers.super_key = false;
    event.modifiers.control = modifiers.super_key;
    event.utf8 = Some(character.to_string());
    event.unshifted_codepoint = Some(character as u32);
}

#[cfg(test)]
mod tests;
