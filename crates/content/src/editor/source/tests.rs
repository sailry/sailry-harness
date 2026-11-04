use super::*;
use crate::markdown::{Mark, Selection};

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
fn preserves_document_behavior() {
    for (scenario, check) in [
        (
            "preserves_unchanged_bytes",
            (|scenario: &'static str| {
                let source = "# Title\r\n\r\nText  \r\nnext\r\n\r\n[ref]: <notes/My File.md>\r\n";
                let document = parse(source);
                assert!(
                    Map::new(source, &document)
                        .patch(source, &document, &document)
                        .is_none(),
                    "{scenario}"
                );
            }) as fn(&'static str),
        ),
        (
            "preserves_formula_delimiters_and_unrelated_bytes",
            (|scenario: &'static str| {
                for (source, part, offset, formula) in [
                    (
                        "Before $x^2$ after\r\n\r\nKeep **this**\r\n",
                        Part::Body,
                        8,
                        "$xy^2$",
                    ),
                    (
                        "$$\r\nx^2\r\n$$\r\n\r\nKeep **this**\r\n",
                        Part::Code,
                        1,
                        "$$\r\nxy^2\r\n$$",
                    ),
                    (
                        "Before \\(x^2\\) after\r\n\r\nKeep **this**\r\n",
                        Part::Body,
                        8,
                        r"\(xy^2\)",
                    ),
                    (
                        "\\[\r\nx^2\r\n\\]\r\n\r\nKeep **this**\r\n",
                        Part::Code,
                        1,
                        "\\[\r\nxy^2\r\n\\]",
                    ),
                ] {
                    let (result, expected) = insert(source, Cursor::new(0, part, offset), "y");
                    assert_eq!(parse(&result), expected, "{scenario}");
                    assert!(result.contains(formula), "{scenario}");
                    assert!(result.ends_with("\r\n\r\nKeep **this**\r\n"), "{scenario}");
                }
            }),
        ),
        (
            "preserves_spelling_during_insertions",
            (|scenario: &'static str| {
                for (case, source, cursor, inserted, original, replacement, unchanged) in [
                    (
                        "inline markup",
                        "# Title\r\n\r\nPlain **bold** and *italic* [link][ref]\r\n\r\n[ref]: notes.md\r\n",
                        Cursor::new(1, Part::Body, 8),
                        "中文",
                        "**bold**",
                        "**bo中文ld**",
                        false,
                    ),
                    (
                        "before marks",
                        "**bold** and more\r\n\r\n[ref]: notes.md\r\n",
                        Cursor::new(0, Part::Body, 0),
                        "x",
                        "**bold**",
                        "x**bold**",
                        false,
                    ),
                    (
                        "table cell",
                        "| Name | Value |\r\n| :--- | ---: |\r\n| one  | **two** |\r\n\r\n[ref]: notes.md\r\n",
                        Cursor::new(0, Part::Cell { row: 1, column: 1 }, 1),
                        "文",
                        "**two**",
                        "**t文wo**",
                        false,
                    ),
                    (
                        "inline HTML",
                        "Press <kbd>Ctrl</kbd> now<br>next\n\nKeep **this**\n",
                        Cursor::new(0, Part::Body, 8),
                        "X",
                        "Ctrl",
                        "CtXrl",
                        false,
                    ),
                    (
                        "HTML block",
                        "<div align=\"center\">\nHello\n</div>\n\nKeep **this**\n",
                        Cursor::new(0, Part::Code, "<div align=\"center\">\n".len()),
                        "X",
                        "Hello",
                        "XHello",
                        true,
                    ),
                    (
                        "code fence",
                        "~~~rust\r\nlet value = 1;\r\n~~~\r\n\r\nKeep **this**\r\n",
                        Cursor::new(0, Part::Code, 4),
                        "new_",
                        "let value",
                        "let new_value",
                        false,
                    ),
                ] {
                    if unchanged {
                        let document = parse(source);
                        assert!(
                            Map::new(source, &document)
                                .patch(source, &document, &document)
                                .is_none(),
                            "{scenario}: {case}: unchanged source"
                        );
                    }
                    let (result, expected) = insert(source, cursor, inserted);
                    assert_eq!(
                        result,
                        source.replacen(original, replacement, 1),
                        "{scenario}: {case}: source bytes"
                    );
                    assert_eq!(
                        parse(&result),
                        expected,
                        "{scenario}: {case}: document meaning"
                    );
                }
            }),
        ),
        (
            "replaces_encoded_characters",
            (|scenario: &'static str| {
                for source in ["A &amp; B\n\nuntouched\n", "A \\* B\n\nuntouched\n"] {
                    let before = parse(source);
                    let mut after = before.clone();
                    after.replace(
                        Selection::new(
                            Cursor::new(0, Part::Body, 2),
                            Cursor::new(0, Part::Body, 3),
                        ),
                        Text::plain("中文"),
                    );
                    let result = apply(source, &before, &after);
                    assert_eq!(parse(&result), after, "{scenario}");
                    assert!(result.ends_with("\n\nuntouched\n"), "{scenario}");
                }
            }),
        ),
        (
            "inserts_into_empty_fences",
            (|scenario: &'static str| {
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
                    assert_eq!(result, expected, "{scenario}: {source:?}");
                    assert_eq!(parse(&result), edited, "{scenario}: {source:?}");
                }
            }),
        ),
        (
            "maps_unicode_carets",
            (|scenario: &'static str| {
                let source = "Text 中文\n\n```\na中b\n```\n\n| A |\n| - |\n| 中文 |\n";
                let document = parse(source);
                let map = Map::new(source, &document);
                for at in [
                    Cursor::new(0, Part::Body, "Text 中".len()),
                    Cursor::new(1, Part::Code, "a中".len()),
                    Cursor::new(2, Part::Cell { row: 1, column: 0 }, "中".len()),
                ] {
                    assert_eq!(
                        map.cursor(map.offset(at).unwrap(), &document),
                        at,
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "confines_structural_edits",
            (|scenario: &'static str| {
                type Change = fn(&mut Doc);
                for (case, source, change, prefix, suffix, crlf) in [
                    (
                        "nested split",
                        "> 3. **中文**tail\r\n\r\nOutside\r\n\r\n[ref]: notes.md\r\n",
                        (|doc| {
                            doc.split(0, "中文".len());
                        }) as Change,
                        "",
                        "\r\n\r\nOutside\r\n\r\n[ref]: notes.md\r\n",
                        true,
                    ),
                    (
                        "formatting",
                        "First\r\n\r\nplain text\r\n\r\n[ref]: notes.md\r\n",
                        |doc| {
                            doc.toggle_mark(
                                Selection::new(
                                    Cursor::new(1, Part::Body, 0),
                                    Cursor::new(1, Part::Body, 5),
                                ),
                                Mark::Bold,
                            );
                        },
                        "First\r\n\r\n",
                        "\r\n\r\n[ref]: notes.md\r\n",
                        false,
                    ),
                    (
                        "alert split",
                        "> [!NOTE]\n> beforeafter\n\nOutside\n",
                        |doc| {
                            doc.split(0, 6);
                        },
                        "",
                        "\n\nOutside\n",
                        false,
                    ),
                    (
                        "remove alert marker",
                        "> [!NOTE]\n> text\n\nOutside\n",
                        |doc| {
                            doc.merge_back(Cursor::new(0, Part::Body, 0)).unwrap();
                        },
                        "",
                        "\n\nOutside\n",
                        false,
                    ),
                    (
                        "quoted list parent",
                        "> - item\n\nplain\n\nOutside\n",
                        |doc| {
                            assert!(doc.indent(1), "quoted list parent: indentation");
                        },
                        "",
                        "\n\nOutside\n",
                        false,
                    ),
                ] {
                    let before = parse(source);
                    let mut after = before.clone();
                    change(&mut after);
                    let result = apply(source, &before, &after);
                    assert_eq!(parse(&result), after, "{scenario}: {case}: document");
                    assert!(result.starts_with(prefix), "{scenario}: {case}: prefix");
                    assert!(result.ends_with(suffix), "{scenario}: {case}: suffix");
                    if crlf {
                        assert!(
                            !result.replace("\r\n", "").contains('\n'),
                            "{scenario}: {case}: line endings"
                        );
                    }
                }
            }),
        ),
        (
            "inserts_after_final_heading",
            (|scenario: &'static str| {
                let source = "# Title";
                let before = parse(source);
                let mut after = before.clone();
                let index = after.split(0, 5);
                assert_eq!(apply(source, &before, &after), "# Title\n\n", "{scenario}");
                after.edit_at(Cursor::new(index, Part::Body, 0), |text| {
                    text.insert(0, "Next")
                });
                let result = apply(source, &before, &after);
                assert_eq!(parse(&result), after, "{scenario}");
                assert_eq!(result, "# Title\n\nNext", "{scenario}");
            }),
        ),
        (
            "maps_trailing_spaces",
            (|scenario: &'static str| {
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
                        initial.replacen("\r\n", &format!("{suffix}\r\n"), 1),
                        "{scenario}"
                    );
                }
            }),
        ),
        (
            "preserves_link_titles",
            (|scenario: &'static str| {
                for inline in [
                    "[label](notes.md \"user title\")",
                    "![caption](image.png \"user title\")",
                    "[https://example.com](https://example.com \"user title\")",
                ] {
                    let source = format!("plain {inline}\r\n\r\nUntouched\r\n");
                    let before = parse(&source);
                    let mut after = before.clone();
                    after.toggle_mark(
                        Selection::new(
                            Cursor::new(0, Part::Body, 0),
                            Cursor::new(0, Part::Body, 5),
                        ),
                        Mark::Bold,
                    );
                    let result = apply(&source, &before, &after);
                    let titles: Vec<_> = Parser::new_ext(&result, crate::markdown::parse::OPTIONS)
                        .filter_map(|event| match event {
                            Event::Start(Tag::Link { title, .. } | Tag::Image { title, .. }) => {
                                Some(title.into_string())
                            }
                            _ => None,
                        })
                        .collect();
                    assert_eq!(titles, ["user title"], "{scenario}: {result}");
                    assert_eq!(parse(&result), after, "{scenario}");
                    assert!(result.ends_with("\r\n\r\nUntouched\r\n"), "{scenario}");
                }
            }),
        ),
        (
            "preserves_hard_breaks",
            (|scenario: &'static str| {
                for marker in ["  ", "\\"] {
                    let source = format!(
                        "plain{marker}\r\nnext [label](notes.md \"user title\")\r\n\r\nUntouched\r\n"
                    );
                    let before = parse(&source);
                    let mut after = before.clone();
                    after.toggle_mark(
                        Selection::new(
                            Cursor::new(0, Part::Body, 0),
                            Cursor::new(0, Part::Body, 5),
                        ),
                        Mark::Bold,
                    );
                    let formatted = apply(&source, &before, &after);
                    assert!(
                        formatted.contains(&format!("**plain**{marker}\r\nnext")),
                        "{scenario}: {formatted:?}"
                    );
                    assert!(formatted.contains("\"user title\""), "{scenario}");
                    assert_eq!(parse(&formatted), after, "{scenario}");

                    let mut split = before.clone();
                    split.split(0, 2);
                    let result = apply(&source, &before, &split);
                    assert!(
                        result.contains(&format!("ain{marker}\r\nnext")),
                        "{scenario}: {result:?}"
                    );
                    assert!(result.contains("\"user title\""), "{scenario}");
                    assert_eq!(parse(&result), split, "{scenario}");
                    assert!(result.ends_with("\r\n\r\nUntouched\r\n"), "{scenario}");
                }
            }),
        ),
        (
            "maps_pasted_leading_spaces",
            (|scenario: &'static str| {
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
                assert_eq!(source, "   xafter", "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
