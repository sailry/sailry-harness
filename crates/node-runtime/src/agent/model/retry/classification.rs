//! Responses mappings follow openai/codex 61f34c35 (api_bridge and responses_error).
//! HTTP errors and failed stream events deliberately have different semantics.
use super::*;
use sailry_protocol::conversation::recovery::{Stop, Transport};
use tokio::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Class {
    Terminal(Stop),
    Backoff,
    Advice(Stop),
    Connection,
}

pub(super) struct Failure {
    pub class: Class,
    pub retry_at: Option<Instant>,
    pub transport: Option<Transport>,
    pub status: Option<u16>,
    pub code: Option<String>,
    pub refused: bool,
    pub switch: bool,
}

impl Failure {
    pub fn new(error: &AdkError) -> Self {
        let metadata = &error.details.metadata;
        let field = |name: &str| metadata.get(name).and_then(serde_json::Value::as_str);
        let transport = match field("transport") {
            Some("http") => Some(Transport::Http),
            Some("websocket") => Some(Transport::WebSocket),
            _ => None,
        };
        let mut retry_at = error.retry_deadline().map(Instant::from_std).or_else(|| {
            error
                .retry
                .retry_after()
                .and_then(|delay| Instant::now().checked_add(delay))
        });
        let status = error.details.upstream_status_code;
        let code = field("provider_error_code");
        let kind = field("provider_error_type");
        let event = field("provider_event_type");
        if retry_at.is_none()
            && event == Some("response.failed")
            && matches!(code, Some("rate_limit_exceeded" | "slow_down"))
        {
            retry_at =
                rate_delay(&error.message).and_then(|delay| Instant::now().checked_add(delay));
        }
        let completed = metadata
            .get("response_completed")
            .and_then(serde_json::Value::as_bool)
            == Some(true);
        let local = error.component != adk_core::ErrorComponent::Model
            || error.category == adk_core::ErrorCategory::Cancelled
            || std::error::Error::source(error)
                .and_then(|source| source.downcast_ref::<Fault>())
                .is_some_and(|fault| {
                    matches!(fault.code, ErrorCode::Cancelled | ErrorCode::OutcomeUnknown)
                });
        let class = if local {
            Class::Terminal(Stop::LocalFailure)
        } else if completed {
            Class::Terminal(Stop::ResponseIncomplete)
        } else if transport == Some(Transport::WebSocket)
            && field("transport_phase") == Some("stream")
            && !error.retry.should_retry
            && event.is_none()
        {
            Class::Terminal(Stop::ResponseIncomplete)
        } else if let Some(status) = status.filter(|status| *status >= 400) {
            if status == 429 && code.is_none() && error.code == "model.plugin.quota" {
                Class::Terminal(Stop::QuotaExceeded)
            } else {
                http(status, code, kind)
            }
        } else if field("transport_error_kind") == Some("connection") {
            Class::Connection
        } else if event == Some("response.failed") {
            stream(code)
        } else if event == Some("response.incomplete")
            || error.code == "model.openai_responses.incomplete"
        {
            match field("incomplete_reason") {
                Some("content_filter") => Class::Backoff,
                _ => Class::Terminal(Stop::ResponseIncomplete),
            }
        } else if event == Some("error") && code == Some("flex_unavailable") {
            Class::Terminal(Stop::FlexUnavailable)
        } else if error.code.starts_with("model.plugin.") && code.is_none() {
            plugin(error)
        } else if error.code.starts_with("model.plugin.")
            && field("transport_phase") == Some("stream")
        {
            stream(code)
        } else if field("transport_phase") == Some("stream") {
            stream(code)
        } else if field("transport_error_kind").is_some() {
            Class::Backoff
        } else if error.code.starts_with("model.plugin.") {
            plugin(error)
        } else if matches!(
            error.category,
            adk_core::ErrorCategory::Unavailable
                | adk_core::ErrorCategory::Timeout
                | adk_core::ErrorCategory::RateLimited
        ) || matches!(
            error.code,
            "model.openai_responses.parse" | "model.openai_responses.unknown"
        ) {
            Class::Backoff
        } else {
            Class::Terminal(Stop::LocalFailure)
        };
        let before = !local
            && !completed
            && transport == Some(Transport::WebSocket)
            && field("transport_phase") == Some("request");
        let refused = before
            && field("websocket_refusal") == Some("definitive")
            && matches!(
                error.code,
                "model.openai_responses.ws_handshake" | "model.openai_responses.unfinished_stream"
            );
        let switch = refused
            || (before
                && status.is_none()
                && retry_at.is_none()
                && field("transport_error_kind") != Some("build")
                && matches!(class, Class::Backoff | Class::Connection));
        Self {
            class,
            retry_at,
            transport,
            status,
            code: code.map(str::to_owned),
            refused,
            switch,
        }
    }
}

fn rate_delay(message: &str) -> Option<std::time::Duration> {
    // Same limited advisory grammar as Codex responses_error at 61f34c35.
    static PATTERN: std::sync::LazyLock<regex::Regex> = std::sync::LazyLock::new(|| {
        regex::Regex::new(r"(?i)try again in\s*(\d+(?:\.\d+)?)\s*(s|ms|seconds?)")
            .expect("fixed retry advisory pattern")
    });
    let matched = PATTERN.captures(message)?;
    let value = matched.get(1)?.as_str().parse::<f64>().ok()?;
    if matched.get(2)?.as_str().eq_ignore_ascii_case("ms") {
        Some(std::time::Duration::from_millis(value as u64))
    } else {
        std::time::Duration::try_from_secs_f64(value).ok()
    }
}

fn http(status: u16, code: Option<&str>, kind: Option<&str>) -> Class {
    match status {
        400 => Class::Terminal(match code {
            Some("invalid_prompt") => Stop::InvalidPrompt,
            Some("cyber_policy" | "bio_policy" | "misalignment_policy_violation") => {
                Stop::PolicyViolation
            }
            _ => Stop::InvalidRequest,
        }),
        403 if code == Some("misalignment_policy_violation") => {
            Class::Terminal(Stop::PolicyViolation)
        }
        429 => match (code, kind) {
            (Some("flex_unavailable"), _) => Class::Terminal(Stop::FlexUnavailable),
            (_, Some("usage_limit_reached")) => Class::Terminal(Stop::UsageLimitReached),
            (_, Some("usage_not_included")) => Class::Terminal(Stop::UsageNotIncluded),
            (_, Some("insufficient_quota")) => Class::Terminal(Stop::QuotaExceeded),
            (
                Some(
                    "insufficient_quota"
                    | "credit_balance_exhausted"
                    | "organization_spend_limit_exceeded"
                    | "project_spend_limit_exceeded"
                    | "organization_usage_limit_exceeded",
                ),
                _,
            ) => Class::Terminal(Stop::QuotaExceeded),
            _ => Class::Advice(Stop::RateLimited),
        },
        503 if code == Some("server_is_overloaded") => Class::Advice(Stop::ServerOverloaded),
        _ => Class::Backoff,
    }
}

fn stream(code: Option<&str>) -> Class {
    match code {
        Some("rate_limit_exceeded" | "slow_down") => Class::Advice(Stop::RateLimited),
        Some("context_length_exceeded") => Class::Terminal(Stop::ContextWindowExceeded),
        Some(
            "insufficient_quota"
            | "credit_balance_exhausted"
            | "organization_spend_limit_exceeded"
            | "project_spend_limit_exceeded",
        ) => Class::Terminal(Stop::QuotaExceeded),
        Some("usage_not_included") => Class::Terminal(Stop::UsageNotIncluded),
        Some("flex_unavailable") => Class::Terminal(Stop::FlexUnavailable),
        Some("invalid_prompt") => Class::Terminal(Stop::InvalidPrompt),
        Some("cyber_policy" | "bio_policy" | "misalignment_policy_violation") => {
            Class::Terminal(Stop::PolicyViolation)
        }
        Some("server_is_overloaded") => Class::Advice(Stop::ServerOverloaded),
        _ => Class::Backoff,
    }
}

fn plugin(error: &AdkError) -> Class {
    match error.code {
        "model.plugin.quota" => Class::Terminal(Stop::QuotaExceeded),
        "model.plugin.context_overflow" => Class::Terminal(Stop::ContextWindowExceeded),
        "model.plugin.invalid_request" => Class::Terminal(Stop::InvalidRequest),
        "model.plugin.runtime"
        | "model.plugin.translation"
        | "model.plugin.resource_budget"
        | "model.plugin.invalid_translation"
        | "model.plugin.unsupported" => Class::Terminal(Stop::LocalFailure),
        _ => Class::Backoff,
    }
}

#[cfg(test)]
mod tests;
