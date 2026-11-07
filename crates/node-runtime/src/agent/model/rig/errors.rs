use super::super::types::{Error, ErrorKind};
use rig_core::error::{ErrorKind as RigKind, ProviderError};
use sailry_protocol::{ErrorCode, Fault, FaultDetails, ModelFailure, Secret};
use std::time::SystemTime;

pub(super) fn provider(error: ProviderError, secrets: &[Secret]) -> Error {
    let report = error.report();
    let kind = if report.is_context_overflow() {
        ErrorKind::ContextOverflow
    } else if matches!(report.http_status, Some(401 | 403))
        || matches!(
            report.code.as_deref(),
            Some("invalid_api_key" | "authentication_error" | "invalid_authentication")
        )
    {
        ErrorKind::Authentication
    } else if report.code.as_deref().is_some_and(|code| {
        matches!(
            code,
            "insufficient_quota" | "credits_exhausted" | "billing_not_active"
        )
    }) {
        ErrorKind::Quota
    } else if report.http_status == Some(429)
        || matches!(
            report.code.as_deref(),
            Some("rate_limit_exceeded" | "rate_limit_error")
        )
    {
        ErrorKind::RateLimited
    } else if matches!(report.http_status, Some(500..=599))
        || matches!(
            report.code.as_deref(),
            Some("server_error" | "overloaded_error" | "service_unavailable")
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
        message: super::super::errors::safe(&report.message, secrets),
        retry_after: report.retry_after(SystemTime::now()),
    }
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

pub(super) fn fault(error: Error) -> Fault {
    let code = if error.kind == ErrorKind::NotConfigured {
        ErrorCode::NotConfigured
    } else {
        ErrorCode::Unavailable
    };
    let reason = match error.kind {
        ErrorKind::Authentication => ModelFailure::Unauthorized,
        ErrorKind::RateLimited | ErrorKind::Quota => ModelFailure::RateLimited,
        _ => ModelFailure::Request,
    };
    let mut fault = Fault::new(code, error.message);
    fault.details = Some(FaultDetails::ModelRequest { reason });
    fault
}
