use super::*;

fn svg(source: &str) -> Output {
    Output {
        source: source.into(),
        width: 24.0,
        height: 20.0,
        baseline: Some(16.0),
        format: Format::Svg,
    }
}

/// A PNG header of the given pixel size; validation reads no further.
fn png(width: u32, height: u32) -> Output {
    use base64::Engine as _;
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend(width.to_be_bytes());
    bytes.extend(height.to_be_bytes());
    bytes.extend([8, 6, 0, 0, 0]);
    Output {
        source: base64::engine::general_purpose::STANDARD.encode(bytes),
        width: 100.0,
        height: 50.0,
        baseline: None,
        format: Format::Png,
    }
}

#[test]
fn accepts_png_within_its_scale() {
    png(200, 100).validate().unwrap();
    png(100, 50).validate().unwrap();
}

#[test]
fn refuses_oversized_or_foreign_png() {
    assert!(png(4000, 100).validate().is_err());
    assert!(png(0, 100).validate().is_err());
    let mut text = png(200, 100);
    text.source = "PHN2Zz4=".into();
    assert!(text.validate().is_err());
    let mut invalid = png(200, 100);
    invalid.source.push('!');
    assert!(invalid.validate().is_err());
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

#[test]
fn prepares_verified_png_bytes() {
    let verified = png(200, 100).decode().unwrap();
    assert!(verified.bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    let image = Prepared::new(verified, 14.).unwrap();
    assert_eq!((image.width, image.height), (100., 50.));
    assert!(png(4000, 100).decode().is_err());
}
