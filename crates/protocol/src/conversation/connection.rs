//! Non-secret connection options, frozen with the selected provider.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Options {
    AzureOpenAi { api_version: String },
    Hosted { product: Product },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Product {
    Metered,
    Coding,
}

#[cfg(test)]
mod tests;
