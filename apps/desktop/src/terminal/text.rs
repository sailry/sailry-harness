use super::{protocol, selection::Position};

pub(super) fn cells(line: &protocol::Line, columns: u16) -> Vec<(u16, u16, String)> {
    let mut cells = Vec::new();
    let mut column = 0;
    for span in &line.spans {
        while column < span.column {
            cells.push((column, column + 1, " ".into()));
            column += 1;
        }
        if span.text.is_ascii() && span.text.len() == span.columns as usize {
            for character in span.text.chars() {
                cells.push((column, column + 1, character.to_string()));
                column += 1;
            }
        } else {
            cells.push((span.column, span.column + span.columns, span.text.clone()));
            column = span.column + span.columns;
        }
    }
    while column < columns {
        cells.push((column, column + 1, " ".into()));
        column += 1;
    }
    cells
}

pub(super) fn find(screen: &protocol::Screen, query: &str) -> Vec<(Position, Position)> {
    if query.is_empty() {
        return Vec::new();
    }
    let mut text = String::new();
    let mut offsets = Vec::new();
    for (row, line) in screen.scrollback.iter().chain(&screen.rows).enumerate() {
        for (column, end, value) in cells(line, screen.columns) {
            let start = text.len();
            text.push_str(&value);
            offsets.push((
                start..text.len(),
                Position { row, column },
                Position { row, column: end },
            ));
        }
        if !line.wrapped {
            text.push('\n');
        }
    }
    text.match_indices(query)
        .filter_map(|(index, matched)| {
            let start = offsets.partition_point(|(range, _, _)| range.end <= index);
            let end = offsets
                .partition_point(|(range, _, _)| range.start < index + matched.len())
                .checked_sub(1)?;
            Some((offsets.get(start)?.1, offsets.get(end)?.2))
        })
        .collect()
}
