//! Source-text formatting for the toolbar: delimiters and block snippets. These edits apply to the caller's Kit buffer, so source and
//! document modes share one undo history.
use super::format::{Insert, Style};
use std::ops::Range;

/// One replacement in source bytes and the selection that follows it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Edit {
    pub range: Range<usize>,
    pub text: String,
    pub selection: Range<usize>,
}

/// Wrap a selection in a delimiter, or remove the delimiter around it.
pub(super) fn wrap(source: &str, range: Range<usize>, style: Style) -> Option<Edit> {
    let mark = style.delimiter();
    let width = mark.len();
    let selected = source.get(range.clone())?;
    if selected.len() >= 2 * width && selected.starts_with(mark) && selected.ends_with(mark) {
        let inner = &selected[width..selected.len() - width];
        return Some(Edit {
            range: range.clone(),
            text: inner.to_owned(),
            selection: range.start..range.start + inner.len(),
        });
    }
    if range.start >= width
        && source.get(range.start - width..range.start) == Some(mark)
        && source.get(range.end..range.end + width) == Some(mark)
    {
        return Some(Edit {
            range: range.start - width..range.end + width,
            text: selected.to_owned(),
            selection: range.start - width..range.end - width,
        });
    }
    Some(Edit {
        range: range.clone(),
        text: format!("{mark}{selected}{mark}"),
        selection: range.start + width..range.end + width,
    })
}

/// Turn a selection into link text and place the caret where the address goes.
pub(super) fn link(source: &str, range: Range<usize>) -> Option<Edit> {
    let selected = source.get(range.clone())?;
    let caret = if selected.is_empty() {
        range.start + 1
    } else {
        range.start + selected.len() + 3
    };
    Some(Edit {
        range,
        text: format!("[{selected}]()"),
        selection: caret..caret,
    })
}

/// Insert a block after the line or block ending at `at`, separated by blank lines.
pub(super) fn place(source: &str, at: usize, insert: Insert) -> Edit {
    let at = at.min(source.len());
    let (body, caret) = insert.snippet();
    let newlines = |text: &mut dyn Iterator<Item = char>| {
        text.take_while(|value| matches!(value, '\n' | '\r'))
            .filter(|value| *value == '\n')
            .count()
    };
    let before = newlines(&mut source[..at].chars().rev());
    let rest = &source[at..];
    let after = newlines(&mut rest.chars());
    let mut text = String::new();
    if at > 0 {
        text.push_str(&"\n".repeat(2usize.saturating_sub(before)));
    }
    let start = at + text.len() + caret;
    text.push_str(body);
    if !rest.trim().is_empty() {
        text.push_str(&"\n".repeat(2usize.saturating_sub(after)));
    }
    Edit {
        range: at..at,
        text,
        selection: start..start,
    }
}

pub(super) fn line_end(source: &str, offset: usize) -> usize {
    let offset = floor(source, offset);
    source[offset..]
        .find('\n')
        .map_or(source.len(), |at| offset + at)
}

fn floor(source: &str, offset: usize) -> usize {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

#[cfg(test)]
mod tests {
    use super::*;

    mod wrap {
        use super::*;

        #[test]
        fn edits_delimiters_and_selection() {
            for (case, source, range, style, expected) in [
                (
                    "add",
                    "say hello",
                    4..9,
                    Style::Bold,
                    Edit {
                        range: 4..9,
                        text: "**hello**".into(),
                        selection: 6..11,
                    },
                ),
                (
                    "surrounding",
                    "say **hello**",
                    6..11,
                    Style::Bold,
                    Edit {
                        range: 4..13,
                        text: "hello".into(),
                        selection: 4..9,
                    },
                ),
                (
                    "selected",
                    "say ~~hello~~",
                    4..13,
                    Style::Strike,
                    Edit {
                        range: 4..13,
                        text: "hello".into(),
                        selection: 4..9,
                    },
                ),
                (
                    "empty",
                    "ab",
                    1..1,
                    Style::Code,
                    Edit {
                        range: 1..1,
                        text: "``".into(),
                        selection: 2..2,
                    },
                ),
            ] {
                assert_eq!(wrap(source, range, style).unwrap(), expected, "{case}");
            }
        }
    }

    #[test]
    fn link_wraps_text_and_targets_address() {
        let edit = link("see docs", 4..8).unwrap();
        assert_eq!(edit.text, "[docs]()");
        assert_eq!(edit.selection, 11..11);
        let empty = link("", 0..0).unwrap();
        assert_eq!(empty.text, "[]()");
        assert_eq!(empty.selection, 1..1);
    }

    mod place {
        use super::*;

        #[test]
        fn preserves_document_behavior() {
            for (scenario, check) in [
                (
                    "spells_blocks_and_places_carets",
                    (|scenario: &'static str| {
                        for (case, source, at, insert, expected) in [
                            (
                                "separation",
                                "first\nnext",
                                5,
                                Insert::Rule,
                                Edit {
                                    range: 5..5,
                                    text: "\n\n---\n".into(),
                                    selection: 10..10,
                                },
                            ),
                            (
                                "code",
                                "",
                                0,
                                Insert::Code,
                                Edit {
                                    range: 0..0,
                                    text: "```\n\n```".into(),
                                    selection: 4..4,
                                },
                            ),
                            (
                                "existing blank lines",
                                "a\n\n",
                                3,
                                Insert::Math,
                                Edit {
                                    range: 3..3,
                                    text: "$$\n\n$$".into(),
                                    selection: 6..6,
                                },
                            ),
                        ] {
                            assert_eq!(place(source, at, insert), expected, "{scenario}: {case}");
                        }
                    }) as fn(&'static str),
                ),
                (
                    "inserts_three_column_table",
                    (|scenario: &'static str| {
                        let edit = place("", 0, Insert::Table);
                        let rows: Vec<_> = edit.text.lines().collect();
                        assert_eq!(rows.len(), 4, "{scenario}");
                        assert!(
                            rows.iter().all(|row| row.matches('|').count() == 4),
                            "{scenario}"
                        );
                    }),
                ),
            ] {
                eprintln!("case: {scenario}");
                check(scenario);
            }
        }
    }
}
