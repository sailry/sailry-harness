use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Outcome {
    Installed {
        node: crate::NodeId,
    },
    NetworkUnconfirmed {
        node: crate::NodeId,
        selection: crate::network::Selection,
    },
}
