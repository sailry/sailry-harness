use super::*;
use crate::content::markdown::{Mark, Selection};

fn apply(source: &str, before: &Doc, after: &Doc) -> String {
    let patch = Map::new(source, before)
        .patch(source, before, after)
        .expect("an edited document must produce a source patch");
    let mut result = source.to_owned();
    result.replace_range(patch.range, &patch.text);
    result
}

fn insert(source: &str, at: Cursor, value: &str) -> (String, Doc) {
    let before = parse(source);
    let mut after = before.clone();
    after.edit_at(at, |text| text.insert(at.offset, value));
    (apply(source, &before, &after), after)
}

#[test]
fn preserves_unchanged_bytes() {
    let source = "# Title\r\n\r\nText  \r\nnext\r\n\r\n[ref]: <notes/My File.md>\r\n";
    let document = parse(source);
    assert!(
        Map::new(source, &document)
            .patch(source, &document, &document)
            .is_none()
    );
}

#[test]
fn preserves_inline_spelling() {
    let source =
        "# Title\r\n\r\nPlain **bold** and *italic* [link][ref]\r\n\r\n[ref]: notes.md\r\n";
    let (result, expected) = insert(source, Cursor::new(1, Part::Body, 8), "中文");
    assert_eq!(result, source.replacen("**bold**", "**bo中文ld**", 1));
    assert_eq!(parse(&result), expected);
}

#[test]
fn typing_before_marks() {
    let source = "**bold** and more\r\n\r\n[ref]: notes.md\r\n";
    let (result, expected) = insert(source, Cursor::new(0, Part::Body, 0), "x");
    assert_eq!(result, format!("x{source}"));
    assert_eq!(parse(&result), expected);
}

#[test]
fn replaces_encoded_characters() {
    for source in ["A &amp; B\n\nuntouched\n", "A \\* B\n\nuntouched\n"] {
        let before = parse(source);
        let mut after = before.clone();
        after.replace(
            Selection::new(Cursor::new(0, Part::Body, 2), Cursor::new(0, Part::Body, 3)),
            Text::plain("中文"),
        );
        let result = apply(source, &before, &after);
        assert_eq!(parse(&result), after);
        assert!(result.ends_with("\n\nuntouched\n"));
    }
}

#[test]
fn isolates_cell_edits() {
    let source =
        "| Name | Value |\r\n| :--- | ---: |\r\n| one  | **two** |\r\n\r\n[ref]: notes.md\r\n";
    let (result, expected) = insert(
        source,
        Cursor::new(0, Part::Cell { row: 1, column: 1 }, 1),
        "文",
    );
    assert_eq!(result, source.replacen("**two**", "**t文wo**", 1));
    assert_eq!(parse(&result), expected);
}

#[test]
fn preserves_code_fences() {
    let source = "~~~rust\r\nlet value = 1;\r\n~~~\r\n\r\nKeep **this**\r\n";
    let (result, expected) = insert(source, Cursor::new(0, Part::Code, 4), "new_");
    assert_eq!(result, source.replacen("let value", "let new_value", 1));
    assert_eq!(parse(&result), expected);
}

#[test]
fn inserts_into_empty_fences() {
    for (source, expected) in [
        ("```\n```\n", "```\nx\n```\n"),
        ("~~~rust\r\n~~~\r\n", "~~~rust\r\nx\r\n~~~\r\n"),
        ("> ```\n> ```\n", "> ```\n> x\n> ```\n"),
        ("> ```\n>```\n", "> ```\n>x\n>```\n"),
        ("- ```\n  ```\n", "- ```\n  x\n  ```\n"),
        ("```rust\n", "```rust\nx"),
        ("> ~~~rust\n", "> ~~~rust\n> x"),
        ("- ~~~rust\n", "- ~~~rust\n  x"),
        ("```", "```\nx"),
        ("```\n\n```\n", "```\nx\n```\n"),
    ] {
        let block = parse(source)
            .blocks
            .iter()
            .position(|block| matches!(block.kind, BlockKind::Code { .. }))
            .expect("a fence must contain a code block");
        let (result, edited) = insert(source, Cursor::new(block, Part::Code, 0), "x");
        assert_eq!(result, expected, "{source:?}");
        assert_eq!(parse(&result), edited, "{source:?}");
    }
}

#[test]
fn maps_unicode_carets() {
    let source = "Text 中文\n\n```\na中b\n```\n\n| A |\n| - |\n| 中文 |\n";
    let document = parse(source);
    let map = Map::new(source, &document);
    for at in [
        Cursor::new(0, Part::Body, "Text 中".len()),
        Cursor::new(1, Part::Code, "a中".len()),
        Cursor::new(2, Part::Cell { row: 1, column: 0 }, "中".len()),
    ] {
        assert_eq!(map.cursor(map.offset(at).unwrap(), &document), at);
    }
}

#[test]
fn splits_nested_blocks() {
    let source = "> 3. **中文**tail\r\n\r\nOutside\r\n\r\n[ref]: notes.md\r\n";
    let before = parse(source);
    let mut after = before.clone();
    after.split(0, "中文".len());
    let result = apply(source, &before, &after);
    assert_eq!(parse(&result), after);
    assert!(result.ends_with("\r\n\r\nOutside\r\n\r\n[ref]: notes.md\r\n"));
    assert!(!result.replace("\r\n", "").contains('\n'));
}

#[test]
fn inserts_after_final_heading() {
    let source = "# Title";
    let before = parse(source);
    let mut after = before.clone();
    let index = after.split(0, 5);
    assert_eq!(apply(source, &before, &after), "# Title\n\n");
    after.edit_at(Cursor::new(index, Part::Body, 0), |text| {
        text.insert(0, "Next")
    });
    let result = apply(source, &before, &after);
    assert_eq!(parse(&result), after);
    assert_eq!(result, "# Title\n\nNext");
}

#[test]
fn isolates_formatting() {
    let source = "First\r\n\r\nplain text\r\n\r\n[ref]: notes.md\r\n";
    let before = parse(source);
    let mut after = before.clone();
    after.toggle_mark(
        Selection::new(Cursor::new(1, Part::Body, 0), Cursor::new(1, Part::Body, 5)),
        Mark::Bold,
    );
    let result = apply(source, &before, &after);
    assert_eq!(parse(&result), after);
    assert!(result.starts_with("First\r\n\r\n"));
    assert!(result.ends_with("\r\n\r\n[ref]: notes.md\r\n"));
}

#[test]
fn preserves_alert_kind() {
    let source = "> [!NOTE]\n> beforeafter\n\nOutside\n";
    let before = parse(source);
    let mut after = before.clone();
    after.split(0, 6);
    let result = apply(source, &before, &after);
    assert_eq!(parse(&result), after);
    assert!(result.ends_with("\n\nOutside\n"));
}

#[test]
fn removes_empty_alert_markers() {
    let source = "> [!NOTE]\n> text\n\nOutside\n";
    let before = parse(source);
    let mut after = before.clone();
    after.merge_back(Cursor::new(0, Part::Body, 0)).unwrap();
    let result = apply(source, &before, &after);
    assert_eq!(parse(&result), after);
    assert!(result.ends_with("\n\nOutside\n"));
}

#[test]
fn preserves_quoted_list_parent() {
    let source = "> - item\n\nplain\n\nOutside\n";
    let before = parse(source);
    let mut after = before.clone();
    assert!(after.indent(1));
    let result = apply(source, &before, &after);
    assert_eq!(parse(&result), after);
    assert!(result.ends_with("\n\nOutside\n"));
}

#[test]
fn maps_trailing_spaces() {
    for initial in [
        "# Original\r\n",
        "Plain **bold** and *italic* [link][ref] tail\r\n\r\n[ref]: notes.md\r\n",
    ] {
        let mut source = initial.to_owned();
        let mut doc = parse(initial);
        let suffix = " Revised 中文";
        for character in suffix.chars() {
            let mut after = doc.clone();
            let offset = after.blocks[0].text_at(Part::Body).unwrap().text.len();
            after.edit_at(Cursor::new(0, Part::Body, offset), |text| {
                text.insert(offset, &character.to_string());
            });
            source = apply(&source, &doc, &after);
            doc = after;
        }
        assert_eq!(
            source,
            initial.replacen("\r\n", &format!("{suffix}\r\n"), 1)
        );
    }
}

#[test]
fn preserves_link_titles() {
    for inline in [
        "[label](notes.md \"user title\")",
        "![caption](image.png \"user title\")",
        "[https://example.com](https://example.com \"user title\")",
    ] {
        let source = format!("plain {inline}\r\n\r\nUntouched\r\n");
        let before = parse(&source);
        let mut after = before.clone();
        after.toggle_mark(
            Selection::new(Cursor::new(0, Part::Body, 0), Cursor::new(0, Part::Body, 5)),
            Mark::Bold,
        );
        let result = apply(&source, &before, &after);
        let titles: Vec<_> = Parser::new_ext(&result, crate::content::markdown::parse::OPTIONS)
            .filter_map(|event| match event {
                Event::Start(Tag::Link { title, .. } | Tag::Image { title, .. }) => {
                    Some(title.into_string())
                }
                _ => None,
            })
            .collect();
        assert_eq!(titles, ["user title"], "{result}");
        assert_eq!(parse(&result), after);
        assert!(result.ends_with("\r\n\r\nUntouched\r\n"));
    }
}

#[test]
fn preserves_hard_breaks() {
    for marker in ["  ", "\\"] {
        let source =
            format!("plain{marker}\r\nnext [label](notes.md \"user title\")\r\n\r\nUntouched\r\n");
        let before = parse(&source);
        let mut after = before.clone();
        after.toggle_mark(
            Selection::new(Cursor::new(0, Part::Body, 0), Cursor::new(0, Part::Body, 5)),
            Mark::Bold,
        );
        let formatted = apply(&source, &before, &after);
        assert!(
            formatted.contains(&format!("**plain**{marker}\r\nnext")),
            "{formatted:?}"
        );
        assert!(formatted.contains("\"user title\""));
        assert_eq!(parse(&formatted), after);

        let mut split = before.clone();
        split.split(0, 2);
        let result = apply(&source, &before, &split);
        assert!(
            result.contains(&format!("ain{marker}\r\nnext")),
            "{result:?}"
        );
        assert!(result.contains("\"user title\""));
        assert_eq!(parse(&result), split);
        assert!(result.ends_with("\r\n\r\nUntouched\r\n"));
    }
}

#[test]
fn maps_pasted_leading_spaces() {
    let mut source = "beforeafter".to_owned();
    let mut doc = parse(&source);
    for (range, value) in [(0..6, "   "), (3..3, "x")] {
        let mut after = doc.clone();
        after.replace(
            Selection::new(
                Cursor::new(0, Part::Body, range.start),
                Cursor::new(0, Part::Body, range.end),
            ),
            Text::plain(value),
        );
        source = apply(&source, &doc, &after);
        doc = after;
    }
    assert_eq!(source, "   xafter");
}
