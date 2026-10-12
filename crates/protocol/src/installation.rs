//! Transient installation observation, separate from durable request outcomes.
use crate::{Fault, RequestId, models::Kind};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Resource {
    Plugin { name: String },
    Model { kind: Kind, id: String },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    pub request: RequestId,
    /// Monotonic within this request's execution Node lifetime.
    pub revision: u64,
    pub resource: Resource,
    pub state: State,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum State {
    Queued,
    Running {
        phase: Phase,
        /// None means the operation has no measured completion total.
        progress: Option<Progress>,
    },
    Failed {
        error: Fault,
    },
    Uncertain {
        error: Fault,
    },
}

impl State {
    pub fn terminal(&self) -> bool {
        matches!(self, Self::Failed { .. } | Self::Uncertain { .. })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Downloading,
    Inspecting,
    Preparing,
    Installing,
    Validating,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Progress {
    Percent {
        percent: u8,
        bytes_per_second: Option<u64>,
    },
    /// Measured work items, such as received Git objects, not an inferred byte total.
    Counts {
        completed: u64,
        total: u64,
        bytes_per_second: Option<u64>,
    },
}

impl Progress {
    pub fn validate(self) -> Result<(), Fault> {
        match self {
            Self::Percent { percent, .. } if percent <= 100 => Ok(()),
            Self::Counts {
                completed, total, ..
            } if total > 0 && completed <= total => Ok(()),
            _ => Err(Fault::invalid("invalid installation progress")),
        }
    }
}

#[cfg(test)]
mod tests;
