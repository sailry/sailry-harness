use gpui_kit::RenderImage;
use image::{DynamicImage, Frame, ImageDecoder, ImageFormat, ImageReader, Limits};
use sailry_protocol::attachment::Attachment;
use std::{io::Cursor, sync::Arc};

pub(super) fn format(attachment: &Attachment) -> Option<ImageFormat> {
    match attachment.spec.media_type.as_str() {
        "image/png" => Some(ImageFormat::Png),
        "image/jpeg" => Some(ImageFormat::Jpeg),
        "image/gif" => Some(ImageFormat::Gif),
        "image/webp" => Some(ImageFormat::WebP),
        "image/tiff" => Some(ImageFormat::Tiff),
        "image/bmp" => Some(ImageFormat::Bmp),
        _ => None,
    }
}

pub(crate) fn decode(
    bytes: &[u8],
    format: ImageFormat,
    side: u32,
) -> Result<Arc<RenderImage>, &'static str> {
    let decoded = read(bytes, format)?;
    let mut pixels = if decoded.width() > side || decoded.height() > side {
        decoded.thumbnail(side, side).into_rgba8()
    } else {
        decoded.into_rgba8()
    };
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(Arc::new(RenderImage::new([Frame::new(pixels)])))
}

/// Rasterizes a static, self-contained SVG so its longest side equals `side`.
pub(crate) fn svg(bytes: &[u8], side: u32) -> Result<Arc<RenderImage>, &'static str> {
    use gpui_kit::{DevicePixels, SvgRenderer, SvgSize, size};
    static RENDERER: std::sync::LazyLock<SvgRenderer> =
        std::sync::LazyLock::new(|| SvgRenderer::new(Arc::new(())));
    sailry_content::media::validate_svg(bytes).map_err(|reason| match reason {
        "too large" => "chat_image_too_large",
        _ => "chat_image_unavailable",
    })?;
    let (width, height) = svg_extent(bytes).ok_or("chat_image_unavailable")?;
    let scale = side as f32 / width.max(height);
    let pixels = |value: f32| DevicePixels(((value * scale).round() as i32).max(1));
    let renderer = RENDERER.clone();
    let tree = renderer
        .parse_svg(bytes)
        .map_err(|_| "chat_image_unavailable")?;
    renderer
        .render_parsed(
            &tree,
            SvgSize::ExactSize(size(pixels(width), pixels(height))),
        )
        .map_err(|_| "chat_image_unavailable")
}

/// Reads the intrinsic size from `width`/`height`, falling back to `viewBox`.
fn svg_extent(bytes: &[u8]) -> Option<(f32, f32)> {
    let document = roxmltree::Document::parse(std::str::from_utf8(bytes).ok()?).ok()?;
    let root = document.root_element();
    let length = |name| {
        let value = root.attribute(name)?.trim();
        value
            .strip_suffix("px")
            .unwrap_or(value)
            .trim()
            .parse::<f32>()
            .ok()
    };
    let (width, height) = match (length("width"), length("height")) {
        (Some(width), Some(height)) => (width, height),
        _ => {
            let mut view = root
                .attribute("viewBox")?
                .split([' ', ','])
                .filter(|part| !part.is_empty())
                .map(str::parse::<f32>);
            let (_, _, width, height) = (
                view.next()?.ok()?,
                view.next()?.ok()?,
                view.next()?.ok()?,
                view.next()?.ok()?,
            );
            (width, height)
        }
    };
    (width.is_finite() && height.is_finite() && width > 0. && height > 0.)
        .then_some((width, height))
}

pub(crate) fn png(bytes: &[u8], format: ImageFormat) -> Result<Vec<u8>, &'static str> {
    let decoded = read(bytes, format)?;
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(decoded.into_rgba8())
        .write_to(&mut output, ImageFormat::Png)
        .map_err(|_| "chat_image_unavailable")?;
    Ok(output.into_inner())
}

fn read(bytes: &[u8], format: ImageFormat) -> Result<DynamicImage, &'static str> {
    const ALLOCATION: u64 = 64 * 1024 * 1024;
    let mut limits = Limits::default();
    limits.max_image_width = Some(16_384);
    limits.max_image_height = Some(16_384);
    limits.max_alloc = Some(ALLOCATION);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    let mut decoder = reader
        .into_decoder()
        .map_err(|_| "chat_image_unavailable")?;
    let (width, height) = decoder.dimensions();
    // Also bound the RGBA conversion; decoder limits vary by codec.
    if decoder.total_bytes() > ALLOCATION || u64::from(width) * u64::from(height) > ALLOCATION / 4 {
        return Err("chat_image_too_large");
    }
    let orientation = decoder
        .orientation()
        .map_err(|_| "chat_image_unavailable")?;
    // DynamicImage decodes one frame; previews do not allocate an animation.
    let mut decoded = DynamicImage::from_decoder(decoder).map_err(|_| "chat_image_unavailable")?;
    decoded.apply_orientation(orientation);
    Ok(decoded)
}
