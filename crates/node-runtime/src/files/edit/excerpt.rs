//! Bounded numbered regions of an edited file, so callers can verify an edit
//! without reading the file again.
use sailry_protocol::{FileExcerpt, FileHunk, FileLine};
use std::ops::Range;

/// Unchanged lines shown before and after each edited region.
const CONTEXT: usize = 3;
const MAX_HUNKS: usize = 16;
const HUNK_LINES: usize = 60;
const LINE_BYTES: usize = 1024;
const TOTAL_BYTES: usize = 12 * 1024;

/// `changes` are ascending byte ranges of replacements within `text`.
pub(super) fn collect(text: &str, changes: &[Range<usize>]) -> FileExcerpt {
    let starts: Vec<usize> = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(index, _)| index + 1))
        .filter(|start| *start < text.len())
        .collect();
    let mut excerpt = FileExcerpt::default();
    if starts.is_empty() {
        return excerpt;
    }
    let line = |position: usize| starts.partition_point(|start| *start <= position) - 1;
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for change in changes {
        let first = line(change.start.min(text.len() - 1));
        let last = if change.end > change.start {
            line(change.end - 1)
        } else {
            first
        };
        let span = (
            first.saturating_sub(CONTEXT),
            (last + CONTEXT).min(starts.len() - 1),
        );
        match spans.last_mut() {
            Some(previous) if span.0 <= previous.1 + 1 => previous.1 = previous.1.max(span.1),
            _ => spans.push(span),
        }
    }
    let mut budget = TOTAL_BYTES;
    for (index, (first, last)) in spans.iter().copied().enumerate() {
        if index == MAX_HUNKS || budget == 0 {
            excerpt.omitted = spans.len() - index;
            break;
        }
        let mut hunk = FileHunk {
            lines: Vec::new(),
            shortened: last - first + 1 > HUNK_LINES,
        };
        for number in first..=last.min(first + HUNK_LINES - 1) {
            let end = starts.get(number + 1).copied().unwrap_or(text.len());
            let full = &text[starts[number]..end];
            let mut limit = full.len().min(LINE_BYTES).min(budget);
            while !full.is_char_boundary(limit) {
                limit -= 1;
            }
            if limit == 0 && !full.is_empty() {
                hunk.shortened = true;
                break;
            }
            hunk.shortened |= limit < full.len();
            budget -= limit;
            hunk.lines.push(FileLine {
                number: number + 1,
                text: full[..limit].into(),
            });
        }
        if hunk.lines.is_empty() {
            excerpt.omitted = spans.len() - index;
            break;
        }
        excerpt.hunks.push(hunk);
    }
    excerpt
}

#[cfg(test)]
mod tests;
