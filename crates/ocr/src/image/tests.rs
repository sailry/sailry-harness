use super::*;
use ::image::Rgb;
use oar_ocr_core::processors::Point;

#[test]
fn preserves_recognition_contracts() {
    for (scenario, check) in [
        (
            "normalizes_recognition_channels_and_padding",
            (|scenario: &'static str| {
                let image = RgbImage::from_pixel(2, 2, Rgb([255, 128, 0]));
                let values = recognition(&image, [48, 320]);
                assert_eq!(values.shape(), &[1, 3, 48, 320], "{scenario}");
                assert_eq!(values[[0, 0, 0, 0]], -1.0, "{scenario}");
                assert_eq!(values[[0, 2, 0, 0]], 1.0, "{scenario}");
                assert!(
                    (values[[0, 1, 0, 0]] - (128.0 / 255.0 - 0.5) / 0.5).abs() < 1e-6,
                    "{scenario}"
                );
                assert_eq!(values[[0, 0, 0, 48]], 0.0, "{scenario}");
            }) as fn(&'static str),
        ),
        (
            "distinguishes_detector_and_orientation_color_order",
            (|scenario: &'static str| {
                let image = RgbImage::from_pixel(32, 32, Rgb([255, 0, 0]));
                let detection = detection(&image, [0, 0]);
                let orientation = orientation(&image);
                assert!(
                    (detection[[0, 0, 0, 0]] + 0.485 / 0.229).abs() < 1e-6,
                    "{scenario}"
                );
                assert!(
                    (orientation[[0, 0, 0, 0]] - (1.0 - 0.485) / 0.229).abs() < 1e-6,
                    "{scenario}"
                );
                assert_eq!(orientation.shape(), &[1, 3, 80, 160], "{scenario}");
            }),
        ),
        (
            "bounds_resize_and_honors_fixed_graph_dimensions",
            (|scenario: &'static str| {
                let image = RgbImage::new(2000, 1000);
                assert_eq!(
                    detection(&image, [0, 0]).shape(),
                    &[1, 3, 480, 960],
                    "{scenario}"
                );
                assert_eq!(
                    detection(&image, [64, 96]).shape(),
                    &[1, 3, 64, 96],
                    "{scenario}"
                );
                assert_eq!(
                    recognition(&image, [48, 0]).shape(),
                    &[1, 3, 48, 320],
                    "{scenario}"
                );
                assert!(validate(&RgbImage::new(0, 1)).is_err(), "{scenario}");
                assert!(validate(&RgbImage::new(16_385, 1)).is_err(), "{scenario}");
            }),
        ),
        (
            "rejects_coordinates_outside_original_image",
            (|scenario: &'static str| {
                let image = RgbImage::new(64, 32);
                let region = BoundingBox {
                    points: vec![
                        Point::new(0.0, 0.0),
                        Point::new(64.0, 0.0),
                        Point::new(64.0, 32.0),
                        Point::new(0.0, 32.0),
                    ],
                };
                assert_eq!(
                    points(&region, &image).unwrap()[2],
                    [64.0, 32.0],
                    "{scenario}"
                );
                let mut invalid = region.clone();
                invalid.points[2].x = 65.0;
                assert!(points(&invalid, &image).is_err(), "{scenario}");
                invalid.points[2].x = f32::NAN;
                assert!(points(&invalid, &image).is_err(), "{scenario}");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
