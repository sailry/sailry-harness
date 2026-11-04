use super::*;
use core::prelude::v1::test;

fn down(key: &'static str, text: Option<&str>) -> KeyDownEvent {
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
fn input_lifecycle() {
    for (scenario, check) in [
        (
            "text_event_phases",
            (|_scenario: &str| {
                let mut state = State::default();
                let mut key = down("shift-a", Some("A"));
                for action in [Action::Press, Action::Repeat] {
                    key.is_held = action == Action::Repeat;
                    assert!(state.press(&key, false).is_none(), "text event phases");
                    let Some(Input::Key { event }) = state.commit("A") else {
                        panic!("key expected")
                    };
                    assert_eq!(event.action, action, "text event phases");
                    assert_eq!(event.utf8.as_deref(), Some("A"), "text event phases");
                    assert_eq!(
                        event.unshifted_codepoint,
                        Some('a' as u32),
                        "text event phases"
                    );
                    assert!(event.modifiers.shift, "text event phases");
                }
                assert_eq!(
                    state.release(&key.keystroke).unwrap().action,
                    Action::Release,
                    "text event phases"
                );
                assert!(state.release(&key.keystroke).is_none(), "text event phases");
            }) as fn(&str),
        ),
        (
            "isolates_composition",
            (|_scenario: &str| {
                let mut state = State::default();
                let key = down("n", Some("n"));
                state.press(&key, false);
                state.composing();
                assert!(state.press(&key, true).is_none(), "isolates composition");
                assert_eq!(
                    state.commit("你好"),
                    Some(Input::Text {
                        text: "你好".into()
                    }),
                    "isolates composition"
                );
                assert!(
                    state.release(&key.keystroke).is_none(),
                    "isolates composition"
                );
                assert!(
                    state.release(&down("cmd-c", None).keystroke).is_none(),
                    "isolates composition"
                );
            }),
        ),
        (
            "release_uses_current_modifiers",
            (|_scenario: &str| {
                let mut state = State::default();
                state.press(&down("shift-a", Some("A")), false);
                state.commit("A");
                let event = state.release(&down("a", Some("a")).keystroke).unwrap();
                assert_eq!(
                    event.action,
                    Action::Release,
                    "release uses current modifiers"
                );
                assert_eq!(
                    event.key.character(),
                    Some('a'),
                    "release uses current modifiers"
                );
                assert!(!event.modifiers.shift, "release uses current modifiers");
            }),
        ),
        (
            "releases_keys_on_blur",
            (|_scenario: &str| {
                let mut state = State::default();
                state.press(&down("up", None), false);
                state.press(&down("a", Some("a")), false);
                let released = state.release_all();
                assert_eq!(released.len(), 1, "releases keys on blur");
                assert_eq!(released[0].action, Action::Release, "releases keys on blur");
                assert_eq!(
                    state.commit("a"),
                    Some(Input::Text { text: "a".into() }),
                    "releases keys on blur"
                );
            }),
        ),
        (
            "special_keys_and_platform_text",
            (|_scenario: &str| {
                let mut state = State::default();
                let event = state
                    .press(&down("shift-enter", Some("\n")), false)
                    .unwrap();
                assert_eq!(event.key, Key::Enter, "special keys and platform text");
                assert!(event.modifiers.shift, "special keys and platform text");
                assert_eq!(event.utf8, None, "special keys and platform text");
                let mut text = down("ctrl-alt-e", Some("€"));
                text.prefer_character_input = true;
                assert!(
                    state.press(&text, false).is_none(),
                    "special keys and platform text"
                );
                assert_eq!(
                    state.commit("€"),
                    Some(Input::Text { text: "€".into() }),
                    "special keys and platform text"
                );
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
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
    fn input_lifecycle() {
        for (scenario, check) in [
            (
                "release_after_blur",
                (|_scenario: &str| {
                    let mut keyboard = State::default();
                    assert_eq!(
                        keyboard.modifier_event(shift(false, true)).unwrap().action,
                        Action::Press,
                        "release after blur"
                    );
                    assert_eq!(keyboard.release_all().len(), 1, "release after blur");
                    assert_eq!(
                        keyboard.modifier_event(shift(false, false)).unwrap().action,
                        Action::Release,
                        "release after blur"
                    );
                    assert!(keyboard.release_all().is_empty(), "release after blur");
                    assert_eq!(
                        keyboard.modifier_event(shift(false, true)).unwrap().action,
                        Action::Press,
                        "release after blur"
                    );
                }) as fn(&str),
            ),
            (
                "simultaneous_modifier_sides",
                (|_scenario: &str| {
                    let mut keyboard = State::default();
                    assert_eq!(
                        keyboard.modifier_event(shift(false, true)).unwrap().action,
                        Action::Press,
                        "simultaneous modifier sides"
                    );
                    assert_eq!(
                        keyboard.modifier_event(shift(true, true)).unwrap().action,
                        Action::Press,
                        "simultaneous modifier sides"
                    );
                    assert_eq!(
                        keyboard.modifier_event(shift(false, true)).unwrap().action,
                        Action::Release,
                        "simultaneous modifier sides"
                    );
                    assert_eq!(
                        keyboard.modifier_event(shift(true, false)).unwrap().action,
                        Action::Release,
                        "simultaneous modifier sides"
                    );
                    assert!(
                        keyboard.release_all().is_empty(),
                        "simultaneous modifier sides"
                    );
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
