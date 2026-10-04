use super::*;
use crate::content::markdown::{doc::QuoteKind, parse::parse};

fn body(doc: &Doc, block: usize) -> &Text {
    doc.blocks[block].text_at(Part::Body).unwrap()
}

mod text {
    use super::*;

    #[test]
    fn inherits_left_mark() {
        let mut text = body(&parse("**ab**"), 0).clone();
        text.insert(2, "中");
        assert_eq!(text.text, "ab中");
        assert_eq!(text.marks[0].range, 0..5);
        text.insert(0, "x");
        assert_eq!(text.text, "xab中");
        assert_eq!(text.marks[0].range, 1..6);
    }

    #[test]
    fn preserves_unicode_marks() {
        let mut text = body(&parse("**中😀文**"), 0).clone();
        let original = text.clone();
        let tail = text.split_off("中😀".len());
        assert_eq!(text.text, "中😀");
        assert_eq!(tail.text, "文");
        text.append(tail);
        assert_eq!(text, original);
        text.remove("中".len().."中😀".len());
        assert_eq!(text.text, "中文");
        assert_eq!(text.marks[0].range, 0..6);
    }

    #[test]
    fn toggles_partial_marks() {
        let mut text = Text::plain("abcde");
        text.toggle(0..5, Mark::Bold);
        text.toggle(1..4, Mark::Bold);
        assert_eq!(
            text.marks
                .iter()
                .map(|span| span.range.clone())
                .collect::<Vec<_>>(),
            vec![0..1, 4..5]
        );
        text.toggle(1..4, Mark::Bold);
        assert_eq!(text.marks.len(), 1);
        assert_eq!(text.marks[0].range, 0..5);
    }
}

mod containers {
    use super::*;

    #[test]
    fn splits_mixed_lists() {
        let mut doc = parse("> 3. alpha\n\n7) unrelated\n");
        let path = doc.blocks[0].containers.clone();
        let untouched = doc.blocks[1].clone();
        assert_eq!(path, vec![Container::Quote(None), Container::List]);
        doc.split(0, 2);
        assert_eq!(body(&doc, 0).text, "al");
        assert_eq!(body(&doc, 1).text, "pha");
        assert_eq!(doc.blocks[0].containers, path);
        assert_eq!(doc.blocks[1].containers, path);
        assert!(matches!(
            doc.blocks[1].kind,
            BlockKind::Ordered { number: 4, .. }
        ));
        assert_eq!(doc.blocks[2], untouched);
    }

    #[test]
    fn preserves_quote_order() {
        let mut doc = parse("> - first\n> - second\n");
        let path = doc.blocks[1].containers.clone();
        assert!(doc.indent(1));
        assert_eq!(
            doc.blocks[1].containers,
            vec![Container::Quote(None), Container::List, Container::List]
        );
        assert_eq!(doc.blocks[1].indent, 1);
        assert!(doc.outdent(1));
        assert_eq!(doc.blocks[1].containers, path);
        assert_eq!(doc.blocks[1].indent, 0);
    }

    #[test]
    fn indents_quoted_descendants() {
        let mut doc = parse("> - parent\n\n- child\n  - descendant\n");
        assert_eq!(doc.blocks.len(), 3);
        assert!(doc.indent(1));
        assert_eq!(
            doc.blocks[1].containers,
            vec![Container::Quote(None), Container::List, Container::List]
        );
        assert_eq!(
            doc.blocks[2].containers,
            vec![
                Container::Quote(None),
                Container::List,
                Container::List,
                Container::List
            ]
        );
        assert_eq!(doc.blocks[1].indent, 1);
        assert_eq!(doc.blocks[2].indent, 2);

        let mut doc = parse("> - item\n\nplain");
        assert!(doc.indent(1));
        assert_eq!(
            doc.blocks[1].containers,
            vec![Container::Quote(None), Container::List]
        );
        assert_eq!(doc.blocks[1].indent, 1);
    }

    #[test]
    fn changes_list_kind() {
        let mut doc = parse("> - value\n");
        doc.set_kind(0, BlockKind::Paragraph(Text::default()));
        assert_eq!(doc.blocks[0].containers, vec![Container::Quote(None)]);
        assert_eq!(body(&doc, 0).text, "value");
        doc.set_kind(
            0,
            BlockKind::Task {
                checked: false,
                text: Text::default(),
            },
        );
        assert_eq!(
            doc.blocks[0].containers,
            vec![Container::Quote(None), Container::List]
        );
    }

    #[test]
    fn splits_inner_quotes() {
        let mut doc = parse("> > hello\n");
        assert_eq!(
            doc.blocks[0].containers,
            vec![Container::Quote(None), Container::Quote(None)]
        );
        doc.split(0, 5);
        assert_eq!(doc.blocks[0].containers.len(), 2);
        assert_eq!(doc.blocks[1].containers, vec![Container::Quote(None)]);
    }

    #[test]
    fn leaves_alerts() {
        let mut doc = parse("> [!NOTE]\n> text\n");
        assert_eq!(
            doc.blocks[0].containers,
            vec![Container::Quote(Some(QuoteKind::Note))]
        );
        assert_eq!(doc.merge_back(Cursor::default()), Some(Cursor::default()));
        assert!(doc.blocks[0].containers.is_empty());
        assert_eq!(body(&doc, 0).text, "text");
    }

    #[test]
    fn preserves_replacement_context() {
        let mut doc = parse("> - first\n> - second\n\n9) untouched\n");
        let untouched = doc.blocks[2].clone();
        let path = doc.blocks[0].containers.clone();
        let splice = doc.replace(
            Selection::new(Cursor::new(0, Part::Body, 2), Cursor::new(1, Part::Body, 3)),
            Text::plain("X"),
        );
        assert_eq!(splice.blocks, -1);
        assert_eq!(splice.caret, Cursor::new(0, Part::Body, 3));
        assert_eq!(body(&doc, 0).text, "fiXond");
        assert_eq!(doc.blocks[0].containers, path);
        assert_eq!(doc.blocks[1], untouched);
    }

    #[test]
    fn pastes_nested_blocks() {
        let mut doc = parse("before after");
        doc.splice(
            Selection::at(Cursor::new(0, Part::Body, 7)),
            parse("> quoted"),
        );
        assert_eq!(doc.blocks[1].containers, vec![Container::Quote(None)]);
        assert_eq!(body(&doc, 1).text, "quoted");

        let mut doc = parse("> - first");
        doc.splice(
            Selection::at(Cursor::new(0, Part::Body, 5)),
            parse("- pasted"),
        );
        assert_eq!(
            doc.blocks[1].containers,
            vec![Container::Quote(None), Container::List]
        );
        assert_eq!(doc.blocks[1].indent, 0);
    }
}

mod selections {
    use super::*;

    #[test]
    fn replaces_across_grapheme_seams() {
        for (source, expected) in [("a\n\ńb", "aX́b"), ("👩\n\n‍💻", "👩X‍💻")] {
            let mut doc = parse(source);
            let start = Cursor::new(0, Part::Body, 0).end(&doc);
            doc.replace(
                Selection::new(start, Cursor::new(1, Part::Body, 0)),
                Text::plain("X"),
            );
            assert_eq!(doc.blocks.len(), 1);
            assert_eq!(body(&doc, 0).text, expected);
        }
    }

    #[test]
    fn replaces_across_cells() {
        let mut doc = parse("| a | b |\n| - | - |\n| c | d |\n");
        let splice = doc.replace(
            Selection::new(
                Cursor::new(0, Part::Cell { row: 0, column: 0 }, 1),
                Cursor::new(0, Part::Cell { row: 1, column: 1 }, 0),
            ),
            Text::plain("X"),
        );
        assert_eq!(splice.blocks, 0);
        let BlockKind::Table { header, rows, .. } = &doc.blocks[0].kind else {
            panic!("replacement must preserve the table");
        };
        assert_eq!(header[0].text, "aX");
        assert!(header[1].is_empty());
        assert!(rows[0][0].is_empty());
        assert_eq!(rows[0][1].text, "d");
    }

    #[test]
    fn edits_empty_document() {
        let mut doc = Doc::default();
        assert!(doc.slice(Selection::default()).blocks.is_empty());
        let splice = doc.replace(Selection::default(), Text::plain("中文"));
        assert_eq!(body(&doc, 0).text, "中文");
        assert_eq!(splice.caret.offset, "中文".len());
    }
}
