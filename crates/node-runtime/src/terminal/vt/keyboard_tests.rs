use super::{
    tests::{appearance, viewport},
    *,
};

#[test]
fn projects_terminal_state() {
    for (scenario, check) in [
        (
            "encodes_legacy_modifiers",
            (|scenario: &'static str| {
                let mut vt =
                    Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
                for (character, control, alt, expected) in [
                    ('a', true, false, b"\x01".as_slice()),
                    ('e', true, false, b"\x05".as_slice()),
                    ('u', true, false, b"\x15".as_slice()),
                    ('b', false, true, b"\x1bb".as_slice()),
                    ('f', false, true, b"\x1bf".as_slice()),
                ] {
                    let event = KeyEvent {
                        key: Key::Character {
                            codepoint: character as u32,
                        },
                        action: Action::Press,
                        modifiers: Modifiers {
                            control,
                            alt,
                            ..Default::default()
                        },
                        utf8: Some(character.to_string()),
                        unshifted_codepoint: Some(character as u32),
                    };
                    assert_eq!(
                        vt.encode_input(&Input::Key { event }).unwrap(),
                        expected,
                        "{scenario}"
                    );
                }
            }) as fn(&'static str),
        ),
        (
            "negotiates_flags_and_encodes_events",
            (|scenario: &'static str| {
                let mut vt =
                    Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
                assert_eq!(
                    vt.write(b"\x1b[?u").unwrap().responses,
                    vec![b"\x1b[?0u".to_vec()],
                    "{scenario}"
                );
                vt.write(b"\x1b[>31u").unwrap();
                assert_eq!(
                    vt.write(b"\x1b[?u").unwrap().responses,
                    vec![b"\x1b[?31u".to_vec()],
                    "{scenario}"
                );
                let mut event = KeyEvent {
                    key: Key::Character {
                        codepoint: 'a' as u32,
                    },
                    action: Action::Press,
                    modifiers: Modifiers::default(),
                    utf8: Some("a".into()),
                    unshifted_codepoint: Some('a' as u32),
                };
                for (action, expected) in [
                    (Action::Press, b"\x1b[97;;97u".as_slice()),
                    (Action::Repeat, b"\x1b[97;1:2;97u".as_slice()),
                    (Action::Release, b"\x1b[97;1:3u".as_slice()),
                ] {
                    event.action = action;
                    assert_eq!(
                        vt.encode_input(&Input::Key {
                            event: event.clone()
                        })
                        .unwrap(),
                        expected,
                        "{scenario}"
                    );
                }
                event.action = Action::Press;
                event.modifiers.shift = true;
                event.utf8 = Some("A".into());
                assert_eq!(
                    vt.encode_input(&Input::Key {
                        event: event.clone()
                    })
                    .unwrap(),
                    b"\x1b[97:65;2;65u",
                    "{scenario}"
                );
                vt.write(b"\x1b[<u").unwrap();
                assert_eq!(
                    vt.write(b"\x1b[?u").unwrap().responses,
                    vec![b"\x1b[?0u".to_vec()],
                    "{scenario}"
                );
                assert_eq!(
                    vt.encode_input(&Input::Key { event }).unwrap(),
                    b"A",
                    "{scenario}"
                );
            }),
        ),
        (
            "preserves_disambiguated_text",
            (|scenario: &'static str| {
                let mut vt =
                    Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
                vt.write(b"\x1b[>1u\x1b[?2004h").unwrap();
                let mut event = KeyEvent {
                    key: Key::Enter,
                    action: Action::Press,
                    modifiers: Modifiers::default(),
                    utf8: None,
                    unshifted_codepoint: None,
                };
                assert_eq!(
                    vt.encode_input(&Input::Key {
                        event: event.clone()
                    })
                    .unwrap(),
                    b"\r",
                    "{scenario}"
                );
                event.modifiers.shift = true;
                assert_eq!(
                    vt.encode_input(&Input::Key { event }).unwrap(),
                    b"\x1b[13;2u",
                    "{scenario}"
                );
                assert_eq!(
                    vt.encode_input(&Input::Text {
                        text: "你好".into()
                    })
                    .unwrap(),
                    "你好".as_bytes(),
                    "{scenario}"
                );
                assert_eq!(
                    vt.encode_input(&Input::Paste {
                        text: "first\nsecond".into()
                    })
                    .unwrap(),
                    b"\x1b[200~first\nsecond\x1b[201~",
                    "{scenario}"
                );
            }),
        ),
        (
            "distinguishes_keypad_and_modifier_sides",
            (|scenario: &'static str| {
                use sailry_protocol::terminal::NamedKey;
                let mut vt =
                    Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
                vt.write(b"\x1b[>31u").unwrap();
                let mut outputs = std::collections::BTreeSet::new();
                for key in [
                    NamedKey::NumpadEnter,
                    NamedKey::Numpad1,
                    NamedKey::ShiftLeft,
                    NamedKey::ShiftRight,
                    NamedKey::AltLeft,
                    NamedKey::AltRight,
                ] {
                    let event = KeyEvent {
                        key: Key::Named { key },
                        action: Action::Press,
                        modifiers: Default::default(),
                        utf8: None,
                        unshifted_codepoint: None,
                    };
                    let output = vt.encode_input(&Input::Key { event }).unwrap();
                    assert!(output.starts_with(b"\x1b["), "{scenario}");
                    assert!(output.ends_with(b"u"), "{scenario}");
                    assert!(outputs.insert(output), "{scenario}");
                }
                assert!(outputs.contains(b"\x1b[57441u".as_slice()), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
