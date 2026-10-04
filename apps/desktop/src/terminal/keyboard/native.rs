//! GPUI 0.3.4 drops physical identity, keypad location and normalized Shift.
//! Read the current AppKit event only during its existing GPUI dispatch; no
//! event monitor or replacement text/IME path is installed.
use super::*;
use protocol::NamedKey;

pub(super) struct Native {
    pub code: u16,
    pub key: Option<NamedKey>,
    pub unshifted: Option<char>,
    pub caps_lock: bool,
    pub modifiers: protocol::Modifiers,
}

pub(super) fn current(release: bool) -> Option<Native> {
    read(if release { 11 } else { 10 })
}
pub(super) fn modifier() -> Option<Native> {
    read(12)
}

#[cfg(all(target_os = "macos", not(test)))]
fn read(kind: usize) -> Option<Native> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSEventModifierFlags as Flags};
    let event = NSApplication::sharedApplication(MainThreadMarker::new()?).currentEvent()?;
    if event.r#type().0 != kind {
        return None;
    }
    let flags = event.modifierFlags();
    let code = event.keyCode();
    let unshifted = if kind != 12 {
        event
            .charactersByApplyingModifiers(Flags::empty())
            .and_then(|text| {
                let text = text.to_string();
                let mut chars = text.chars();
                let ch = chars.next()?;
                (chars.next().is_none()
                    && !ch.is_control()
                    && !matches!(ch as u32, 0xF700..=0xF8FF))
                .then_some(ch)
            })
    } else {
        None
    };
    Some(Native {
        code,
        key: physical(code),
        unshifted,
        caps_lock: flags.contains(Flags::CapsLock),
        modifiers: protocol::Modifiers {
            shift: flags.contains(Flags::Shift),
            control: flags.contains(Flags::Control),
            alt: flags.contains(Flags::Option),
            super_key: flags.contains(Flags::Command),
            caps_lock: flags.contains(Flags::CapsLock),
            num_lock: false,
        },
    })
}

#[cfg(any(not(target_os = "macos"), test))]
fn read(_: usize) -> Option<Native> {
    None
}

pub(super) fn identity(stroke: &Keystroke, native: Option<&Native>) -> String {
    native.map_or_else(
        || stroke.key.clone(),
        |event| format!("native:{}", event.code),
    )
}

pub(super) fn normalize(stroke: &Keystroke, native: Option<&Native>) -> Keystroke {
    let mut stroke = stroke.clone();
    if let Some(event) = native {
        stroke.modifiers.shift = event.modifiers.shift;
        if stroke.key.chars().count() == 1
            && let Some(character) = event.unshifted
        {
            stroke.key = character.to_string();
        }
    }
    stroke
}

pub(super) fn apply(event: &mut KeyEvent, native: Option<&Native>) {
    if let Some(native) = native {
        event.modifiers.caps_lock = native.caps_lock;
        if let Some(key) = native.key {
            event.key = Key::Named { key };
            event.unshifted_codepoint = None;
        }
    }
}

#[cfg(any(target_os = "macos", test))]
fn physical(code: u16) -> Option<NamedKey> {
    // AppKit keyCode values from Carbon HIToolbox Events.h (kVK_*).
    Some(match code {
        54 => NamedKey::MetaRight,
        55 => NamedKey::MetaLeft,
        56 => NamedKey::ShiftLeft,
        60 => NamedKey::ShiftRight,
        57 => NamedKey::CapsLock,
        58 => NamedKey::AltLeft,
        61 => NamedKey::AltRight,
        59 => NamedKey::ControlLeft,
        62 => NamedKey::ControlRight,
        63 => NamedKey::Fn,
        65 => NamedKey::NumpadDecimal,
        67 => NamedKey::NumpadMultiply,
        69 => NamedKey::NumpadAdd,
        71 => NamedKey::NumpadClear,
        75 => NamedKey::NumpadDivide,
        76 => NamedKey::NumpadEnter,
        78 => NamedKey::NumpadSubtract,
        81 => NamedKey::NumpadEqual,
        82 => NamedKey::Numpad0,
        83 => NamedKey::Numpad1,
        84 => NamedKey::Numpad2,
        85 => NamedKey::Numpad3,
        86 => NamedKey::Numpad4,
        87 => NamedKey::Numpad5,
        88 => NamedKey::Numpad6,
        89 => NamedKey::Numpad7,
        91 => NamedKey::Numpad8,
        92 => NamedKey::Numpad9,
        95 => NamedKey::NumpadComma,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;
    #[test]
    fn preserves_physical_identity() {
        assert_eq!(physical(76), Some(NamedKey::NumpadEnter));
        assert_eq!(physical(54), Some(NamedKey::MetaRight));
        assert_eq!(physical(58), Some(NamedKey::AltLeft));
        let native = Native {
            code: 18,
            key: None,
            unshifted: Some('1'),
            caps_lock: false,
            modifiers: protocol::Modifiers {
                shift: true,
                ..Default::default()
            },
        };
        let normalized = normalize(&Keystroke::parse("!").unwrap(), Some(&native));
        assert_eq!(normalized.key, "1");
        assert!(normalized.modifiers.shift);
        assert_eq!(identity(&normalized, Some(&native)), "native:18");
    }
}
