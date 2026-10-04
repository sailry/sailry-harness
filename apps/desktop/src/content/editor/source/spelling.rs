//! Preserve source-only link titles and hard-break spelling in local rewrites.
//! These annotations belong to the original bytes, not the presentation model.
use super::{Map, Patch, change};
use crate::content::markdown::{BlockKind, Doc, Mark, Part, parse::OPTIONS};
use pulldown_cmark::{Event, Parser, Tag};
use std::ops::Range;

struct Layout {
    text: String,
    parts: Vec<(usize, Part, Range<usize>)>,
}

impl Layout {
    fn new(doc: &Doc) -> Self {
        let mut text = String::new();
        let mut parts = Vec::new();
        for (index, block) in doc.blocks.iter().enumerate() {
            for part in block.parts() {
                let start = text.len();
                text.push_str(&block.text_at(part).unwrap().text);
                parts.push((index, part, start..text.len()));
                // A block/cell boundary cannot be mistaken for its own newline.
                text.push('\0');
            }
        }
        Self { text, parts }
    }

    fn global(&self, block: usize, part: Part, range: Range<usize>) -> Option<Range<usize>> {
        let (_, _, bounds) = self
            .parts
            .iter()
            .find(|(b, p, _)| *b == block && *p == part)?;
        Some(bounds.start + range.start..bounds.start + range.end)
    }

    fn local(&self, range: Range<usize>) -> Option<(usize, Part, Range<usize>)> {
        self.parts.iter().find_map(|(block, part, bounds)| {
            (bounds.start <= range.start && range.end <= bounds.end).then(|| {
                (
                    *block,
                    *part,
                    range.start - bounds.start..range.end - bounds.start,
                )
            })
        })
    }
}

fn text_range(map: &Map, raw: &Range<usize>) -> Option<(usize, Part, Range<usize>)> {
    map.leaves.iter().enumerate().find_map(|(block, leaf)| {
        leaf.parts.iter().find_map(|(part, span)| {
            let mut characters = span.characters.iter().filter(|character| {
                raw.start <= character.raw.start && character.raw.end <= raw.end
            });
            let first = characters.next()?;
            let end = characters.next_back().unwrap_or(first).text.end;
            Some((block, *part, first.text.start..end))
        })
    })
}

fn shifted(range: Range<usize>, changed: &Range<usize>, added: usize) -> Option<Range<usize>> {
    let shift = |offset: usize| offset + added - changed.len();
    if range.end <= changed.start {
        Some(range)
    } else if range.start >= changed.end {
        Some(shift(range.start)..shift(range.end))
    } else if range.start <= changed.start && changed.end <= range.end {
        Some(range.start..shift(range.end))
    } else {
        None
    }
}

fn quoted(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

pub(super) fn preserve(
    map: &Map,
    source: &str,
    before: &Doc,
    after: &Doc,
    mut patch: Patch,
) -> Patch {
    let old = Layout::new(before);
    let new = Layout::new(after);
    let (changed, added) = change(&old.text, &new.text);
    let mut candidate = source.to_owned();
    candidate.replace_range(patch.range.clone(), &patch.text);
    let mapped = Map::new(&candidate, after);
    let events: Vec<_> = Parser::new_ext(&candidate, OPTIONS)
        .into_offset_iter()
        .collect();
    let mut edits = Vec::<(Range<usize>, String)>::new();

    for (event, raw) in Parser::new_ext(source, OPTIONS).into_offset_iter() {
        let title = match &event {
            Event::Start(Tag::Link {
                dest_url, title, ..
            }) if !title.is_empty() => Some((false, dest_url.as_ref(), title.as_ref())),
            Event::Start(Tag::Image {
                dest_url, title, ..
            }) if !title.is_empty() => Some((true, dest_url.as_ref(), title.as_ref())),
            Event::HardBreak => None,
            _ => continue,
        };
        let Some((block, part, range)) = text_range(map, &raw) else {
            continue;
        };
        let Some(range) = old
            .global(block, part, range)
            .and_then(|range| shifted(range, &changed, added.len()))
        else {
            continue;
        };
        let Some((block, part, range)) = new.local(range) else {
            continue;
        };
        let Some(span) = mapped.leaves.get(block).and_then(|leaf| {
            leaf.parts
                .iter()
                .find(|(p, _)| *p == part)
                .map(|(_, span)| span)
        }) else {
            continue;
        };

        if let Some((image, url, title)) = title {
            let value = &after.blocks[block];
            let same_target = value.text_at(part).is_some_and(|text| {
                text.marks.iter().any(|mark| {
                    mark.range == range
                        && match &mark.mark {
                            Mark::Link(target) => !image && target == url,
                            Mark::Image(target) => image && target == url,
                            _ => false,
                        }
                })
            }) || matches!(&value.kind, BlockKind::Image { url: target, .. } if image && target == url);
            if !same_target {
                continue;
            }
            let node = events.iter().find_map(|(event, raw)| {
                let (node_image, target, present) = match event {
                    Event::Start(Tag::Link {
                        dest_url, title, ..
                    }) => (false, dest_url.as_ref(), title.as_ref()),
                    Event::Start(Tag::Image {
                        dest_url, title, ..
                    }) => (true, dest_url.as_ref(), title.as_ref()),
                    _ => return None,
                };
                (node_image == image
                    && target == url
                    && text_range(&mapped, raw) == Some((block, part, range.clone())))
                .then_some((raw, present))
            });
            if let Some((raw, present)) = node {
                if present.is_empty() && candidate.as_bytes().get(raw.end - 1) == Some(&b')') {
                    edits.push((raw.end - 1..raw.end - 1, format!(" \"{}\"", quoted(title))));
                }
            } else if !image && let Some(raw) = span.raw_range(range.clone()) {
                // The serializer abbreviates a label equal to its URL. A title
                // requires the explicit link form even for that label.
                let label = &candidate[raw.clone()];
                if after.blocks[block].text_at(part).unwrap().text[range] == *url {
                    let destination = url
                        .replace('\\', "\\\\")
                        .replace('<', "\\<")
                        .replace('>', "\\>");
                    edits.push((
                        raw,
                        format!("[{label}](<{destination}> \"{}\")", quoted(title)),
                    ));
                }
            }
        } else if let Some(target) = span.raw_range(range) {
            let original = &source[raw];
            let current = &candidate[target.clone()];
            if current.ends_with('\n') {
                let ending = if current.ends_with("\r\n") {
                    "\r\n"
                } else {
                    "\n"
                };
                let text = format!("{}{ending}", original.trim_end_matches(['\r', '\n']));
                if text != current {
                    edits.push((target, text));
                }
            }
        }
    }
    edits.sort_by_key(|(range, _)| std::cmp::Reverse(range.start));
    let end = patch.range.start + patch.text.len();
    for (range, text) in edits {
        if patch.range.start <= range.start && range.end <= end {
            patch.text.replace_range(
                range.start - patch.range.start..range.end - patch.range.start,
                &text,
            );
        }
    }
    patch
}
