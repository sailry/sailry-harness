//! The bounded settings form subset of the current Sailry extension contract.
use serde::{Deserialize, Serialize};
use serde_json::{Number, Value};
use std::collections::BTreeMap;

mod binding;
pub use binding::Binding;

pub const SCHEMA: &str = "https://json-schema.org/draft/2020-12/schema";
pub const MAX_FIELDS: usize = 32;
pub const MAX_BYTES: usize = 64 * 1024;
pub const MAX_STRING: usize = 4096;
pub const MAX_CHOICES: usize = 64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Schema {
    #[serde(rename = "$schema")]
    pub schema: String,
    #[serde(rename = "type")]
    pub kind: Object,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(
        default,
        rename = "x-sailry-locales",
        skip_serializing_if = "BTreeMap::is_empty"
    )]
    pub locales: BTreeMap<String, BTreeMap<String, String>>,
    #[serde(
        default,
        rename = "x-sailry-tabs",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub tabs: Vec<Tab>,
    pub properties: BTreeMap<String, Field>,
    #[serde(default)]
    pub required: Vec<String>,
    #[serde(rename = "additionalProperties")]
    pub additional_properties: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Tab {
    pub id: String,
    pub title: String,
    pub fields: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Object {
    Object,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Field {
    #[serde(rename = "type")]
    pub kind: Kind,
    #[serde(
        default,
        rename = "x-sailry-model",
        skip_serializing_if = "std::ops::Not::not"
    )]
    pub model: bool,
    #[serde(
        rename = "x-sailry-model-effort",
        skip_serializing_if = "Option::is_none"
    )]
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub default: Option<Value>,
    #[serde(rename = "enum", skip_serializing_if = "Option::is_none")]
    pub choices: Option<Vec<Value>>,
    #[serde(rename = "minLength", skip_serializing_if = "Option::is_none")]
    pub min_length: Option<usize>,
    #[serde(rename = "maxLength", skip_serializing_if = "Option::is_none")]
    pub max_length: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub minimum: Option<Number>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maximum: Option<Number>,
    #[serde(rename = "x-sailry-secret", skip_serializing_if = "Option::is_none")]
    pub secret: Option<Binding>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    String,
    Integer,
    Number,
    Boolean,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct State {
    pub package: super::Reference,
    pub values: BTreeMap<String, Value>,
    pub configured: Vec<String>,
    /// Secret fields whose binding still matches the stored configuration.
    pub keepable: Vec<String>,
    pub ready: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum SecretUpdate {
    Keep,
    Replace(crate::Secret),
    Clear,
}
