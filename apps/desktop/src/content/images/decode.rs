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

pub(super) fn decode(
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
