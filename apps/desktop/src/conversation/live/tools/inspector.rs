//! Existing diagnostic copy retains the captured raw result without rendering it.
use super::*;

pub(super) fn copy(call: &Call, page: &Page) -> Option<String> {
    if !call.raw_result(page)
        || !call.diagnostics(page)
        || call
            .approval
            .as_ref()
            .is_some_and(|approval| approval.state == ApprovalState::Denied)
    {
        return None;
    }
    call.result(page).map(output::raw)
}
