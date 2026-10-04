//! Transient device authorization state; access and refresh tokens stay on the Node.
use crate::{Fault, ProviderId, RequestId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attempt {
    pub id: RequestId,
    pub provider: ProviderId,
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
    Pending {
        verification_url: String,
        user_code: String,
        expires_at_ms: u64,
    },
    Connected,
    Cancelled,
    Failed(Fault),
}

impl State {
    pub fn active(&self) -> bool {
        matches!(self, Self::Starting | Self::Pending { .. })
    }
}
