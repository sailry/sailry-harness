//! Execution-Node search configuration and bounded, untrusted search results.
use crate::{Fault, plugin::settings::SecretUpdate};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const DEFAULT_LIMIT: u8 = 5;
pub const MAX_LIMIT: u8 = u8::MAX;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Driver {
    Exa,
    Tavily,
    Brave,
    Searxng,
    Bocha,
}

impl Driver {
    pub const ALL: [Self; 5] = [
        Self::Exa,
        Self::Tavily,
        Self::Brave,
        Self::Searxng,
        Self::Bocha,
    ];

    pub fn key_required(self) -> bool {
        matches!(self, Self::Tavily | Self::Brave | Self::Bocha)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "driver", rename_all = "snake_case")]
pub enum Selection {
    Auto,
    Driver(Driver),
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub enabled: bool,
    pub endpoint: Option<String>,
    pub key_configured: bool,
    pub basic_configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub driver: Option<Selection>,
    pub prefer_native: bool,
    pub fallback: bool,
    pub providers: BTreeMap<Driver, Config>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            driver: Some(Selection::Auto),
            prefer_native: true,
            fallback: true,
            providers: Driver::ALL
                .into_iter()
                .map(|driver| {
                    (
                        driver,
                        Config {
                            enabled: driver == Driver::Exa,
                            ..Config::default()
                        },
                    )
                })
                .collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub key: SecretUpdate,
    /// A private username:password value; public settings expose only its presence.
    pub basic: SecretUpdate,
}

impl Default for Credentials {
    fn default() -> Self {
        Self {
            key: SecretUpdate::Keep,
            basic: SecretUpdate::Keep,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub settings: Settings,
    pub credentials: BTreeMap<Driver, Credentials>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: u8,
}

fn default_limit() -> u8 {
    DEFAULT_LIMIT
}

impl Request {
    pub fn validate(&self) -> std::result::Result<(), Fault> {
        if self.query.trim().is_empty() || self.query.contains('\0') || self.limit == 0 {
            return Err(Fault::invalid("invalid search query or result limit"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Result {
    pub title: String,
    pub url: String,
    pub snippet: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub published: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page {
    pub driver: Driver,
    pub results: Vec<Result>,
    pub untrusted: bool,
    pub fetch_hint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_select_auto_and_bound_results() {
        let settings = Settings::default();
        assert_eq!(settings.driver, Some(Selection::Auto));
        assert!(settings.prefer_native && settings.fallback);
        assert!(settings.providers[&Driver::Exa].enabled);
        assert!(
            settings
                .providers
                .iter()
                .all(|(driver, provider)| *driver == Driver::Exa || !provider.enabled)
        );
        let request: Request = serde_json::from_value(json!({"query":"current sources"})).unwrap();
        assert_eq!(request.limit, DEFAULT_LIMIT);
        request.validate().unwrap();
        let expanded = Request {
            query: "source ".repeat(700),
            limit: 50,
        };
        expanded.validate().unwrap();
        assert!(
            Request {
                limit: 0,
                ..expanded
            }
            .validate()
            .is_err()
        );
        assert!(
            serde_json::from_value::<Request>(json!({"query":"source","key":"secret"})).is_err()
        );
    }

    #[test]
    fn secret_updates_never_enter_public_settings() {
        let credentials = Credentials {
            key: SecretUpdate::Replace(crate::Secret::new("private-key".into())),
            basic: SecretUpdate::Replace(crate::Secret::new(
                "private-user:private-password".into(),
            )),
        };
        assert!(!format!("{credentials:?}").contains("private-key"));
        let settings = serde_json::to_string(&Settings::default()).unwrap();
        for secret in ["private-key", "private-user", "private-password"] {
            assert!(!settings.contains(secret));
        }
    }
}
