//! Kernel-independent access to version-four notebook cells.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_CELLS: usize = 100;
pub const MAX_PAGE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Read {
    pub path: String,
    #[serde(default)]
    pub offset: usize,
    #[serde(default = "limit")]
    pub limit: usize,
}

fn limit() -> usize {
    20
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    pub path: String,
    pub expected_revision: String,
    /// Indices refer to the notebook before this batch is applied.
    pub edits: Vec<Change>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Change {
    Replace {
        index: usize,
        source: String,
    },
    Insert {
        index: usize,
        cell_type: Kind,
        source: String,
    },
    Delete {
        index: usize,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Code,
    Markdown,
    Raw,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub path: String,
    pub revision: String,
    pub nbformat: u32,
    pub nbformat_minor: u32,
    pub metadata: Value,
    pub metadata_truncated: bool,
    pub total: usize,
    pub cells: Vec<Cell>,
    pub next_offset: Option<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cell {
    pub index: usize,
    /// Original cell JSON, or a bounded source projection when truncated.
    pub value: Value,
    pub truncated: bool,
}
