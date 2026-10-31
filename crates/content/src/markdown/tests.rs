use super::doc::Container;
use super::*;
use core::prelude::v1::test;

mod parsing {
    use super::*;

    #[test]
    fn retains_formula_marks_and_partitions() {
        let source =
            "Before $\\frac{x}{2}$ and $y$.\n\n$$\n\\sum_{i=1}^{n}i\n$$\n\n```text\n$x$\n```\n";
        let parsed = with_ranges(source);
        let body = parsed.doc.blocks[0].text_at(Part::Body).unwrap();
        assert_eq!(body.text, "Before \\frac{x}{2} and y.");
        assert_eq!(
            body.marks
                .iter()
                .filter(|mark| mark.mark == Mark::Math)
                .count(),
            2
        );
        assert!(
            matches!(&parsed.doc.blocks[1].kind,BlockKind::Math(text) if text.text=="\\sum_{i=1}^{n}i")
        );
        assert!(matches!(&parsed.doc.blocks[2].kind,BlockKind::Code{code,..} if code.text=="$x$"));
        assert_eq!(
            parsed
                .block_ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<String>(),
            source
        );
    }

    #[test]
    fn keeps_mixed_container_order() {
        let doc = parse(
            "> - first\n>   > quoted\n>   >\n>   > ```rust\n>   > let value = 1;\n>   > ```\n",
        );
        assert_eq!(
            doc.blocks[0].containers,
            vec![Container::Quote(None), Container::List]
        );
        let nested = vec![
            Container::Quote(None),
            Container::List,
            Container::Quote(None),
        ];
        assert_eq!(doc.blocks[1].containers, nested);
        assert_eq!(doc.blocks[2].containers, nested);
        assert!(matches!(doc.blocks[2].kind, BlockKind::Code { .. }));
        let quote_in_list = parse("- > nested\n");
        assert_eq!(quote_in_list.blocks.len(), 1);
        assert_eq!(
            quote_in_list.blocks[0].containers,
            vec![Container::List, Container::Quote(None)]
        );
        assert!(matches!(quote_in_list.blocks[0].kind, BlockKind::Bullet(_)));
    }

    #[test]
    fn separates_numbered_lists() {
        let doc = parse("3. first\n4. next\n\n9) separate\n");
        let numbers: Vec<_> = doc
            .blocks
            .iter()
            .filter_map(|block| match block.kind {
                BlockKind::Ordered { number, .. } => Some(number),
                _ => None,
            })
            .collect();
        assert_eq!(numbers, vec![3, 4, 9]);
    }

    #[test]
    fn preserves_source_ranges() {
        for source in [
            "# Title\r\n\r\n> - **中文**  \r\n>   continuation\r\n\r\n[id]: https://example.com \"Title\"\r\n",
            "- [x] done\n  > | a | b |\n  > | - | - |\n  > | *one* | [two](url) |\n",
            "```unknown\n👩‍💻 é\n```\n\nraw <span>HTML</span>\n",
        ] {
            let parsed = with_ranges(source);
            assert_eq!(parsed.block_ranges.len(), parsed.doc.blocks.len());
            let reconstructed: String = parsed
                .block_ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect();
            assert_eq!(reconstructed, source);
        }
    }
}

mod html {
    use super::*;

    #[test]
    fn keeps_block_source() {
        let source = "Intro\n\n<div align=\"center\">\n  <img src=\"logo.png\">\n</div>\n\n<details>\n<summary>More</summary>\n\nBody\n\n</details>\n";
        let parsed = with_ranges(source);
        let blocks: Vec<_> = parsed.doc.blocks.iter().map(|block| &block.kind).collect();
        assert_eq!(blocks.len(), 5, "{blocks:?}");
        assert!(
            matches!(blocks[1], BlockKind::Html(text) if text.text == "<div align=\"center\">\n  <img src=\"logo.png\">\n</div>")
        );
        assert!(
            matches!(blocks[2], BlockKind::Html(text) if text.text == "<details>\n<summary>More</summary>")
        );
        assert!(matches!(blocks[3], BlockKind::Paragraph(text) if text.text == "Body"));
        assert!(matches!(blocks[4], BlockKind::Html(text) if text.text == "</details>"));
        assert_eq!(
            parsed.doc.blocks[1].parts(),
            vec![Part::Code],
            "an HTML block is edited as source"
        );
        let reconstructed: String = parsed
            .block_ranges
            .iter()
            .map(|range| &source[range.clone()])
            .collect();
        assert_eq!(reconstructed, source);
    }

    #[test]
    fn maps_inline_tags_to_marks() {
        let doc = parse(concat!(
            "Press <kbd>Ctrl</kbd>+<b>C</b>, H<sub>2</sub>O<br>x<sup>2</sup> <mark>hot</mark> ",
            "<u>u</u> <span style=\"color: red\">red</span> <a href=\"https://example.com\">site</a> ",
            "<x-tag>keep</x-tag>",
        ));
        let text = doc.blocks[0].text_at(Part::Body).unwrap();
        assert_eq!(
            text.text,
            "Press Ctrl+C, H2O\nx2 hot u red site <x-tag>keep</x-tag>"
        );
        let marked = |mark: Mark| {
            text.marks
                .iter()
                .find(|span| span.mark == mark)
                .map(|span| &text.text[span.range.clone()])
        };
        assert_eq!(marked(Mark::Key), Some("Ctrl"));
        assert_eq!(marked(Mark::Bold), Some("C"));
        assert_eq!(marked(Mark::Subscript), Some("2"));
        assert_eq!(marked(Mark::Superscript), Some("2"));
        assert_eq!(marked(Mark::Highlight), Some("hot"));
        assert_eq!(marked(Mark::Underline), Some("u"));
        assert_eq!(marked(Mark::Color("red".into())), Some("red"));
        assert_eq!(
            marked(Mark::Link("https://example.com".into())),
            Some("site")
        );
        assert_eq!(text.marks.len(), 8);
    }

    #[test]
    fn extends_unclosed_tags_and_keeps_stray_ones() {
        let open = parse("a <u>b c");
        let text = open.blocks[0].text_at(Part::Body).unwrap();
        assert_eq!(text.text, "a b c");
        assert_eq!(text.marks[0].range, 2..5);
        let stray = parse("a </u> b");
        let text = stray.blocks[0].text_at(Part::Body).unwrap();
        assert_eq!(text.text, "a </u> b");
        assert!(text.marks.is_empty());
    }
}

mod selection {
    use super::*;

    #[test]
    fn never_splits_extended_graphemes() {
        let source = "A é 👩‍💻 🇨🇳 Z";
        let doc = parse(source);
        let emoji = source.find('👩').unwrap();
        let start = Cursor::new(0, Part::Body, emoji);
        assert_eq!(start.right(&doc).offset, emoji + "👩‍💻".len());
        assert_eq!(start.right(&doc).left(&doc), start);
        assert_eq!(Cursor::new(0, Part::Body, emoji + 5).clamp(&doc), start);
        let combining = source.find('e').unwrap();
        assert_eq!(
            Cursor::new(0, Part::Body, combining + 1).clamp(&doc).offset,
            combining
        );
    }

    #[test]
    fn copies_structured_blocks() {
        let doc = parse("| A | B |\n| - | - |\n| 中文 | 👩‍💻 |\n\n```text\nx\ny\n```\n");
        assert_eq!(
            selectable::copied(&doc, Selection::all(&doc)),
            "A\nB\n中文\n👩‍💻\nx\ny"
        );
        let empty = Doc::default();
        assert_eq!(selectable::copied(&empty, Selection::all(&empty)), "");
    }
}
