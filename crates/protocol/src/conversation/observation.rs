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
    #[serde(default)]
    pub phases: Phases,
    pub calls: u64,
    pub replies: u64,
    pub compactions: u64,
    pub tools: BTreeMap<String, u64>,
    pub children: u64,
    /// Unknown when older canonical responses did not retain cache metadata.
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    pub id: String,
    pub turn: TurnId,
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
    pub headers_us: Option<u64>,
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
