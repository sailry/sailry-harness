//! Office inspection, conversion and the execution Node authoring environment.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub path: String,
    /// Zero-based section offset. Long document parts continue in subsequent sections.
    #[serde(default)]
    pub offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Export {
    pub source: String,
    pub path: String,
    pub expected_revision: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Written {
    pub file: crate::FileWritten,
    pub warnings: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Preview {
    pub download: crate::FileDownload,
    pub source_revision: String,
    pub warnings: Vec<String>,
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

/// Managed authoring libraries on the execution Node, independent of the controller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Environment {
    pub python: String,
    pub python_version: String,
    pub packages: Vec<String>,
}
