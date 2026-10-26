use super::*;

fn bounded(screen: &Screen) {
    screen.validate().unwrap();
    for span in screen
        .scrollback
        .iter()
        .chain(&screen.rows)
        .flat_map(|line| &line.spans)
    {
        assert!(
            span.column + span.columns <= screen.columns,
            "{} columns: {span:?}",
            screen.columns
        );
    }
}

#[test]
fn bounds_wide_cells_during_resize() {
    for alternate in [false, true] {
        for wrap in [false, true] {
            let mut terminal =
                Vt::new(&viewport(24, 4), &appearance(ColorScheme::Dark), 100).unwrap();
            terminal
                .write("中文边界测试\r\n第一行\r\n第二行\r\n第三行\r\n".as_bytes())
                .unwrap();
            assert!(!terminal.checkpoint().scrollback.is_empty());
            if alternate {
                terminal.write(b"\x1b[?1049h").unwrap();
            }
            if !wrap {
                terminal.write(b"\x1b[?7l").unwrap();
            }
            for columns in (1..=24).rev().chain(2..=24) {
                let resized = terminal.resize(&viewport(columns, 4)).unwrap();
                assert_eq!(resized.columns, columns);
                assert_eq!(resized.alternate, alternate);
                bounded(&resized);
                // Exercise a wide cell at both edges, including DECAWM disabled.
                terminal
                    .write(format!("\x1b[1;1H中文\x1b[2;{columns}H界").as_bytes())
                    .unwrap();
                bounded(&terminal.checkpoint());
            }
            if alternate {
                terminal.write(b"\x1b[?1049l").unwrap();
                bounded(&terminal.checkpoint());
            }
        }
    }
}

#[test]
fn repairs_clipped_projection() {
    let mut screen = empty_screen(&viewport(3, 2), &appearance(ColorScheme::Dark));
    let clipped = Span {
        column: 2,
        columns: 2,
        text: "界".into(),
        style: Default::default(),
        hyperlink: Some("https://example.com".into()),
    };
    screen.rows[0].spans.push(clipped.clone());
    screen.scrollback.push(Line {
        spans: vec![clipped],
        wrapped: false,
    });
    screen.cursor = Some(Cursor {
        column: 3,
        row: 2,
        at_wide_tail: true,
        blinking: true,
        style: CursorStyle::Block,
    });
    assert!(screen.validate().is_err());
    sanitize(&mut screen).unwrap();
    bounded(&screen);
    for line in [&screen.rows[0], &screen.scrollback[0]] {
        assert_eq!(line.spans[0].columns, 1);
        assert_eq!(line.spans[0].text, " ");
        assert!(line.spans[0].hyperlink.is_none());
    }
    let cursor = screen.cursor.unwrap();
    assert_eq!((cursor.column, cursor.row), (2, 1));
    assert!(!cursor.at_wide_tail);
}
