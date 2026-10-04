use sailry_protocol::{Effort, NodeId, Permission, ProviderId, WorkMode};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Composer {
    pub model: Option<Model>,
    pub mode: WorkMode,
    pub permission: Permission,
}

impl Default for Composer {
    fn default() -> Self {
        Self {
            model: None,
            mode: WorkMode::Code,
            permission: Permission::Ask,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Model {
    pub source: NodeId,
    pub provider: ProviderId,
    pub model: String,
    pub effort: Effort,
}
