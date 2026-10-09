//! CPU text recognition for Node-owned image and document extraction.
mod decode;
mod image;
mod onnx;

use ::image::RgbImage;
use oar_ocr_core::processors::{DBPostProcess, ImageScaleInfo, sort_quad_boxes};
use sailry_protocol::models::{Definition, Format};
use std::path::Path;

pub const MAX_IMAGE_PIXELS: u64 = 40_000_000;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("OCR model is invalid: {0}")]
    Invalid(String),
    #[error("OCR storage failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("OCR inference failed: {0}")]
    Inference(#[from] ort::Error),
    #[error(transparent)]
    Runtime(#[from] sailry_model_assets::runtime::RuntimeError),
    #[error("OCR cancelled")]
    Cancelled,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Line {
    pub text: String,
    pub confidence: f32,
    pub points: [[f32; 2]; 4],
}

pub struct Engine {
    detector: onnx::Model,
    recognizer: onnx::Model,
    orientation: Option<onnx::Model>,
    dictionary: decode::Dictionary,
    postprocess: DBPostProcess,
    pool: rayon::ThreadPool,
}

impl Engine {
    pub fn load(definition: &Definition, directory: &Path) -> Result<Self, Error> {
        definition
            .validate()
            .map_err(|error| Error::Invalid(error.to_string()))?;
        let Format::PaddleOcr { orientation, .. } = definition.format else {
            return Err(Error::Invalid("unsupported OCR family".into()));
        };
        let files = definition
            .assets
            .iter()
            .map(|asset| sailry_model_assets::File {
                name: &asset.name,
                size: asset.bytes,
                sha256: &asset.sha256,
            })
            .collect::<Vec<_>>();
        if !sailry_model_assets::ready(directory, &files) {
            return Err(Error::Invalid("pinned OCR assets are unavailable".into()));
        }
        let dictionary = decode::Dictionary::load(&directory.join("dictionary.txt"))?;
        sailry_model_assets::runtime::initialize()?;
        let detector = onnx::Model::load(&directory.join("detector.onnx"), onnx::Role::Detector)?;
        let recognizer = onnx::Model::load(
            &directory.join("recognizer.onnx"),
            onnx::Role::Recognizer(dictionary.len()),
        )?;
        let orientation = orientation
            .then(|| {
                onnx::Model::load(&directory.join("orientation.onnx"), onnx::Role::Orientation)
            })
            .transpose()?;
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(sailry_model_assets::session::threads())
            .thread_name(|index| format!("sailry-ocr-{index}"))
            .start_handler(|_| sailry_model_assets::session::background())
            .build()
            .map_err(|error| Error::Invalid(error.to_string()))?;
        Ok(Self {
            detector,
            recognizer,
            orientation,
            dictionary,
            postprocess: DBPostProcess::new(None, None, None, None, None, None, None),
            pool,
        })
    }

    pub fn recognize(
        &mut self,
        image: &RgbImage,
        cancelled: impl Fn() -> bool,
    ) -> Result<Vec<Line>, Error> {
        image::validate(image)?;
        check(&cancelled)?;
        let prepared = image::detection(image, self.detector.dimensions());
        let prediction = self.detector.run(prepared)?;
        check(&cancelled)?;
        let shape = ImageScaleInfo::new(image.height() as f32, image.width() as f32, 1.0, 1.0);
        let (boxes, _) = self.pool.install(|| {
            self.postprocess.apply(
                &prediction
                    .into_dimensionality::<ndarray::Ix4>()
                    .expect("validated detection rank"),
                vec![shape],
                None,
            )
        });
        let mut lines = Vec::new();
        for region in sort_quad_boxes(&boxes[0]) {
            check(&cancelled)?;
            let points = image::points(&region, image)?;
            let mut crop = self.pool.install(|| {
                oar_ocr_core::utils::get_rotate_crop_image(image, &region.points)
                    .map_err(|error| Error::Invalid(error.to_string()))
            })?;
            if let Some(orientation) = &mut self.orientation {
                let output = orientation.run(image::orientation(&crop))?;
                if output[[0, 1]] > 0.9 && output[[0, 1]] > output[[0, 0]] {
                    crop = ::image::imageops::rotate180(&crop);
                }
                check(&cancelled)?;
            }
            let input = image::recognition(&crop, self.recognizer.dimensions());
            let output = self.recognizer.run(input)?;
            check(&cancelled)?;
            let (text, confidence) = self.dictionary.decode(&output)?;
            if !text.is_empty() {
                lines.push(Line {
                    text,
                    confidence,
                    points,
                });
            }
        }
        Ok(lines)
    }
}

fn check(cancelled: &impl Fn() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests;
