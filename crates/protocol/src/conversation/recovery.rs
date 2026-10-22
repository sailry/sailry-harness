//! Public diagnostics for recovery of an unfinished model generation.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transport {
    Http,
    WebSocket,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stop {
    AttemptsExhausted,
    RateLimited,
    ServerOverloaded,
    InvalidRequest,
    InvalidPrompt,
    ContextWindowExceeded,
    QuotaExceeded,
    UsageNotIncluded,
    UsageLimitReached,
    ResponseIncomplete,
    FlexUnavailable,
    PolicyViolation,
    LocalFailure,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Retry {
    pub attempt: u64,
    #[serde(default, skip_serializing_if = "is_false")]
    pub fallback: bool,
    /// None identifies the independent persistent connection recovery path.
    pub limit: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delay_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transport: Option<Transport>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

fn is_false(value: &bool) -> bool {
    !*value
}

impl Retry {
    pub fn valid(&self) -> bool {
        (self.fallback && self.attempt == 0 && self.limit.is_some_and(|limit| limit <= 100))
            || (!self.fallback
                && self.attempt > 0
                && self
                    .limit
                    .is_none_or(|limit| limit <= 100 && self.attempt <= limit))
    }

    pub fn from_resource(value: &Value) -> Option<Self> {
        if value.get("type")?.as_str()? != "model_retry" {
            return None;
        }
        let retry: Self = serde_json::from_value(value.clone()).ok()?;
        retry.valid().then_some(retry)
    }

    pub fn resource(&self) -> Value {
        let mut value = serde_json::to_value(self).expect("model retry serializes");
        value["type"] = "model_retry".into();
        value
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn preserves_finite_and_connection_diagnostics() {
        let finite = Retry::from_resource(
            &json!({"type":"model_retry","attempt":1,"limit":5,"error":"upstream error"}),
        )
        .unwrap();
        assert_eq!(finite.delay_ms, None);
        assert_eq!(finite.error.as_deref(), Some("upstream error"));
        let persistent = Retry {
            attempt: 9,
            fallback: false,
            limit: None,
            delay_ms: Some(60_000),
            transport: Some(Transport::Http),
            status: None,
            code: None,
            error: Some("connection failed".into()),
        };
        assert_eq!(
            Retry::from_resource(&persistent.resource()),
            Some(persistent)
        );
        assert!(
            Retry::from_resource(&json!({"type":"model_retry","attempt":6,"limit":5})).is_none()
        );
        assert!(
            Retry::from_resource(&json!({"type":"model_retry","attempt":0,"limit":null})).is_none()
        );
    }
}

#[cfg(test)]
mod provider {
    use crate::conversation::Provider;

    #[test]
    fn applies_captured_override_bounds() {
        let mut provider: Provider = serde_json::from_value(serde_json::json!({
            "id": crate::ProviderId::new(), "revision": 1, "name": "Fixture",
            "api": "responses", "authentication": "api_key", "endpoint": "https://fixture.invalid",
            "enabled": true, "models": [], "default_model": "fixture"
        }))
        .unwrap();
        assert_eq!(
            (provider.request_retries(), provider.stream_retries()),
            (4, 5)
        );
        provider.request_max_retries = Some(0);
        provider.stream_max_retries = Some(101);
        let captured: Provider =
            serde_json::from_value(serde_json::to_value(provider).unwrap()).unwrap();
        assert_eq!(
            (captured.request_retries(), captured.stream_retries()),
            (0, 100)
        );
    }
}
