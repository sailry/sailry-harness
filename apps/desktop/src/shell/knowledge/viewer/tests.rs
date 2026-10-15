use super::*;
use core::prelude::v1::test;

#[test]
fn presents_untrusted_context_as_literal_text() {
    let text = "```\n[private](file:///private)\n![image](https://invalid.example/pixel)\n```";
    let displayed = literal(text);
    assert!(displayed.starts_with("````\n"));
    assert!(displayed.contains(text));
    assert!(displayed.ends_with("\n````"));
    let parsed = crate::content::markdown::with_ranges(&displayed);
    assert_eq!(parsed.doc.blocks.len(), 1);
    assert!(
        matches!(&parsed.doc.blocks[0].kind, crate::content::markdown::BlockKind::Code { code, .. } if code.text == text && code.marks.is_empty())
    );
}
