use super::*;
use core::prelude::v1::test;

fn decoration(fit: Fit) -> Decoration {
    Decoration {
        path: "wallpaper.png".into(),
        fit,
        alignment: [0., 0.],
        opacity: 0.6,
        height: 36,
        tile_size: 32,
        license: None,
    }
}

fn source(pixels: image::RgbaImage) -> RenderImage {
    RenderImage::new([image::Frame::new(pixels)])
}

const RED: image::Rgba<u8> = image::Rgba([255, 0, 0, 128]);
const BLUE: image::Rgba<u8> = image::Rgba([0, 0, 255, 128]);

#[test]
fn retains_contain_alignment_and_transparency() {
    let source = source(image::RgbaImage::from_pixel(2, 1, RED));
    let mut decoration = decoration(Fit::Contain);
    let dimensions = size(DevicePixels(9), DevicePixels(3));
    let left = compose(&decoration, &source, dimensions, 1., None);
    assert_eq!(*left.get_pixel(0, 0), RED);
    assert_eq!(*left.get_pixel(5, 2), RED);
    assert_eq!(*left.get_pixel(6, 0), image::Rgba([0; 4]));
    decoration.alignment[0] = 1.;
    let right = compose(&decoration, &source, dimensions, 1., None);
    assert_eq!(*right.get_pixel(0, 0), image::Rgba([0; 4]));
    assert_eq!(*right.get_pixel(3, 0), RED);
    assert_eq!(*right.get_pixel(8, 2), RED);
}

#[test]
fn retains_cover_alignment() {
    let source = source(image::RgbaImage::from_fn(2, 4, |_, y| {
        if y == 3 { BLUE } else { RED }
    }));
    let mut decoration = decoration(Fit::Cover);
    let dimensions = size(DevicePixels(4), DevicePixels(1));
    let top = compose(&decoration, &source, dimensions, 1., None);
    assert_eq!(*top.get_pixel(0, 0), RED);
    decoration.alignment[1] = 1.;
    let bottom = compose(&decoration, &source, dimensions, 1., None);
    assert_eq!(*bottom.get_pixel(0, 0), BLUE);
}

#[test]
fn retains_fill_and_bounds_extreme_aspect_ratios() {
    let source = source(image::RgbaImage::from_pixel(4096, 1, RED));
    for fit in [Fit::Fill, Fit::Cover] {
        let pixels = compose(
            &decoration(fit),
            &source,
            size(DevicePixels(4), DevicePixels(96)),
            2.,
            None,
        );
        assert_eq!(pixels.dimensions(), (4, 96));
        assert!(pixels.pixels().all(|pixel| *pixel == RED));
    }
}

#[test]
fn tiles_without_rounding_individual_tiles() {
    let source = source(image::RgbaImage::from_fn(2, 1, |x, _| {
        if x == 0 { RED } else { BLUE }
    }));
    let decoration = decoration(Fit::Tile);
    for scale in [1., 2.] {
        let period = (32. * scale) as u32;
        let pixels = compose(
            &decoration,
            &source,
            size(DevicePixels((period * 2 + 1) as i32), DevicePixels(17)),
            scale,
            None,
        );
        for y in 0..17 {
            for x in 0..period {
                assert_eq!(pixels.get_pixel(x, y), pixels.get_pixel(x + period, y));
            }
            assert_eq!(pixels.get_pixel(0, y), pixels.get_pixel(period * 2, y));
        }
    }
}

#[test]
fn caps_strips_to_control_bounds() {
    let source = source(image::RgbaImage::from_pixel(2, 1, RED));
    for scale in [1., 2.] {
        let dimensions = size(
            DevicePixels((100. * scale) as i32),
            DevicePixels((32. * scale) as i32),
        );
        let full = compose(&decoration(Fit::Fill), &source, dimensions, scale, Some(36));
        assert!(full.pixels().all(|pixel| *pixel == RED));
        let strip = compose(&decoration(Fit::Fill), &source, dimensions, scale, Some(4));
        for (.., y, pixel) in strip.enumerate_pixels() {
            assert_eq!(
                *pixel,
                if y < (4. * scale) as u32 {
                    RED
                } else {
                    image::Rgba([0; 4])
                }
            );
        }
    }
}
