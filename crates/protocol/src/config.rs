use serde::{Deserialize, Serialize};

use crate::{
    CredentialId, NodeId, ProjectId, ProviderId, RequestId, SessionId, TurnId, WorktreeId,
};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    #[default]
    Default,
    #[serde(rename = "none")]
    Disabled,
    Minimal,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
    /// Native token budget; -1 requests Gemini's dynamic budget.
    Budget(i32),
}

/// Mutation authorization for newly admitted turns, not a controller preference.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    Ask,
    Project,
    Full,
}

/// The purpose and available tools of a turn, independent of mutation permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkMode {
    Plan,
    Code,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CredentialRef {
    pub node: NodeId,
    pub id: CredentialId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub assistant: Option<crate::plugin::conversation::Binding>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resource: Option<crate::connection::Resource>,
    pub provider: ProviderId,
    pub model: String,
    #[serde(default)]
    pub effort: Effort,
    pub mode: WorkMode,
    pub permission: Permission,
    pub credential: Option<CredentialRef>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Defaults {
    pub revision: u64,
    pub config: Option<SessionConfig>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub archived: bool,
    pub activity: crate::activity::Summary,
    /// Project conversations have a project; connection conversations use config.resource.
    pub project: Option<ProjectId>,
    pub worktree: WorktreeId,
    pub revision: u64,
    pub config: SessionConfig,
    pub roles: crate::role::Snapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile: Option<SessionProfile>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fork: Option<crate::conversation::Fork>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delegation: Option<Box<Delegation>>,
}

/// A child execution is owned by one canonical parent tool call.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delegation {
    pub session: SessionId,
    pub turn: TurnId,
    pub entry: String,
    pub index: usize,
    pub role: Option<crate::RoleId>,
}

/// A session-owned provider snapshot, not an execution Node default.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionProfile {
    pub source: NodeId,
    pub provider: crate::conversation::Provider,
}

/// Write-only material forwarded directly between paired Nodes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionImport {
    pub source: NodeId,
    pub project: Option<ProjectId>,
    pub worktree: Option<WorktreeId>,
    pub config: SessionConfig,
    pub provider: crate::conversation::Provider,
    pub secret: Option<crate::Secret>,
    pub expires_at_ms: Option<u64>,
    pub roles: crate::role::Snapshot,
    pub role_credentials: Vec<crate::role::ProviderSecret>,
}

/// Admission metadata only, not another Agent conversation history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedTurn {
    pub id: TurnId,
    pub kind: crate::conversation::RunKind,
    pub session: SessionId,
    pub request: RequestId,
    pub revision: u64,
    pub config: SessionConfig,
    pub roles: crate::role::Snapshot,
    pub plugins: Vec<crate::plugin::Reference>,
}

#[cfg(test)]
mod permission {
    use super::*;

    #[test]
    fn round_trips_explicit_modes() {
        for (mode, name) in [
            (Permission::Ask, "ask"),
            (Permission::Project, "project"),
            (Permission::Full, "full"),
        ] {
            let config = SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Low,
                mode: WorkMode::Code,
                permission: mode,
                credential: None,
            };
            let mut body = serde_json::to_value(&config).unwrap();
            assert_eq!(body["permission"], name);
            assert_eq!(
                serde_json::from_value::<SessionConfig>(body.clone()).unwrap(),
                config
            );
            let mut extended = body.clone();
            extended["unrecognized"] = serde_json::json!(true);
            assert_eq!(
                serde_json::from_value::<SessionConfig>(extended).unwrap(),
                config
            );
            body.as_object_mut().unwrap().remove("permission");
            assert!(serde_json::from_value::<SessionConfig>(body).is_err());
        }
    }
}

#[cfg(test)]
mod configuration {
    use super::*;
    use serde_json::{Value, json};

    fn value() -> Value {
        json!({
            "provider": ProviderId::new(),
            "model": "fixture",
            "mode": "plan",
            "permission": "ask"
        })
    }

    #[test]
    fn ignores_unknown_fields() {
        let mut body = value();
        let expected: SessionConfig = serde_json::from_value(body.clone()).unwrap();
        body["unused"] = json!({"nested": [1, 2, 3]});
        assert_eq!(
            serde_json::from_value::<SessionConfig>(body).unwrap(),
            expected
        );
    }

    #[test]
    fn defaults_only_optional_choices() {
        let config: SessionConfig = serde_json::from_value(value()).unwrap();
        assert_eq!(config.effort, Effort::Default);
        assert!(config.assistant.is_none());
        assert!(config.resource.is_none());
        assert!(config.credential.is_none());
    }

    #[test]
    fn requires_execution_choices() {
        for field in ["provider", "model", "mode", "permission"] {
            let mut body = value();
            body.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<SessionConfig>(body).is_err(),
                "accepted missing {field}"
            );
        }
    }

    #[test]
    fn rejects_malformed_choices() {
        for (field, invalid) in [
            ("provider", json!("invalid")),
            ("model", json!(0)),
            ("effort", json!("automatic")),
            ("mode", json!("automatic")),
            ("permission", json!("automatic")),
        ] {
            let mut body = value();
            body[field] = invalid;
            assert!(
                serde_json::from_value::<SessionConfig>(body).is_err(),
                "accepted malformed {field}"
            );
        }
    }
}

#[cfg(test)]
mod work_mode {
    use super::*;

    #[test]
    fn requires_an_explicit_choice() {
        for (mode, name) in [(WorkMode::Plan, "plan"), (WorkMode::Code, "code")] {
            let config = SessionConfig {
                assistant: None,
                resource: None,
                provider: ProviderId::new(),
                model: "fixture".into(),
                effort: Effort::Default,
                mode,
                permission: Permission::Full,
                credential: None,
            };
            let mut value = serde_json::to_value(&config).unwrap();
            assert_eq!(value["mode"], name);
            assert_eq!(
                serde_json::from_value::<SessionConfig>(value.clone()).unwrap(),
                config
            );
            value["mode"] = serde_json::json!("automatic");
            assert!(serde_json::from_value::<SessionConfig>(value.clone()).is_err());
            value.as_object_mut().unwrap().remove("mode");
            assert!(serde_json::from_value::<SessionConfig>(value).is_err());
        }
    }
}
