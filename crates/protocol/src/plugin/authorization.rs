//! MCP browser authorization is transient; grants and PKCE state stay on the Node.
use super::Reference;
use crate::{Fault, RequestId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub package: Reference,
    pub server: String,
    pub configured: bool,
    /// Confirmed discovery and an available client registration path, not grant state.
    #[serde(default)]
    pub can_authorize: bool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_stored_status_without_capability() {
        let package = Reference {
            name: "example".into(),
            digest: "a".repeat(64),
            settings_revision: 3,
        };
        let stored = json!({"Ok":{"kind":"mcp_authorization","data":{
            "package":package,"server":"input","configured":true
        }}});
        let response: Result<crate::Output, Fault> = serde_json::from_value(stored).unwrap();
        let crate::Output::McpAuthorization(status) = response.unwrap() else {
            panic!("stored authorization status expected");
        };
        assert_eq!(status.package, package);
        assert_eq!(status.server, "input");
        assert!(status.configured);
        assert!(!status.can_authorize);
    }
}
