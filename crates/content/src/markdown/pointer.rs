//! Pointer selection adapted from Bezel 4a7505ab (MIT).
//! See third_party_licenses/bezel.md.
use super::{Cursor, Doc, Selection};
use unicode_segmentation::UnicodeSegmentation;

pub(crate) fn word(doc: &Doc, cursor: Cursor) -> Selection {
    let Some(text) = doc
        .blocks
        .get(cursor.block)
        .and_then(|block| block.text_at(cursor.part))
    else {
        return Selection::at(cursor);
    };
    let range = text
        .text
        .split_word_bound_indices()
        .find_map(|(start, word)| {
            let end = start + word.len();
            (start <= cursor.offset && cursor.offset < end).then_some(start..end)
        })
        .unwrap_or(cursor.offset..cursor.offset);
    Selection::new(
        Cursor {
            offset: range.start,
            ..cursor
        },
        Cursor {
            offset: range.end,
            ..cursor
        },
    )
    .clamp(doc)
}

pub(crate) fn edge_delta(pointer: f32, top: f32, bottom: f32) -> f32 {
    let margin = 24_f32.min((bottom - top) / 2.);
    if pointer < top + margin {
        -((top + margin - pointer) * 0.5).min(24.)
    } else if pointer > bottom - margin {
        ((pointer - bottom + margin) * 0.5).min(24.)
    } else {
        0.
    }
}
