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
