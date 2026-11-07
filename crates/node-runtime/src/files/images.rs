//! Worktree images as bounded model input. Originals that vision providers accept
//! pass through unchanged; others are decoded, downscaled and re-encoded.
use super::{Control, Scope, io_error, path, read_bytes};
use base64::Engine as _;
use image::{
    AnimationDecoder, DynamicImage, ImageDecoder, ImageFormat, ImageReader, Limits,
    imageops::FilterType, metadata::Orientation,
};
use sailry_protocol::{
    ErrorCode, Fault, FileImage, ImageFacts, InlineImage, MAX_IMAGE_BYTES, MAX_IMAGE_SIDE,
};
use std::io::Cursor;

/// Sent bytes; their base64 stays below common 5 MB provider image limits.
const SENT_BYTES: usize = 3 * 1024 * 1024;
/// Pixel memory one decode may allocate.
const DECODED_BYTES: u64 = 256 * 1024 * 1024;
const QUALITY: u8 = 85;
const EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "gif", "webp", "bmp"];
const FORMATS: [ImageFormat; 5] = [
    ImageFormat::Png,
    ImageFormat::Jpeg,
    ImageFormat::Gif,
    ImageFormat::WebP,
    ImageFormat::Bmp,
];

/// Whether the extension of `path` names a supported image format.
pub(crate) fn named(path: &str) -> bool {
    std::path::Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            EXTENSIONS
                .iter()
                .any(|known| extension.eq_ignore_ascii_case(known))
        })
}

pub(super) fn read(root: Scope<'_>, path: String, control: &Control) -> Result<FileImage, Fault> {
    control.check()?;
    let mut file = path::file(root, &path)?;
    let before = file.metadata().map_err(io_error)?;
    if before.len() > MAX_IMAGE_BYTES as u64 {
        return Err(Fault::invalid("image exceeds the 5 MiB read limit"));
    }
    let bytes = read_bytes(&mut file, MAX_IMAGE_BYTES + 1, control)?;
    let after = file.metadata().map_err(io_error)?;
    if bytes.len() as u64 != before.len()
        || before.len() != after.len()
        || before.modified().ok() != after.modified().ok()
    {
        return Err(Fault::new(
            ErrorCode::RevisionConflict,
            "file changed during inspection",
        ));
    }
    let revision = blake3::hash(&bytes).to_hex().to_string();
    let prepared = prepare(bytes, control)?;
    let mime_type = prepared
        .sent
        .as_ref()
        .unwrap_or(&prepared.source)
        .mime_type
        .clone();
    Ok(FileImage {
        path,
        revision,
        inline_data: vec![InlineImage {
            mime_type,
            data: base64::engine::general_purpose::STANDARD.encode(&prepared.bytes),
        }],
        source: prepared.source,
        sent: prepared.sent,
    })
}

struct Prepared {
    source: ImageFacts,
    sent: Option<ImageFacts>,
    bytes: Vec<u8>,
}

fn prepare(bytes: Vec<u8>, control: &Control) -> Result<Prepared, Fault> {
    let format = image::guess_format(&bytes)
        .ok()
        .filter(|format| FORMATS.contains(format))
        .ok_or_else(|| Fault::invalid("file is not a PNG, JPEG, GIF, WebP or BMP image"))?;
    let mut reader = ImageReader::with_format(Cursor::new(bytes.as_slice()), format);
    let mut limits = Limits::default();
    limits.max_alloc = Some(DECODED_BYTES);
    reader.limits(limits);
    let mut decoder = reader.into_decoder().map_err(decoding)?;
    let (width, height) = decoder.dimensions();
    if width == 0 || height == 0 {
        return Err(Fault::invalid("image could not be decoded"));
    }
    if decoder.total_bytes() > DECODED_BYTES {
        return Err(Fault::invalid("image dimensions exceed the decoding limit"));
    }
    let source = ImageFacts {
        mime_type: format.to_mime_type().into(),
        width,
        height,
        size: bytes.len() as u64,
    };
    if width.max(height) <= MAX_IMAGE_SIDE && bytes.len() <= SENT_BYTES && accepted(&bytes, format)
    {
        drop(decoder);
        return Ok(Prepared {
            source,
            sent: None,
            bytes,
        });
    }
    control.check()?;
    let orientation = decoder.orientation().unwrap_or(Orientation::NoTransforms);
    let mut image = DynamicImage::from_decoder(decoder).map_err(decoding)?;
    image.apply_orientation(orientation);
    if image.width().max(image.height()) > MAX_IMAGE_SIDE {
        image = image.resize(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE, FilterType::Triangle);
    }
    control.check()?;
    let (format, encoded) = encode(&image, format)?;
    Ok(Prepared {
        source,
        sent: Some(ImageFacts {
            mime_type: format.to_mime_type().into(),
            width: image.width(),
            height: image.height(),
            size: encoded.len() as u64,
        }),
        bytes: encoded,
    })
}

/// Providers accept PNG, JPEG and still GIF or WebP images, not BMP or animations.
fn accepted(bytes: &[u8], format: ImageFormat) -> bool {
    match format {
        ImageFormat::Png | ImageFormat::Jpeg => true,
        ImageFormat::Gif => image::codecs::gif::GifDecoder::new(Cursor::new(bytes))
            .is_ok_and(|decoder| decoder.into_frames().take(2).count() < 2),
        ImageFormat::WebP => image::codecs::webp::WebPDecoder::new(Cursor::new(bytes))
            .is_ok_and(|decoder| !decoder.has_animation()),
        _ => false,
    }
}

/// JPEG sources stay JPEG; others use PNG unless it exceeds the sent byte budget.
fn encode(image: &DynamicImage, source: ImageFormat) -> Result<(ImageFormat, Vec<u8>), Fault> {
    if source != ImageFormat::Jpeg {
        let mut png = Vec::new();
        image
            .write_with_encoder(image::codecs::png::PngEncoder::new(&mut png))
            .map_err(encoding)?;
        if png.len() <= SENT_BYTES {
            return Ok((ImageFormat::Png, png));
        }
    }
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, QUALITY)
        .encode_image(&opaque(image))
        .map_err(encoding)?;
    if jpeg.len() > SENT_BYTES {
        return Err(Fault::invalid(
            "image cannot be reduced below the model input limit",
        ));
    }
    Ok((ImageFormat::Jpeg, jpeg))
}

/// JPEG has no alpha channel; transparent pixels are composited over white.
fn opaque(image: &DynamicImage) -> image::RgbImage {
    if !image.color().has_alpha() {
        return image.to_rgb8();
    }
    let rgba = image.to_rgba8();
    image::RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let [red, green, blue, alpha] = rgba.get_pixel(x, y).0;
        let alpha = u32::from(alpha);
        let blend = |channel: u8| ((u32::from(channel) * alpha + 255 * (255 - alpha)) / 255) as u8;
        image::Rgb([blend(red), blend(green), blend(blue)])
    })
}

fn decoding(error: image::ImageError) -> Fault {
    Fault::invalid(match error {
        image::ImageError::Limits(_) => "image dimensions exceed the decoding limit",
        _ => "image could not be decoded",
    })
}

fn encoding(_: image::ImageError) -> Fault {
    Fault::new(ErrorCode::Internal, "image could not be encoded")
}

#[cfg(test)]
mod tests;
