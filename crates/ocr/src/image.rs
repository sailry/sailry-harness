use crate::{Error, MAX_IMAGE_PIXELS};
use ::image::{
    RgbImage,
    imageops::{FilterType, resize},
};
use ndarray::Array4;
use oar_ocr_core::processors::BoundingBox;

pub(super) fn validate(image: &RgbImage) -> Result<(), Error> {
    if image.width() == 0
        || image.height() == 0
        || u64::from(image.width()) * u64::from(image.height()) > MAX_IMAGE_PIXELS
        || image.width().max(image.height()) > 16_384
    {
        return Err(Error::Invalid("OCR image dimensions are invalid".into()));
    }
    Ok(())
}

pub(super) fn detection(image: &RgbImage, fixed: [usize; 2]) -> Array4<f32> {
    let ratio = (960.0 / image.width().max(image.height()) as f32).min(1.0);
    let dimensions = [image.height(), image.width()]
        .map(|value| ((value as f32 * ratio / 32.0).round() as usize * 32).max(32));
    let [height, width] = std::array::from_fn(|index| {
        if fixed[index] == 0 {
            dimensions[index]
        } else {
            fixed[index]
        }
    });
    normalized(
        &resize(image, width as u32, height as u32, FilterType::Triangle),
        width,
        true,
        false,
    )
}

pub(super) fn recognition(image: &RgbImage, fixed: [usize; 2]) -> Array4<f32> {
    let needed = (48.0 * image.width() as f32 / image.height().max(1) as f32).ceil() as usize;
    let width = if fixed[1] == 0 {
        needed.clamp(320, 3200)
    } else {
        fixed[1]
    };
    let resized = resize(
        image,
        needed.clamp(1, width) as u32,
        48,
        FilterType::Triangle,
    );
    normalized(&resized, width, true, true)
}

pub(super) fn orientation(image: &RgbImage) -> Array4<f32> {
    normalized(
        &resize(image, 160, 80, FilterType::Triangle),
        160,
        false,
        false,
    )
}

fn normalized(image: &RgbImage, width: usize, bgr: bool, recognition: bool) -> Array4<f32> {
    let mut output = Array4::zeros((1, 3, image.height() as usize, width));
    let mean = if recognition {
        [0.5; 3]
    } else {
        [0.485, 0.456, 0.406]
    };
    let scale = if recognition {
        [0.5; 3]
    } else {
        [0.229, 0.224, 0.225]
    };
    for (x, y, pixel) in image.enumerate_pixels() {
        for channel in 0..3 {
            let source = if bgr { 2 - channel } else { channel };
            output[[0, channel, y as usize, x as usize]] =
                (f32::from(pixel[source]) / 255.0 - mean[channel]) / scale[channel];
        }
    }
    output
}

pub(super) fn points(region: &BoundingBox, image: &RgbImage) -> Result<[[f32; 2]; 4], Error> {
    if region.points.len() != 4
        || region.points.iter().any(|point| {
            !point.x.is_finite()
                || !point.y.is_finite()
                || point.x < 0.0
                || point.y < 0.0
                || point.x > image.width() as f32
                || point.y > image.height() as f32
        })
    {
        return Err(Error::Invalid(
            "detector returned invalid coordinates".into(),
        ));
    }
    Ok(std::array::from_fn(|index| {
        [region.points[index].x, region.points[index].y]
    }))
}

#[cfg(test)]
mod tests;
