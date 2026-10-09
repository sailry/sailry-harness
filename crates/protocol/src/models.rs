//! Public model discovery metadata and pinned installation definitions.
use crate::Fault;
use serde::{Deserialize, Serialize};

pub const MAX_ASSETS: usize = 8;
pub const MAX_ASSET_BYTES: u64 = 2 * 1024 * 1024 * 1024;
pub const MAX_PREFIX_BYTES: usize = 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Hub {
    #[default]
    HuggingFace,
    ModelScope,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Embedding,
    Speech,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pooling {
    Cls,
    Mean,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Format {
    Transformer {
        dimension: u16,
        pooling: Pooling,
        token_types: bool,
    },
    Static {
        dimension: u16,
    },
    SenseVoice {},
}

impl Format {
    pub fn kind(self) -> Kind {
        match self {
            Self::Transformer { .. } | Self::Static { .. } => Kind::Embedding,
            Self::SenseVoice {} => Kind::Speech,
        }
    }

    pub fn dimension(self) -> Option<u16> {
        match self {
            Self::Transformer { dimension, .. } | Self::Static { dimension } => Some(dimension),
            Self::SenseVoice {} => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub hub: Hub,
    pub repository: String,
    pub revision: String,
}

impl Source {
    pub fn validate(&self) -> Result<(), Fault> {
        let valid = |value: &str, slash: bool| {
            !value.is_empty()
                && value.len() <= 256
                && value
                    .split('/')
                    .all(|part| !matches!(part, "" | "." | ".."))
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(byte, b'-' | b'_' | b'.')
                        || slash && byte == b'/'
                })
        };
        if !valid(&self.repository, true)
            || self.repository.split('/').count() > 2
            || !valid(&self.revision, true)
        {
            return Err(Fault::invalid("invalid model repository or revision"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Asset {
    pub source: Source,
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Definition {
    pub source: Source,
    pub name: String,
    pub license: String,
    pub format: Format,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query_prefix: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub document_prefix: Option<String>,
    pub assets: Vec<Asset>,
}

impl Definition {
    pub fn validate(&self) -> Result<(), Fault> {
        self.source.validate()?;
        if self.name.trim().is_empty()
            || self.name.len() > 256
            || self.name.contains('\0')
            || self.license.len() > 256
            || self.license.contains('\0')
            || [&self.query_prefix, &self.document_prefix]
                .into_iter()
                .flatten()
                .any(|prefix| prefix.len() > MAX_PREFIX_BYTES || prefix.contains('\0'))
            || self.format.kind() == Kind::Speech
                && (self.query_prefix.is_some() || self.document_prefix.is_some())
            || self
                .format
                .dimension()
                .is_some_and(|size| !(1..=4096).contains(&size))
            || self.assets.is_empty()
            || self.assets.len() > MAX_ASSETS
        {
            return Err(Fault::invalid("invalid model definition"));
        }
        let expected: &[&str] = match self.format {
            Format::Transformer { .. } => &["config.json", "tokenizer.json", "model.onnx"],
            Format::Static { .. } => &["config.json", "tokenizer.json", "model.safetensors"],
            Format::SenseVoice {} => &["model.onnx", "tokens.txt"],
        };
        let mut names = std::collections::BTreeSet::new();
        for asset in &self.assets {
            asset.source.validate()?;
            if asset.source.revision.len() != 40
                || !asset
                    .source
                    .revision
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
                || asset.path.len() > 1024
                || asset.path.split('/').any(|part| {
                    matches!(part, "" | "." | "..")
                        || !part.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')
                        })
                })
                || !expected.contains(&asset.name.as_str())
                || !names.insert(asset.name.as_str())
                || asset.bytes == 0
                || asset.bytes > MAX_ASSET_BYTES
                || asset.sha256.len() != 64
                || !asset.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(Fault::invalid("invalid pinned model asset"));
            }
        }
        if expected.iter().any(|name| !names.contains(name)) {
            return Err(Fault::invalid("model assets do not match its format"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub hub: Hub,
    pub kind: Kind,
    pub query: String,
    pub cursor: Option<String>,
}

impl Search {
    pub fn validate(&self) -> Result<(), Fault> {
        if self.query.trim().is_empty()
            || self.query.len() > 512
            || self.query.contains('\0')
            || self
                .cursor
                .as_ref()
                .is_some_and(|value| value.len() > 8192 || value.contains('\0'))
        {
            return Err(Fault::invalid("invalid model search"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Item {
    pub repository: String,
    pub name: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub items: Vec<Item>,
    pub next_cursor: Option<String>,
    pub partial: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Version {
    pub name: String,
    pub revision: String,
}

#[cfg(test)]
mod tests;
