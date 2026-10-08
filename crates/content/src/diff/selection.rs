use std::ops::Range;
use unicode_segmentation::UnicodeSegmentation;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Cursor {
    pub row: usize,
    pub byte: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Selection {
    pub anchor: Cursor,
    pub cursor: Cursor,
}

impl Selection {
    pub fn range(self, row: usize, length: usize) -> Option<Range<usize>> {
        let (start, end) = (self.anchor.min(self.cursor), self.anchor.max(self.cursor));
        if row < start.row || row > end.row {
            return None;
        }
        let start = if row == start.row {
            start.byte.min(length)
        } else {
            0
        };
        let end = if row == end.row {
            end.byte.min(length)
        } else {
            length
        };
        (start < end).then_some(start..end)
    }
}

pub(super) fn clamp(text: &str, byte: usize) -> usize {
    if byte >= text.len() {
        return text.len();
    }
    text.grapheme_indices(true)
        .map(|(at, _)| at)
        .take_while(|at| *at <= byte)
        .last()
        .unwrap_or(0)
}

pub(super) fn word(text: &str, byte: usize) -> Range<usize> {
    let byte = clamp(text, byte);
    text.split_word_bound_indices()
        .find_map(|(start, word)| {
            let end = start + word.len();
            (byte >= start && byte < end).then_some(start..end)
        })
        .unwrap_or(byte..byte)
}
