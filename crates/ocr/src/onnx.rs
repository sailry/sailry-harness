use crate::Error;
use ndarray::{Array4, ArrayD};
use ort::{
    session::{Session, SessionInputValue},
    value::{Tensor, TensorElementType, ValueType},
};
use std::path::Path;

#[derive(Clone, Copy)]
pub(super) enum Role {
    Detector,
    Recognizer(usize),
    Orientation,
}

pub(super) struct Model {
    session: Session,
    role: Role,
    dimensions: [usize; 2],
    input: String,
}

impl Model {
    pub fn load(path: &Path, role: Role) -> Result<Self, Error> {
        sailry_model_assets::onnx::validate(path)?;
        let session = sailry_model_assets::session::builder()?.commit_from_file(path)?;
        if session.inputs().len() != 1 || session.outputs().len() != 1 {
            return Err(Error::Invalid(
                "OCR models require one input and one output".into(),
            ));
        }
        let input = session.inputs()[0].name().to_owned();
        let shape = tensor(session.inputs()[0].dtype())?;
        if shape.len() != 4 || !matches!(shape[0], -1 | 1) || shape[1] != 3 {
            return Err(Error::Invalid(
                "OCR image input must be float32 NCHW".into(),
            ));
        }
        let dimensions = match role {
            Role::Detector
                if shape[2..]
                    .iter()
                    .all(|&value| value == -1 || (32..=2048).contains(&value)) =>
            {
                [
                    usize::try_from(shape[2]).unwrap_or(0),
                    usize::try_from(shape[3]).unwrap_or(0),
                ]
            }
            Role::Recognizer(_)
                if shape[2] == 48 && (shape[3] == -1 || (32..=3200).contains(&shape[3])) =>
            {
                [48, usize::try_from(shape[3]).unwrap_or(0)]
            }
            Role::Orientation if shape[2..] == [80, 160] => [80, 160],
            _ => return Err(Error::Invalid("unsupported OCR image dimensions".into())),
        };
        let output = tensor(session.outputs()[0].dtype())?;
        validate_shape(role, &output, true)?;
        let mut model = Self {
            session,
            role,
            dimensions,
            input,
        };
        let [height, width] = dimensions.map(|value| if value == 0 { 64 } else { value });
        model.run(Array4::zeros((1, 3, height, width)))?;
        Ok(model)
    }

    pub fn dimensions(&self) -> [usize; 2] {
        self.dimensions
    }

    pub fn run(&mut self, values: Array4<f32>) -> Result<ArrayD<f32>, Error> {
        let tensor = Tensor::from_array(values)?;
        let inputs: Vec<(String, SessionInputValue<'_>)> =
            vec![(self.input.clone(), tensor.into())];
        let output = self.session.run(inputs)?;
        let (shape, values) = output[0].try_extract_tensor::<f32>()?;
        validate_shape(self.role, shape, false)?;
        if values
            .iter()
            .any(|value| !value.is_finite() || !(0.0..=1.001).contains(value))
        {
            return Err(Error::Invalid(
                "OCR output probabilities are invalid".into(),
            ));
        }
        let shape = shape
            .iter()
            .map(|&value| value as usize)
            .collect::<Vec<_>>();
        ArrayD::from_shape_vec(shape, values.to_vec())
            .map_err(|error| Error::Invalid(error.to_string()))
    }
}

fn tensor(value: &ValueType) -> Result<Vec<i64>, Error> {
    match value {
        ValueType::Tensor {
            ty: TensorElementType::Float32,
            shape,
            ..
        } => Ok(shape.to_vec()),
        _ => Err(Error::Invalid("OCR tensors must be float32".into())),
    }
}

fn validate_shape(role: Role, shape: &[i64], dynamic: bool) -> Result<(), Error> {
    let matches = |value: i64, expected: i64| value == expected || dynamic && value == -1;
    let positive = |value: i64, limit: i64| (1..=limit).contains(&value) || dynamic && value == -1;
    let valid = match role {
        Role::Detector => {
            shape.len() == 4
                && matches(shape[0], 1)
                && matches(shape[1], 1)
                && shape[2..].iter().all(|&value| positive(value, 2048))
        }
        Role::Recognizer(classes) => {
            shape.len() == 3
                && matches(shape[0], 1)
                && positive(shape[1], 3200)
                && matches(shape[2], classes as i64)
        }
        Role::Orientation => shape.len() == 2 && matches(shape[0], 1) && matches(shape[1], 2),
    };
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid("unsupported OCR output dimensions".into()))
    }
}

#[cfg(test)]
mod tests;
