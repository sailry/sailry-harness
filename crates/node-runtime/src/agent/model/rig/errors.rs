use super::super::types::{Error, ErrorKind};
use rig_core::error::{ErrorKind as RigKind, ProviderError};
use sailry_protocol::{ErrorCode, Fault, FaultDetails, Secret};
use std::time::SystemTime;

pub(super) fn provider(error: ProviderError, secrets: &[Secret]) -> Error {
    let report = error.report();
    let message = if matches!(&error, ProviderError::Truncated) {
        "Model stream ended before a completion event".to_string()
    } else {
        diagnostic(&report)
    };
    let kind = if redirected(&error) {
        ErrorKind::InvalidRequest
    } else if report.is_context_overflow() {
        ErrorKind::ContextOverflow
    } else if matches!(report.http_status, Some(401 | 403))
        || matches!(
            report.code.as_deref(),
            Some(
                "invalid_api_key"
                    | "authentication_error"
                    | "invalid_authentication"
                    | "unauthorized"
                    | "permission_denied"
            )
        )
    {
        ErrorKind::Authentication
    } else if report.code.as_deref().is_some_and(|code| {
        matches!(
            code,
            "insufficient_quota"
                | "credits_exhausted"
                | "billing_not_active"
                | "quota_exceeded"
                | "billing_hard_limit_reached"
        )
    }) {
        ErrorKind::Quota
    } else if report.http_status == Some(429)
        || matches!(
            report.code.as_deref(),
            Some("rate_limit_exceeded" | "rate_limit_error" | "too_many_requests")
        )
    {
        ErrorKind::RateLimited
    } else if matches!(report.http_status, Some(500..=599))
        || matches!(
            report.code.as_deref(),
            Some(
                "upstream_bad_gateway"
                    | "upstream_unavailable"
                    | "upstream_timeout"
                    | "server_error"
                    | "internal_error"
                    | "internal_server_error"
                    | "bad_gateway"
                    | "gateway_timeout"
                    | "service_unavailable"
                    | "service_unavailable_error"
                    | "server_is_overloaded"
                    | "overloaded_error"
            )
        )
    {
        ErrorKind::Unavailable
    } else {
        match error {
            ProviderError::Http(_) | ProviderError::Stalled(_) => ErrorKind::Network,
            ProviderError::InvalidAuthentication(_) => ErrorKind::Authentication,
            ProviderError::UnsupportedOption(_) => ErrorKind::Unsupported,
            ProviderError::Request(_) | ProviderError::Url(_) => ErrorKind::InvalidRequest,
            ProviderError::Json(_) | ProviderError::Response(_) | ProviderError::Truncated => {
                ErrorKind::InvalidResponse
            }
            _ if matches!(report.http_status, Some(400..=499)) => ErrorKind::InvalidRequest,
            _ if report.kind == RigKind::Cancelled => ErrorKind::Cancelled,
            _ => ErrorKind::Other,
        }
    };
    Error {
        kind,
        message: super::super::errors::safe(&message, secrets),
        retry_after: report.retry_after(SystemTime::now()),
    }
}

fn redirected(error: &ProviderError) -> bool {
    let mut source: Option<&(dyn std::error::Error + 'static)> = Some(match error {
        ProviderError::Http(error) => error.as_ref(),
        _ => error,
    });
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<reqwest::Error>()
            && error.is_redirect()
        {
            return true;
        }
        source = error.source();
    }
    false
}

fn diagnostic(report: &rig_core::error::ErrorReport) -> String {
    let Some(body) = report.provider_response_body() else {
        return report.message.clone();
    };
    let value = serde_json::from_str::<serde_json::Value>(body).ok();
    let message = value
        .as_ref()
        .and_then(|value| {
            [
                "/error/message",
                "/response/error/message",
                "/message",
                "/detail",
            ]
            .into_iter()
            .find_map(|path| value.pointer(path).and_then(serde_json::Value::as_str))
        })
        .filter(|message| !message.is_empty());
    if let Some(message) = message {
        return match (&report.code, report.http_status) {
            (Some(code), None) => format!("{code}: {message}"),
            (_, Some(status)) => format!("{message} (HTTP {status})"),
            _ => message.into(),
        };
    }
    report.http_status.map_or_else(
        || "Model request failed".into(),
        |status| format!("Model request failed (HTTP {status})"),
    )
}

pub(super) fn local(error: Fault) -> Error {
    let kind = match error.code {
        ErrorCode::Cancelled => ErrorKind::Cancelled,
        ErrorCode::NotConfigured => ErrorKind::NotConfigured,
        ErrorCode::InvalidRequest => ErrorKind::InvalidRequest,
        ErrorCode::PermissionDenied => ErrorKind::Authentication,
        _ if matches!(error.details, Some(FaultDetails::Network)) => ErrorKind::Network,
        _ => ErrorKind::Other,
    };
    Error::new(kind, error.message)
}
