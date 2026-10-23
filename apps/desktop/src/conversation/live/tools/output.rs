use sailry_protocol::{
    ErrorCode, Fault,
    tool::{Failure, ResultDisplay},
};
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

pub(super) fn fault(observed: Option<&Failure>, value: Option<&Value>) -> Option<Fault> {
    observed?;
    serde_json::from_value(value?.get("error")?.clone()).ok()
}

pub(super) fn failure(observed: Option<&Failure>, value: Option<&Value>) -> Option<String> {
    if let Some(fault) = fault(observed, value) {
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
    observed?;
    if let Some(error) = value.get("error").and_then(Value::as_str)
        && !error.trim().is_empty()
    {
        return Some(error.to_owned());
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

pub(super) fn is_error(observed: Option<&Failure>, value: Option<&Value>) -> bool {
    let Some(observed) = observed else {
        return false;
    };
    if expected(Some(observed), value).is_some() {
        return false;
    }
    !matches!(
        observed.code,
        Some(ErrorCode::Cancelled | ErrorCode::OutcomeUnknown)
    )
}

pub(super) fn expected(observed: Option<&Failure>, value: Option<&Value>) -> Option<&'static str> {
    use sailry_protocol::tool::Expected;
    if observed?.code != Some(ErrorCode::NotFound) {
        return None;
    }
    Some(match ResultDisplay::from_value(value?)?.expected? {
        Expected::DirectoryNotFound => "tool_directory_missing",
        Expected::FileNotFound => "tool_file_missing",
    })
}

pub(super) fn tool_preview(observed: Option<&Failure>, value: &Value) -> Preview {
    if let Some(reason) = failure(observed, Some(value))
        && !reason.trim().is_empty()
    {
        return Preview {
            text: reason,
            notices: Vec::new(),
        };
    }
    preview(observed, value)
}

pub(super) fn preview(observed: Option<&Failure>, value: &Value) -> Preview {
    if let Some(observed) = observed {
        return Preview {
            text: String::new(),
            notices: vec![match observed.code {
                Some(ErrorCode::NotFound) => "tool_not_found",
                Some(ErrorCode::InvalidRequest) => "tool_invalid",
                Some(ErrorCode::PermissionDenied) => "tool_denied",
                Some(ErrorCode::Cancelled) => "turn_cancelled",
                Some(ErrorCode::OutcomeUnknown) => "tool_interrupted",
                Some(ErrorCode::Unavailable) => "tool_unavailable",
                _ => "tool_failed",
            }],
        };
    }
    Preview {
        text: sailry_client::conversation::tools::display::literal(value)
            .map(std::borrow::Cow::into_owned)
            .unwrap_or_default(),
        notices: Vec::new(),
    }
}

/// Raw inspection is explicit and preserves every public business field.
pub(super) fn raw(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("tool JSON is serializable")
}

#[cfg(test)]
mod tests;
