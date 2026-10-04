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

fn file_image(url: &str) -> bool {
    (!url.contains("://") || url.starts_with("file://"))
        && crate::content::images::supports(url.split(['?', '#']).next().unwrap_or(url))
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
    fn keeps_order_and_marks() {
        let doc = expand(
            super::super::parse_ranges(
                "**Before** ![First](one.png) then ![Second](two.jpg) after",
            )
            .doc,
        );
        assert_eq!(doc.blocks.len(), 5);
        let BlockKind::Paragraph(text) = &doc.blocks[0].kind else {
            panic!("text expected")
        };
        assert_eq!(text.text, "Before ");
        assert_eq!(text.marks[0].range, 0..6);
        assert!(matches!(&doc.blocks[1].kind, BlockKind::Image { url, .. } if url == "one.png"));
        assert!(matches!(&doc.blocks[3].kind, BlockKind::Image { url, .. } if url == "two.jpg"));
    }

    #[test]
    fn shows_file_links() {
        let doc = expand(
            super::super::parse_ranges(
                "Done: [Download](output.png) [Website](https://example.test/photo.png)",
            )
            .doc,
        );
        assert_eq!(doc.blocks.len(), 3);
        assert!(matches!(&doc.blocks[1].kind, BlockKind::Image { url, .. } if url == "output.png"));
        let BlockKind::Paragraph(text) = &doc.blocks[2].kind else {
            panic!("text expected")
        };
        assert!(matches!(&text.marks[0].mark, Mark::Link(url) if url.starts_with("https://")));
    }
}
