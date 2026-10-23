//! Captured presentation and execution facts never change canonical tool data.
use super::{Call, State};
use sailry_protocol::{
    conversation::{ApprovalState, Page},
    tool::{Failure, Presentation, ResultDisplay},
};
use serde_json::Value;

impl Call {
    pub fn diagnostics(&self, page: &Page) -> bool {
        if self.failure.is_some() {
            return true;
        }
        let Some(result) = self.result(page) else {
            return false;
        };
        serde_json::from_value::<sailry_protocol::Fault>(result["error"].clone()).is_ok()
            || result.get("isError").and_then(Value::as_bool) == Some(true)
            || result["output"].get("isError").and_then(Value::as_bool) == Some(true)
            || ResultDisplay::from_value(result)
                .is_some_and(|display| !display.diagnostics.is_empty())
    }

    pub fn hidden(&self, page: &Page) -> bool {
        self.presentation == Presentation::Hidden
            && self.state == State::Returned
            && self.approval.is_none()
            && self.question.is_none()
            && !self.terminal_interaction
            && !self.diagnostics(page)
            && self.images(page).is_empty()
    }

    pub fn details(&self, page: &Page) -> bool {
        if self
            .approval
            .as_ref()
            .is_some_and(|approval| approval.state == ApprovalState::Denied)
        {
            return false;
        }
        if self.diagnostics(page) {
            return self.result(page).is_some();
        }
        if !self.images(page).is_empty() {
            return true;
        }
        if matches!(
            self.presentation,
            Presentation::Summary | Presentation::Hidden
        ) {
            return false;
        }
        let Some(result) = self.result(page) else {
            return false;
        };
        if let Some(content) = &self.content {
            return content.has_visible(result);
        }
        if let Some(output) = self
            .resolved
            .as_ref()
            .and_then(|resolved| resolved.output.as_ref())
        {
            return output.preview_deferred
                || !output.preview.trim().is_empty()
                || !output.notices.is_empty()
                || !output.paths.is_empty()
                || output.table.is_some()
                || output
                    .body
                    .as_ref()
                    .is_some_and(|body| !body.text.is_empty() || body.empty.is_some());
        }
        visible(result)
    }

    pub fn raw_result(&self, page: &Page) -> bool {
        self.result(page).is_some() && self.display(page).is_some_and(|display| display.raw_result)
    }
}

pub(super) fn failure(call: &Call, page: &Page) -> Option<Failure> {
    let response = call.response.as_ref()?;
    let id = call.id.as_deref()?;
    let entry = page
        .entries
        .iter()
        .find(|entry| entry.id == response.entry)?;
    entry.parts.iter().find_map(|part| {
        let sailry_protocol::conversation::Part::Resource(value) = part else {
            return None;
        };
        (value["type"] == "tool_failed"
            && value["index"].as_u64() == Some(response.index as u64)
            && value["name"].as_str() == Some(call.name.as_str())
            && value["id"].as_str() == Some(id))
        .then(|| serde_json::from_value(value["failure"].clone()).ok())
        .flatten()
    })
}

fn visible(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(text) => !text.is_empty(),
        Value::Array(items) => items.iter().any(visible),
        Value::Object(fields) => fields.values().any(visible),
        _ => true,
    }
}
