//! Plugin-owned key-value data stored on the execution Node.
use serde::{Deserialize, Serialize};

pub const MAX_KEY_BYTES: usize = 128;
pub const MAX_VALUE_BYTES: usize = 256 * 1024;
pub const MAX_NAMESPACE_BYTES: usize = 128 * 1024 * 1024;
pub const MAX_NAMESPACE_KEYS: usize = 4096;
pub const MAX_PAGE_LIMIT: u16 = 100;
pub const MAX_INDEX_BYTES: usize = 64 * 1024;
pub const MAX_TAGS: usize = 16;
pub const MAX_TAG_BYTES: usize = 128;
pub const MAX_TERMS: usize = 256;
pub const MAX_QUERY_BYTES: usize = 8 * 1024;
pub const MAX_SEARCH_BYTES: usize = 512 * 1024;
pub const MAX_COLLECTIONS: usize = 32;
pub const MAX_SCHEMA_BYTES: usize = 64 * 1024;
pub const MAX_SCHEMA_DEPTH: usize = 64;

/// Schemas remain package declarations, not SQL tables or migration versions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "StorageDeclaration")]
pub struct Declaration {
    #[schemars(length(max = MAX_COLLECTIONS))]
    pub collections: Vec<Collection>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(rename = "StorageCollection", transform = collection_schema)]
pub struct Collection {
    pub scope: Scope,
    /// Select one exact key or one nonempty prefix, never both.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string", "minLength" = 1, "maxLength" = MAX_KEY_BYTES))]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(extend("type" = "string", "minLength" = 1, "maxLength" = MAX_KEY_BYTES))]
    pub prefix: Option<String>,
    #[schemars(extend("type" = ["object", "boolean"]))]
    pub schema: serde_json::Value,
}

fn collection_schema(schema: &mut schemars::Schema) {
    schema.insert(
        "oneOf".into(),
        serde_json::json!([
            {"required":["key"], "not":{"required":["prefix"]}},
            {"required":["prefix"], "not":{"required":["key"]}}
        ]),
    );
}

impl Collection {
    pub fn matches(&self, scope: Scope, key: &str) -> bool {
        self.scope == scope
            && (self.key.as_deref() == Some(key)
                || self
                    .prefix
                    .as_ref()
                    .is_some_and(|prefix| key.starts_with(prefix)))
    }

    fn overlaps(&self, other: &Self) -> bool {
        self.scope == other.scope
            && match (&self.key, &self.prefix, &other.key, &other.prefix) {
                (Some(left), None, Some(right), None) => left == right,
                (Some(key), None, None, Some(prefix)) | (None, Some(prefix), Some(key), None) => {
                    key.starts_with(prefix)
                }
                (None, Some(left), None, Some(right)) => {
                    left.starts_with(right) || right.starts_with(left)
                }
                _ => true,
            }
    }
}

impl Declaration {
    pub fn valid(&self) -> bool {
        self.collections.len() <= MAX_COLLECTIONS
            && self
                .collections
                .iter()
                .enumerate()
                .all(|(index, collection)| {
                    let selector = match (&collection.key, &collection.prefix) {
                        (Some(key), None) => key,
                        (None, Some(prefix)) => prefix,
                        _ => return false,
                    };
                    !selector.is_empty()
                        && selector.len() <= MAX_KEY_BYTES
                        && !selector.contains('\0')
                        && (collection.schema.is_object() || collection.schema.is_boolean())
                        && !self.collections[..index]
                            .iter()
                            .any(|other| collection.overlaps(other))
                })
    }
}

/// Private data either belongs to the Node or follows the captured conversation history.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
#[schemars(rename = "StorageScope")]
pub enum Scope {
    Node,
    Conversation,
}

/// Searchable fields and opaque package-owned filters, not a product schema.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Index {
    pub fields: [String; 2],
    pub tags: Vec<String>,
    pub order: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    pub terms: Vec<String>,
    #[serde(default = "default_weights")]
    pub weights: [u16; 2],
    #[serde(default)]
    pub all: Vec<String>,
    #[serde(default)]
    pub any: Vec<String>,
    #[serde(default)]
    pub offset: u16,
    #[serde(default = "default_limit")]
    pub limit: u16,
}

fn default_weights() -> [u16; 2] {
    [1, 1]
}
fn default_limit() -> u16 {
    MAX_PAGE_LIMIT
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchPage {
    pub entries: Vec<Entry>,
    pub next: Option<u16>,
    /// Execution Node time observed by the read, not a mutation commit timestamp.
    pub now_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub key: String,
    /// Zero means no value has ever been written for this key.
    pub revision: u64,
    /// Missing or deleted values are JSON null; consult `present` for existence.
    pub value: serde_json::Value,
    /// Distinguishes a stored JSON null from a missing or deleted value.
    pub present: bool,
}

/// A private conversation value restored from an opaque history checkpoint.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConversationEntry {
    pub key: String,
    /// Live revisions advance on writes and restoration, not back to checkpoint revisions.
    pub revision: u64,
    pub value: serde_json::Value,
    pub present: bool,
    /// History restoration is not authority to resume background work.
    pub restored: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub keys: Vec<String>,
    /// The last returned key when another page is available.
    pub after: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Command;
    use serde_json::json;

    #[test]
    fn isolates_collection_selectors() {
        let declaration = |collections| Declaration { collections };
        let exact = Collection {
            scope: Scope::Node,
            key: Some("entry".into()),
            prefix: None,
            schema: json!(true),
        };
        let prefix = Collection {
            scope: Scope::Node,
            key: None,
            prefix: Some("entry/".into()),
            schema: json!({}),
        };
        assert!(declaration(vec![exact.clone(), prefix.clone()]).valid());
        assert!(exact.matches(Scope::Node, "entry"));
        assert!(!exact.matches(Scope::Conversation, "entry"));
        assert!(prefix.matches(Scope::Node, "entry/one"));
        assert!(!prefix.matches(Scope::Node, "entry"));
        assert!(!declaration(vec![exact.clone(), exact.clone()]).valid());
        assert!(
            !declaration(vec![
                Collection {
                    key: Some("entry/one".into()),
                    ..exact.clone()
                },
                prefix.clone()
            ])
            .valid()
        );
        assert!(
            !declaration(vec![Collection {
                prefix: Some("entry".into()),
                ..exact.clone()
            }])
            .valid()
        );
        assert!(
            !declaration(vec![Collection {
                key: None,
                prefix: Some(String::new()),
                ..exact.clone()
            }])
            .valid()
        );
        assert!(!declaration(vec![exact; MAX_COLLECTIONS + 1]).valid());
        assert!(
            declaration(vec![
                prefix.clone(),
                Collection {
                    scope: Scope::Conversation,
                    ..prefix
                }
            ])
            .valid()
        );
    }

    #[test]
    fn command_durability() {
        assert!(
            !Command::ReadPluginConversationValue {
                key: "state".into()
            }
            .durable()
        );
        assert!(
            Command::WritePluginConversationValue {
                key: "state".into(),
                value: json!(null),
                expected_revision: 0
            }
            .durable()
        );
        assert!(
            Command::RemovePluginConversationValue {
                key: "state".into(),
                expected_revision: 1
            }
            .durable()
        );
        assert_eq!(serde_json::to_value(Scope::Node).unwrap(), json!("node"));
        assert_eq!(
            serde_json::to_value(Scope::Conversation).unwrap(),
            json!("conversation")
        );
    }
}
