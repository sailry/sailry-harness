//! Non-secret cloud routing options, frozen with the selected provider.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Options {
    AzureOpenAi { api_version: String },
    Bedrock { region: String },
    Vertex { project: String, location: String },
}
