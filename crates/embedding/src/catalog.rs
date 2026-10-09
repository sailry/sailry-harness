use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelId {
    GraniteMulti,
    BgeSmallZh,
    PotionMulti,
}

#[derive(Clone, Copy, Debug)]
pub struct Asset {
    pub name: &'static str,
    pub bytes: u64,
    pub sha256: &'static str,
    pub source: Option<Source>,
}

#[derive(Clone, Copy, Debug)]
pub struct Source {
    pub repository: &'static str,
    pub revision: &'static str,
    pub path: &'static str,
}

#[derive(Clone, Copy, Debug)]
pub struct Model {
    pub id: ModelId,
    pub repository: &'static str,
    pub revision: &'static str,
    pub license: &'static str,
    pub dimension: usize,
    pub context: Option<usize>,
    pub assets: &'static [Asset],
}

impl Model {
    pub fn url(&self, asset: &Asset) -> String {
        self.url_at(asset, "https://huggingface.co")
    }

    pub fn url_at(&self, asset: &Asset, base: &str) -> String {
        let source = asset.source.unwrap_or(Source {
            repository: self.repository,
            revision: self.revision,
            path: asset.name,
        });
        format!(
            "{}/{}/resolve/{}/{}",
            base.trim_end_matches('/'),
            source.repository,
            source.revision,
            source.path
        )
    }
}

impl ModelId {
    pub const fn descriptor(self) -> &'static Model {
        match self {
            Self::GraniteMulti => &GRANITE,
            Self::BgeSmallZh => &BGE,
            Self::PotionMulti => &POTION,
        }
    }
}

pub const MODELS: &[Model] = &[GRANITE, BGE, POTION];

const GRANITE: Model = Model {
    id: ModelId::GraniteMulti,
    repository: "ibm-granite/granite-embedding-97m-multilingual-r2",
    revision: "835ad14087e140460703cf0fae09f97d469d65c2",
    license: "Apache-2.0",
    dimension: 384,
    context: Some(32768),
    assets: &[
        Asset {
            name: "config.json",
            bytes: 1216,
            sha256: "de948b0bdc6f356afad7a84b276d8dd7e7fe10fb9add1bb5e610621c28e41ebc",
            source: None,
        },
        Asset {
            name: "tokenizer.json",
            bytes: 25301672,
            sha256: "4f2842d568e2724370aec203652a42ac783c7937f8347a1a2cc7506d71f1582f",
            source: None,
        },
        Asset {
            name: "model.onnx",
            bytes: 390004608,
            sha256: "68e592b160673d30250824c1116bc6ab33f70efb22b97c9e1d7ce1e69c1c9d70",
            source: Some(Source {
                repository: "ibm-granite/granite-embedding-97m-multilingual-r2",
                revision: "835ad14087e140460703cf0fae09f97d469d65c2",
                path: "onnx/model.onnx",
            }),
        },
    ],
};

const BGE: Model = Model {
    id: ModelId::BgeSmallZh,
    repository: "BAAI/bge-small-zh-v1.5",
    revision: "7999e1d3359715c523056ef9478215996d62a620",
    license: "MIT",
    dimension: 512,
    context: Some(512),
    assets: &[
        Asset {
            name: "config.json",
            bytes: 776,
            sha256: "3853a7979202c348751b753e36f579c41d8da7d36af617d3d907e1fc9b441f2a",
            source: None,
        },
        Asset {
            name: "tokenizer.json",
            bytes: 439125,
            sha256: "48cea5d44424912a6fd1ea647bf4fe50b55ab8b1e5879c3275f80e339e8fae26",
            source: None,
        },
        Asset {
            name: "model.onnx",
            bytes: 94851877,
            sha256: "69a0b846f4f116b5e6aabf9546ea6754d02264f3211a13a1bd69b31b8040749a",
            source: Some(Source {
                repository: "Xenova/bge-small-zh-v1.5",
                revision: "75c43b069aac4d136ba6bc1122f995fedcfd2781",
                path: "onnx/model.onnx",
            }),
        },
    ],
};

const POTION: Model = Model {
    id: ModelId::PotionMulti,
    repository: "minishlab/potion-multilingual-128M",
    revision: "73908c3438cf03b6a01bcb9611d62b23d0726f08",
    license: "MIT",
    dimension: 256,
    context: None,
    assets: &[
        Asset {
            name: "config.json",
            bytes: 271,
            sha256: "595e4cab2093732efd5dbe084fd5c1826b5eea693b73b4c1fd971672867d2e54",
            source: None,
        },
        Asset {
            name: "tokenizer.json",
            bytes: 18616131,
            sha256: "19f1909063da3cfe3bd83a782381f040dccea475f4816de11116444a73e1b6a1",
            source: None,
        },
        Asset {
            name: "model.safetensors",
            bytes: 512361560,
            sha256: "14b5eb39cb4ce5666da8ad1f3dc6be4346e9b2d601c073302fa0a31bf7943397",
            source: None,
        },
    ],
};
