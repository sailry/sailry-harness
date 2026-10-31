use super::*;

mod inline_tags {
    use super::*;

    #[test]
    fn maps_styles_links_and_breaks() {
        let mut tags = Tags::default();
        assert_eq!(tags.accept("<kbd>"), Some(Role::Open(Mark::Key)));
        assert_eq!(tags.accept("<B>"), Some(Role::Open(Mark::Bold)));
        assert_eq!(tags.accept("</b>"), Some(Role::Close(1)));
        assert_eq!(tags.accept("</kbd>"), Some(Role::Close(0)));
        for raw in ["<br>", "<br/>", "<BR />"] {
            assert_eq!(tags.accept(raw), Some(Role::Break), "{raw}");
        }
        assert_eq!(
            tags.accept(r#"<a href="https://example.com/?a=1&amp;b=2" title="x">"#),
            Some(Role::Open(Mark::Link(
                "https://example.com/?a=1&b=2".into()
            )))
        );
        assert_eq!(
            tags.accept(r#"<span style="color: #c00;">"#),
            Some(Role::Open(Mark::Color("#c00".into())))
        );
    }

    #[test]
    fn keeps_unknown_and_unpaired_literal() {
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
            assert_eq!(tags.accept(raw), None, "{raw}");
        }
    }

    #[test]
    fn closes_the_matching_tag() {
        let mut tags = Tags::default();
        tags.accept("<u>");
        tags.accept("<mark>");
        assert_eq!(tags.accept("</u>"), Some(Role::Close(0)));
        assert_eq!(tags.accept("</u>"), None);
        assert_eq!(tags.accept("</mark>"), Some(Role::Close(0)));
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
    fn removes_scripts_and_handlers() {
        let clean = sanitize(
            r#"<div onclick="run()" onmouseover=run()>Hi<script>alert(1)</script><img src=x onerror="run()"></div>"#,
        );
        assert!(clean.contains("Hi"), "{clean}");
        for needle in [
            "script",
            "alert",
            "onclick",
            "onmouseover",
            "onerror",
            "run()",
        ] {
            assert!(!clean.contains(needle), "{needle}: {clean}");
        }
    }

    #[test]
    fn removes_script_urls() {
        let clean = sanitize(concat!(
            r#"<a href="javascript:alert(1)">a</a><img src="javascript:alert(1)">"#,
            r#"<div style="color: red" title="javascript:x">b</div>"#,
        ));
        assert!(
            !clean.to_ascii_lowercase().contains("javascript"),
            "{clean}"
        );
        assert!(clean.contains("color"), "{clean}");
    }

    #[test]
    fn blocks_external_resources() {
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
            assert!(!clean.contains(needle), "{needle}: {clean}");
        }
        assert!(
            clean.contains(r#"src="data:image/png;base64,AAAA""#),
            "{clean}"
        );
    }

    #[test]
    fn keeps_structure_and_inline_styles() {
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
            assert!(clean.contains(needle), "{needle}: {clean}");
        }
        assert!(!clean.contains("position"), "{clean}");
    }

    #[test]
    fn hides_only_comments() {
        assert!(hidden("<!-- a -->\n<!-- b -->"));
        assert!(!hidden("<!-- a --><b>x</b>"));
        assert!(!hidden("<!-- open"));
        assert!(sanitize("<!-- a -->").trim().is_empty());
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
