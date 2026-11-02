//! Stable business identities are separate from execution-resource authority.
use crate::{Fault, Session, SessionId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Association {
    pub namespace: String,
    pub kind: String,
    pub id: String,
}

impl Association {
    pub fn validate(&self) -> Result<(), Fault> {
        let label = |value: &str| {
            !value.is_empty()
                && value.len() <= 64
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        };
        if !label(&self.namespace)
            || !label(&self.kind)
            || self.id.trim().is_empty()
            || self.id.len() > 256
            || self.id.trim() != self.id
            || self.id.chars().any(char::is_control)
        {
            return Err(Fault::invalid("session association is invalid"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssociatedSessions {
    pub sessions: Vec<Session>,
    pub next_before: Option<SessionId>,
}
