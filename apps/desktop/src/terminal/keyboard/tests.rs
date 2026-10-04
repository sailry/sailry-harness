use super::*;
use core::prelude::v1::test;

fn down(key: &str, text: Option<&str>) -> KeyDownEvent {
    KeyDownEvent {
        keystroke: Keystroke {
            key_char: text.map(str::to_owned),
            ..Keystroke::parse(key).unwrap()
        },
        is_held: false,
        prefer_character_input: false,
    }
}

#[test]
fn text_event_phases() {
    let mut state = State::default();
    let mut key = down("shift-a", Some("A"));
    for action in [Action::Press, Action::Repeat] {
        key.is_held = action == Action::Repeat;
        assert!(state.press(&key, false).is_none());
        let Some(Input::Key { event }) = state.commit("A") else {
            panic!("key expected")
        };
        assert_eq!(event.action, action);
        assert_eq!(event.utf8.as_deref(), Some("A"));
        assert_eq!(event.unshifted_codepoint, Some('a' as u32));
        assert!(event.modifiers.shift);
    }
    assert_eq!(
        state.release(&key.keystroke).unwrap().action,
        Action::Release
    );
    assert!(state.release(&key.keystroke).is_none());
}

#[test]
fn isolates_composition() {
    let mut state = State::default();
    let key = down("n", Some("n"));
    state.press(&key, false);
    state.composing();
    assert!(state.press(&key, true).is_none());
    assert_eq!(
        state.commit("你好"),
        Some(Input::Text {
            text: "你好".into()
        })
    );
    assert!(state.release(&key.keystroke).is_none());
    assert!(state.release(&down("cmd-c", None).keystroke).is_none());
}

#[test]
fn release_uses_current_modifiers() {
    let mut state = State::default();
    state.press(&down("shift-a", Some("A")), false);
    state.commit("A");
    let event = state.release(&down("a", Some("a")).keystroke).unwrap();
    assert_eq!(event.action, Action::Release);
    assert_eq!(event.key.character(), Some('a'));
    assert!(!event.modifiers.shift);
}

#[test]
fn releases_keys_on_blur() {
    let mut state = State::default();
    state.press(&down("up", None), false);
    state.press(&down("a", Some("a")), false);
    let released = state.release_all();
    assert_eq!(released.len(), 1);
    assert_eq!(released[0].action, Action::Release);
    assert_eq!(state.commit("a"), Some(Input::Text { text: "a".into() }));
}

#[test]
fn special_keys_and_platform_text() {
    let mut state = State::default();
    let event = state
        .press(&down("shift-enter", Some("\n")), false)
        .unwrap();
    assert_eq!(event.key, Key::Enter);
    assert!(event.modifiers.shift);
    assert_eq!(event.utf8, None);
    let mut text = down("ctrl-alt-e", Some("€"));
    text.prefer_character_input = true;
    assert!(state.press(&text, false).is_none());
    assert_eq!(state.commit("€"), Some(Input::Text { text: "€".into() }));
}

#[test]
#[cfg(target_os = "macos")]
fn restores_natural_editing() {
    for (key, character, control, alt) in [
        ("cmd-left", 'a', true, false),
        ("cmd-right", 'e', true, false),
        ("cmd-backspace", 'u', true, false),
        ("alt-left", 'b', false, true),
        ("alt-right", 'f', false, true),
    ] {
        let event = semantic(&down(key, None).keystroke).unwrap();
        assert_eq!(event.key.character(), Some(character));
        assert_eq!(event.modifiers.control, control);
        assert_eq!(event.modifiers.alt, alt);
        assert!(!event.modifiers.super_key);
    }
    assert_eq!(
        semantic(&down("cmd-shift-left", None).keystroke)
            .unwrap()
            .key,
        Key::ArrowLeft
    );
}

mod modifiers {
    use super::super::native::Native;
    use super::*;

    fn shift(right: bool, held: bool) -> Native {
        Native {
            code: if right { 60 } else { 56 },
            key: Some(if right {
                protocol::NamedKey::ShiftRight
            } else {
                protocol::NamedKey::ShiftLeft
            }),
            unshifted: None,
            caps_lock: false,
            modifiers: protocol::Modifiers {
                shift: held,
                ..Default::default()
            },
        }
    }

    #[test]
    fn release_after_blur() {
        let mut keyboard = State::default();
        assert_eq!(
            keyboard.modifier_event(shift(false, true)).unwrap().action,
            Action::Press
        );
        assert_eq!(keyboard.release_all().len(), 1);
        assert_eq!(
            keyboard.modifier_event(shift(false, false)).unwrap().action,
            Action::Release
        );
        assert!(keyboard.release_all().is_empty());
        assert_eq!(
            keyboard.modifier_event(shift(false, true)).unwrap().action,
            Action::Press
        );
    }

    #[test]
    fn simultaneous_modifier_sides() {
        let mut keyboard = State::default();
        assert_eq!(
            keyboard.modifier_event(shift(false, true)).unwrap().action,
            Action::Press
        );
        assert_eq!(
            keyboard.modifier_event(shift(true, true)).unwrap().action,
            Action::Press
        );
        assert_eq!(
            keyboard.modifier_event(shift(false, true)).unwrap().action,
            Action::Release
        );
        assert_eq!(
            keyboard.modifier_event(shift(true, false)).unwrap().action,
            Action::Release
        );
        assert!(keyboard.release_all().is_empty());
    }
}
