//! Captured package presentation with explicit controller-only capabilities.
use super::{Reference, desktop::Navigation};
use crate::{NodeId, RequestId, SessionId, TurnId, WorktreeId};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_CARDS: usize = 16;
pub const MAX_PROPS_BYTES: usize = 16 * 1024;
pub const MAX_SCHEMA_BYTES: usize = 32 * 1024;
pub const MAX_QR_BYTES: usize = 2048;

/// This identifies one canonical result block, not a permission token.
#[derive(
    Clone,
    Copy,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    #[schemars(with = "String")]
    pub session: SessionId,
    #[schemars(with = "String")]
    pub turn: TurnId,
    #[schemars(with = "String")]
    pub call: RequestId,
    pub block: u8,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CardDeclaration")]
pub struct Declaration {
    pub key: String,
    #[schemars(transform = super::schema::resource_path)]
    pub entry: String,
    /// Closed, bounded scalar properties. Credentials are never presentation data.
    pub props_schema: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_server: Option<String>,
    #[serde(default)]
    pub capabilities: Capabilities,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CardCapabilities")]
pub struct Capabilities {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qr: Option<Qr>,
    #[serde(default)]
    pub refresh: bool,
    #[serde(default)]
    pub browser: Vec<Target>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CardQr")]
pub struct Qr {
    pub payload: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires_at: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "CardBrowserTarget")]
pub struct Target {
    pub name: String,
    /// Fixed HTTPS origins, without paths, queries or credential material.
    pub origins: Vec<String>,
    pub source: Source,
    pub modes: Vec<Mode>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
#[schemars(rename = "CardBrowserSource")]
pub enum Source {
    Props {
        pointer: String,
    },
    /// The native controller owns the pending URL; package scripts never receive it.
    McpAuthorization,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "CardBrowserMode")]
pub enum Mode {
    Embedded,
    External,
}

/// A package supplies this draft. Only Node adds canonical ownership.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Draft {
    pub key: String,
    pub props: Value,
    pub fallback: Navigation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub receipt: Receipt,
    pub node: NodeId,
    pub worktree: WorktreeId,
    pub package: Reference,
    pub key: String,
    pub props: Value,
    pub fallback: Navigation,
}

impl Summary {
    pub fn valid(&self) -> bool {
        self.receipt.block < MAX_CARDS as u8
            && super::ui::identifier(&self.key)
            && !self.package.name.is_empty()
            && self.package.digest.len() == 64
            && self
                .package
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
            && public_props(&self.props)
            && self.fallback.valid()
    }
}

impl Declaration {
    pub fn accepts(&self, value: &Value) -> bool {
        let Some(object) = value.as_object().filter(|_| public_props(value)) else {
            return false;
        };
        let Some(properties) = self.props_schema["properties"].as_object() else {
            return false;
        };
        self.props_schema
            .get("required")
            .and_then(Value::as_array)
            .is_none_or(|required| {
                required
                    .iter()
                    .all(|name| name.as_str().is_some_and(|name| object.contains_key(name)))
            })
            && object.iter().all(|(name, value)| {
                let Some(schema) = properties.get(name) else {
                    return false;
                };
                let valid = match schema["type"].as_str() {
                    Some("string") => value.as_str().is_some_and(|text| {
                        let length = text.chars().count() as u64;
                        schema["maxLength"]
                            .as_u64()
                            .is_some_and(|max| length <= max)
                            && schema["minLength"].as_u64().is_none_or(|min| length >= min)
                    }),
                    Some("boolean") => value.is_boolean(),
                    Some("integer") => value.as_i64().is_some_and(|number| {
                        schema["minimum"].as_i64().is_none_or(|min| number >= min)
                            && schema["maximum"].as_i64().is_none_or(|max| number <= max)
                    }),
                    _ => false,
                };
                valid
                    && schema.get("enum").is_none_or(|choices| {
                        choices
                            .as_array()
                            .is_some_and(|choices| choices.contains(value))
                    })
            })
    }

    pub fn valid(&self, extension: &super::Extension) -> bool {
        let Some(desktop) = &extension.desktop else {
            return false;
        };
        super::ui::identifier(&self.key)
            && self.entry.starts_with("dev.sailry.platform/")
            && (self.entry.ends_with(".js") || self.entry.ends_with(".mjs"))
            && desktop.resources.contains(&self.entry)
            && schema(&self.props_schema)
            && self
                .mcp_server
                .as_ref()
                .is_none_or(|name| super::ui::identifier(name))
            && self.capabilities.qr.as_ref().is_none_or(|qr| {
                self.property(&qr.payload, "string")
                    && qr
                        .expires_at
                        .as_ref()
                        .is_none_or(|pointer| self.property(pointer, "integer"))
            })
            && self.capabilities.browser.len() <= 8
            && self
                .capabilities
                .browser
                .iter()
                .enumerate()
                .all(|(index, target)| {
                    super::ui::identifier(&target.name)
                        && !self.capabilities.browser[..index]
                            .iter()
                            .any(|other| other.name == target.name)
                        && !target.origins.is_empty()
                        && target.origins.len() <= 8
                        && target.origins.iter().all(|origin| {
                            url::Url::parse(origin).is_ok_and(|url| {
                                url.scheme() == "https"
                                    && url.username().is_empty()
                                    && url.password().is_none()
                                    && url.query().is_none()
                                    && url.fragment().is_none()
                                    && url.path() == "/"
                                    && url.origin().ascii_serialization() == *origin
                            })
                        })
                        && !target.modes.is_empty()
                        && target.modes.len() <= 2
                        && target
                            .modes
                            .iter()
                            .enumerate()
                            .all(|(index, mode)| !target.modes[..index].contains(mode))
                        && match &target.source {
                            Source::Props { pointer } => self.property(pointer, "string"),
                            Source::McpAuthorization => self.mcp_server.is_some(),
                        }
                })
    }

    fn property(&self, pointer: &str, kind: &str) -> bool {
        pointer
            .strip_prefix('/')
            .filter(|name| !name.contains('/'))
            .is_some_and(|name| {
                self.props_schema["properties"][name]["type"].as_str() == Some(kind)
            })
    }
}

pub fn public_props(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    object.len() <= 32
        && object.iter().all(|(name, value)| {
            public_name(name)
                && match value {
                    Value::String(text) => text.len() <= 4096 && !authorization_url(text),
                    Value::Bool(_) | Value::Null => true,
                    Value::Number(number) => number.as_i64().is_some() || number.as_u64().is_some(),
                    _ => false,
                }
        })
        && serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= MAX_PROPS_BYTES)
}

fn public_name(name: &str) -> bool {
    super::ui::identifier(name)
        && !matches!(
            name.to_ascii_lowercase().as_str(),
            "token"
                | "access_token"
                | "refresh_token"
                | "client_secret"
                | "credential"
                | "credentials"
                | "password"
                | "authorization"
                | "authorization_url"
                | "callback"
                | "callback_url"
                | "code_verifier"
                | "pkce"
                | "script"
                | "html"
                | "command"
        )
}

fn authorization_url(value: &str) -> bool {
    url::Url::parse(value).is_ok_and(|url| {
        url.query_pairs().any(|(name, _)| {
            matches!(
                name.as_ref(),
                "access_token"
                    | "refresh_token"
                    | "client_secret"
                    | "code_verifier"
                    | "code_challenge"
                    | "redirect_uri"
                    | "response_type"
            )
        })
    })
}

fn schema(value: &Value) -> bool {
    if value["type"] != "object"
        || value["additionalProperties"] != false
        || !serde_json::to_vec(value).is_ok_and(|bytes| bytes.len() <= MAX_SCHEMA_BYTES)
    {
        return false;
    }
    let Some(properties) = value["properties"].as_object() else {
        return false;
    };
    let Some(required) = value.get("required").map_or(Some(&[][..]), |required| {
        required.as_array().map(Vec::as_slice)
    }) else {
        return false;
    };
    properties.len() <= 32
        && value.as_object().is_some_and(|object| {
            object.keys().all(|key| {
                matches!(
                    key.as_str(),
                    "type" | "properties" | "required" | "additionalProperties"
                )
            })
        })
        && required.iter().all(|name| {
            name.as_str()
                .is_some_and(|name| properties.contains_key(name))
        })
        && properties.iter().all(|(name, property)| {
            public_name(name)
                && property.as_object().is_some_and(|object| {
                    object.keys().all(|key| {
                        matches!(
                            key.as_str(),
                            "type"
                                | "maxLength"
                                | "minLength"
                                | "enum"
                                | "description"
                                | "minimum"
                                | "maximum"
                        )
                    })
                })
                && match property["type"].as_str() {
                    Some("string") => property["maxLength"]
                        .as_u64()
                        .is_some_and(|length| length > 0 && length <= 4096),
                    Some("integer" | "boolean") => true,
                    _ => false,
                }
        })
}
