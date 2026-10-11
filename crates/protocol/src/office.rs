//! Native Office inspection.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub source: Source,
    /// Zero-based section offset. Long document parts continue in subsequent sections.
    #[serde(default)]
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Source {
    Path(String),
    Attachment(#[schemars(with = "String")] crate::AttachmentId),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Inspection {
    pub path: String,
    pub revision: String,
    pub sections: Vec<Section>,
    pub next: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub name: String,
    pub text: String,
    pub truncated: bool,
}
