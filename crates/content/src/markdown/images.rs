//! Expand read-only image spans into ordered blocks while preserving surrounding text marks.
//! The editable document and original Markdown remain unchanged.
use super::doc::{Block, BlockKind, Doc, Mark, MarkSpan, Text};
use std::ops::Range;

pub(super) fn expand(doc: Doc) -> Doc {
    let mut blocks = Vec::new();
    for block in doc.blocks {
        let BlockKind::Paragraph(text) = &block.kind else {
            blocks.push(block);
            continue;
        };
        let mut images: Vec<_> = text
            .marks
            .iter()
            .filter_map(|span| {
                let url = match &span.mark {
                    Mark::Image(url) => url,
                    Mark::Link(url) if file_image(url) => url,
                    _ => return None,
                };
                Some((span.range.clone(), url.clone()))
            })
            .collect();
        images.sort_by_key(|(range, _)| range.start);
        if images.is_empty() {
            blocks.push(block);
            continue;
        }
        let mut offset = 0;
        for (range, url) in images {
            if range.start < offset {
                continue;
            }
            append(&mut blocks, &block, slice(text, offset..range.start));
            blocks.push(Block {
                kind: BlockKind::Image {
                    url,
                    alt: Text::plain(&text.text[range.clone()]),
                    width: None,
                },
                indent: block.indent,
                containers: block.containers.clone(),
            });
            offset = range.end;
        }
        append(&mut blocks, &block, slice(text, offset..text.text.len()));
    }
    Doc { blocks }
}

/// Adjacent top-level pictures share a row without changing authored containers.
pub(super) fn end(doc: &Doc, start: usize) -> usize {
    let block = &doc.blocks[start];
    // Container paths encode shape, not identity. Keep separate quote/list frames
    // intact even when consecutive blocks have the same nesting shape.
    if !matches!(block.kind, BlockKind::Image { .. }) || !block.containers.is_empty() {
        return start + 1;
    }
    start
        + 1
        + doc.blocks[start + 1..]
            .iter()
            .take_while(|next| {
                matches!(next.kind, BlockKind::Image { .. })
                    && next.indent == block.indent
                    && next.containers == block.containers
            })
            .count()
}

fn file_image(url: &str) -> bool {
    (!url.contains("://") || url.starts_with("file://"))
        && crate::media::file_format(url.split(['?', '#']).next().unwrap_or(url)).is_some()
}

fn append(blocks: &mut Vec<Block>, parent: &Block, text: Text) {
    if !text.text.trim().is_empty() {
        blocks.push(Block {
            kind: BlockKind::Paragraph(text),
            indent: parent.indent,
            containers: parent.containers.clone(),
        });
    }
}

fn slice(text: &Text, range: Range<usize>) -> Text {
    Text {
        text: text.text[range.clone()].to_owned(),
        marks: text
            .marks
            .iter()
            .filter_map(|span| {
                let start = span.range.start.max(range.start);
                let end = span.range.end.min(range.end);
                (start < end).then(|| MarkSpan {
                    range: start - range.start..end - range.start,
                    mark: span.mark.clone(),
                })
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_document_behavior() {
        for (scenario, check) in [
            (
                "keeps_order_and_marks",
                (|scenario: &'static str| {
                    let doc = expand(
                        super::super::with_ranges(
                            "**Before** ![First](one.png) then ![Second](two.jpg) after",
                        )
                        .doc,
                    );
                    assert_eq!(doc.blocks.len(), 5, "{scenario}");
                    let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
                        panic!("{scenario}: text expected")
                    };
                    assert_eq!(text.text, "Before ", "{scenario}");
                    assert_eq!(text.marks[0].range, 0..6, "{scenario}");
                    assert!(
                        matches!(&doc.blocks[1].kind, BlockKind::Image { url, .. } if url == "one.png"),
                        "{scenario}"
                    );
                    assert!(
                        matches!(&doc.blocks[3].kind, BlockKind::Image { url, .. } if url == "two.jpg"),
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "shows_file_links",
                (|scenario: &'static str| {
                    let doc = expand(
                    super::super::with_ranges(
                        "Done: [Download](output.png) [Website](https://example.test/photo.png)",
                    )
                    .doc,
                );
                    assert_eq!(doc.blocks.len(), 3, "{scenario}");
                    assert!(
                        matches!(&doc.blocks[1].kind, BlockKind::Image { url, .. } if url == "output.png"),
                        "{scenario}"
                    );
                    let BlockKind::Paragraph(text) = &doc.blocks[2].kind else {
                        panic!("{scenario}: text expected")
                    };
                    assert!(
                        matches!(&text.marks[0].mark, Mark::Link(url) if url.starts_with("https://")),
                        "{scenario}"
                    );
                }),
            ),
            (
                "groups_only_adjacent_pictures",
                (|scenario: &'static str| {
                    for (case, source, ends) in [
                        (
                            "prose boundary",
                            "![First](one.png) ![Second](two.png)\n\nBetween\n\n![Third](three.png)",
                            vec![(0, 2), (2, 3), (3, 4)],
                        ),
                        (
                            "quote boundary",
                            "> ![First](one.png)\n>\n> ![Second](two.png)\n\n![Outside](three.png)",
                            vec![(0, 1), (1, 2), (2, 3)],
                        ),
                        (
                            "separate quotes",
                            "> ![First](one.png)\n\n> ![Second](two.png)",
                            vec![(0, 1), (1, 2)],
                        ),
                    ] {
                        let doc = expand(super::super::with_ranges(source).doc);
                        for (start, expected) in ends {
                            assert_eq!(
                                end(&doc, start),
                                expected,
                                "{scenario}: {case}: group at {start}"
                            );
                        }
                        if case == "prose boundary" {
                            assert!(
                                matches!(&doc.blocks[0].kind, BlockKind::Image { alt, .. } if alt.text == "First"),
                                "{scenario}: {case}: first caption"
                            );
                            assert!(
                                matches!(&doc.blocks[1].kind, BlockKind::Image { alt, .. } if alt.text == "Second"),
                                "{scenario}: {case}: second caption"
                            );
                        }
                        if case == "separate quotes" {
                            assert_eq!(doc.blocks.len(), 2, "{scenario}: {case}: block count");
                            assert_eq!(
                                doc.blocks[0].containers, doc.blocks[1].containers,
                                "{scenario}: {case}: same container kinds"
                            );
                        }
                    }
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}
