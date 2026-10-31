//! Source-text formatting for the toolbar: delimiters, line prefixes and block
//! snippets. These edits apply to the caller's Kit buffer, so source and
//! document modes share one undo history.
use super::format::{Insert, Kind, Style};
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

/// The block a source line spells, read from its prefix.
pub(super) fn kind(source: &str, offset: usize) -> Kind {
    let start = line_start(source, offset);
    if fenced(&source[..start]) {
        return Kind::Code;
    }
    let line = &source[start..line_end(source, offset)];
    let trimmed = line.trim_start();
    if trimmed.starts_with("```") {
        return Kind::Code;
    }
    if let Some(quoted) = trimmed.strip_prefix('>') {
        let quoted = quoted.trim_start();
        if quoted.starts_with("[!") {
            return Kind::Callout;
        }
        let previous = source[..start.saturating_sub(1)]
            .rsplit('\n')
            .next()
            .unwrap_or("");
        return if previous.trim_start().starts_with("> [!") {
            Kind::Callout
        } else {
            Kind::Quote
        };
    }
    let (_, marker) = strip(trimmed);
    marker
}

/// Replace the prefixes of every line a selection touches.
pub(super) fn prefix(source: &str, range: Range<usize>, kind: Kind) -> Option<Edit> {
    let start = line_start(source, range.start);
    let end = line_end(source, range.end.max(range.start));
    let lines: Vec<&str> = source.get(start..end)?.split('\n').collect();
    let mut output = Vec::with_capacity(lines.len() + 2);
    if kind == Kind::Code {
        output.push("```".to_owned());
    }
    if kind == Kind::Callout {
        output.push("> [!NOTE]".to_owned());
    }
    for (index, line) in lines.iter().enumerate() {
        let (line, ending) = match line.strip_suffix('\r') {
            Some(line) => (line, "\r"),
            None => (*line, ""),
        };
        if line.trim_start().starts_with("```") || line.trim_start().starts_with("> [!") {
            continue;
        }
        let indent = &line[..line.len() - line.trim_start().len()];
        let (body, _) = strip(unquote(line.trim_start()));
        let marker = match kind {
            Kind::Paragraph | Kind::Code => String::new(),
            Kind::Heading(level) => format!("{} ", "#".repeat(usize::from(level.clamp(1, 6)))),
            Kind::Bullet => "- ".to_owned(),
            Kind::Ordered => format!("{}. ", index + 1),
            Kind::Task => "- [ ] ".to_owned(),
            Kind::Quote | Kind::Callout => "> ".to_owned(),
        };
        output.push(format!("{indent}{marker}{body}{ending}"));
    }
    if kind == Kind::Code {
        output.push("```".to_owned());
    }
    let text = output.join("\n");
    let caret = start + text.len();
    Some(Edit {
        range: start..end,
        text,
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

pub(super) fn line_start(source: &str, offset: usize) -> usize {
    let offset = floor(source, offset);
    source[..offset].rfind('\n').map_or(0, |at| at + 1)
}

pub(super) fn line_end(source: &str, offset: usize) -> usize {
    let offset = floor(source, offset);
    source[offset..]
        .find('\n')
        .map_or(source.len(), |at| offset + at)
}

/// Whether an inline delimiter surrounds `offset` on its line.
pub(super) fn styled(source: &str, offset: usize, style: Style) -> bool {
    let offset = floor(source, offset);
    let start = line_start(source, offset);
    let line = &source[start..line_end(source, offset)];
    let at = offset - start;
    // Single-star italics must not count the stars of bold delimiters.
    let line = if style == Style::Italic {
        line.replace("**", "\u{1}\u{1}")
    } else {
        line.to_owned()
    };
    let mark = style.delimiter();
    let before = line[..at].matches(mark).count();
    before % 2 == 1 && line[at..].contains(mark)
}

fn floor(source: &str, offset: usize) -> usize {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

fn fenced(before: &str) -> bool {
    before
        .lines()
        .filter(|line| line.trim_start().starts_with("```"))
        .count()
        % 2
        == 1
}

fn unquote(line: &str) -> &str {
    let mut line = line;
    while let Some(rest) = line.strip_prefix('>') {
        line = rest.strip_prefix(' ').unwrap_or(rest);
    }
    line
}

/// Split a block prefix from its text.
fn strip(line: &str) -> (&str, Kind) {
    for (prefix, kind) in [
        ("- [ ] ", Kind::Task),
        ("- [x] ", Kind::Task),
        ("- [X] ", Kind::Task),
        ("###### ", Kind::Heading(6)),
        ("##### ", Kind::Heading(5)),
        ("#### ", Kind::Heading(4)),
        ("### ", Kind::Heading(3)),
        ("## ", Kind::Heading(2)),
        ("# ", Kind::Heading(1)),
        ("- ", Kind::Bullet),
        ("* ", Kind::Bullet),
        ("+ ", Kind::Bullet),
    ] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return (rest, kind);
        }
    }
    let digits = line.bytes().take_while(u8::is_ascii_digit).count();
    if digits > 0
        && let Some(rest) = line[digits..]
            .strip_prefix(". ")
            .or_else(|| line[digits..].strip_prefix(") "))
    {
        return (rest, Kind::Ordered);
    }
    (line, Kind::Paragraph)
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

    mod prefix {
        use super::*;

        #[test]
        fn sets_heading_on_current_line() {
            let edit = prefix("one\ntwo", 5..5, Kind::Heading(2)).unwrap();
            assert_eq!(edit.range, 4..7);
            assert_eq!(edit.text, "## two");
        }

        #[test]
        fn replaces_existing_markers() {
            let edit = prefix("- a\n- b", 0..7, Kind::Ordered).unwrap();
            assert_eq!(edit.text, "1. a\n2. b");
            let plain = prefix("## title", 3..3, Kind::Paragraph).unwrap();
            assert_eq!(plain.text, "title");
        }

        #[test]
        fn keeps_indentation_and_quotes_lines() {
            let edit = prefix("  - [ ] item", 4..4, Kind::Quote).unwrap();
            assert_eq!(edit.text, "  > item");
        }

        #[test]
        fn fences_and_callouts_lines() {
            assert_eq!(
                prefix("a\nb", 0..3, Kind::Code).unwrap().text,
                "```\na\nb\n```"
            );
            assert_eq!(
                prefix("note", 0..0, Kind::Callout).unwrap().text,
                "> [!NOTE]\n> note"
            );
        }
    }

    mod kind {
        use super::*;

        #[test]
        fn reads_line_prefixes() {
            assert_eq!(kind("# a", 1), Kind::Heading(1));
            assert_eq!(kind("x\n- [ ] a", 4), Kind::Task);
            assert_eq!(kind("3. a", 3), Kind::Ordered);
            assert_eq!(kind("> a", 2), Kind::Quote);
            assert_eq!(kind("> [!TIP]\n> a", 11), Kind::Callout);
            assert_eq!(kind("plain", 2), Kind::Paragraph);
        }

        #[test]
        fn detects_fenced_code() {
            assert_eq!(kind("```\nlet a = 1;\n```", 6), Kind::Code);
        }
    }

    #[test]
    fn styled_detects_enclosing_delimiters() {
        assert!(styled("a **bold** b", 6, Style::Bold));
        assert!(!styled("a **bold** b", 11, Style::Bold));
        assert!(styled("a *it* b", 4, Style::Italic));
        assert!(!styled("a **bold** b", 6, Style::Italic));
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
