//! Transient Status contributions computed by the execution Node.
use super::{Contribution, Slot, State};
use crate::{Fault, NodeId, SessionId, WorktreeId, plugin::Reference};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub worktree: Option<WorktreeId>,
    pub session: Option<SessionId>,
}

impl Scope {
    pub fn valid(&self) -> bool {
        self.worktree.is_some() || self.session.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub package: Reference,
    /// The Status declaration captured from this package reference.
    pub declaration: Contribution,
    pub state: State,
    /// Host export for Button invocations; absent for controller-local events.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invoke: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub package: Reference,
    pub fault: Fault,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub node: NodeId,
    pub scope: Scope,
    pub entries: Vec<Entry>,
    pub failures: Vec<Failure>,
}

/// Status values contain localized data; controller interaction stays local.
pub fn valid(state: &State, entry: &Contribution) -> bool {
    entry.slot == Slot::Status
        && state.valid(entry)
        && state.reply_to.is_none()
        && state.scope.is_none()
        && state.value.is_null()
        && state.choices.is_none()
}
