//! Public model choices contain no provider endpoints or credential material.
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Provider,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub kind: Kind,
    pub id: String,
    pub provider: String,
    pub model: String,
    /// The provider's configured default model.
    pub default: bool,
    pub reasoning: bool,
    pub efforts: Vec<crate::Effort>,
    pub default_effort: crate::Effort,
}

pub fn effort_key(effort: crate::Effort) -> String {
    use crate::Effort;
    match effort {
        Effort::Default => "default",
        Effort::Disabled => "none",
        Effort::Minimal => "minimal",
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
        Effort::Budget(tokens) => return tokens.to_string(),
    }
    .into()
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Catalog {
    pub models: Vec<Model>,
}
