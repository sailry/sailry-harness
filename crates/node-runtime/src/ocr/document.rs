//! Scanned pages are rasterized on the CPU; ordinary PDF text remains native.
use super::Service;
use crate::office::Part;
use sailry_link::CancellationToken;
use sailry_protocol::{Fault, knowledge::Position};

pub(crate) fn supported(path: &str) -> bool {
    path.rsplit('.').next().is_some_and(|extension| {
        ["png", "jpg", "jpeg", "webp", "gif"]
            .iter()
            .any(|value| extension.eq_ignore_ascii_case(value))
    })
}

pub(crate) fn image(
    service: &Service,
    bytes: &[u8],
    cancel: &CancellationToken,
) -> Result<Vec<Part>, Fault> {
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(super::failure)?;
    let (width, height) = reader.into_dimensions().map_err(super::failure)?;
    if width == 0
        || height == 0
        || u64::from(width) * u64::from(height) > sailry_ocr::MAX_IMAGE_PIXELS
    {
        return Err(Fault::invalid("OCR image dimensions exceed their limit"));
    }
    let image = image::load_from_memory(bytes)
        .map_err(super::failure)?
        .into_rgb8();
    let text = service
        .recognize(&image, cancel)?
        .into_iter()
        .map(|line| line.text)
        .collect::<Vec<_>>()
        .join("\n");
    Ok(vec![Part {
        name: "Image".into(),
        text,
        position: Position::Page { page: 1 },
    }])
}

pub(crate) fn pdf(
    service: &Service,
    bytes: &[u8],
    mut parts: Vec<Part>,
    cancel: &CancellationToken,
) -> Result<Vec<Part>, Fault> {
    if parts.iter().all(|part| !part.text.trim().is_empty()) || service.signature() == [0; 32] {
        return Ok(parts);
    }
    let pdf = hayro::hayro_syntax::Pdf::new(bytes.to_vec())
        .map_err(|error| super::failure(format!("{error:?}")))?;
    if pdf.pages().len() != parts.len() {
        return Err(Fault::invalid("PDF page count changed during extraction"));
    }
    let cache = hayro::RenderCache::new();
    for (part, page) in parts.iter_mut().zip(pdf.pages().iter()) {
        if !part.text.trim().is_empty() {
            continue;
        }
        if cancel.is_cancelled() {
            return Err(super::cancelled());
        }
        let (width, height) = page.render_dimensions();
        if !width.is_finite() || !height.is_finite() || width <= 0.0 || height <= 0.0 {
            return Err(Fault::invalid("PDF page dimensions are invalid"));
        }
        let scale = (2000.0 / width.max(height)).min(2.0);
        let settings = hayro::PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        };
        let pixmap = hayro::render(
            page,
            &cache,
            &hayro::hayro_interpret::InterpreterSettings::default(),
            &hayro::RenderSettings::default(),
            &settings,
        );
        let (width, height) = (u32::from(pixmap.width()), u32::from(pixmap.height()));
        // The white raster background makes every premultiplied pixel opaque.
        let (pixels, remainder) = pixmap.data_as_u8_slice().as_chunks::<4>();
        if !remainder.is_empty() {
            return Err(Fault::invalid("PDF rasterization size is invalid"));
        }
        let rgb = pixels
            .iter()
            .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect::<Vec<_>>();
        let image = image::RgbImage::from_raw(width, height, rgb)
            .ok_or_else(|| Fault::invalid("PDF rasterization size is invalid"))?;
        match service.recognize(&image, cancel) {
            Ok(lines) => {
                part.text = lines
                    .into_iter()
                    .map(|line| line.text)
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            Err(error) if error.code == sailry_protocol::ErrorCode::NotConfigured => {
                return Ok(parts);
            }
            Err(error) => return Err(error),
        }
    }
    Ok(parts)
}
