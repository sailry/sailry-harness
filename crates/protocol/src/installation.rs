//! Transient installation observation, separate from durable request outcomes.
use crate::{
    Command, ErrorCode, Fault, ModelId, RequestId, knowledge,
    models::{Definition, Kind},
};
use serde::{Deserialize, Serialize};

pub const MAX_REQUESTS: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    Node,
    Controller,
}

/// A registered model in the profile that owns its actual execution service.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelRef {
    Ocr {
        target: Target,
        id: String,
    },
    Embedding {
        target: Target,
        model: knowledge::Model,
    },
    Speech {
        target: Target,
        id: Option<ModelId>,
    },
}

impl ModelRef {
    pub fn validate(&self) -> Result<(), Fault> {
        match self {
            Self::Ocr {
                target: Target::Node,
                id,
            } if crate::ocr::valid_id(id) => Ok(()),
            Self::Embedding {
                target: Target::Node,
                ..
            }
            | Self::Speech {
                target: Target::Controller,
                ..
            } => Ok(()),
            _ => Err(Fault::invalid(
                "model installation target or identity is invalid",
            )),
        }
    }

    pub fn resource(&self) -> Resource {
        let (kind, id) = match self {
            Self::Ocr { id, .. } => (Kind::Ocr, id.clone()),
            Self::Embedding { model, .. } => (Kind::Embedding, model.key().into_owned()),
            Self::Speech { id, .. } => (
                Kind::Speech,
                id.map(|id| id.to_string())
                    .unwrap_or_else(|| "sensevoice-int8".into()),
            ),
        };
        Resource::Model { kind, id }
    }

    pub fn node_command(&self) -> Result<Command, Fault> {
        self.validate()?;
        match self {
            Self::Ocr { id, .. } => Ok(Command::InstallOcrModel { id: id.clone() }),
            Self::Embedding { model, .. } => Ok(Command::InstallKnowledgeModel { model: *model }),
            Self::Speech { .. } => Err(Fault::new(
                ErrorCode::Unavailable,
                "speech installation requires a controller",
            )),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Registration {
    pub target: Target,
    pub definition: Definition,
}

impl Registration {
    pub fn validate(&self) -> Result<(), Fault> {
        self.definition.validate()?;
        match (self.target, self.definition.format.kind()) {
            (Target::Node, Kind::Ocr | Kind::Embedding) | (Target::Controller, Kind::Speech) => {
                Ok(())
            }
            _ => Err(Fault::invalid("model registration target is invalid")),
        }
    }

    pub fn node_command(&self) -> Result<Command, Fault> {
        self.validate()?;
        match self.definition.format.kind() {
            Kind::Ocr => Ok(Command::RegisterOcrModel {
                definition: self.definition.clone(),
            }),
            Kind::Embedding => Ok(Command::RegisterKnowledgeModel {
                definition: self.definition.clone(),
            }),
            Kind::Speech => Err(Fault::new(
                ErrorCode::Unavailable,
                "speech registration requires a controller",
            )),
        }
    }
}

pub fn validate_requests(requests: &[RequestId]) -> Result<(), Fault> {
    let unique = requests.iter().collect::<std::collections::BTreeSet<_>>();
    if requests.is_empty() || requests.len() > MAX_REQUESTS || unique.len() != requests.len() {
        return Err(Fault::invalid(
            "installation request IDs must be bounded and unique",
        ));
    }
    Ok(())
}

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
    Language { package: String, version: String },
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
