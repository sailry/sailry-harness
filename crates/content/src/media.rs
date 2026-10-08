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
}
