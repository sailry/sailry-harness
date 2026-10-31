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
        fn adds_delimiters_around_selection() {
            let edit = wrap("say hello", 4..9, Style::Bold).unwrap();
            assert_eq!(edit.range, 4..9);
            assert_eq!(edit.text, "**hello**");
            assert_eq!(edit.selection, 6..11);
        }

        #[test]
        fn removes_surrounding_delimiters() {
            let edit = wrap("say **hello**", 6..11, Style::Bold).unwrap();
            assert_eq!(edit.range, 4..13);
            assert_eq!(edit.text, "hello");
            assert_eq!(edit.selection, 4..9);
        }

        #[test]
        fn removes_selected_delimiters() {
            let edit = wrap("say ~~hello~~", 4..13, Style::Strike).unwrap();
            assert_eq!(edit.text, "hello");
            assert_eq!(edit.selection, 4..9);
        }

        #[test]
        fn places_caret_inside_empty_pair() {
            let edit = wrap("ab", 1..1, Style::Code).unwrap();
            assert_eq!(edit.text, "``");
            assert_eq!(edit.selection, 2..2);
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
        fn separates_block_with_blank_lines() {
            let edit = place("first\nnext", 5, Insert::Rule);
            assert_eq!(edit.range, 5..5);
            assert_eq!(edit.text, "\n\n---\n");
        }

        #[test]
        fn caret_inside_code() {
            let edit = place("", 0, Insert::Code);
            assert_eq!(edit.text, "```\n\n```");
            assert_eq!(edit.selection, 4..4);
        }

        #[test]
        fn reuses_existing_blank_lines() {
            let edit = place("a\n\n", 3, Insert::Math);
            assert_eq!(edit.text, "$$\n\n$$");
            assert_eq!(edit.selection, 6..6);
        }

        #[test]
        fn inserts_three_column_table() {
            let edit = place("", 0, Insert::Table);
            let rows: Vec<_> = edit.text.lines().collect();
            assert_eq!(rows.len(), 4);
            assert!(rows.iter().all(|row| row.matches('|').count() == 4));
        }
    }
}
