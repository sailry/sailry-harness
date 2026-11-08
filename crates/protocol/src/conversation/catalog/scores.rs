use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScoreStatus {
    pub revision: u64,
    pub updated_at_ms: Option<u64>,
    pub checked_at_ms: Option<u64>,
    pub batch: Option<String>,
    pub published_at: Option<String>,
    pub models: u32,
    pub fault: Option<crate::Fault>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: crate::ProviderId,
    pub model: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scores {
    pub revision: u64,
    pub batch: Option<String>,
    pub published_at: Option<String>,
    pub scores: Vec<ModelScore>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ModelScore {
    pub provider: crate::ProviderId,
    pub model: String,
    pub efforts: Vec<EffortScore>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EffortScore {
    pub effort: crate::Effort,
    pub overall: f32,
    pub axes: [u8; 5],
    pub elapsed_ms: Option<u64>,
    pub cost_usd: Option<f32>,
}
