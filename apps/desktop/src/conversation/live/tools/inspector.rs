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
    let source = call.response.as_ref()?;
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == source.entry && entry.turn == call.turn)?;
    match entry.parts.get(source.index)? {
        sailry_protocol::conversation::Part::ToolResult {
            id, name, result, ..
        } if *id == call.id && *name == call.name => Some(output::raw(result)),
        _ => None,
    }
}
