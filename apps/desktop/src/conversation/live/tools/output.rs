use sailry_protocol::{
    ErrorCode,
    tool::{Failure, detail::Summary},
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

pub(super) fn failure(summary: Option<&Summary>) -> Option<&str> {
    summary?
        .error
        .as_deref()
        .filter(|error| !error.trim().is_empty())
}

pub(super) fn tool_preview(
    observed: Option<&Failure>,
    summary: Option<&Summary>,
    value: &Value,
) -> Preview {
    if let Some(reason) = failure(summary) {
        return Preview {
            text: reason.to_owned(),
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
