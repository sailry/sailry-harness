//! Collapsed tool semantics are shared by Desktop, Mobile and other controllers.
use super::{Call, State};
use sailry_protocol::{
    ErrorCode,
    conversation::{ApprovalState, Page},
    plugin::desktop::Icon,
    tool::{LocalText, detail::Text},
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RowState {
    Denied,
    Cancelled,
    Interrupted,
    Failed,
    Pending,
    NotExecuted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Row {
    pub label: Option<LocalText>,
    pub icon: Option<Icon>,
    pub detail: Option<Text>,
    pub state: Option<RowState>,
    pub failed: bool,
    pub warning: bool,
    pub pending: bool,
    pub running: bool,
    pub has_details: bool,
}

impl Call {
    pub fn row(&self, page: &Page) -> Row {
        let summary = self.summary(page);
        let resolved = summary.and_then(|summary| summary.resolved.as_ref());
        let approval = self.approval.as_ref().map(|approval| approval.state);
        let denied = approval == Some(ApprovalState::Denied);
        let pending = approval == Some(ApprovalState::Pending);
        let code = self.failure.as_ref().and_then(|failure| failure.code);
        let expected = code == Some(ErrorCode::NotFound)
            && summary.is_some_and(|summary| summary.expected.is_some());
        let failed = self.failure.is_some()
            && !expected
            && !matches!(code, Some(ErrorCode::Cancelled | ErrorCode::OutcomeUnknown));
        let state = if denied {
            Some(RowState::Denied)
        } else if matches!(code, Some(ErrorCode::Cancelled | ErrorCode::OutcomeUnknown)) {
            Some(if code == Some(ErrorCode::Cancelled) {
                RowState::Cancelled
            } else {
                RowState::Interrupted
            })
        } else if failed {
            Some(RowState::Failed)
        } else if pending {
            Some(RowState::Pending)
        } else {
            match self.state {
                State::Waiting | State::Running | State::Returned => None,
                State::Cancelled => Some(RowState::Cancelled),
                State::NotExecuted => Some(RowState::NotExecuted),
                State::Interrupted => Some(RowState::Interrupted),
            }
        };
        let running = self.state == State::Running && !denied && !failed && !pending;
        Row {
            label: resolved.and_then(|resolved| resolved.label.clone()),
            icon: summary.and_then(|summary| summary.icon.clone()),
            detail: summary
                .filter(|_| failed && !denied)
                .and_then(|summary| summary.error.as_ref())
                .filter(|error| !error.trim().is_empty())
                .map(|error| Text {
                    text: error.clone(),
                    code: false,
                })
                .or_else(|| resolved.and_then(|resolved| resolved.input.summary.clone()))
                .or_else(|| {
                    summary
                        .and_then(|summary| summary.path.as_ref())
                        .map(|path| Text {
                            text: path.clone(),
                            code: false,
                        })
                }),
            state: if running { None } else { state },
            failed: !denied && failed,
            warning: !failed
                && matches!(
                    state,
                    Some(
                        RowState::Denied
                            | RowState::Cancelled
                            | RowState::Interrupted
                            | RowState::NotExecuted
                    )
                ),
            pending,
            running,
            has_details: self.details(page),
        }
    }
}
