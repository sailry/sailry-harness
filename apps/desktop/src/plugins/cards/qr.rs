//! Kit's script catalog has no QR encoder or private in-memory bitmap component.
//! Encoding is an adapter only: it has no login, filesystem or transport access.
use gpui_kit::RenderImage;
use sailry_protocol::{Fault, plugin::card::MAX_QR_BYTES};
use std::sync::Arc;

pub(crate) struct Bitmap {
    pub image: Arc<RenderImage>,
    pub width: u32,
}

pub(crate) fn render(payload: &str) -> Result<Bitmap, Fault> {
    if payload.is_empty() || payload.len() > MAX_QR_BYTES {
        return Err(Fault::invalid("QR payload exceeds the size limit"));
    }
    let code = qrcode::QrCode::new(payload.as_bytes())
        .map_err(|_| Fault::invalid("QR payload cannot be encoded"))?;
    let width = (code.width() as u32 + 8) * 4;
    if width > 768 {
        return Err(Fault::invalid("QR image exceeds the size limit"));
    }
    let mut pixels = code
        .render::<image::Rgba<u8>>()
        .quiet_zone(true)
        .module_dimensions(4, 4)
        .build();
    // GPUI consumes BGRA. QR's monochrome channels are equal, but retain the
    // ordinary image conversion rather than relying on that representation.
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    Ok(Bitmap {
        image: Arc::new(RenderImage::new(vec![image::Frame::new(pixels)])),
        width,
    })
}

pub(super) fn live(expires_at: Option<i64>, now: i64) -> bool {
    expires_at.is_none_or(|expires_at| expires_at > now)
}
