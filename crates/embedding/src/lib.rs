pub mod catalog;

mod onnx;
mod pool;
mod table;

#[cfg(test)]
mod tests;

pub use catalog::ModelId;
pub use pool::background;
use sailry_protocol::models::{Definition, Format, Pooling};
use std::borrow::Cow;
use std::{fs, path::Path};
use tokenizers::Tokenizer;

pub const MAX_INPUT_TOKENS: usize = 512;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Input {
    Query,
    Document,
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("embedding file could not be read: {0}")]
    Io(#[from] std::io::Error),
    #[error("embedding configuration is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("embedding inference failed: {0}")]
    Ort(#[from] ort::Error),
    #[error("embedding runtime could not load: {0}")]
    Runtime(#[from] sailry_model_assets::runtime::RuntimeError),
    #[error("embedding tokenizer failed: {0}")]
    Tokenizer(String),
    #[error("embedding model is invalid: {0}")]
    Invalid(String),
    #[error("embedding input exceeds the {MAX_INPUT_TOKENS}-token window")]
    TooLong,
    #[error("embedding worker pool could not start: {0}")]
    Pool(#[from] rayon::ThreadPoolBuildError),
}

pub struct Engine {
    dimension: usize,
    tokenizer: Tokenizer,
    kernel: Kernel,
    pool: rayon::ThreadPool,
    query_prefix: Option<String>,
    document_prefix: Option<String>,
}

enum Kernel {
    Onnx(Box<onnx::Model>),
    Static(table::Table),
}

impl Engine {
    /// Loads files already acquired and verified against the pinned catalog.
    pub fn load(id: ModelId, directory: &Path) -> Result<Self, Error> {
        let dimension = id.descriptor().dimension as u16;
        let format = match id {
            ModelId::GraniteMulti | ModelId::BgeSmallZh => Format::Transformer {
                dimension,
                pooling: Pooling::Cls,
                token_types: id == ModelId::BgeSmallZh,
            },
            ModelId::PotionMulti => Format::Static { dimension },
        };
        Self::load_format(format, directory)
    }

    /// Loads a supported algorithm from files verified by the model owner.
    pub fn load_format(format: Format, directory: &Path) -> Result<Self, Error> {
        let dimension = format
            .dimension()
            .filter(|dimension| (1..=4096).contains(dimension))
            .ok_or_else(|| Error::Invalid("model format is not a supported embedding".into()))?;
        let pool = pool::create()?;
        let tokenizer = Tokenizer::from_file(directory.join("tokenizer.json"))
            .map_err(|error| Error::Tokenizer(error.to_string()))?;
        Self::prepare(format, usize::from(dimension), tokenizer, directory, pool)
    }

    /// Loads the immutable algorithm and input prefixes selected by the owner.
    pub fn load_definition(definition: &Definition, directory: &Path) -> Result<Self, Error> {
        definition
            .validate()
            .map_err(|fault| Error::Invalid(fault.message))?;
        let mut engine = Self::load_format(definition.format, directory)?;
        if let Some(prefix) = definition.document_prefix.as_deref() {
            let special = !matches!(engine.kernel, Kernel::Static(_));
            let tokens = engine
                .pool
                .install(|| engine.tokenizer.encode(prefix, special))
                .map_err(|error| Error::Tokenizer(error.to_string()))?
                .len();
            if tokens > MAX_INPUT_TOKENS / 2 {
                return Err(Error::Invalid(
                    "document prefix leaves fewer than 256 content tokens".into(),
                ));
            }
        }
        engine.query_prefix = definition.query_prefix.clone();
        engine.document_prefix = definition.document_prefix.clone();
        Ok(engine)
    }

    fn prepare(
        format: Format,
        dimension: usize,
        mut tokenizer: Tokenizer,
        directory: &Path,
        pool: rayon::ThreadPool,
    ) -> Result<Self, Error> {
        tokenizer
            .with_truncation(None)
            .map_err(|error| Error::Tokenizer(error.to_string()))?;
        tokenizer.with_padding(None);
        let kernel = pool.install(|| Kernel::load(format, directory, &tokenizer))?;
        Ok(Self {
            dimension,
            tokenizer,
            kernel,
            pool,
            query_prefix: None,
            document_prefix: None,
        })
    }

    pub fn dimension(&self) -> usize {
        self.dimension
    }

    /// Returns original input byte offsets without special tokens or truncation.
    pub fn offsets(&self, text: &str) -> Result<Vec<(usize, usize)>, Error> {
        self.pool.install(|| {
            let encoded = self
                .tokenizer
                .encode(text, false)
                .map_err(|error| Error::Tokenizer(error.to_string()))?;
            Ok(encoded.get_offsets().to_vec())
        })
    }

    pub fn encode(&mut self, inputs: &[&str]) -> Result<Vec<Vec<f32>>, Error> {
        self.encode_for(Input::Document, inputs)
    }

    pub fn encode_for(&mut self, input: Input, inputs: &[&str]) -> Result<Vec<Vec<f32>>, Error> {
        let dimension = self.dimension();
        let tokenizer = &self.tokenizer;
        let kernel = &mut self.kernel;
        let special = !matches!(kernel, Kernel::Static(_));
        let prefix = match input {
            Input::Query => self.query_prefix.as_deref(),
            Input::Document => self.document_prefix.as_deref(),
        };
        self.pool.install(|| {
            inputs
                .iter()
                .map(|text| {
                    let text = match prefix {
                        Some(prefix) => Cow::Owned(format!("{prefix}{text}")),
                        None => Cow::Borrowed(*text),
                    };
                    let encoded = tokenizer
                        .encode(text.as_ref(), special)
                        .map_err(|error| Error::Tokenizer(error.to_string()))?;
                    if encoded.len() > MAX_INPUT_TOKENS {
                        return Err(Error::TooLong);
                    }
                    let mut vector = match kernel {
                        Kernel::Onnx(model) => model.encode(&encoded)?,
                        Kernel::Static(table) => table.mean(encoded.get_ids())?,
                    };
                    if vector.len() != dimension {
                        return Err(Error::Invalid(
                            "output dimension does not match the model".into(),
                        ));
                    }
                    normalize(&mut vector)?;
                    Ok(vector)
                })
                .collect()
        })
    }
}

impl Kernel {
    fn load(format: Format, directory: &Path, tokenizer: &Tokenizer) -> Result<Self, Error> {
        let config = fs::read(directory.join("config.json"))?;
        match format {
            Format::Static { dimension } => {
                let config: serde_json::Value = serde_json::from_slice(&config)?;
                if config.get("normalize").and_then(serde_json::Value::as_bool) != Some(true) {
                    return Err(Error::Invalid("static model requires normalization".into()));
                }
                let unknown = match tokenizer.get_model() {
                    tokenizers::models::ModelWrapper::BPE(model) => model
                        .get_unk_token()
                        .as_deref()
                        .and_then(|token| tokenizer.token_to_id(token)),
                    tokenizers::models::ModelWrapper::WordPiece(model) => {
                        tokenizer.token_to_id(&model.unk_token)
                    }
                    tokenizers::models::ModelWrapper::WordLevel(model) => {
                        tokenizer.token_to_id(&model.unk_token)
                    }
                    tokenizers::models::ModelWrapper::Unigram(model) => {
                        let model = serde_json::to_value(model)?;
                        match model.get("unk_id") {
                            Some(serde_json::Value::Null) => None,
                            Some(value) => Some(
                                value
                                    .as_u64()
                                    .and_then(|id| u32::try_from(id).ok())
                                    .ok_or_else(|| {
                                        Error::Invalid("unigram unknown token ID is invalid".into())
                                    })?,
                            ),
                            None => {
                                return Err(Error::Invalid(
                                    "unigram model is missing its unknown token ID".into(),
                                ));
                            }
                        }
                    }
                };
                let table = table::Table::load(&directory.join("model.safetensors"), unknown)?;
                if table.dimension() != usize::from(dimension)
                    || table.rows() != tokenizer.get_vocab_size(true)
                {
                    return Err(Error::Invalid(
                        "static table does not match its tokenizer".into(),
                    ));
                }
                Ok(Self::Static(table))
            }
            Format::Transformer {
                dimension,
                pooling,
                token_types,
            } => {
                let config: serde_json::Value = serde_json::from_slice(&config)?;
                if !config.is_object() {
                    return Err(Error::Invalid(
                        "model configuration must be an object".into(),
                    ));
                }
                Ok(Self::Onnx(Box::new(onnx::Model::load(
                    usize::from(dimension),
                    pooling,
                    token_types,
                    directory,
                )?)))
            }
            Format::SenseVoice {} => Err(Error::Invalid(
                "speech format cannot produce embeddings".into(),
            )),
        }
    }
}

fn normalize(vector: &mut [f32]) -> Result<(), Error> {
    if vector.iter().any(|value| !value.is_finite()) {
        return Err(Error::Invalid("output contains a nonfinite value".into()));
    }
    let norm = vector
        .iter()
        .map(|&value| f64::from(value).powi(2))
        .sum::<f64>()
        .sqrt();
    if norm > 0.0 {
        for value in vector {
            *value = (f64::from(*value) / norm) as f32;
        }
    }
    Ok(())
}
