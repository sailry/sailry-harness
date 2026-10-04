use super::*;

mod cursor {
    use super::*;

    #[test]
    fn preserves_explicit_style() {
        let mut terminal = Vt::new(&viewport(40, 8), &appearance(ColorScheme::Dark), 100).unwrap();
        assert!(terminal.checkpoint().cursor.unwrap().blinking);
        terminal.write(b"\x1b[6 q").unwrap();
        let cursor = terminal.checkpoint().cursor.unwrap();
        assert_eq!(cursor.style, CursorStyle::Bar);
        assert!(!cursor.blinking);
        terminal
            .appearance(&appearance(ColorScheme::Light))
            .unwrap();
        assert_eq!(terminal.checkpoint().cursor.unwrap(), cursor);
        terminal.write(b"\x1b[0 q").unwrap();
        assert!(terminal.checkpoint().cursor.unwrap().blinking);
        terminal.write(b"\x1b[?25l").unwrap();
        assert!(terminal.checkpoint().cursor.is_none());
        terminal.write(b"\x1bc").unwrap();
        assert!(terminal.checkpoint().cursor.unwrap().blinking);
    }
}

#[test]
fn isolates_prompt_redraw() {
    for columns in [21, 22, 25, 39, 40] {
        let mut terminal = Vt::new(&viewport(20, 4), &appearance(ColorScheme::Dark), 100).unwrap();
        // zsh PROMPT_SP pads unterminated output to wrap onto a fresh prompt row.
        terminal.write(b"output%                    \r \r\x1b[J\x1b]133;A;redraw=1\x07\x1b[45mresize-fixture\x1b[0m> \x1b]133;B\x07").unwrap();
        terminal.resize(&viewport(columns, 4)).unwrap();
        let screen = terminal.checkpoint();
        let text: String = screen
            .scrollback
            .iter()
            .chain(&screen.rows)
            .flat_map(|line| &line.spans)
            .map(|span| span.text.as_str())
            .collect();
        assert_eq!(text.trim(), "output%", "{columns}: {screen:#?}");
        assert!(
            screen
                .rows
                .iter()
                .flat_map(|line| &line.spans)
                .all(|span| span.style.background.is_none())
        );
    }
}

#[test]
fn retains_unmarked_prompts() {
    let mut terminal = Vt::new(&viewport(20, 4), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal
        .write(
            b"output%                    \r \r\x1b[J\x1b]133;A;redraw=0\x07prompt> \x1b]133;B\x07",
        )
        .unwrap();
    terminal.resize(&viewport(21, 4)).unwrap();
    let screen = terminal.checkpoint();
    let text: String = screen
        .rows
        .iter()
        .flat_map(|line| &line.spans)
        .map(|span| span.text.as_str())
        .collect();
    assert!(text.starts_with("output%"));
    assert!(text.ends_with("prompt> "));
}

#[test]
fn retains_synchronized_cursor() {
    let mut terminal = Vt::new(&viewport(40, 8), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal.write(b"\x1b[4;3Hinput").unwrap();
    let before = terminal.checkpoint();
    // A TUI frame can span multiple PTY reads, moving the cursor to paint each row.
    terminal.write(b"\x1b[?2026h\x1b[1;1Hparticle").unwrap();
    assert_eq!(terminal.checkpoint(), before);
    terminal.write(b"\x1b[7;1Hfooter").unwrap();
    assert_eq!(terminal.checkpoint(), before);
    terminal.write(b"\x1b[4;8H\x1b[?2026l").unwrap();
    let after = terminal.checkpoint();
    assert_eq!(after.cursor, before.cursor);
    assert_ne!(after.rows, before.rows);
}

#[test]
fn recovers_synchronized_output() {
    let mut terminal = Vt::new(&viewport(40, 8), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal.write(b"ready").unwrap();
    let before = terminal.checkpoint();
    terminal.write(b"\x1b[?202").unwrap();
    let write = terminal.write(b"6h\x1b[6;2Hpending\x1b[6n").unwrap();
    assert!(write.screen.is_none());
    assert_eq!(write.responses, vec![b"\x1b[6;9R".to_vec()]);
    assert_eq!(terminal.checkpoint(), before);
    let since = terminal.synchronized_since.unwrap();
    assert!(
        terminal
            .flush(since + Duration::from_millis(999), false)
            .unwrap()
            .is_none()
    );
    // More output must not postpone recovery indefinitely.
    terminal.write(b".").unwrap();
    assert!(
        terminal
            .flush(since + Duration::from_secs(1), false)
            .unwrap()
            .is_some()
    );
    assert_ne!(terminal.checkpoint(), before);
    assert!(!terminal.terminal.mode(Mode::SYNC_OUTPUT).unwrap());
    assert!(
        terminal
            .flush(since + Duration::from_secs(2), false)
            .unwrap()
            .is_none()
    );
}

#[test]
fn releases_synchronized_output() {
    let mut terminal = Vt::new(&viewport(40, 8), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal.write(b"\x1b[?2026hpending").unwrap();
    let resized = terminal.resize(&viewport(50, 9)).unwrap();
    assert_eq!(resized.rows[0].spans[0].text, "pending");
    assert!(terminal.synchronized_since.is_none());
    terminal.write(b"\x1b[?2026h exit").unwrap();
    assert!(terminal.flush(Instant::now(), true).unwrap().is_some());
    assert!(terminal.checkpoint().rows[0].spans[0].text.contains("exit"));
}

pub(super) fn viewport(columns: u16, rows: u16) -> Viewport {
    Viewport {
        columns,
        rows,
        pixel_width: u32::from(columns) * 8,
        pixel_height: u32::from(rows) * 16,
    }
}

pub(super) fn appearance(color_scheme: ColorScheme) -> Appearance {
    Appearance {
        foreground: Rgb {
            red: 240,
            green: 241,
            blue: 242,
        },
        background: Rgb {
            red: 20,
            green: 21,
            blue: 22,
        },
        palette: [Rgb {
            red: 128,
            green: 128,
            blue: 128,
        }; 16],
        color_scheme,
    }
}

#[test]
fn answers_pty_queries() {
    let mut terminal = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();

    let write = terminal.write(b"\x1b[6n").unwrap();

    assert_eq!(write.responses.len(), 1);
    assert!(write.responses[0].starts_with(b"\x1b["));
    assert!(write.responses[0].ends_with(b"R"));
}

#[test]
fn projects_styles_links_and_modes() {
    let mut terminal = Vt::new(&viewport(20, 3), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal
        .write(b"\x1b[1;4:3;31mred\x1b[0m \x1b]8;;https://example.com\x1b\\link\x1b]8;;\x1b\\\x1b[?2004h")
        .unwrap();

    let screen = terminal.checkpoint();

    assert!(screen.bracketed_paste);
    assert!(screen.rows[0].spans.iter().any(|span| span.style.bold));
    assert!(
        screen.rows[0]
            .spans
            .iter()
            .any(|span| { span.hyperlink.as_deref() == Some("https://example.com") })
    );
}

#[test]
fn encodes_semantic_keys() {
    let mut terminal = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    let control_c = Input::Key {
        event: KeyEvent {
            key: Key::Character {
                codepoint: 'c' as u32,
            },
            action: Action::Press,
            modifiers: Modifiers {
                control: true,
                ..Modifiers::default()
            },
            utf8: Some("c".to_owned()),
            unshifted_codepoint: Some('c' as u32),
        },
    };
    assert_eq!(terminal.encode_input(&control_c).unwrap(), b"\x03");

    let release = Input::Key {
        event: KeyEvent {
            key: Key::ArrowUp,
            action: Action::Release,
            modifiers: Modifiers::default(),
            utf8: None,
            unshifted_codepoint: None,
        },
    };
    assert!(terminal.encode_input(&release).unwrap().is_empty());
}

#[test]
fn encodes_bracketed_paste() {
    let mut terminal = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal.write(b"\x1b[?2004h").unwrap();

    let encoded = terminal
        .encode_input(&Input::Paste {
            text: "first\nsecond".to_owned(),
        })
        .unwrap();

    assert_eq!(encoded, b"\x1b[200~first\nsecond\x1b[201~");
}

#[test]
fn encodes_mouse_coordinates() {
    use sailry_protocol::terminal::{MouseAction, MouseButton, MouseEvent};
    let mut terminal = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal.write(b"\x1b[?1003h\x1b[?1006h").unwrap();
    assert!(terminal.checkpoint().mouse_tracking);
    let mut event = MouseEvent {
        action: MouseAction::Press,
        button: Some(MouseButton::Left),
        modifiers: Default::default(),
        column: 3,
        row: 2,
        x: 25,
        y: 40,
    };
    assert_eq!(
        terminal.encode_input(&Input::Mouse { event }).unwrap(),
        b"\x1b[<0;4;3M"
    );
    event.action = MouseAction::Release;
    assert_eq!(
        terminal.encode_input(&Input::Mouse { event }).unwrap(),
        b"\x1b[<0;4;3m"
    );
    event.action = MouseAction::Press;
    event.button = Some(MouseButton::WheelUp);
    assert_eq!(
        terminal.encode_input(&Input::Mouse { event }).unwrap(),
        b"\x1b[<64;4;3M"
    );
    terminal.write(b"\x1b[?1016h").unwrap();
    event.button = Some(MouseButton::Left);
    assert_eq!(
        terminal.encode_input(&Input::Mouse { event }).unwrap(),
        b"\x1b[<0;25;40M"
    );
    terminal.write(b"\x1b[?1016l\x1b[?1006h").unwrap();
    event.action = MouseAction::Motion;
    event.button = None;
    event.column = 4;
    assert_eq!(
        terminal.encode_input(&Input::Mouse { event }).unwrap(),
        b"\x1b[<35;5;3M"
    );
    assert!(
        terminal
            .encode_input(&Input::Mouse { event })
            .unwrap()
            .is_empty()
    );
    terminal.write(b"\x1b[?1003l").unwrap();
    assert!(!terminal.checkpoint().mouse_tracking);
    assert!(
        terminal
            .encode_input(&Input::Mouse { event })
            .unwrap()
            .is_empty()
    );
}

#[test]
fn updates_theme_reports() {
    let mut terminal = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal.write(b"\x1b[31mred\x1b[0m\x1b[?2031h").unwrap();
    let mut light = appearance(ColorScheme::Light);
    light.foreground = Rgb {
        red: 15,
        green: 16,
        blue: 17,
    };
    light.background = Rgb {
        red: 250,
        green: 251,
        blue: 252,
    };
    light.palette[1] = Rgb {
        red: 230,
        green: 10,
        blue: 20,
    };
    let update = terminal.appearance(&light).unwrap();
    assert_eq!(terminal.checkpoint().foreground, light.foreground);
    assert_eq!(terminal.checkpoint().background, light.background);
    assert_eq!(
        terminal.checkpoint().rows[0].spans[0].style.foreground,
        Some(light.palette[1])
    );
    assert_eq!(update.responses, vec![b"\x1b[?997;2n".to_vec()]);
    let report = terminal.write(b"\x1b[?996n").unwrap();
    assert_eq!(report.responses, vec![b"\x1b[?997;2n".to_vec()]);
}

#[test]
fn bounds_scrollback_patches() {
    let mut terminal = Vt::new(&viewport(8, 2), &appearance(ColorScheme::Light), 2).unwrap();
    terminal.write(b"one\r\ntwo\r\nthree\r\n").unwrap();
    let write = terminal.write(b"four\r\n").unwrap();

    let Some(ScreenUpdate::Patch { patch }) = write.screen else {
        panic!("expected incremental screen patch");
    };
    assert!(patch.dropped_scrollback_rows <= 2);
    assert!(terminal.checkpoint().scrollback.len() <= 2);
}

#[test]
fn reprojects_mismatched_scrollback() {
    enum Scenario {
        Exact,
        CachedPlusOne,
        LargeJump,
        UnderlyingReset,
    }

    for scenario in [
        Scenario::Exact,
        Scenario::CachedPlusOne,
        Scenario::LargeJump,
        Scenario::UnderlyingReset,
    ] {
        let mut terminal = Vt::new(&viewport(8, 2), &appearance(ColorScheme::Light), 2).unwrap();
        terminal.write(b"one\r\ntwo\r\nthree\r\nfour\r\n").unwrap();

        let (bytes, must_replace) = match scenario {
            Scenario::Exact => (b"five\r\n".to_vec(), false),
            Scenario::CachedPlusOne => {
                let extra = terminal
                    .screen
                    .scrollback
                    .last()
                    .expect("base write has scrollback")
                    .clone();
                terminal.screen.scrollback.push(extra);
                (b"five\r\n".to_vec(), true)
            }
            Scenario::LargeJump => (
                (0..128)
                    .map(|index| format!("{index}\r\n"))
                    .collect::<String>()
                    .into_bytes(),
                false,
            ),
            Scenario::UnderlyingReset => {
                terminal.history_anchor = None;
                (b"five\r\n".to_vec(), true)
            }
        };
        let update = terminal.write(&bytes).unwrap().screen;
        if must_replace {
            assert!(matches!(update, Some(ScreenUpdate::Replace { .. })));
        }
        assert!(terminal.checkpoint().scrollback.len() <= 2);

        // Reprojection resets the incremental baseline; the next delta remains usable.
        terminal.write(b"tail\r\n").unwrap();
        assert!(terminal.checkpoint().scrollback.len() <= 2);
    }
}

#[test]
fn rejects_invalid_rebuild() {
    let mut terminal = Vt::new(&viewport(8, 2), &appearance(ColorScheme::Light), 2).unwrap();
    terminal.write(b"one\r\ntwo\r\nthree\r\n").unwrap();
    terminal.viewport.columns = 0;
    terminal.screen.scrollback.clear();

    let error = match terminal.write(b"four\r\n") {
        Ok(_) => panic!("invalid canonical projection unexpectedly rebuilt"),
        Err(error) => error,
    };
    assert!(error.to_string().contains("canonical reprojection failed"));
}

#[test]
fn parses_fragmented_osc_metadata() {
    let mut terminal = Vt::new(&viewport(80, 24), &appearance(ColorScheme::Dark), 100).unwrap();
    terminal.write(b"\x1b]0;Project shell\x07").unwrap();
    assert_eq!(terminal.checkpoint().features.title, "Project shell");
    terminal.write(b"\x1b[22;0t\x1b]2;Running ").unwrap();
    terminal.write(b"server\x1b").unwrap();
    terminal.write(b"\\").unwrap();
    assert_eq!(terminal.checkpoint().features.title, "Running server");
    terminal.write(b"\x1b[23;0t").unwrap();
    assert_eq!(terminal.checkpoint().features.title, "Project shell");
    assert_eq!(
        terminal.write(b"\x1b[21t").unwrap().responses,
        vec![b"\x1b]lProject shell\x1b\\".to_vec()]
    );
    terminal
        .write(b"\x1b[22;2;5t\x1b]2;Nested\x07\x1b[23;2;5t")
        .unwrap();
    assert_eq!(terminal.checkpoint().features.title, "Project shell");
    terminal.write(b"\x1b[23;2;5t").unwrap();
    assert_eq!(terminal.checkpoint().features.title, "Project shell");
    terminal
        .write(b"\x1b]7;file://localhost/tmp/project%20one\x07")
        .unwrap();
    assert_eq!(
        terminal.checkpoint().features.directory,
        "file://localhost/tmp/project%20one"
    );
}
