use super::*;
use core::prelude::v1::test;
use image::{Rgba, RgbaImage};

fn pixels() -> RgbaImage {
    RgbaImage::from_fn(2, 3, |x, y| Rgba([(y * 2 + x + 1) as u8, 20, 30, 128]))
}

fn values(image: &RenderImage) -> Vec<u8> {
    image
        .as_bytes(0)
        .unwrap()
        .as_chunks::<4>()
        .0
        .iter()
        .map(|pixel| {
            assert_eq!(&pixel[1..], &[20, 30, 128]);
            pixel[0]
        })
        .collect()
}

#[test]
fn transforms_preserve_channels() {
    let image = RenderImage::new([image::Frame::new(pixels())]);
    let rotated = Change::Rotate.apply(&image).unwrap();
    assert_eq!(rotated.size(0), size(DevicePixels(3), DevicePixels(2)));
    assert_eq!(values(&rotated), [5, 3, 1, 6, 4, 2]);
    assert_eq!(
        values(&Change::Horizontal.apply(&image).unwrap()),
        [2, 1, 4, 3, 6, 5]
    );
    assert_eq!(
        values(&Change::Vertical.apply(&image).unwrap()),
        [5, 6, 3, 4, 1, 2]
    );
    let restored = (0..4).fold(Arc::new(image), |image, _| {
        Change::Rotate.apply(&image).unwrap()
    });
    assert_eq!(values(&restored), [1, 2, 3, 4, 5, 6]);
}
