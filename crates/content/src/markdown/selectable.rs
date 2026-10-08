// Adapted from Bezel 4a7505ab, crates/markdown (MIT). See third_party_licenses/bezel.md.
//! Copy only the selected presentation text, retaining document part boundaries.
use super::{Doc, Selection};

pub fn copied(doc: &Doc, selection: Selection) -> String {
    doc.spans(selection)
        .into_iter()
        .filter_map(|(at, range)| {
            let text = &doc.blocks.get(at.block)?.text_at(at.part)?.text;
            text.get(range).map(str::to_owned)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
