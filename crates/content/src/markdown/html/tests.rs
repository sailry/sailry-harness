use super::*;

mod inline_tags {
    use super::*;

    #[test]
    fn preserves_document_behavior() {
        for (scenario, check) in [
            (
                "maps_styles_links_and_breaks",
                (|scenario: &'static str| {
                    let mut tags = Tags::default();
                    assert_eq!(
                        tags.accept("<kbd>"),
                        Some(Role::Open(Mark::Key)),
                        "{scenario}"
                    );
                    assert_eq!(
                        tags.accept("<B>"),
                        Some(Role::Open(Mark::Bold)),
                        "{scenario}"
                    );
                    assert_eq!(tags.accept("</b>"), Some(Role::Close(1)), "{scenario}");
                    assert_eq!(tags.accept("</kbd>"), Some(Role::Close(0)), "{scenario}");
                    for raw in ["<br>", "<br/>", "<BR />"] {
                        assert_eq!(tags.accept(raw), Some(Role::Break), "{scenario}: {raw}");
                    }
                    assert_eq!(
                        tags.accept(r#"<a href="https://example.com/?a=1&amp;b=2" title="x">"#),
                        Some(Role::Open(Mark::Link(
                            "https://example.com/?a=1&b=2".into()
                        ))),
                        "{scenario}"
                    );
                    assert_eq!(
                        tags.accept(r#"<span style="color: #c00;">"#),
                        Some(Role::Open(Mark::Color("#c00".into()))),
                        "{scenario}"
                    );
                }) as fn(&'static str),
            ),
            (
                "keeps_unknown_and_unpaired_literal",
                (|scenario: &'static str| {
                    let mut tags = Tags::default();
                    for raw in [
                        "</u>",
                        "<span>",
                        r#"<span style="color: red; font-weight: bold">"#,
                        r#"<span style="color: nope">"#,
                        r#"<span class="x" style="color: red">"#,
                        "<a>",
                        "<div>",
                        "<u/>",
                        "</br>",
                        "<kbd",
                        r#"<b class="x>"#,
                    ] {
                        assert_eq!(tags.accept(raw), None, "{scenario}: {raw}");
                    }
                }),
            ),
            (
                "closes_the_matching_tag",
                (|scenario: &'static str| {
                    let mut tags = Tags::default();
                    tags.accept("<u>");
                    tags.accept("<mark>");
                    assert_eq!(tags.accept("</u>"), Some(Role::Close(0)), "{scenario}");
                    assert_eq!(tags.accept("</u>"), None, "{scenario}");
                    assert_eq!(tags.accept("</mark>"), Some(Role::Close(0)), "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod colors {
    use super::*;

    #[test]
    fn parses_css_forms() {
        let red: Hsla = rgb(0xff0000).into();
        for value in ["red", " RED ", "#f00", "#FF0000", "rgb(255, 0, 0)"] {
            assert_eq!(color(value), Some(red), "{value}");
        }
        assert_eq!(color("rgba(255, 0, 0, 0.5)"), Some(rgba(0xff000080).into()));
        for value in [
            "",
            "#ff000",
            "rgb(256, 0, 0)",
            "rgba(1, 2, 3, 2)",
            "url(x)",
            "var(--x)",
        ] {
            assert_eq!(color(value), None, "{value}");
        }
    }
}

mod sanitizer {
    use super::*;

    #[test]
    fn preserves_document_behavior() {
        for (scenario, check) in [
            (
                "removes_scripts_and_handlers",
                (|scenario: &'static str| {
                    let clean = sanitize(
                        r#"<div onclick="run()" onmouseover=run()>Hi<script>alert(1)</script><img src=x onerror="run()"></div>"#,
                    );
                    assert!(clean.contains("Hi"), "{scenario}: {clean}");
                    for needle in [
                        "script",
                        "alert",
                        "onclick",
                        "onmouseover",
                        "onerror",
                        "run()",
                    ] {
                        assert!(!clean.contains(needle), "{scenario}: {needle}: {clean}");
                    }
                }) as fn(&'static str),
            ),
            (
                "removes_script_urls",
                (|scenario: &'static str| {
                    let clean = sanitize(concat!(
                        r#"<a href="javascript:alert(1)">a</a><img src="javascript:alert(1)">"#,
                        r#"<div style="color: red" title="javascript:x">b</div>"#,
                    ));
                    assert!(
                        !clean.to_ascii_lowercase().contains("javascript"),
                        "{scenario}: {clean}"
                    );
                    assert!(clean.contains("color"), "{scenario}: {clean}");
                }),
            ),
            (
                "blocks_external_resources",
                (|scenario: &'static str| {
                    let clean = sanitize(concat!(
                        r#"<img src="https://example.com/a.png"><img src="/local.png">"#,
                        r#"<p style="background: url(https://example.com/b.png)">p</p>"#,
                        r#"<table background="https://example.com/c.png"><tr><td>c</td></tr></table>"#,
                        r#"<iframe src="https://example.com"></iframe><object data="x"></object><embed src="x">"#,
                        r#"<link rel="stylesheet" href="https://example.com/s.css"><style>@import url(x)</style>"#,
                        r#"<img srcset="https://example.com/d.png 2x" src="data:image/png;base64,AAAA">"#,
                        r#"<img src="data:image/svg+xml;base64,AAAA">"#,
                    ));
                    for needle in [
                        "example.com",
                        "iframe",
                        "object",
                        "embed",
                        "<link",
                        "<style",
                        "@import",
                        "local.png",
                        "srcset",
                        "svg",
                        "background",
                    ] {
                        assert!(!clean.contains(needle), "{scenario}: {needle}: {clean}");
                    }
                    assert!(
                        clean.contains(r#"src="data:image/png;base64,AAAA""#),
                        "{scenario}: {clean}"
                    );
                }),
            ),
            (
                "keeps_structure_and_inline_styles",
                (|scenario: &'static str| {
                    let clean = sanitize(concat!(
                        r#"<details><summary>More</summary><center><b>Bold</b></center>"#,
                        r#"<table><tr><td colspan="2" align="right" style="color: #333; position: fixed">x</td></tr></table></details>"#,
                    ));
                    for needle in [
                        "<details>",
                        "<summary>More</summary>",
                        "<center><b>Bold</b></center>",
                        r#"colspan="2""#,
                        r#"align="right""#,
                        "color",
                    ] {
                        assert!(clean.contains(needle), "{scenario}: {needle}: {clean}");
                    }
                    assert!(!clean.contains("position"), "{scenario}: {clean}");
                }),
            ),
            (
                "hides_only_comments",
                (|scenario: &'static str| {
                    assert!(hidden("<!-- a -->\n<!-- b -->"), "{scenario}");
                    assert!(!hidden("<!-- a --><b>x</b>"), "{scenario}");
                    assert!(!hidden("<!-- open"), "{scenario}");
                    assert!(sanitize("<!-- a -->").trim().is_empty(), "{scenario}");
                }),
            ),
        ] {
            eprintln!("case: {scenario}");
            check(scenario);
        }
    }
}

mod readme_layout {
    use super::super::layout::{Align, Item, Layout, Picture, layout};
    use super::*;

    fn pictures(layout: &Layout) -> Vec<&Picture> {
        layout
            .flows
            .iter()
            .flat_map(|flow| &flow.items)
            .filter_map(|item| match item {
                Item::Picture(picture) => Some(picture),
                Item::Text(_) => None,
            })
            .collect()
    }

    fn texts(layout: &Layout) -> Vec<&str> {
        layout
            .flows
            .iter()
            .flat_map(|flow| &flow.items)
            .filter_map(|item| match item {
                Item::Text(text) => Some(text.text.as_str()),
                Item::Picture(_) => None,
            })
            .collect()
    }

    #[test]
    fn centers_a_sized_local_logo() {
        let result = layout(
            "<p align=\"center\">\n  <img src=\"assets/branding/mark.svg\" alt=\"Sailry\" width=\"88\" height=\"40px\" />\n</p>",
        );
        assert!(result.complete);
        assert_eq!(result.flows.len(), 1);
        assert_eq!(result.flows[0].align, Align::Center);
        assert_eq!(
            pictures(&result),
            [&Picture {
                url: "assets/branding/mark.svg".into(),
                alt: "Sailry".into(),
                width: Some(88.),
                height: Some(40.),
                link: None,
            }]
        );
    }

    #[test]
    fn keeps_badge_links_in_one_row() {
        let result = layout(
            r#"<p align="center"><a href="LICENSE"><img src="https://img.shields.io/a.svg?x=1&amp;y=2" alt="License"></a> <a href="https://example.test/"><img src="https://img.shields.io/b.svg" alt="Build"></a></p>"#,
        );
        assert!(result.complete);
        assert_eq!(result.flows.len(), 1);
        let found = pictures(&result);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].url, "https://img.shields.io/a.svg?x=1&y=2");
        assert_eq!(found[0].link.as_deref(), Some("LICENSE"));
        assert_eq!(found[1].link.as_deref(), Some("https://example.test/"));
        assert!(texts(&result).is_empty());
    }

    #[test]
    fn splits_lines_and_marks_headings() {
        let result = layout(
            "<h2 align=\"right\">Sailry   <b>Harness</b></h2>\n<center>One<br>Two &amp; <a href=\"a.md\">three</a></center>",
        );
        assert!(result.complete);
        let shape: Vec<_> = result
            .flows
            .iter()
            .map(|flow| (flow.align, flow.heading))
            .collect();
        assert_eq!(
            shape,
            [
                (Align::End, Some(2)),
                (Align::Center, None),
                (Align::Center, None),
            ]
        );
        assert_eq!(texts(&result), ["Sailry Harness", "One", "Two & three"]);
        let Item::Text(heading) = &result.flows[0].items[0] else {
            panic!("heading text expected");
        };
        assert_eq!(heading.marks[0].mark, Mark::Bold);
        assert_eq!(&heading.text[heading.marks[0].range.clone()], "Harness");
        let Item::Text(link) = &result.flows[2].items[0] else {
            panic!("link text expected");
        };
        assert_eq!(link.marks[0].mark, Mark::Link("a.md".into()));
    }

    #[test]
    fn falls_back_from_picture_sources_to_img() {
        let result = layout(
            r#"<picture><source media="(prefers-color-scheme: dark)" srcset="dark.png"><img src="light.png" alt="Logo"></picture>"#,
        );
        assert!(result.complete);
        assert_eq!(pictures(&result)[0].url, "light.png");
    }

    #[test]
    fn shows_alt_for_rejected_addresses() {
        for src in [
            "javascript:run()",
            "//example.test/a.png",
            "file:///etc/passwd.png",
            "mailto:a@example.test",
            "C:\\a.png",
        ] {
            let result = layout(&format!(r#"<p><img src="{src}" alt="Name"></p>"#));
            assert!(pictures(&result).is_empty(), "{src}");
            assert_eq!(texts(&result), ["Name"], "{src}");
        }
    }

    #[test]
    fn strips_active_content() {
        let result = layout(
            r#"<div onclick="run()"><script>alert(1)</script><iframe src="https://x.test"></iframe><style>p{}</style><a href="javascript:run()">Go</a><img src="a.png" onerror="run()"></div>"#,
        );
        assert_eq!(texts(&result), ["Go"]);
        let Item::Text(text) = &result.flows[0].items[0] else {
            panic!("text expected");
        };
        assert!(text.marks.is_empty(), "{:?}", text.marks);
        assert_eq!(pictures(&result)[0].url, "a.png");
    }

    #[test]
    fn leaves_richer_markup_to_the_picture() {
        for source in [
            "<table><tr><td>Cell</td></tr></table>",
            "<details><summary>More</summary>Body</details>",
            r#"<div style="color: red">Red</div>"#,
            r#"<p><img src="data:image/png;base64,AAAA" alt="Inline"></p>"#,
        ] {
            let result = layout(source);
            assert!(!result.complete, "{source}");
            assert!(!raster(source).is_empty(), "{source}");
            assert!(!result.flows.is_empty(), "{source}");
        }
    }

    #[test]
    fn rasters_nothing_for_native_blocks() {
        assert_eq!(raster(r#"<p align="center">Hi</p>"#), "");
    }

    #[test]
    fn accepts_style_with_only_alignment() {
        let result = layout(r#"<div style="text-align: center;">Hi</div>"#);
        assert!(result.complete);
        assert_eq!(result.flows[0].align, Align::Center);
    }

    #[test]
    fn skips_comments_and_bare_wrappers() {
        for source in ["<!-- note -->", "<div align=\"center\">", "</div>"] {
            assert!(layout(source).flows.is_empty(), "{source}");
        }
    }
}

mod spelling {
    use super::*;

    #[test]
    fn writes_tags_the_parser_reads_back() {
        for mark in [
            Mark::Underline,
            Mark::Highlight,
            Mark::Key,
            Mark::Subscript,
            Mark::Superscript,
            Mark::Color("#c00".into()),
        ] {
            let (open, close) = spelling(&mark).unwrap();
            let mut tags = Tags::default();
            assert_eq!(tags.accept(&open), Some(Role::Open(mark)));
            assert_eq!(tags.accept(close), Some(Role::Close(0)));
        }
        assert_eq!(spelling(&Mark::Bold), None);
    }
}
