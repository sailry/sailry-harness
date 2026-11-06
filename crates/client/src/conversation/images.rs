//! Assistant Markdown resolves image references only within its own conversation.
use sailry_protocol::conversation::{
    Draft, Entry, Image, Page, Part,
    images::{Reference, SCHEME},
};
use std::ops::Range;

/// A reference found in assistant text and the conversation image it names.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Resolved {
    pub call: String,
    pub index: usize,
    pub image: Image,
}

/// The latest committed tool result image named by the reference.
pub fn resolve<'a>(page: &'a Page, reference: &Reference) -> Option<&'a Image> {
    find(&page.entries, reference)
}

fn find<'a>(entries: &'a [Entry], reference: &Reference) -> Option<&'a Image> {
    entries
        .iter()
        .rev()
        .flat_map(|entry| entry.parts.iter().rev())
        .find_map(|part| match part {
            Part::ToolResult {
                id: Some(id),
                images,
                ..
            } if *id == reference.call => {
                images.iter().find(|image| image.index == reference.index)
            }
            _ => None,
        })
}

/// Reference destinations in Markdown source, in text order.
pub fn spans(text: &str) -> Vec<(Range<usize>, Reference)> {
    let mut spans = Vec::new();
    let mut resume = 0;
    for (colon, _) in text.match_indices(':') {
        let Some(start) = colon.checked_sub(SCHEME.len()) else {
            continue;
        };
        if start < resume
            || !text
                .get(start..colon)
                .is_some_and(|scheme| scheme.eq_ignore_ascii_case(SCHEME))
        {
            continue;
        }
        let end = text[colon..]
            .find(|character: char| {
                character.is_whitespace()
                    || matches!(character, '(' | ')' | '<' | '>' | '[' | ']' | '"' | '\'')
            })
            .map_or(text.len(), |length| colon + length);
        if let Some(reference) = Reference::parse(&text[start..end]) {
            spans.push((start..end, reference));
        }
        resume = end;
    }
    spans
}

/// Resolvable references in committed and streaming assistant text, once each.
pub fn referenced(page: &Page, drafts: &[Draft]) -> Vec<Resolved> {
    let texts = page
        .entries
        .iter()
        .filter(|entry| entry.author != "user")
        .flat_map(|entry| &entry.parts)
        .chain(
            drafts
                .iter()
                .filter(|draft| draft.author != "user")
                .flat_map(|draft| &draft.parts),
        )
        .filter_map(|part| match part {
            Part::Text(text) => Some(text.as_str()),
            _ => None,
        });
    let mut resolved: Vec<Resolved> = Vec::new();
    for text in texts {
        for (_, reference) in spans(text) {
            if resolved
                .iter()
                .any(|item| item.call == reference.call && item.index == reference.index)
            {
                continue;
            }
            if let Some(image) = find(&page.entries, &reference) {
                resolved.push(Resolved {
                    image: image.clone(),
                    call: reference.call,
                    index: reference.index,
                });
            }
        }
    }
    resolved
}

#[cfg(test)]
mod tests;
