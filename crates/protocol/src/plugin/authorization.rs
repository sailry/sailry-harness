//! MCP browser authorization is transient; grants and PKCE state stay on the Node.
use super::Reference;
use crate::{Fault, RequestId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub package: Reference,
    pub server: String,
    pub configured: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub id: RequestId,
    pub package: Reference,
    pub server: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Update {
    pub attempt: Attempt,
    pub revision: u64,
    pub state: State,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum State {
    Starting,
    Pending { url: String, expires_at_ms: u64 },
    Exchanging,
    Connected(Reference),
    Cancelled,
    Failed(Fault),
}

impl State {
    pub fn active(&self) -> bool {
        matches!(
            self,
            Self::Starting | Self::Pending { .. } | Self::Exchanging
        )
    }
}
