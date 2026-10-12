//! Responses error normalization and safe, non-replaying stream failures.

use adk_core::{AdkError, ErrorCategory, ErrorComponent, RetryHint};
use async_openai::error::{ApiErrorResponse, OpenAIError};
use async_openai::types::responses::ResponseErrorEvent;
use chrono::{DateTime, Datelike, NaiveDateTime, Utc};
use serde::Deserialize;
use serde_json::Value;
use std::{
    fmt,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RetryAfter {
    Fallback,
    Delay(u64),
    Unsupported,
}

struct HttpError {
    error: ApiErrorResponse,
    retry_after: RetryAfter,
}

impl fmt::Debug for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("HttpError")
            .field("status", &self.error.status_code.as_u16())
            .field("retry_after", &self.retry_after)
            .finish()
    }
}

impl fmt::Display for HttpError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "OpenAI Responses HTTP {}",
            self.error.status_code.as_u16()
        )
    }
}

impl std::error::Error for HttpError {}

pub(super) fn retry_after(headers: &http::HeaderMap) -> RetryAfter {
    parse_delay(
        headers
            .get(http::header::RETRY_AFTER)
            .and_then(|value| value.to_str().ok()),
        Utc::now(),
        Instant::now(),
    )
}

fn parse_delay(value: Option<&str>, now: DateTime<Utc>, started: Instant) -> RetryAfter {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return RetryAfter::Fallback;
    };
    let delay = if value.bytes().all(|byte| byte.is_ascii_digit()) {
        let Ok(seconds) = value.parse::<u64>() else {
            return RetryAfter::Unsupported;
        };
        Duration::from_secs(seconds)
    } else {
        let Some(date) = parse_date(value, now) else {
            return RetryAfter::Fallback;
        };
        date.signed_duration_since(now)
            .to_std()
            .unwrap_or(Duration::ZERO)
    };
    // Round dates up to the retry hint's millisecond precision, never earlier.
    let Ok(millis) = u64::try_from(delay.as_nanos().div_ceil(1_000_000)) else {
        return RetryAfter::Unsupported;
    };
    if started.checked_add(Duration::from_millis(millis)).is_none() {
        return RetryAfter::Unsupported;
    }
    RetryAfter::Delay(millis)
}

fn parse_date(value: &str, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if let Ok(date) = DateTime::parse_from_rfc2822(value) {
        return Some(date.with_timezone(&Utc));
    }
    if let Ok(date) = NaiveDateTime::parse_from_str(value, "%a %b %e %H:%M:%S %Y") {
        return Some(date.and_utc());
    }
    let mut parsed = chrono::format::Parsed::new();
    chrono::format::parse(
        &mut parsed,
        value,
        chrono::format::StrftimeItems::new("%A, %d-%b-%y %H:%M:%S GMT"),
    )
    .ok()?;
    // An obsolete HTTP date resolves to the most recent year no more than
    // fifty years ahead, then validates its weekday in that resolved year.
    let cutoff = now.year() + 50;
    let mut year = cutoff.div_euclid(100) * 100 + parsed.year_mod_100?;
    if year > cutoff {
        year -= 100;
    }
    let weekday = parsed.weekday.take();
    parsed.set_year(i64::from(year)).ok()?;
    let date = parsed.to_naive_datetime_with_offset(0).ok()?.and_utc();
    if (date.year(), date.month(), date.day(), date.time())
        > (cutoff, now.month(), now.day(), now.time())
    {
        parsed.year = Some(year - 100);
    }
    parsed.weekday = weekday;
    Some(parsed.to_naive_datetime_with_offset(0).ok()?.and_utc())
}

pub(super) fn http_error(error: ApiErrorResponse, retry_after: RetryAfter) -> OpenAIError {
    match retry_after {
        RetryAfter::Fallback => OpenAIError::ApiError(error),
        _ => OpenAIError::Boxed(Box::new(HttpError { error, retry_after })),
    }
}

pub(super) fn mapped_http_error(error: &OpenAIError) -> Option<AdkError> {
    let OpenAIError::Boxed(error) = error else {
        return None;
    };
    let error = error.downcast_ref::<HttpError>()?;
    let mut mapped = super::map_api_error(&error.error);
    match error.retry_after {
        RetryAfter::Delay(millis)
            if Instant::now()
                .checked_add(Duration::from_millis(millis))
                .is_some() =>
        {
            if mapped.retry.should_retry {
                mapped.retry.retry_after_ms = Some(millis);
            }
        }
        RetryAfter::Delay(_) | RetryAfter::Unsupported => {
            // Unsupported valid hints cannot authorize an earlier fallback retry.
            mapped.retry.should_retry = false;
            mapped.retry.retry_after_ms = None;
        }
        RetryAfter::Fallback => {}
    }
    Some(mapped)
}

#[derive(Deserialize)]
struct NestedError {
    sequence_number: u64,
    error: ErrorDetail,
}

#[derive(Deserialize)]
struct ErrorDetail {
    code: Option<String>,
    message: String,
    param: Option<String>,
}

pub(super) fn normalize_event(value: &mut Value) -> bool {
    if value.get("type").and_then(Value::as_str) != Some("error") || value.get("error").is_none() {
        return false;
    }
    // SIWC may nest the error fields. async-openai 0.41.3 requires the flat
    // ResponseErrorEvent schema even when the public OpenAI endpoint is used.
    let Ok(nested) = serde_json::from_value::<NestedError>(value.clone()) else {
        return false;
    };
    let event = ResponseErrorEvent {
        sequence_number: nested.sequence_number,
        code: nested.error.code,
        message: nested.error.message,
        param: nested.error.param,
    };
    let Ok(mut flattened) = serde_json::to_value(event) else {
        return false;
    };
    flattened["type"] = "error".into();
    *value = flattened;
    true
}

pub(super) fn sharing_error(code: Option<&str>) -> Option<AdkError> {
    let (category, diagnostic, message) = match code? {
        "subscription_sharing_usage_limit_exceeded" => (
            ErrorCategory::RateLimited,
            "model.openai_responses.subscription_sharing_usage_limit_exceeded",
            "ChatGPT shared usage limit exceeded",
        ),
        "subscription_sharing_usage_unavailable" => (
            ErrorCategory::Unavailable,
            "model.openai_responses.subscription_sharing_usage_unavailable",
            "ChatGPT shared usage is unavailable",
        ),
        _ => return None,
    };
    Some(
        AdkError::new(ErrorComponent::Model, category, diagnostic, message)
            .with_provider("openai-responses")
            .with_retry(RetryHint::default()),
    )
}

pub(super) fn stream_error(code: Option<&str>) -> AdkError {
    // Provider messages and unknown codes can echo request credentials. Only
    // known static diagnostics leave the adapter; no failed stream is replayed.
    sharing_error(code).unwrap_or_else(|| {
        AdkError::new(
            ErrorComponent::Model,
            ErrorCategory::Internal,
            "model.openai_responses.response_failed",
            "OpenAI Responses generation failed",
        )
        .with_provider("openai-responses")
    })
}

#[cfg(test)]
mod retry_after {
    use super::*;
    use async_openai::error::ApiError;

    fn now() -> DateTime<Utc> {
        "2026-10-12T00:00:00Z".parse().unwrap()
    }

    fn parse(value: Option<&str>) -> RetryAfter {
        parse_delay(value, now(), Instant::now())
    }

    #[test]
    fn parses_seconds_and_zero() {
        assert_eq!(parse(Some(" 12 ")), RetryAfter::Delay(12_000));
        assert_eq!(parse(Some("0")), RetryAfter::Delay(0));
    }

    #[test]
    fn parses_http_dates_in_utc() {
        for value in [
            "Mon, 12 Oct 2026 00:00:12 GMT",
            "Monday, 12-Oct-26 00:00:12 GMT",
            "Mon Oct 12 00:00:12 2026",
        ] {
            assert_eq!(parse(Some(value)), RetryAfter::Delay(12_000), "{value}");
        }
        assert_eq!(
            parse(Some("Mon, 12 Oct 2026 00:00:00 GMT")),
            RetryAfter::Delay(0)
        );
        assert_eq!(
            parse(Some("Sun, 11 Oct 2026 23:59:59 GMT")),
            RetryAfter::Delay(0)
        );
    }

    #[test]
    fn rounds_up_fractional_date_delays() {
        let now = "2026-10-12T00:00:00.999500Z".parse().unwrap();
        assert_eq!(
            parse_delay(Some("Mon, 12 Oct 2026 00:00:01 GMT"), now, Instant::now()),
            RetryAfter::Delay(1),
        );
    }

    #[test]
    fn resolves_obsolete_years_before_weekdays() {
        let now = "2080-01-01T00:00:00Z".parse().unwrap();
        let future: DateTime<Utc> = "2130-01-01T00:00:00Z".parse().unwrap();
        let past: DateTime<Utc> = "2030-01-02T00:00:00Z".parse().unwrap();
        assert_eq!(
            parse_date(&future.format("%A, %d-%b-%y %H:%M:%S GMT").to_string(), now)
                .unwrap()
                .year(),
            2130,
        );
        assert_eq!(
            parse_date(&past.format("%A, %d-%b-%y %H:%M:%S GMT").to_string(), now)
                .unwrap()
                .year(),
            2030,
        );
    }

    #[test]
    fn falls_back_for_missing_or_invalid_values() {
        for value in [
            None,
            Some(""),
            Some("invalid"),
            Some("-1"),
            Some("+1"),
            Some("1.5"),
        ] {
            assert_eq!(parse(value), RetryAfter::Fallback, "{value:?}");
        }
    }

    #[test]
    fn rejects_valid_numeric_overflow() {
        for value in ["18446744073709551616", "18446744073709551615"] {
            assert_eq!(parse(Some(value)), RetryAfter::Unsupported);
        }
    }

    #[tokio::test]
    async fn retains_headers_across_body_normalization() {
        for mode in [false, true] {
            for body in [
                r#"{"error":{"message":"private provider body","code":"server_error"}}"#,
                r#"{"detail":"private provider body"}"#,
                "private malformed body",
            ] {
                for (header, delay, retryable) in [
                    ("12", Some(12_000), true),
                    ("invalid", None, true),
                    ("18446744073709551616", None, false),
                ] {
                    let response = http::Response::builder()
                        .status(503)
                        .header(http::header::RETRY_AFTER, header)
                        .header("x-private-header", "private header content")
                        .body(reqwest_openai::Body::from(body.to_owned()))
                        .unwrap();
                    let error = super::super::normalize_error_response(response.into(), mode)
                        .await
                        .unwrap_err();
                    if header != "invalid" {
                        assert!(!format!("{error:?}").contains("private"));
                    }
                    let mapped = super::super::map_openai_error(error);
                    assert_eq!(mapped.category, ErrorCategory::Unavailable);
                    assert_eq!(mapped.details.upstream_status_code, Some(503));
                    assert_eq!(mapped.retry.should_retry, retryable);
                    assert_eq!(mapped.retry.retry_after_ms, delay);
                }
            }
        }
    }

    fn error(status: u16, retry_after: RetryAfter, code: Option<&str>) -> OpenAIError {
        http_error(
            ApiErrorResponse {
                status_code: reqwest_openai::StatusCode::from_u16(status).unwrap(),
                api_error: ApiError {
                    message: "private provider body".into(),
                    r#type: Some("private provider type".into()),
                    param: Some("private provider parameter".into()),
                    code: code.map(str::to_owned),
                },
            },
            retry_after,
        )
    }

    #[test]
    fn carries_hints_without_changing_classification() {
        for status in [400, 401, 403, 408, 429, 503] {
            let ordinary =
                super::super::map_openai_error(error(status, RetryAfter::Fallback, None));
            let hinted =
                super::super::map_openai_error(error(status, RetryAfter::Delay(12_000), None));
            assert_eq!(hinted.code, ordinary.code);
            assert_eq!(hinted.category, ordinary.category);
            assert_eq!(
                hinted.details.upstream_status_code,
                ordinary.details.upstream_status_code
            );
            assert_eq!(hinted.details.provider, ordinary.details.provider);
            assert_eq!(hinted.details.metadata, ordinary.details.metadata);
            assert_eq!(hinted.retry.should_retry, ordinary.retry.should_retry);
            assert_eq!(
                hinted.retry.retry_after_ms,
                ordinary.retry.should_retry.then_some(12_000)
            );
        }
    }

    #[test]
    fn retains_non_replaying_shared_usage_errors() {
        for code in [
            "subscription_sharing_usage_limit_exceeded",
            "subscription_sharing_usage_unavailable",
        ] {
            let mapped =
                super::super::map_openai_error(error(503, RetryAfter::Delay(12_000), Some(code)));
            assert!(!mapped.retry.should_retry);
            assert!(mapped.retry.retry_after_ms.is_none());
            assert_eq!(mapped.details.metadata["provider_error_code"], code);
        }
    }

    #[test]
    fn stops_retrying_unsupported_delays() {
        let mapped = super::super::map_openai_error(error(503, RetryAfter::Unsupported, None));
        assert_eq!(mapped.code, "model.openai_responses.unavailable");
        assert_eq!(mapped.details.upstream_status_code, Some(503));
        assert!(!mapped.retry.should_retry);
        assert!(mapped.retry.retry_after_ms.is_none());
    }

    #[test]
    fn wrapper_formats_only_status_and_delay() {
        let error = error(503, RetryAfter::Delay(12_000), Some("private_code"));
        assert_eq!(error.to_string(), "OpenAI Responses HTTP 503");
        let OpenAIError::Boxed(error) = error else {
            panic!("boxed HTTP error expected")
        };
        assert_eq!(
            format!("{error:?}"),
            "HttpError { status: 503, retry_after: Delay(12000) }"
        );
        assert!(error.source().is_none());
    }
}
