//! Only an isolated file link opts into a card; prose, code and quotes keep Markdown semantics.
use super::doc::{Block, BlockKind, Mark};

pub(super) fn reference(block: &Block) -> Option<(&str, &str)> {
    if !block.containers.is_empty() {
        return None;
    }
    let BlockKind::Paragraph(text) = &block.kind else {
        return None;
    };
    let link = text
        .marks
        .iter()
        .find(|span| matches!(span.mark, Mark::Link(_)))?;
    if !text.text[..link.range.start].trim().is_empty()
        || !text.text[link.range.end..].trim().is_empty()
    {
        return None;
    }
    let Mark::Link(url) = &link.mark else {
        unreachable!();
    };
    Some((url, &text.text[link.range.clone()]))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_standalone_links() {
        for markdown in ["[Report](report.docx)", "[报告](<a b.pdf>)"] {
            let doc = super::super::parse_ranges(markdown).doc;
            assert!(reference(&doc.blocks[0]).is_some());
        }
        for markdown in [
            "See [Report](report.docx)",
            "`[Report](report.docx)`",
            "> [Report](report.docx)",
            "[A](a.pdf) [B](b.pdf)",
            "```html\n<a>example</a>\n```",
        ] {
            let doc = super::super::parse_ranges(markdown).doc;
            assert!(
                doc.blocks.iter().all(|block| reference(block).is_none()),
                "{markdown}"
            );
        }
    }
}
