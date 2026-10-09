use super::*;
use ::image::Rgb;
use oar_ocr_core::processors::Point;

#[test]
fn normalizes_recognition_channels_and_padding() {
    let image = RgbImage::from_pixel(2, 2, Rgb([255, 128, 0]));
    let values = recognition(&image, [48, 320]);
    assert_eq!(values.shape(), &[1, 3, 48, 320]);
    assert_eq!(values[[0, 0, 0, 0]], -1.0);
    assert_eq!(values[[0, 2, 0, 0]], 1.0);
    assert!((values[[0, 1, 0, 0]] - (128.0 / 255.0 - 0.5) / 0.5).abs() < 1e-6);
    assert_eq!(values[[0, 0, 0, 48]], 0.0);
}

#[test]
fn distinguishes_detector_and_orientation_color_order() {
    let image = RgbImage::from_pixel(32, 32, Rgb([255, 0, 0]));
    let detection = detection(&image, [0, 0]);
    let orientation = orientation(&image);
    assert!((detection[[0, 0, 0, 0]] + 0.485 / 0.229).abs() < 1e-6);
    assert!((orientation[[0, 0, 0, 0]] - (1.0 - 0.485) / 0.229).abs() < 1e-6);
    assert_eq!(orientation.shape(), &[1, 3, 80, 160]);
}

#[test]
fn bounds_resize_and_honors_fixed_graph_dimensions() {
    let image = RgbImage::new(2000, 1000);
    assert_eq!(detection(&image, [0, 0]).shape(), &[1, 3, 480, 960]);
    assert_eq!(detection(&image, [64, 96]).shape(), &[1, 3, 64, 96]);
    assert_eq!(recognition(&image, [48, 0]).shape(), &[1, 3, 48, 320]);
    assert!(validate(&RgbImage::new(0, 1)).is_err());
    assert!(validate(&RgbImage::new(16_385, 1)).is_err());
}

#[test]
fn rejects_coordinates_outside_original_image() {
    let image = RgbImage::new(64, 32);
    let region = BoundingBox {
        points: vec![
            Point::new(0.0, 0.0),
            Point::new(64.0, 0.0),
            Point::new(64.0, 32.0),
            Point::new(0.0, 32.0),
        ],
    };
    assert_eq!(points(&region, &image).unwrap()[2], [64.0, 32.0]);
    let mut invalid = region.clone();
    invalid.points[2].x = 65.0;
    assert!(points(&invalid, &image).is_err());
    invalid.points[2].x = f32::NAN;
    assert!(points(&invalid, &image).is_err());
}
