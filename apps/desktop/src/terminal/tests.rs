use super::*;
use core::prelude::v1::test;

#[test]
fn selection_boundaries() {
    use selection::{Position, Selection};
    let line = |text: &str, columns, wrapped| protocol::Line {
        spans: vec![protocol::Span {
            column: 0,
            columns,
            text: text.into(),
            style: Default::default(),
            hyperlink: None,
        }],
        wrapped,
    };
    let color = protocol::Rgb {
        red: 0,
        green: 0,
        blue: 0,
    };
    let screen = protocol::Screen {
        columns: 4,
        scrollback: vec![line("abcd", 4, true)],
        rows: vec![line("ef", 2, false), line("中", 2, false)],
        cursor: None,
        foreground: color,
        background: color,
        cursor_color: color,
        bracketed_paste: false,
        mouse_tracking: false,
        alternate: false,
        features: Default::default(),
        graphics: Default::default(),
    };
    let selection = Selection {
        anchor: Some(Position { row: 0, column: 1 }),
        end: Some(Position { row: 2, column: 1 }),
        dragging: false,
        ..Default::default()
    };
    assert_eq!(selection.text(&screen), "bcdef\n中");
    assert_eq!(
        super::text::find(&screen, "cdef"),
        vec![(
            Position { row: 0, column: 2 },
            Position { row: 1, column: 2 }
        )]
    );
    assert_eq!(
        super::text::find(&screen, "中"),
        vec![(
            Position { row: 2, column: 0 },
            Position { row: 2, column: 2 }
        )]
    );
    assert_eq!(
        super::selection::extent(&screen, Position { row: 0, column: 2 }, true),
        (
            Position { row: 0, column: 0 },
            Position { row: 1, column: 2 }
        )
    );
    assert_eq!(
        super::selection::extent(&screen, Position { row: 0, column: 2 }, false),
        (
            Position { row: 0, column: 0 },
            Position { row: 1, column: 4 }
        )
    );
    let rectangular = Selection {
        anchor: Some(Position { row: 1, column: 2 }),
        end: Some(Position { row: 0, column: 0 }),
        rectangular: true,
        ..Default::default()
    };
    assert_eq!(rectangular.text(&screen), "ab\nef");
}
