use sailry_protocol::{ErrorCode, Fault, tool::ResultDisplay};
use serde_json::Value;

pub(super) struct Preview {
    pub text: String,
    pub notices: Vec<&'static str>,
}

impl Preview {
    /// Copy literal output unchanged; notice-only results copy their visible message.
    pub(super) fn copy_text(&self) -> String {
        if self.text.is_empty() {
            self.notices
                .iter()
                .map(|key| crate::tr(key).to_string())
                .collect::<Vec<_>>()
                .join("\n")
        } else {
            self.text.clone()
        }
    }
}

pub(super) fn fault(value: Option<&Value>) -> Option<Fault> {
    serde_json::from_value(value?.get("error")?.clone()).ok()
}

pub(super) fn failure(_name: &str, value: Option<&Value>) -> Option<String> {
    if let Some(fault) = fault(value) {
        return Some(fault.message);
    }
    let value = value?;
    if let Some(display) = ResultDisplay::from_value(value)
        && !display.diagnostics.is_empty()
    {
        return Some(
            display
                .diagnostics
                .into_iter()
                .map(|item| item.text)
                .collect::<Vec<_>>()
                .join("\n"),
        );
    }
    mcp_failure(value).or_else(|| value.get("output").and_then(mcp_failure))
}

fn mcp_failure(value: &Value) -> Option<String> {
    if value.get("isError").and_then(Value::as_bool) != Some(true) {
        return None;
    }
    let text = value
        .get("content")?
        .as_array()?
        .iter()
        .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
        .filter_map(|item| item.get("text").and_then(Value::as_str))
        .filter(|text| !text.trim().is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    (!text.is_empty()).then_some(text)
}

pub(super) fn is_error(_name: &str, value: Option<&Value>) -> bool {
    if let Some(fault) = fault(value) {
        return !matches!(fault.code, ErrorCode::Cancelled | ErrorCode::OutcomeUnknown);
    }
    value.is_some_and(|value| {
        value.get("isError").and_then(Value::as_bool) == Some(true)
            || value
                .get("output")
                .and_then(|value| value.get("isError"))
                .and_then(Value::as_bool)
                == Some(true)
            || ResultDisplay::from_value(value)
                .is_some_and(|display| display.diagnostics.iter().any(|item| item.error))
    })
}

pub(super) fn status(name: &str, value: Option<&Value>) -> Option<String> {
    if fault(value).is_some() || is_error(name, value) {
        return None;
    }
    let status = ResultDisplay::from_value(value?)?.status?;
    Some(status.label(&rust_i18n::locale()).to_owned())
}

pub(super) fn tool_preview(
    name: &str,
    presentation: sailry_protocol::tool::Presentation,
    value: &Value,
) -> Preview {
    if (presentation == sailry_protocol::tool::Presentation::Summary
        || fault(Some(value)).is_some()
        || is_error(name, Some(value))
        || ResultDisplay::from_value(value).is_some_and(|display| !display.diagnostics.is_empty()))
        && let Some(reason) = failure(name, Some(value))
        && !reason.trim().is_empty()
    {
        return Preview {
            text: reason,
            notices: Vec::new(),
        };
    }
    preview(value)
}

pub(super) fn preview(value: &Value) -> Preview {
    if let Some(error) = fault(Some(value)) {
        return Preview {
            text: String::new(),
            notices: vec![match error.code {
                ErrorCode::NotFound => "tool_not_found",
                ErrorCode::InvalidRequest => "tool_invalid",
                ErrorCode::PermissionDenied => "tool_denied",
                ErrorCode::Cancelled => "turn_cancelled",
                ErrorCode::OutcomeUnknown => "tool_interrupted",
                ErrorCode::Unavailable => "tool_unavailable",
                _ => "tool_failed",
            }],
        };
    }
    Preview {
        text: readable(value),
        notices: Vec::new(),
    }
}

pub(super) fn has_output(value: &Value) -> bool {
    let body = preview(value);
    !body.text.is_empty() || !body.notices.is_empty()
}

fn readable(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) => items.iter().map(readable).collect::<Vec<_>>().join("\n"),
        Value::Object(fields) => fields
            .iter()
            .filter(|(_, value)| !value.is_null())
            .map(|(key, value)| format!("{key}: {}", readable(value)))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        _ => value.to_string(),
    }
}
#[cfg(test)]
mod tests;
