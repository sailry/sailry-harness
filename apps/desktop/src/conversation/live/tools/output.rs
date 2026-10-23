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
        if matches!(
            fault.details,
            Some(sailry_protocol::FaultDetails::SystemDiagnostic(_))
        ) {
            return Some(crate::tr(crate::conversation::live::failure_key(&fault)).to_string());
        }
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
    if expected(value).is_some() {
        return false;
    }
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

pub(super) fn expected(value: Option<&Value>) -> Option<&'static str> {
    use sailry_protocol::tool::Expected;
    if fault(value)?.code != ErrorCode::NotFound {
        return None;
    }
    Some(match ResultDisplay::from_value(value?)?.expected? {
        Expected::DirectoryNotFound => "tool_directory_missing",
        Expected::FileNotFound => "tool_file_missing",
    })
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

/// Raw inspection is explicit and preserves every public business field.
pub(super) fn raw(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("tool JSON is serializable")
}

fn readable(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Array(items) if items.iter().all(Value::is_string) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null | Value::Array(_) | Value::Object(_) => String::new(),
        _ => value.to_string(),
    }
}

pub(super) fn observed_failure(
    failure: Option<&sailry_protocol::tool::Failure>,
    result: &Value,
) -> Option<String> {
    failure?;
    result.get("error")?.as_str().map(str::to_owned)
}
#[cfg(test)]
mod tests;
