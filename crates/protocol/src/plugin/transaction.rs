//! Bounded atomic operations within one plugin's Node-owned data.
use crate::{Command, dispatch, notification};
use serde::{Deserialize, Serialize};

pub mod sdk;

pub const MAX_OPERATIONS: usize = 64;
pub const MAX_BYTES: usize = 2 * 1024 * 1024;

/// No nested transactions, external side effects, arbitrary SQL, or foreign namespaces.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "data",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Operation {
    Write {
        key: String,
        value: serde_json::Value,
        expected_revision: u64,
    },
    Index {
        key: String,
        value: serde_json::Value,
        index: super::storage::Index,
        expected_revision: u64,
    },
    Remove {
        key: String,
        expected_revision: u64,
    },
    ConversationWrite {
        key: String,
        value: serde_json::Value,
        expected_revision: u64,
    },
    ConversationRemove {
        key: String,
        expected_revision: u64,
    },
    Dispatch(dispatch::Command),
    Notify(notification::Draft),
    Submit {
        session: crate::SessionId,
        expected_revision: u64,
        message: crate::conversation::Input,
    },
    Continue {
        session: crate::SessionId,
        after: Option<crate::TurnId>,
        message: crate::conversation::Input,
        key: String,
        expected_revision: u64,
        scope: super::storage::Scope,
    },
    Stop {
        turn: crate::TurnId,
    },
}

impl Operation {
    /// Reuses ordinary command validation and the captured package identity.
    pub fn command(&self, package: &super::Reference) -> Command {
        match self {
            Self::Stop { turn } => Command::StopTurn { turn: *turn },
            Self::Submit {
                session,
                expected_revision,
                message,
            } => Command::SubmitTurn {
                session: *session,
                expected_revision: *expected_revision,
                message: message.clone(),
            },
            Self::Continue {
                session,
                after,
                message,
                key,
                expected_revision,
                scope,
            } => Command::ContinueTurn {
                session: *session,
                after: *after,
                message: message.clone(),
                key: key.clone(),
                expected_revision: *expected_revision,
                scope: *scope,
            },
            Self::Write {
                key,
                value,
                expected_revision,
            } => Command::WritePluginValue {
                key: key.clone(),
                value: value.clone(),
                expected_revision: *expected_revision,
            },
            Self::Index {
                key,
                value,
                index,
                expected_revision,
            } => Command::WriteIndexedPluginValue {
                key: key.clone(),
                value: value.clone(),
                index: index.clone(),
                expected_revision: *expected_revision,
            },
            Self::Remove {
                key,
                expected_revision,
            } => Command::RemovePluginValue {
                key: key.clone(),
                expected_revision: *expected_revision,
            },
            Self::ConversationWrite {
                key,
                value,
                expected_revision,
            } => Command::WritePluginConversationValue {
                key: key.clone(),
                value: value.clone(),
                expected_revision: *expected_revision,
            },
            Self::ConversationRemove {
                key,
                expected_revision,
            } => Command::RemovePluginConversationValue {
                key: key.clone(),
                expected_revision: *expected_revision,
            },
            Self::Dispatch(action) => Command::Dispatch {
                package: package.clone(),
                action: action.clone(),
            },
            Self::Notify(content) => Command::PublishNotification {
                package: package.clone(),
                content: content.clone(),
            },
        }
    }
}

pub fn validate(operations: &[Operation]) -> Result<(), crate::Fault> {
    if operations
        .iter()
        .filter(|operation| {
            matches!(
                operation,
                Operation::Submit { .. } | Operation::Continue { .. }
            )
        })
        .count()
        > 1
    {
        return Err(crate::Fault::new(
            crate::ErrorCode::InvalidRequest,
            "a transaction may admit only one turn",
        ));
    }
    if operations.is_empty()
        || operations.len() > MAX_OPERATIONS
        || serde_json::to_vec(operations).map_or(true, |bytes| bytes.len() > MAX_BYTES)
    {
        return Err(crate::Fault::new(
            crate::ErrorCode::InvalidRequest,
            "invalid plugin transaction size",
        ));
    }
    if operations.iter().any(|operation| {
        matches!(
            operation,
            Operation::Dispatch(
                dispatch::Command::ListHandlers
                    | dispatch::Command::ListSchedules
                    | dispatch::Command::ListJobs { .. }
                    | dispatch::Command::ReadJob { .. }
                    | dispatch::Command::ReadResult { .. }
            )
        )
    }) {
        return Err(crate::Fault::new(
            crate::ErrorCode::InvalidRequest,
            "plugin transactions require mutations",
        ));
    }
    Ok(())
}
