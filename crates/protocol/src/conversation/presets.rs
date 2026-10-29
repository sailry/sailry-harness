//! Channel declarations and stable configuration identities, independent of credentials.
use super::{ModelApi, connection::Product as ProductKind};
use crate::{Authentication, plugin::Reference};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Catalog {
    pub version: u32,
    #[serde(default)]
    pub builtin: Vec<Preset>,
    #[serde(default)]
    pub partners: Vec<Preset>,
    #[serde(default)]
    pub policies: Vec<RequestPolicy>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub locales: BTreeMap<String, String>,
    pub logo: Option<String>,
    pub description: Option<String>,
    pub homepage: Option<String>,
    pub key_url: Option<String>,
    pub badge: Option<String>,
    /// Group identity scoped to this preset's source, independent of its route.
    pub brand: Option<String>,
    #[serde(default)]
    pub brand_display: Option<crate::plugin::desktop::Navigation>,
    #[serde(default = "offered")]
    pub offered: bool,
    pub routes: Vec<Route>,
    #[serde(default)]
    pub auth: Auth,
    #[serde(default)]
    pub models: Models,
    #[serde(default)]
    pub policies: Vec<RequestPolicy>,
    #[serde(default)]
    pub products: Vec<Product>,
    #[serde(default)]
    pub adapter: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Product {
    pub kind: ProductKind,
    #[serde(default)]
    pub display: Option<crate::plugin::desktop::Navigation>,
    pub default_model: Option<String>,
    #[serde(default = "offered")]
    pub discover: bool,
}

fn offered() -> bool {
    true
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub protocol: Protocol,
    pub endpoint: String,
    pub region: Option<Region>,
    pub product: Option<ProductKind>,
}

impl Route {
    pub fn key(&self) -> RouteKey {
        RouteKey {
            protocol: self.protocol,
            region: self.region,
            product: self.product,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RouteKey {
    pub protocol: Protocol,
    pub region: Option<Region>,
    pub product: Option<ProductKind>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Region {
    China,
    Global,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Protocol {
    #[serde(rename = "openai-responses")]
    OpenAiResponses,
    #[serde(rename = "openai-chat-completions")]
    OpenAiChatCompletions,
    Anthropic,
    Gemini,
    #[serde(rename = "deepseek")]
    DeepSeek,
    #[serde(rename = "opencode-go")]
    OpenCodeGo,
    #[serde(rename = "opencode-zen")]
    OpenCodeZen,
    #[serde(rename = "azure-openai")]
    AzureOpenAi,
    AzureAi,
    Plugin,
}

impl Protocol {
    pub fn api(self) -> ModelApi {
        match self {
            Self::OpenAiResponses => ModelApi::Responses,
            Self::OpenAiChatCompletions => ModelApi::ChatCompletions,
            Self::Anthropic => ModelApi::Anthropic,
            Self::Gemini => ModelApi::Gemini,
            Self::DeepSeek => ModelApi::DeepSeek,
            Self::OpenCodeGo => ModelApi::OpenCodeGo,
            Self::OpenCodeZen => ModelApi::OpenCodeZen,
            Self::AzureOpenAi => ModelApi::AzureOpenAi,
            Self::AzureAi => ModelApi::AzureAi,
            Self::Plugin => ModelApi::Plugin,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Auth {
    ApiKey {
        header: Option<String>,
        prefix: Option<String>,
    },
    None,
    Builtin {
        authentication: Authentication,
    },
    Plugin {
        flow: String,
    },
}

impl Default for Auth {
    fn default() -> Self {
        Self::ApiKey {
            header: None,
            prefix: None,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Models {
    #[default]
    Discover,
    Static {
        ids: Vec<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RequestPolicy {
    pub models: Vec<String>,
    #[serde(default)]
    pub reasoning_replay: Option<ReplayField>,
    #[serde(default)]
    pub body: serde_json::Map<String, serde_json::Value>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum ReplayField {
    ReasoningContent,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum Source {
    Builtin,
    Partner,
    Package { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresetRef {
    pub source: Source,
    pub id: String,
    pub route: Option<RouteKey>,
}

/// Resolved by Node at admission. Partner-only policies are also saved with
/// configured channels; executable package bindings cannot be controller-authored.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub package: Option<Reference>,
    #[serde(default)]
    pub origin: Option<super::consent::Origin>,
    #[serde(default)]
    pub auth: Option<crate::plugin::model_auth::Binding>,
    #[serde(default)]
    pub adapter: Option<crate::plugin::model_adapter::Binding>,
    #[serde(default)]
    pub policies: Vec<RequestPolicy>,
    #[serde(default)]
    pub key_injection: Option<crate::plugin::model_auth::Injection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub partners_unavailable: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub source: Source,
    pub preset: Preset,
    pub provenance: Provenance,
    pub package: Option<Reference>,
    #[serde(default)]
    pub origin: Option<super::consent::Origin>,
    #[serde(default)]
    pub requested: super::consent::Scopes,
    /// Resolved immutable package logo as a data URI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub login: Option<crate::plugin::model_auth::Presentation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    Official,
    Bundled,
    ThirdParty,
}

#[cfg(test)]
mod tests;
