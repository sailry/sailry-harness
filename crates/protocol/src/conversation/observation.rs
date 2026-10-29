//! Lightweight request telemetry; no request bodies, credentials or message text.
use super::Usage;
use crate::TurnId;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// Recent request history is bounded independently of cumulative counters.
pub const RECENT_LIMIT: usize = 64;

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Summary {
    pub recent: Vec<Request>,
    /// Per-field means over all recorded attempts, independently of recent history limits.
    #[serde(default)]
    pub latency: Latency,
    #[serde(default)]
    pub phases: Phases,
    pub calls: u64,
    pub replies: u64,
    pub compactions: u64,
    pub tools: BTreeMap<String, u64>,
    pub children: u64,
    /// Unknown when the provider does not report cache metadata.
    pub cache_read: Option<u64>,
    pub reasoning: Option<u64>,
    /// Sum of recorded model intervals, including failed attempts but not backoff.
    pub model_us: u64,
    pub output_us: u64,
    pub output_tokens: u64,
    pub output_samples: u64,
    /// Cumulative estimates count each actual request once, separately from usage.
    pub composition: Vec<Category>,
}

/// Mean observed microseconds; unmeasured fields remain unknown and zero is a sample.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Latency {
    #[serde(default)]
    pub response_us: Option<u64>,
    #[serde(default)]
    pub first_text_us: Option<u64>,
    #[serde(default)]
    pub waiting_us: Option<u64>,
    #[serde(default)]
    pub elapsed_us: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub turn: TurnId,
    /// One-based task round in this conversation, derived from visible history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turn_index: Option<u64>,
    pub model: String,
    pub context_limit: Option<u32>,
    pub timestamp_ms: i64,
    /// Canonical history position preceding this generation.
    pub boundary: Option<u64>,
    pub attempts: Vec<Attempt>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub index: u64,
    pub transport: String,
    pub reused: Option<bool>,
    /// Actual HTTP response headers, when exposed by the provider.
    pub headers_us: Option<u64>,
    /// The first HTTP response, regardless of its status.
    #[serde(default)]
    pub response_us: Option<u64>,
    pub first_text_us: Option<u64>,
    pub elapsed_us: Option<u64>,
    pub status: Option<u16>,
    pub outcome: Outcome,
    /// Estimates of the provider-adapted payload, never normalized to billed tokens.
    pub categories: Vec<Category>,
    /// False for continuation payloads whose earlier input remains on the provider.
    pub complete_context: bool,
    pub usage: Option<Usage>,
    pub cache_reported: bool,
    pub reasoning_reported: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Succeeded,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Category {
    pub kind: CategoryKind,
    /// None means attribution is unavailable; it is distinct from an observed zero.
    pub items: Option<u64>,
    pub tokens: Option<u64>,
    pub method: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CategoryKind {
    System,
    Tools,
    User,
    Skills,
    Injected,
    Assistant,
    Results,
}

/// Disjoint wall-clock intervals; missing historical observations remain unknown.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Phases {
    pub active_us: Option<u64>,
    pub waiting_us: Option<u64>,
    pub output_us: Option<u64>,
    pub tools_us: Option<u64>,
    pub other_us: Option<u64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_unrecorded_latency() {
        let mut wire = serde_json::to_value(Summary::default()).unwrap();
        wire.as_object_mut().unwrap().remove("latency");
        let summary: Summary = serde_json::from_value(wire).unwrap();
        assert_eq!(summary.latency, Latency::default());
        let latency = Latency {
            response_us: Some(0),
            first_text_us: Some(20),
            waiting_us: None,
            elapsed_us: Some(u64::MAX),
        };
        assert_eq!(
            serde_json::from_value::<Latency>(serde_json::to_value(&latency).unwrap()).unwrap(),
            latency
        );
        assert_eq!(
            serde_json::from_value::<Latency>(serde_json::json!({})).unwrap(),
            Latency::default()
        );
    }

    #[test]
    fn defaults_unprojected_turn_index() {
        let wire = serde_json::json!({
            "id": "call", "turn": TurnId::new(), "model": "fixture",
            "context_limit": null, "timestamp_ms": 1, "boundary": null,
            "attempts": []
        });
        let mut request: Request = serde_json::from_value(wire).unwrap();
        assert_eq!(request.turn_index, None);
        request.turn_index = Some(u64::MAX);
        let decoded: Request =
            serde_json::from_value(serde_json::to_value(&request).unwrap()).unwrap();
        assert_eq!(decoded, request);
    }

    #[test]
    fn defaults_unrecorded_response_time() {
        let wire = serde_json::json!({
            "index": 1, "transport": "http", "reused": false,
            "headers_us": null, "first_text_us": 40, "elapsed_us": 50,
            "status": null, "outcome": "succeeded", "categories": [],
            "complete_context": true, "usage": null,
            "cache_reported": false, "reasoning_reported": false
        });
        let mut attempt: Attempt = serde_json::from_value(wire).unwrap();
        assert_eq!(attempt.response_us, None);
        attempt.response_us = Some(20);
        attempt.outcome = Outcome::Failed;
        let decoded: Attempt =
            serde_json::from_value(serde_json::to_value(&attempt).unwrap()).unwrap();
        assert_eq!(decoded, attempt);
        assert_eq!(decoded.headers_us, None);
        assert_eq!(decoded.outcome, Outcome::Failed);
    }
}
