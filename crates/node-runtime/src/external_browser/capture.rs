//! Managed captures reach the model as bounded images; oversized ones are reduced.
use sailry_protocol::{
    Fault,
    browser::{Format, MAX_IMAGE_BYTES, MAX_IMAGE_SIDE},
};

pub(super) struct Bounded {
    pub(super) bytes: Vec<u8>,
    pub(super) format: Format,
    pub(super) width: u32,
    pub(super) height: u32,
}

pub(super) fn bound(bytes: Vec<u8>) -> Result<Bounded, Fault> {
    let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .ok()
        .filter(|image| image.width() > 0 && image.height() > 0)
        .ok_or_else(|| Fault::invalid("browser capture is not a valid PNG image"))?;
    if image.width().max(image.height()) <= MAX_IMAGE_SIDE && bytes.len() <= MAX_IMAGE_BYTES {
        return Ok(Bounded {
            width: image.width(),
            height: image.height(),
            bytes,
            format: Format::Png,
        });
    }
    let fitted = if image.width().max(image.height()) > MAX_IMAGE_SIDE {
        image.thumbnail(MAX_IMAGE_SIDE, MAX_IMAGE_SIDE)
    } else {
        image
    };
    let encoded = |image: &image::DynamicImage, format: Format| {
        let mut bytes = Vec::new();
        let written = match format {
            Format::Png => image.write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            ),
            Format::Jpeg => image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 80)
                .encode_image(&image.to_rgb8()),
        };
        (written.is_ok() && bytes.len() <= MAX_IMAGE_BYTES).then(|| Bounded {
            width: image.width(),
            height: image.height(),
            bytes,
            format,
        })
    };
    let mut side = MAX_IMAGE_SIDE;
    let mut candidate = fitted;
    loop {
        if let Some(bounded) =
            encoded(&candidate, Format::Png).or_else(|| encoded(&candidate, Format::Jpeg))
        {
            return Ok(bounded);
        }
        side = side * 3 / 4;
        if side < 320 {
            return Err(Fault::invalid("browser capture exceeds the image limit"));
        }
        candidate = candidate.thumbnail(side, side);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let pixels = image::RgbImage::from_fn(width, height, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, ((x ^ y) % 256) as u8])
        });
        let mut bytes = Vec::new();
        pixels
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        bytes
    }

    #[test]
    fn keeps_small_captures_unchanged() {
        let original = png(40, 20);
        let bounded = bound(original.clone()).unwrap();
        assert_eq!(bounded.bytes, original);
        assert_eq!(bounded.format, Format::Png);
        assert_eq!((bounded.width, bounded.height), (40, 20));
    }

    #[test]
    fn reduces_oversized_captures() {
        let bounded = bound(png(MAX_IMAGE_SIDE * 2, 100)).unwrap();
        assert_eq!(bounded.width, MAX_IMAGE_SIDE);
        assert!(bounded.height <= 50);
        assert!(bounded.bytes.len() <= MAX_IMAGE_BYTES);
        let decoded = image::load_from_memory(&bounded.bytes).unwrap();
        assert_eq!(decoded.width(), bounded.width);
    }

    #[test]
    fn rejects_invalid_images() {
        assert!(bound(b"not an image".to_vec()).is_err());
    }
}
