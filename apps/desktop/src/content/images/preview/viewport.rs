use super::*;
use gpui_kit::Size;

pub(super) const MIN_ZOOM: f32 = 0.25;
pub(super) const MAX_ZOOM: f32 = 8.;

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Viewport {
    pub zoom: f32,
    pub pan: Point<Pixels>,
    pub drag: Option<Point<Pixels>>,
}

impl Default for Viewport {
    fn default() -> Self {
        Self {
            zoom: 1.,
            pan: Point::default(),
            drag: None,
        }
    }
}

impl Viewport {
    pub fn zoom_by(&mut self, factor: f32) {
        if !factor.is_finite() || factor <= 0. {
            return;
        }
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if self.zoom <= 1. {
            self.pan = Point::default();
            self.drag = None;
        }
    }

    pub fn clamp_pan(&mut self, rendered: Size<Pixels>, canvas: Size<Pixels>) {
        let horizontal = ((rendered.width - canvas.width) / 2.).max(px(0.));
        let vertical = ((rendered.height - canvas.height) / 2.).max(px(0.));
        self.pan.x = self.pan.x.clamp(-horizontal, horizontal);
        self.pan.y = self.pan.y.clamp(-vertical, vertical);
    }
}

pub(super) fn fit(image: Size<DevicePixels>, canvas: Size<Pixels>) -> Size<Pixels> {
    let width = image.width.0 as f32;
    let height = image.height.0 as f32;
    let scale = 1_f32
        .min(960. / width)
        .min(640. / height)
        .min(f32::from(canvas.width) * 0.82 / width)
        .min(f32::from(canvas.height) * 0.82 / height);
    size(px(width * scale), px(height * scale))
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::prelude::v1::test;

    #[test]
    fn fits_image_sizes() {
        for (image, canvas, expected) in [
            ((3840, 1920), (1600., 1000.), (960., 480.)),
            ((960, 1920), (1600., 1000.), (320., 640.)),
            ((2000, 1000), (500., 400.), (410., 205.)),
            ((1000, 2000), (500., 400.), (164., 328.)),
            ((32, 16), (500., 400.), (32., 16.)),
        ] {
            let actual = fit(
                size(DevicePixels(image.0), DevicePixels(image.1)),
                size(px(canvas.0), px(canvas.1)),
            );
            assert!((f32::from(actual.width) - expected.0).abs() < 0.001);
            assert!((f32::from(actual.height) - expected.1).abs() < 0.001);
        }
    }

    #[test]
    fn zoom_limits() {
        let mut viewport = Viewport::default();
        viewport.zoom_by(100.);
        assert_eq!(viewport.zoom, MAX_ZOOM);
        viewport.pan = point(px(35.), px(-20.));
        viewport.drag = Some(point(px(8.), px(13.)));
        let previous = viewport.clone();
        for factor in [0., -1., f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
            viewport.zoom_by(factor);
            assert_eq!(viewport, previous);
        }
        viewport.zoom_by(0.5);
        assert_eq!(viewport.zoom, 4.);
        assert_eq!(viewport.pan, previous.pan);
        assert_eq!(viewport.drag, previous.drag);
        viewport.zoom_by(0.25);
        assert_eq!(viewport, Viewport::default());
        viewport.zoom_by(0.01);
        assert_eq!(viewport.zoom, MIN_ZOOM);
        assert_eq!(viewport.pan, Point::default());
        assert_eq!(viewport.drag, None);
    }

    #[test]
    fn pan_limits() {
        let mut viewport = Viewport {
            zoom: 2.,
            pan: point(px(300.), px(-400.)),
            drag: Some(point(px(8.), px(13.))),
        };
        viewport.clamp_pan(size(px(1000.), px(800.)), size(px(600.), px(500.)));
        assert_eq!(viewport.pan, point(px(200.), px(-150.)));
        viewport.pan = point(px(-300.), px(400.));
        viewport.clamp_pan(size(px(1000.), px(800.)), size(px(600.), px(500.)));
        assert_eq!(viewport.pan, point(px(-200.), px(150.)));
        viewport.clamp_pan(size(px(1000.), px(400.)), size(px(600.), px(500.)));
        assert_eq!(viewport.pan, point(px(-200.), px(0.)));
        viewport.clamp_pan(size(px(400.), px(400.)), size(px(600.), px(500.)));
        assert_eq!(viewport.pan, Point::default());
        assert_eq!(viewport.drag, Some(point(px(8.), px(13.))));
    }
}
