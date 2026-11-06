//! Viewport CSS geometry mapped to captured and encoded pixels.
use sailry_protocol::browser::{MAX_IMAGE_SIDE, Scale};

/// Decoded RGBA pixels kept below the native capture buffer limit.
const CAPTURE_PIXELS: f64 = 7_500_000.;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Rect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Plan {
    /// Visible part of the requested region, in viewport CSS pixels.
    pub region: Rect,
    pub viewport: (f64, f64),
    pub ratio: f64,
    /// Encoded pixel size before any size-bounded reduction.
    pub output: (u32, u32),
    /// macOS snapshot width in points; Windows captures at its native size.
    pub snapshot: f64,
}

pub(super) fn plan(bounds: Rect, viewport: (f64, f64), ratio: f64, scale: Scale) -> Option<Plan> {
    let valid = |value: f64| value.is_finite() && value > 0.;
    if !valid(viewport.0) || !valid(viewport.1) || !valid(ratio) {
        return None;
    }
    let ratio = ratio.clamp(0.25, 8.);
    let region = visible(bounds, viewport)?;
    let output = output(region, ratio, scale);
    Some(Plan {
        region,
        viewport,
        ratio,
        output,
        snapshot: snapshot(viewport, region.width, ratio, output.0),
    })
}

fn visible(region: Rect, viewport: (f64, f64)) -> Option<Rect> {
    if ![region.x, region.y, region.width, region.height]
        .iter()
        .all(|value| value.is_finite())
    {
        return None;
    }
    let left = region.x.max(0.);
    let top = region.y.max(0.);
    let right = (region.x + region.width).min(viewport.0);
    let bottom = (region.y + region.height).min(viewport.1);
    (right - left >= 1. && bottom - top >= 1.).then_some(Rect {
        x: left,
        y: top,
        width: right - left,
        height: bottom - top,
    })
}

fn output(region: Rect, ratio: f64, scale: Scale) -> (u32, u32) {
    let factor = match scale {
        Scale::Css => 1.,
        Scale::Device => ratio,
    };
    let (width, height) = (region.width * factor, region.height * factor);
    let fit = (f64::from(MAX_IMAGE_SIDE) / width.max(height)).min(1.);
    (
        ((width * fit).round() as u32).clamp(1, MAX_IMAGE_SIDE),
        ((height * fit).round() as u32).clamp(1, MAX_IMAGE_SIDE),
    )
}

/// Enough points for the region's output pixels, without upscaling or an oversized buffer.
fn snapshot(viewport: (f64, f64), region: f64, ratio: f64, output: u32) -> f64 {
    let wanted = f64::from(output) * viewport.0 / (region * ratio);
    let limit = (CAPTURE_PIXELS * viewport.0 / viewport.1).sqrt() / ratio;
    wanted.min(viewport.0).min(limit).max(1.)
}

/// Pixel crop of a region from a capture of the whole CSS viewport.
pub(super) fn crop(region: Rect, viewport: (f64, f64), image: (u32, u32)) -> (u32, u32, u32, u32) {
    let axis = |start: f64, length: f64, css: f64, pixels: u32| {
        let factor = f64::from(pixels) / css;
        let first = ((start * factor).floor().max(0.) as u32).min(pixels.saturating_sub(1));
        let last = (((start + length) * factor).ceil() as u32).clamp(first + 1, pixels.max(1));
        (first, last - first)
    };
    let (x, width) = axis(region.x, region.width, viewport.0, image.0);
    let (y, height) = axis(region.y, region.height, viewport.1, image.1);
    (x, y, width, height)
}

/// The next smaller size when an encoding exceeds its byte limit.
pub(super) fn shrink(size: (u32, u32)) -> (u32, u32) {
    ((size.0 * 3 / 4).max(1), (size.1 * 3 / 4).max(1))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(x: f64, y: f64, width: f64, height: f64) -> Rect {
        Rect {
            x,
            y,
            width,
            height,
        }
    }

    mod planning {
        use super::*;

        #[test]
        fn clips_regions_to_viewport() {
            let clipped =
                plan(rect(-20., 700., 300., 200.), (1000., 800.), 1., Scale::Css).unwrap();
            assert_eq!(clipped.region, rect(0., 700., 280., 100.));
            assert_eq!(clipped.output, (280, 100));
            for outside in [
                rect(1000., 0., 10., 10.),
                rect(0., -50., 10., 50.),
                rect(10., 10., 0.5, 10.),
                rect(f64::NAN, 0., 10., 10.),
            ] {
                assert_eq!(plan_at(outside), None);
            }
            assert_eq!(
                plan(rect(0., 0., 10., 10.), (0., 800.), 1., Scale::Css),
                None
            );
            assert_eq!(
                plan(rect(0., 0., 10., 10.), (100., 80.), 0., Scale::Css),
                None
            );
        }

        fn plan_at(bounds: Rect) -> Option<Plan> {
            plan(bounds, (1000., 800.), 2., Scale::Css)
        }

        #[test]
        fn scales_by_device_ratio() {
            let region = rect(10., 20., 200., 50.);
            assert_eq!(
                plan(region, (1000., 800.), 2., Scale::Css).unwrap().output,
                (200, 50)
            );
            assert_eq!(
                plan(region, (1000., 800.), 2., Scale::Device)
                    .unwrap()
                    .output,
                (400, 100)
            );
            assert_eq!(
                plan(region, (1000., 800.), 1.5, Scale::Device)
                    .unwrap()
                    .output,
                (300, 75)
            );
        }

        #[test]
        fn caps_long_side() {
            let wide = plan(rect(0., 0., 1200., 600.), (1200., 800.), 2., Scale::Device).unwrap();
            assert_eq!(wide.output, (MAX_IMAGE_SIDE, 784));
            let tall = plan(rect(0., 0., 100., 3000.), (1200., 4000.), 1., Scale::Css).unwrap();
            assert_eq!(tall.output, (52, MAX_IMAGE_SIDE));
        }

        #[test]
        fn bounds_snapshot_width() {
            let native = plan(
                rect(100., 100., 100., 40.),
                (1000., 800.),
                2.,
                Scale::Device,
            )
            .unwrap();
            assert_eq!(native.snapshot, 1000.);
            let viewport = plan(rect(0., 0., 1000., 800.), (1000., 800.), 2., Scale::Css).unwrap();
            assert_eq!(viewport.snapshot, 500.);
            let large = plan(rect(10., 10., 50., 50.), (1920., 1200.), 2., Scale::Device).unwrap();
            assert!(large.snapshot < 1920.);
            let pixels = (large.snapshot * 2.).powi(2) * 1200. / 1920.;
            assert!(pixels <= CAPTURE_PIXELS + 1.);
        }
    }

    mod pixels {
        use super::*;

        #[test]
        fn crops_device_pixels() {
            assert_eq!(
                crop(rect(10., 20., 100., 50.), (1000., 800.), (2000, 1600)),
                (20, 40, 200, 100)
            );
            assert_eq!(
                crop(rect(10.25, 20.5, 100.5, 50.), (1000., 800.), (1000, 800)),
                (10, 20, 101, 51)
            );
        }

        #[test]
        fn stays_inside_image() {
            assert_eq!(
                crop(rect(990., 790., 10., 10.), (1000., 800.), (500, 400)),
                (495, 395, 5, 5)
            );
            assert_eq!(
                crop(rect(999.9, 799.9, 0.1, 0.1), (1000., 800.), (10, 8)),
                (9, 7, 1, 1)
            );
        }

        #[test]
        fn shrinks_without_zero_sides() {
            assert_eq!(shrink((1568, 800)), (1176, 600));
            assert_eq!(shrink((1, 1)), (1, 1));
        }
    }
}
