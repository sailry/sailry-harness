//! Public connection suggestions, independent of model capabilities and credentials.
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    pub partners: Vec<Partner>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Partner {
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logo: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub badge: Option<String>,
    pub routes: Vec<Route>,
    /// Suggested identifiers only; capability metadata still comes from model discovery.
    #[serde(default)]
    pub models: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub protocol: Protocol,
    pub endpoint: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    #[serde(rename = "openai-responses")]
    OpenAiResponses,
    #[serde(rename = "openai-chat-completions")]
    OpenAiChatCompletions,
    Anthropic,
    Gemini,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub catalog: Catalog,
    /// Online loading failed; the catalog remains the last valid or bundled document.
    pub unavailable: bool,
}
