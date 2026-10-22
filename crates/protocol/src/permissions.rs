//! Standard OS permissions and package-owned authorization subjects.
use crate::{
    NodeId,
    plugin::{Action, Extension},
};
use serde::{Deserialize, Serialize};

#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    FullDisk,
    ScreenRecording,
    Accessibility,
    Microphone,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Identity {
    pub device: NodeId,
    pub subject: String,
    pub kind: Kind,
}

/// Standard permissions can use the controller's native handler or package callbacks.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Contribution {
    pub kind: Kind,
    /// Stable authorization subject, such as an application bundle identifier.
    pub subject: String,
    /// An explicitly read-only host export. None uses the controller's native handler.
    pub check: Option<String>,
    pub request: Option<String>,
    /// One onboarding operation may cover several permissions for the same subject.
    pub request_group: Option<String>,
    /// Exact SDK capabilities the handlers need, also required in extension.actions.
    #[serde(default)]
    pub actions: Vec<Action>,
}

impl Contribution {
    pub fn valid(&self, extension: &Extension) -> bool {
        !self.subject.trim().is_empty()
            && self.subject.len() <= 256
            && self
                .request_group
                .as_ref()
                .is_none_or(|group| !group.is_empty() && group.len() <= 128)
            && self
                .actions
                .iter()
                .all(|action| extension.actions.contains(action))
            && match &self.check {
                None => {
                    self.subject == "sailry" && self.request.is_none() && self.actions.is_empty()
                }
                Some(check) => extension.host.as_ref().is_some_and(|host| {
                    host.valid()
                        && host.read_only.contains(check)
                        && self
                            .request
                            .as_ref()
                            .is_none_or(|request| host.handlers.contains(request))
                }),
            }
    }
}

pub fn valid(extension: &Extension) -> bool {
    extension.permissions.len() <= 32
        && extension
            .permissions
            .iter()
            .all(|entry| entry.valid(extension))
}

/// Callback results are scoped to the declaration's device and subject by the caller.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub kind: Kind,
    pub authorized: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Status {
    pub grants: Vec<Grant>,
    pub can_request: bool,
}
