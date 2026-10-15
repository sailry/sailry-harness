use super::*;
use core::prelude::v1::test;

#[test]
fn references_and_commands_preserve_source_ranges() {
    let text = "/skill:example:analysis 看 @目录/文件.rs， @project 🙂";
    assert_eq!(
        tokens(text)
            .iter()
            .map(|range| &text[range.clone()])
            .collect::<Vec<_>>(),
        ["/skill:example:analysis", "@目录/文件.rs", "@project"]
    );
    for text in [
        "name@example.com /tmp/file",
        "`@code`",
        "```\n@code\n```",
        "[link](https://example.com/@name)",
        "<span title='@name'>text</span>",
    ] {
        assert!(tokens(text).is_empty(), "unexpected token in {text}");
    }
    assert_eq!(tokens("/").as_slice(), std::slice::from_ref(&(0..1)));
    assert_eq!(tokens("文字 @").as_slice(), std::slice::from_ref(&(7..8)));
}
