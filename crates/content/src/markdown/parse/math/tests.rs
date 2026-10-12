use crate::markdown::{BlockKind, Mark, Part, doc::Container, with_ranges};

#[test]
fn keeps_price_prose_and_amount_lists_literal() {
    for source in [
        "Plans cost $5 and $10 per month",
        "Pro costs $39/month, with a $100 allowance",
        "Pro $100、$200 或 $500/月",
        "Plans cost $100, $200",
        "| Plan | Price |\n| --- | --- |\n| Pro | $39/month with $100 usage |",
        "中文套餐 $39每月，对应额度 $100/月",
    ] {
        let parsed = with_ranges(source);
        assert!(
            parsed
                .doc
                .blocks
                .iter()
                .flat_map(|block| block
                    .parts()
                    .into_iter()
                    .filter_map(|part| block.text_at(part)))
                .all(|text| text.marks.iter().all(|mark| mark.mark != Mark::Math)),
            "{source}"
        );
        let rendered = parsed
            .doc
            .blocks
            .iter()
            .flat_map(|block| {
                block
                    .parts()
                    .into_iter()
                    .filter_map(|part| block.text_at(part))
            })
            .map(|text| text.text.as_str())
            .collect::<String>();
        assert_eq!(rendered.matches('$').count(), source.matches('$').count());
        assert_eq!(
            parsed
                .block_ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<String>(),
            source
        );
    }
}

#[test]
fn preserves_numeric_formulas_and_protected_prices() {
    for source in [
        "$2$3",
        "$2+3$4",
        "$2x$3",
        "$2 x $3",
        "$x$",
        r"$\text{price $5}$",
        "$$2+3$$",
        r"\(2+3\)",
    ] {
        let parsed = with_ranges(source);
        assert!(
            parsed
                .doc
                .blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::Math(_)))
                || parsed
                    .doc
                    .blocks
                    .iter()
                    .flat_map(|block| block
                        .parts()
                        .into_iter()
                        .filter_map(|part| block.text_at(part)))
                    .any(|text| text.marks.iter().any(|mark| mark.mark == Mark::Math)),
            "{source}"
        );
    }
    let source = "Price $39/month with $100 allowance and $x^2$. `Cost $5 and $10` [price](https://example.com/$5/$10)";
    let parsed = with_ranges(source);
    let text = parsed.doc.blocks[0].text_at(Part::Body).unwrap();
    assert!(text.text.contains("$39/month with $100"));
    assert!(
        text.marks
            .iter()
            .any(|mark| mark.mark == Mark::Math && &text.text[mark.range.clone()] == "x^2")
    );
    assert!(
        text.marks.iter().any(
            |mark| matches!(&mark.mark, Mark::Link(url) if url == "https://example.com/$5/$10")
        )
    );
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
fn recognizes_normal_and_bayes_formulas() {
    let density =
        r"f_X(x)=\frac{1}{\sigma\sqrt{2\pi}}\exp\left(-\frac{(x-\mu)^2}{2\sigma^2}\right)";
    let bayes = r"P(A\mid B)=\frac{P(B\mid A)P(A)}{P(B)}";
    let source = format!("Normal \\({density}\\).\n\n\\[\n{bayes}\n\\]\n");
    let parsed = with_ranges(&source);
    let text = parsed.doc.blocks[0].text_at(Part::Body).unwrap();
    let mark = text
        .marks
        .iter()
        .find(|mark| mark.mark == Mark::Math)
        .unwrap();
    assert_eq!(&text.text[mark.range.clone()], density);
    assert_eq!(text.text, format!("Normal {density}."));
    assert!(matches!(&parsed.doc.blocks[1].kind, BlockKind::Math(text) if text.text == bayes));
    assert_eq!(parsed.doc.blocks.len(), 2);
}

#[test]
fn preserves_payload_and_utf8_offsets() {
    let aligned = r"\begin{aligned}a &= b \\ c &= d\end{aligned}";
    let source = format!(
        "中文 \\(\\text{{price $5 & *literal*}}\\) and $y$.\r\n\r\n\\[\r\n{aligned}\r\n\\]\r\n\r\n尾部\r\n"
    );
    let parsed = with_ranges(&source);
    let text = parsed.doc.blocks[0].text_at(Part::Body).unwrap();
    let formulas: Vec<_> = text
        .marks
        .iter()
        .filter(|mark| mark.mark == Mark::Math)
        .map(|mark| &text.text[mark.range.clone()])
        .collect();
    assert_eq!(formulas, [r"\text{price $5 & *literal*}", "y"]);
    assert!(matches!(&parsed.doc.blocks[1].kind, BlockKind::Math(text) if text.text == aligned));
    assert_eq!(parsed.block_ranges.len(), parsed.doc.blocks.len());
    assert_eq!(
        parsed
            .block_ranges
            .iter()
            .map(|range| &source[range.clone()])
            .collect::<String>(),
        source
    );
    assert!(parsed.block_ranges.iter().all(|range| source.is_char_boundary(range.start) && source.is_char_boundary(range.end)));
}

#[test]
fn keeps_code_links_and_escapes_literal() {
    let source = r"`\(code\)` and \\(escaped\\) and [\(label\)](https://example.com/\(path\)) and $\text{\(nested\)}$.

```latex
\[fenced\]
```

    \(indented\)

[ref]: https://example.com/\(reference\)
";
    let parsed = with_ranges(source);
    let text = parsed.doc.blocks[0].text_at(Part::Body).unwrap();
    let math: Vec<_> = text
        .marks
        .iter()
        .filter(|mark| mark.mark == Mark::Math)
        .map(|mark| &text.text[mark.range.clone()])
        .collect();
    assert_eq!(math, [r"\text{\(nested\)}"]);
    assert!(
        text.marks.iter().any(
            |mark| matches!(&mark.mark, Mark::Link(url) if url == "https://example.com/(path)")
        )
    );
    assert!(
        matches!(&parsed.doc.blocks[1].kind, BlockKind::Code { code, .. } if code.text == r"\[fenced\]")
    );
    assert!(
        matches!(&parsed.doc.blocks[2].kind, BlockKind::Code { code, .. } if code.text == r"\(indented\)")
    );
    assert!(
        !parsed
            .doc
            .blocks
            .iter()
            .any(|block| matches!(block.kind, BlockKind::Math(_)))
    );
}

#[test]
fn preserves_quote_and_list_containers() {
    for source in ["> \\[\n> x^2\n> \\]\n", "- \\[\n  x^2\n  \\]\n"] {
        let parsed = with_ranges(source);
        let formula = parsed
            .doc
            .blocks
            .iter()
            .find(|block| matches!(block.kind, BlockKind::Math(_)))
            .unwrap();
        assert!(matches!(&formula.kind, BlockKind::Math(text) if text.text == "x^2"));
        assert!(formula.containers.contains(if source.starts_with('>') {
            &Container::Quote(None)
        } else {
            &Container::List
        }));
        assert_eq!(
            parsed
                .block_ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<String>(),
            source
        );
    }
}

#[test]
fn keeps_unfinished_and_empty_delimiters_literal() {
    for source in [
        r"\(unfinished",
        r"\[unfinished",
        r"\(\)",
        r"\[ \]",
        "\\[\n\nx\n\\]",
        r"\(unfinished `code \)`",
        r"\[unfinished [link](https://example.com/\])",
    ] {
        let parsed = with_ranges(source);
        assert!(
            !parsed
                .doc
                .blocks
                .iter()
                .any(|block| matches!(block.kind, BlockKind::Math(_)))
        );
        assert!(
            parsed
                .doc
                .blocks
                .iter()
                .flat_map(|block| block
                    .parts()
                    .into_iter()
                    .filter_map(|part| block.text_at(part)))
                .all(|text| text.marks.iter().all(|mark| mark.mark != Mark::Math))
        );
        assert_eq!(
            parsed
                .block_ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<String>(),
            source
        );
        assert!(
            !parsed
                .doc
                .blocks
                .iter()
                .filter_map(|block| block.text_at(Part::Body))
                .any(|text| text.text.contains("xxxx"))
        );
    }
}
