use super::*;
#[test]
fn maps_unicode_and_crlf_without_splitting_scalars() {
    let text = "中文🙂 symbol\r\nnext";
    assert_eq!(
        Encoding::Utf8.symbol(text, 1, "symbol").unwrap(),
        Position::new(0, 11)
    );
    assert_eq!(
        Encoding::Utf16.symbol(text, 1, "symbol").unwrap(),
        Position::new(0, 5)
    );
    assert_eq!(
        Encoding::Utf16
            .display(text, Position::new(0, 5))
            .unwrap()
            .column,
        5
    );
    assert!(Encoding::Utf16.display(text, Position::new(0, 3)).is_none());
    assert!(Encoding::Utf8.display(text, Position::new(0, 7)).is_none());
    assert!(Encoding::Utf16.symbol("same same", 1, "same").is_err());
    assert!(Encoding::Utf16.symbol(text, 0, "symbol").is_err());
}

#[test]
fn converts_editor_positions_without_guessing_symbols() {
    let text = "中文🙂 symbol\r\nnext";
    let position = sailry_protocol::code::Position { line: 1, column: 5 };
    assert_eq!(
        Encoding::Utf8.at(text, position).unwrap(),
        Position::new(0, 11)
    );
    assert_eq!(
        Encoding::Utf16.at(text, position).unwrap(),
        Position::new(0, 5)
    );
    assert!(
        Encoding::Utf16
            .at(
                text,
                sailry_protocol::code::Position {
                    line: 1,
                    column: 99
                }
            )
            .is_err()
    );
    assert!(
        Encoding::Utf16
            .at(text, sailry_protocol::code::Position { line: 0, column: 1 })
            .is_err()
    );
}
