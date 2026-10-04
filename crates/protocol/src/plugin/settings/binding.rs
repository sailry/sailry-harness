//! Unknown protected-slot metadata remains readable but cannot authorize credentials.
use serde::{Deserialize, Deserializer, Serialize, de::IgnoredAny};
use std::collections::BTreeMap;

/// A declared MCP or HTTP credential slot, filled only by the Node.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Binding {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub server: Option<String>,
    pub env: Option<String>,
    pub header: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Unknown declaration fields are never interpreted as a supported credential target.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub unsupported: bool,
}

impl<'de> Deserialize<'de> for Binding {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct Raw {
            server: Option<String>,
            env: Option<String>,
            header: Option<String>,
            origin: Option<String>,
            prefix: Option<String>,
            #[serde(default)]
            unsupported: bool,
            #[serde(flatten)]
            extra: BTreeMap<String, IgnoredAny>,
        }

        let raw = Raw::deserialize(deserializer)?;
        Ok(Self {
            server: raw.server,
            env: raw.env,
            header: raw.header,
            origin: raw.origin,
            prefix: raw.prefix,
            unsupported: raw.unsupported || !raw.extra.is_empty(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_known_targets() {
        for value in [
            json!({"server": "fixture", "env": "TOKEN", "header": null}),
            json!({"header": "Authorization", "origin": "https://example.test", "prefix": "Bearer "}),
        ] {
            let binding: Binding = serde_json::from_value(value).unwrap();
            assert!(!binding.unsupported);
            let encoded = serde_json::to_value(&binding).unwrap();
            assert!(encoded.get("unsupported").is_none());
            assert_eq!(serde_json::from_value::<Binding>(encoded).unwrap(), binding);
        }
    }

    #[test]
    fn unknown_metadata_is_inert() {
        for key in ["connector", "unrecognized", "$ref"] {
            let binding: Binding = serde_json::from_value(json!({
                "origin": "https://example.test",
                "header": "Authorization",
                "unsupported": false,
                key: {"secret": "unknown-payload-marker"}
            }))
            .unwrap();
            assert!(binding.unsupported);
            assert_eq!(binding.origin.as_deref(), Some("https://example.test"));
            assert_eq!(binding.header.as_deref(), Some("Authorization"));
            let encoded = serde_json::to_value(&binding).unwrap();
            assert_eq!(encoded["unsupported"], true);
            assert!(encoded.get(key).is_none());
            assert!(!encoded.to_string().contains("unknown-payload-marker"));
            assert!(!format!("{binding:?}").contains("unknown-payload-marker"));
            assert_eq!(serde_json::from_value::<Binding>(encoded).unwrap(), binding);
        }
    }

    #[test]
    fn preserves_an_explicit_unsupported_marker() {
        let binding: Binding = serde_json::from_value(json!({"unsupported": true})).unwrap();
        assert!(binding.unsupported);
        assert!(binding.server.is_none());
        assert!(binding.origin.is_none());
    }

    #[test]
    fn rejects_malformed_known_fields() {
        for key in ["server", "env", "header", "origin", "prefix"] {
            for invalid in [json!(true), json!(1), json!({}), json!([])] {
                assert!(
                    serde_json::from_value::<Binding>(json!({key: invalid, "unknown": true}))
                        .is_err(),
                    "accepted malformed {key}"
                );
            }
        }
        assert!(serde_json::from_value::<Binding>(json!({"unsupported": "true"})).is_err());
    }
}
