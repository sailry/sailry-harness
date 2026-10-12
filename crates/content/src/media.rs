//! Shared image identification and grouped-card geometry, without loading resources.

pub const GROUP_WIDTH: f32 = 160.;

pub fn file_format(path: &str) -> Option<image::ImageFormat> {
    let extension = std::path::Path::new(path).extension()?.to_str()?;
    match extension.to_ascii_lowercase().as_str() {
        "png" => Some(image::ImageFormat::Png),
        "jpg" | "jpeg" => Some(image::ImageFormat::Jpeg),
        "gif" => Some(image::ImageFormat::Gif),
        "webp" => Some(image::ImageFormat::WebP),
        "tif" | "tiff" => Some(image::ImageFormat::Tiff),
        "bmp" => Some(image::ImageFormat::Bmp),
        _ => None,
    }
}

pub use crate::markdown::visuals::MAX_SVG_BYTES;

/// Whether the path names a static SVG image.
pub fn is_svg(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case("svg"))
}

/// Accepts SVG bytes within the size limit that are static and self-contained.
pub fn validate_svg(bytes: &[u8]) -> Result<(), &'static str> {
    if bytes.len() > MAX_SVG_BYTES {
        return Err("too large");
    }
    let source = std::str::from_utf8(bytes).map_err(|_| "not utf-8")?;
    crate::markdown::visuals::validate_source(source).map_err(|_| "unsupported")
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::ImageFormat;

    #[test]
    fn recognizes_formats() {
        for (extension, format) in [
            ("png", ImageFormat::Png),
            ("jpg", ImageFormat::Jpeg),
            ("jpeg", ImageFormat::Jpeg),
            ("gif", ImageFormat::Gif),
            ("webp", ImageFormat::WebP),
            ("tif", ImageFormat::Tiff),
            ("tiff", ImageFormat::Tiff),
            ("bmp", ImageFormat::Bmp),
        ] {
            assert_eq!(
                file_format(&format!("images/photo.{extension}")),
                Some(format)
            );
            assert_eq!(
                file_format(&format!("images/photo.{}", extension.to_ascii_uppercase())),
                Some(format)
            );
        }
        for path in ["photo", "photo.svg", "photo.pdf", "photo.png/data"] {
            assert_eq!(file_format(path), None, "{path}");
        }
    }

    #[test]
    fn detects_svg_paths() {
        assert!(is_svg("output/a.svg"));
        assert!(is_svg("output/A.SVG"));
        for path in ["a.svgz", "svg", "a.svg/b", "a.png"] {
            assert!(!is_svg(path), "{path}");
        }
    }

    #[test]
    fn validates_svg_bytes() {
        let ok = br#"<svg xmlns="http://www.w3.org/2000/svg" width="4" height="4"><rect width="4" height="4"/></svg>"#;
        assert_eq!(validate_svg(ok), Ok(()));
        let oversized = vec![b' '; MAX_SVG_BYTES + 1];
        assert_eq!(validate_svg(&oversized), Err("too large"));
        assert!(validate_svg(b"<svg").is_err());
        for hostile in [
            r#"<svg xmlns="http://www.w3.org/2000/svg"><script>1</script></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><image href="file:///etc/passwd"/></svg>"#,
            r#"<svg xmlns="http://www.w3.org/2000/svg"><rect fill="url(http://x/y)"/></svg>"#,
        ] {
            assert!(validate_svg(hostile.as_bytes()).is_err(), "{hostile}");
        }
    }
}
