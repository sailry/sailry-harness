//! Execution-Node search configuration and bounded, untrusted search results.
use crate::{Fault, plugin::settings::SecretUpdate};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const DEFAULT_LIMIT: u8 = 5;
pub const MAX_LIMIT: u8 = u8::MAX;
pub const MAX_DOMAINS: usize = 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Driver {
    Exa,
    Tinyfish,
    Tavily,
    Brave,
    Searxng,
    Bocha,
    Volcengine,
    Deepseek,
    Model,
}

impl Driver {
    pub const ALL: [Self; 9] = [
        Self::Exa,
        Self::Tinyfish,
        Self::Tavily,
        Self::Brave,
        Self::Searxng,
        Self::Bocha,
        Self::Volcengine,
        Self::Deepseek,
        Self::Model,
    ];

    pub fn key_required(self) -> bool {
        matches!(
            self,
            Self::Tinyfish
                | Self::Tavily
                | Self::Brave
                | Self::Bocha
                | Self::Volcengine
                | Self::Deepseek
        )
    }

    pub fn capability(self, key_configured: bool) -> Capability {
        Capability {
            key_required: self.key_required(),
            quota: match self {
                Self::Exa if !key_configured => Quota::AnonymousQuota,
                Self::Searxng => Quota::Keyless,
                Self::Bocha | Self::Deepseek | Self::Model => Quota::Paid,
                _ => Quota::FreeQuota,
            },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Quota {
    Keyless,
    AnonymousQuota,
    FreeQuota,
    Paid,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capability {
    pub key_required: bool,
    pub quota: Quota,
}

fn capabilities() -> BTreeMap<Driver, Capability> {
    Driver::ALL
        .into_iter()
        .map(|driver| (driver, driver.capability(false)))
        .collect()
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<ModelConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelConfig {
    pub provider: crate::ProviderId,
    pub model: String,
}

fn order() -> Vec<Driver> {
    Driver::ALL.to_vec()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub revision: u64,
    pub driver: Option<Selection>,
    pub fallback: bool,
    #[serde(default = "order")]
    pub order: Vec<Driver>,
    pub providers: BTreeMap<Driver, Config>,
    /// Node-owned driver declarations; clients cannot change them through a settings update.
    #[serde(default = "capabilities")]
    pub capabilities: BTreeMap<Driver, Capability>,
}

impl Settings {
    pub fn describe(&mut self) {
        self.capabilities = self
            .providers
            .iter()
            .map(|(driver, config)| (*driver, driver.capability(config.key_configured)))
            .collect();
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            revision: 0,
            driver: Some(Selection::Auto),
            fallback: true,
            order: order(),
            capabilities: capabilities(),
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Freshness {
    Hour,
    Day,
    Week,
    Month,
    Year,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub freshness: Option<Freshness>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub include_domains: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exclude_domains: Vec<String>,
}

impl Default for Request {
    fn default() -> Self {
        Self {
            query: String::new(),
            limit: DEFAULT_LIMIT,
            freshness: None,
            include_domains: Vec::new(),
            exclude_domains: Vec::new(),
        }
    }
}

fn default_limit() -> u8 {
    DEFAULT_LIMIT
}

impl Request {
    pub fn validate(&self) -> std::result::Result<(), Fault> {
        if self.query.trim().is_empty() || self.query.contains('\0') || self.limit == 0 {
            return Err(Fault::invalid("invalid search query or result limit"));
        }
        if self.include_domains.len() > MAX_DOMAINS || self.exclude_domains.len() > MAX_DOMAINS {
            return Err(Fault::invalid("search domain lists exceed the host limit"));
        }
        let included = self
            .include_domains
            .iter()
            .map(|value| domain(value))
            .collect::<std::result::Result<BTreeSet<_>, _>>()?;
        for value in &self.exclude_domains {
            if included.contains(&domain(value)?) {
                return Err(Fault::invalid("search domain filters overlap"));
            }
        }
        Ok(())
    }

    /// Normalize a validated request without changing its query syntax.
    pub fn normalized(&self) -> Self {
        let mut request = self.clone();
        request.query = request.query.trim().to_owned();
        for values in [&mut request.include_domains, &mut request.exclude_domains] {
            for value in values.iter_mut() {
                if let Ok(host) = domain(value) {
                    *value = host;
                }
            }
            values.sort();
            values.dedup();
        }
        request
    }
}

fn domain(value: &str) -> std::result::Result<String, Fault> {
    if value.contains('%') {
        return Err(Fault::invalid("search domain filters require hostnames"));
    }
    let Ok(url::Host::Domain(host)) = url::Host::parse(value) else {
        return Err(Fault::invalid("search domain filters require hostnames"));
    };
    let host = host.strip_suffix('.').unwrap_or(&host);
    if host.is_empty()
        || host.len() > 253
        || host.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(Fault::invalid("search domain filters require hostnames"));
    }
    Ok(host.to_owned())
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    pub untrusted: bool,
    pub fetch_hint: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn declares_quota_and_key_requirements() {
        let mut settings = Settings::default();
        assert_eq!(
            settings.capabilities[&Driver::Exa].quota,
            Quota::AnonymousQuota
        );
        assert_eq!(
            settings.capabilities[&Driver::Searxng].quota,
            Quota::Keyless
        );
        for driver in [
            Driver::Tinyfish,
            Driver::Tavily,
            Driver::Brave,
            Driver::Volcengine,
        ] {
            assert_eq!(settings.capabilities[&driver].quota, Quota::FreeQuota);
            assert!(settings.capabilities[&driver].key_required);
        }
        for driver in [Driver::Bocha, Driver::Deepseek, Driver::Model] {
            assert_eq!(settings.capabilities[&driver].quota, Quota::Paid);
        }
        assert!(!settings.capabilities[&Driver::Model].key_required);
        settings
            .providers
            .get_mut(&Driver::Exa)
            .unwrap()
            .key_configured = true;
        settings.describe();
        assert_eq!(settings.capabilities[&Driver::Exa].quota, Quota::FreeQuota);
        assert_eq!(
            serde_json::from_value::<Settings>(serde_json::to_value(&settings).unwrap()).unwrap(),
            settings
        );
    }

    #[test]
    fn preserves_order_and_defaults_missing_field() {
        let mut settings = Settings::default();
        settings.order.reverse();
        let mut value = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            serde_json::from_value::<Settings>(value.clone()).unwrap(),
            settings
        );
        value.as_object_mut().unwrap().remove("order");
        assert_eq!(
            serde_json::from_value::<Settings>(value).unwrap().order,
            Driver::ALL
        );
    }

    #[test]
    fn defaults_optional_model_fields() {
        let config: Config = serde_json::from_value(json!({
            "enabled":false,"endpoint":null,"key_configured":false,"basic_configured":false
        }))
        .unwrap();
        assert_eq!(config.model, None);
        let page: Page = serde_json::from_value(json!({
            "driver":"exa","results":[],"untrusted":true,"fetch_hint":"Read source pages"
        }))
        .unwrap();
        assert_eq!(page.summary, None);
        let binding = ModelConfig {
            provider: crate::ProviderId::new(),
            model: "search-model".into(),
        };
        assert_eq!(
            serde_json::from_value::<ModelConfig>(serde_json::to_value(&binding).unwrap()).unwrap(),
            binding
        );
    }

    #[test]
    fn defaults_select_auto_and_bound_results() {
        let settings = Settings::default();
        assert_eq!(settings.driver, Some(Selection::Auto));
        assert!(settings.fallback);
        assert!(settings.providers[&Driver::Exa].enabled);
        assert!(
            settings
                .providers
                .iter()
                .all(|(driver, provider)| *driver == Driver::Exa || !provider.enabled)
        );
        let request: Request = serde_json::from_value(json!({"query":"current sources"})).unwrap();
        assert_eq!(request.limit, DEFAULT_LIMIT);
        assert_eq!(request.freshness, None);
        assert!(request.include_domains.is_empty());
        assert!(request.exclude_domains.is_empty());
        assert_eq!(
            serde_json::to_value(&request).unwrap(),
            json!({"query":"current sources", "limit":DEFAULT_LIMIT})
        );
        request.validate().unwrap();
        let expanded = Request {
            query: "source ".repeat(700),
            limit: 50,
            ..Default::default()
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
    fn serializes_each_freshness() {
        for (freshness, name) in [
            (Freshness::Hour, "hour"),
            (Freshness::Day, "day"),
            (Freshness::Week, "week"),
            (Freshness::Month, "month"),
            (Freshness::Year, "year"),
        ] {
            let value = json!({
                "query":"current sources", "freshness":name,
                "include_domains":["example.com"], "exclude_domains":["other.test"]
            });
            let request: Request = serde_json::from_value(value).unwrap();
            request.validate().unwrap();
            assert_eq!(request.freshness, Some(freshness));
            assert_eq!(serde_json::to_value(&request).unwrap()["freshness"], name);
        }
        assert!(
            serde_json::from_value::<Request>(json!({
                "query":"current sources", "freshness":"minute"
            }))
            .is_err()
        );
    }

    #[test]
    fn normalizes_hosts_and_preserves_query_syntax() {
        let request = Request {
            query: "  site:example.com \"two  words\"\nOR source  ".into(),
            include_domains: vec![
                "EXAMPLE.COM.".into(),
                "example.com".into(),
                "b.example.com".into(),
            ],
            exclude_domains: vec!["OTHER.test.".into(), "other.test".into()],
            ..Default::default()
        };
        request.validate().unwrap();
        let normalized = request.normalized();
        assert_eq!(
            normalized.query,
            "site:example.com \"two  words\"\nOR source"
        );
        assert_eq!(normalized.include_domains, ["b.example.com", "example.com"]);
        assert_eq!(normalized.exclude_domains, ["other.test"]);
        assert_eq!(normalized.normalized(), normalized);

        let request = Request {
            query: "Unicode hostname".into(),
            include_domains: vec!["例子.测试".into()],
            ..Default::default()
        };
        request.validate().unwrap();
        assert_eq!(
            request.normalized().include_domains,
            ["xn--fsqu00a.xn--0zwm56d"]
        );
    }

    #[test]
    fn rejects_overlapping_or_oversized_domains() {
        for (include_domains, exclude_domains) in [
            (vec!["EXAMPLE.com.".into()], vec!["example.com".into()]),
            (vec!["example.com".into(); MAX_DOMAINS + 1], vec![]),
            (vec![], vec!["example.com".into(); MAX_DOMAINS + 1]),
        ] {
            assert!(
                Request {
                    query: "current sources".into(),
                    include_domains,
                    exclude_domains,
                    ..Default::default()
                }
                .validate()
                .is_err()
            );
        }
        let request = Request {
            query: "current sources".into(),
            include_domains: (0..MAX_DOMAINS)
                .map(|index| format!("host{index}.example"))
                .collect(),
            exclude_domains: vec!["other.test".into(); MAX_DOMAINS],
            ..Default::default()
        };
        request.validate().unwrap();
    }

    #[test]
    fn rejects_non_host_filters() {
        for value in [
            "",
            "https://example.com",
            "example.com/path",
            "example.com:443",
            "user@example.com",
            "*.example.com",
            ".example.com",
            "example..com",
            "example.com..",
            "example_com",
            "-example.com",
            "example-.com",
            " example.com",
            "example.com ",
            "example.com?x=1",
            "example.com#fragment",
            "127.0.0.1",
            "[::1]",
            "::1",
            "%65xample.com",
        ] {
            for exclude in [false, true] {
                let mut request = Request {
                    query: "current sources".into(),
                    ..Default::default()
                };
                if exclude {
                    request.exclude_domains.push(value.into());
                } else {
                    request.include_domains.push(value.into());
                }
                assert!(request.validate().is_err(), "{value:?}");
            }
        }
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
