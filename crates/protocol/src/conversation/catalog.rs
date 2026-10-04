use serde::{Deserialize, Serialize};
mod reasoning;

/// Reference metadata cached by the execution Node, independent of provider access.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub revision: u64,
    pub updated_at_ms: Option<u64>,
    pub providers: u32,
    pub models: u32,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Query {
    pub provider: Option<String>,
    /// Exact model identifiers; required for a lookup across providers.
    pub ids: Vec<String>,
    pub revision: Option<u64>,
    pub after: Option<String>,
    pub limit: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub revision: u64,
    pub models: Vec<Model>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub id: String,
    pub name: String,
    pub context: Option<u32>,
    pub output: Option<u32>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub tools: Option<bool>,
    pub reasoning: Option<bool>,
    pub options: Vec<Reasoning>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Reasoning {
    Toggle,
    Effort { values: Vec<Option<String>> },
    BudgetTokens { min: Option<i32>, max: Option<u32> },
}
