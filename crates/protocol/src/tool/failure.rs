use crate::ErrorCode;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    #[default]
    Runtime,
    Tool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Failure {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<ErrorCode>,
    #[serde(default)]
    pub origin: Origin,
}

impl Failure {
    pub fn runtime(code: Option<ErrorCode>) -> Self {
        Self {
            code,
            origin: Origin::Runtime,
        }
    }

    pub fn tool(code: Option<ErrorCode>) -> Self {
        Self {
            code,
            origin: Origin::Tool,
        }
    }
}

impl From<ErrorCode> for Failure {
    fn from(code: ErrorCode) -> Self {
        Self {
            code: Some(code),
            origin: origin(code),
        }
    }
}

fn origin(code: ErrorCode) -> Origin {
    match code {
        ErrorCode::InvalidRequest
        | ErrorCode::NotFound
        | ErrorCode::Conflict
        | ErrorCode::RevisionConflict => Origin::Tool,
        _ => Origin::Runtime,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn defaults_missing_origin_to_runtime() {
        assert_eq!(
            serde_json::from_value::<Failure>(json!({})).unwrap(),
            Failure::runtime(None)
        );
        for code in [None, Some(ErrorCode::NotFound), Some(ErrorCode::Internal)] {
            assert_eq!(
                serde_json::from_value::<Failure>(json!({"code":code})).unwrap(),
                Failure::runtime(code)
            );
        }
    }

    #[test]
    fn preserves_explicit_origins() {
        for failure in [
            Failure::runtime(None),
            Failure::runtime(Some(ErrorCode::Unavailable)),
            Failure::tool(None),
            Failure::tool(Some(ErrorCode::InvalidRequest)),
        ] {
            let encoded = serde_json::to_value(&failure).unwrap();
            assert_eq!(
                encoded["origin"],
                match failure.origin {
                    Origin::Runtime => "runtime",
                    Origin::Tool => "tool",
                }
            );
            assert_eq!(serde_json::from_value::<Failure>(encoded).unwrap(), failure);
        }
    }

    #[test]
    fn classifies_business_codes() {
        for code in [
            ErrorCode::InvalidRequest,
            ErrorCode::NotFound,
            ErrorCode::Conflict,
            ErrorCode::RevisionConflict,
        ] {
            assert_eq!(Failure::from(code), Failure::tool(Some(code)));
        }
        for code in [
            ErrorCode::WrongTarget,
            ErrorCode::NotConfigured,
            ErrorCode::PermissionDenied,
            ErrorCode::Expired,
            ErrorCode::Busy,
            ErrorCode::Unavailable,
            ErrorCode::Cancelled,
            ErrorCode::OutcomeUnknown,
            ErrorCode::Internal,
        ] {
            assert_eq!(Failure::from(code), Failure::runtime(Some(code)));
        }
    }
}
