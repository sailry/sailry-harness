//! Element captures crop the viewport screenshot. Image pixels per CSS pixel come
//! from the screenshot size, which keeps the crop correct at any device scale.
use std::sync::Arc;

const PADDING: f64 = 8.;
const QUALITY: u8 = 85;

/// Pixel rectangle `[x, y, width, height]` covering CSS `bounds` plus padding.
pub(crate) fn region(bounds: [f64; 4], viewport: [f64; 2], size: (u32, u32)) -> Option<[u32; 4]> {
    let (width, height) = (f64::from(size.0), f64::from(size.1));
    if size.0 == 0
        || size.1 == 0
        || bounds
            .iter()
            .chain(&viewport)
            .any(|value| !value.is_finite())
        || viewport.iter().any(|value| *value <= 0.)
    {
        return None;
    }
    let (horizontal, vertical) = (width / viewport[0], height / viewport[1]);
    let left = ((bounds[0] - PADDING) * horizontal)
        .floor()
        .clamp(0., width);
    let top = ((bounds[1] - PADDING) * vertical).floor().clamp(0., height);
    let right = ((bounds[0] + bounds[2] + PADDING) * horizontal)
        .ceil()
        .clamp(0., width);
    let bottom = ((bounds[1] + bounds[3] + PADDING) * vertical)
        .ceil()
        .clamp(0., height);
    (right > left && bottom > top).then(|| {
        [
            left as u32,
            top as u32,
            (right - left) as u32,
            (bottom - top) as u32,
        ]
    })
}

/// Crops an encoded viewport screenshot to the element as a JPEG.
pub(crate) fn encode(
    screenshot: &[u8],
    bounds: [f64; 4],
    viewport: [f64; 2],
) -> Result<Arc<[u8]>, ()> {
    let image = image::load_from_memory(screenshot).map_err(|_| ())?;
    let [x, y, width, height] =
        region(bounds, viewport, (image.width(), image.height())).ok_or(())?;
    let crop = image.crop_imm(x, y, width, height).to_rgb8();
    let mut data = Vec::new();
    image::codecs::jpeg::JpegEncoder::new_with_quality(&mut data, QUALITY)
        .encode_image(&crop)
        .map_err(|_| ())?;
    Ok(data.into())
}
