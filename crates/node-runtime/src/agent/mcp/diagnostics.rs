//! MCP transport completion and tool execution outcomes are separate facts.
use crate::logging::{Context, Entry, Phase, Span};
use sailry_link::CancellationToken;
use sailry_protocol::ErrorCode;
use serde_json::Value;
use std::future::Future;

pub(super) fn begin(context: Context, stage: &'static str, stop: &CancellationToken) -> Span {
    let mut entry = Entry::new(crate::logging::Area::Plugin, Phase::Begin);
    entry.context = context;
    entry.operation = Some("mcp_call");
    entry.stage = Some(stage);
    entry.communication_outcome = Some("not_started");
    entry.execution_outcome = Some("not_started");
    Span::new(entry).cancel_when(stop.clone())
}

pub(super) async fn stage<T, E>(
    parent: &mut Span,
    stop: &CancellationToken,
    stage: &'static str,
    code: ErrorCode,
    future: impl Future<Output = Result<T, E>>,
) -> Result<T, E> {
    let mut span = begin(parent.entry.context.clone(), stage, stop);
    let result = future.await;
    if result.is_ok() {
        span.finish(Phase::Completed, None);
    } else if stop.is_cancelled() {
        parent.entry.code = Some(ErrorCode::Cancelled);
        span.finish(Phase::Cancelled, Some(ErrorCode::Cancelled));
    } else {
        parent.entry.code = Some(code);
        span.finish(Phase::Failed, Some(code));
    }
    result
}

pub(super) fn external(span: &mut Span) {
    span.entry.communication_outcome = Some("unknown");
    span.entry.execution_outcome = Some("unknown");
}

pub(super) fn finish(span: &mut Span, result: &adk_core::Result<Value>, cancelled: bool) {
    let (phase, code) = match result {
        Ok(value) if uncertain(value) => {
            span.entry.communication_outcome = Some("unknown");
            span.entry.execution_outcome = Some("unknown");
            span.entry.reason = Some("result_unavailable");
            (Phase::Interrupted, Some(ErrorCode::OutcomeUnknown))
        }
        Ok(value) if server_error(value) => {
            span.entry.communication_outcome = Some("completed");
            span.entry.execution_outcome = Some("failed");
            span.entry.reason = Some("server_error");
            (Phase::Failed, None)
        }
        Ok(_) => {
            span.entry.communication_outcome = Some("completed");
            span.entry.execution_outcome = Some("succeeded");
            (Phase::Completed, None)
        }
        Err(_) if cancelled => {
            if span.entry.communication_outcome != Some("not_started") {
                span.entry.communication_outcome = Some("cancelled");
            }
            (Phase::Cancelled, Some(ErrorCode::Cancelled))
        }
        Err(_) => (
            Phase::Failed,
            Some(span.entry.code.unwrap_or(ErrorCode::Unavailable)),
        ),
    };
    span.finish(phase, code);
}

fn server_error(value: &Value) -> bool {
    // Pinned ADK's server_error_response wraps isError=true in exactly this
    // shape. Successful structured content is nested under `output`, even when
    // that business object has its own `error` field.
    value
        .as_object()
        .is_some_and(|value| value.len() == 1 && value.get("error").is_some_and(Value::is_string))
}

fn uncertain(value: &Value) -> bool {
    // This wrapper is created by Bound after an unconfirmed tools/call result.
    value.as_object().is_some_and(|value| value.len() == 1)
        && value.pointer("/error/code").and_then(Value::as_str) == Some("outcome_unknown")
}

#[cfg(test)]
mod tests;
