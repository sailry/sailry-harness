use super::*;

fn svg(source: &str) -> Svg {
    Svg {
        source: source.into(),
        width: 24.0,
        height: 20.0,
        baseline: Some(16.0),
    }
}

#[test]
fn accepts_local_paths_and_defs() {
    svg(r##"<svg xmlns="http://www.w3.org/2000/svg"><defs><path id="x" d="M0 0L2 2"/></defs><use href="#x" fill="url(#ink)"/></svg>"##).validate().unwrap();
}

#[test]
fn refuses_external_and_active_content() {
    for content in [
        r#"<script/>"#,
        r#"<foreignObject/>"#,
        r#"<use href="https://example.com/x"/>"#,
        r#"<path onclick="run()"/>"#,
        r#"<style>@import 'https://example.com';</style>"#,
        r#"<style>path{fill:url(https://example.com)}</style>"#,
    ] {
        assert!(
            svg(&format!("<svg>{content}</svg>")).validate().is_err(),
            "{content}"
        );
    }
    let mut result = svg("<svg/>");
    result.width = f32::INFINITY;
    assert!(result.validate().is_err());
    result.width = 24.0;
    result.baseline = Some(25.0);
    assert!(result.validate().is_err());
}
