use crate::Error;
use ort::{
    session::{Session, SessionInputValue},
    value::{Tensor, TensorElementType, ValueType},
};
use sailry_protocol::models::Pooling;
use std::path::Path;
use tokenizers::Encoding;

#[cfg(test)]
#[path = "onnx/tests.rs"]
mod tests;

pub(super) struct Model {
    session: Session,
    dimension: usize,
    types: bool,
    pooling: Pooling,
}

impl Model {
    pub fn load(
        dimension: usize,
        pooling: Pooling,
        types: bool,
        directory: &Path,
    ) -> Result<Self, Error> {
        sailry_model_assets::onnx::validate(&directory.join("model.onnx"))?;
        sailry_model_assets::runtime::initialize()?;
        let session = sailry_model_assets::session::builder()?
            .commit_from_file(directory.join("model.onnx"))?;
        let names: &[&str] = if types {
            &["input_ids", "attention_mask", "token_type_ids"]
        } else {
            &["input_ids", "attention_mask"]
        };
        if session.inputs().len() != names.len()
            || !names.iter().all(|name| {
                session.inputs().iter().any(|input| {
                    input.name() == *name
                        && matches!(input.dtype(), ValueType::Tensor {
                            ty: TensorElementType::Int64, shape, ..
                        } if shape.len() == 2 && matches!(shape[0], -1 | 1) && shape[1] == -1)
                })
            })
            || session.outputs().len() != 1
            || !matches!(session.outputs()[0].dtype(), ValueType::Tensor {
                ty: TensorElementType::Float32, shape, ..
            } if shape.len() == 3 && matches!(shape[0], -1 | 1) && shape[1] == -1 && (shape[2] == -1 || shape[2] == dimension as i64))
        {
            return Err(Error::Invalid("ONNX model interface is invalid".into()));
        }
        Ok(Self {
            session,
            dimension,
            types,
            pooling,
        })
    }

    pub fn encode(&mut self, encoding: &Encoding) -> Result<Vec<f32>, Error> {
        let mut inputs = vec![
            input("input_ids", encoding.get_ids())?,
            input("attention_mask", encoding.get_attention_mask())?,
        ];
        if self.types {
            inputs.push(input("token_type_ids", encoding.get_type_ids())?);
        }
        let outputs = self.session.run(inputs)?;
        let (shape, values) = outputs[0].try_extract_tensor::<f32>()?;
        pool_output(
            shape,
            values,
            encoding.get_attention_mask(),
            self.dimension,
            self.pooling,
        )
    }
}

fn input(name: &str, values: &[u32]) -> Result<(String, SessionInputValue<'static>), Error> {
    let values = values
        .iter()
        .map(|&value| i64::from(value))
        .collect::<Vec<_>>();
    Ok((
        name.to_owned(),
        Tensor::from_array(([1, values.len()], values))?.into(),
    ))
}

fn pool_output(
    shape: &[i64],
    values: &[f32],
    mask: &[u32],
    dimension: usize,
    pooling: Pooling,
) -> Result<Vec<f32>, Error> {
    let tokens = mask.len();
    let length = tokens.checked_mul(dimension);
    if tokens == 0
        || dimension == 0
        || shape != [1, tokens as i64, dimension as i64]
        || length != Some(values.len())
    {
        return Err(Error::Invalid(
            "ONNX output shape does not match its input".into(),
        ));
    }
    match pooling {
        Pooling::Cls => Ok(values[..dimension].to_vec()),
        Pooling::Mean => {
            let mut sum = vec![0.0f64; dimension];
            let mut count = 0usize;
            for (row, &included) in values.chunks_exact(dimension).zip(mask) {
                if included != 0 {
                    for (sum, &value) in sum.iter_mut().zip(row) {
                        *sum += f64::from(value);
                    }
                    count += 1;
                }
            }
            Ok(sum
                .into_iter()
                .map(|sum| {
                    if count == 0 {
                        0.0
                    } else {
                        (sum / count as f64) as f32
                    }
                })
                .collect())
        }
    }
}
