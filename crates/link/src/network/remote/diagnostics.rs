//! Controller timings include service work. QUIC RTT is reported separately;
//! neither value subtracts clocks or claims to isolate network-only latency.
use crate::{ConnectionInfo, Response, Route};
use sailry_protocol::{ErrorCode, Fault, NodeId, RequestId};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU8, Ordering},
    },
    time::Instant,
};

#[derive(Clone)]
pub(super) struct CommandLog {
    state: Arc<State>,
}

const NOT_SENT: u8 = 0;
const MAYBE_SENT: u8 = 1;
const ADMITTED: u8 = 2;
const DURABLE: u8 = 3;
const FINISHED: u8 = 4;

struct State {
    progress: AtomicU8,
    node: String,
    request: String,
    started: Instant,
    dispatch: tracing::Dispatch,
}

impl CommandLog {
    pub(super) fn new(node: NodeId, request: RequestId) -> Self {
        Self {
            state: Arc::new(State {
                progress: AtomicU8::new(NOT_SENT),
                node: node.to_string(),
                request: request.to_string(),
                started: Instant::now(),
                dispatch: tracing::dispatcher::get_default(Clone::clone),
            }),
        }
    }

    pub(super) fn sending(&self) {
        self.state.progress.store(MAYBE_SENT, Ordering::Relaxed);
    }

    pub(super) fn admitted(&self, durable: bool) {
        self.state
            .progress
            .store(if durable { DURABLE } else { ADMITTED }, Ordering::Relaxed);
    }

    pub(super) fn begin(&self) {
        tracing::dispatcher::with_default(&self.state.dispatch, || {
            tracing::info!(
                target: "sailry",
                area = "link",
                operation = "remote_command",
                phase = "begin",
                node = self.state.node.as_str(),
                request = self.state.request.as_str(),
                stage = "dispatch",
            );
        });
    }

    pub(super) fn completed(&self, result: &Response, durable: bool, connection: ConnectionInfo) {
        if self.state.progress.swap(FINISHED, Ordering::Relaxed) == FINISHED {
            return;
        }
        let end_to_end_us = self.elapsed_us();
        let transport = match connection.route {
            Some(Route::Local) => "local",
            Some(Route::P2p) => "p2p",
            Some(Route::Relay) => "relay",
            None => "unknown",
        };
        let (phase, execution_outcome, code) = match result {
            Ok(_) => ("completed", "succeeded", None),
            Err(error) if error.code == ErrorCode::OutcomeUnknown => {
                ("completed", "unknown", Some(code(error.code)))
            }
            Err(error) if error.code == ErrorCode::Cancelled => {
                ("completed", "cancelled", Some(code(error.code)))
            }
            Err(error) => ("completed", "failed", Some(code(error.code))),
        };
        tracing::dispatcher::with_default(&self.state.dispatch, || {
            // A complete fault response is successful communication. The Node's
            // business outcome remains distinct, including uncertain side effects.
            tracing::info!(
                target: "sailry",
                area = "link",
                operation = "remote_command",
                phase,
                node = self.state.node.as_str(),
                request = self.state.request.as_str(),
                stage = "completion",
                communication_outcome = "succeeded",
                execution_outcome,
                recovery_decision = if execution_outcome == "unknown" { "same_request" } else { "none" },
                end_to_end_us,
                rtt_ms = connection.rtt_ms,
                transport,
                code,
                durable,
            );
        });
    }

    pub(super) fn rejected(&self, error: &Fault) {
        if self.state.progress.swap(FINISHED, Ordering::Relaxed) == FINISHED {
            return;
        }
        let end_to_end_us = self.elapsed_us();
        let code = code(error.code);
        tracing::dispatcher::with_default(&self.state.dispatch, || {
            tracing::warn!(
                target: "sailry",
                area = "link",
                operation = "remote_command",
                phase = "rejected",
                node = self.state.node.as_str(),
                request = self.state.request.as_str(),
                stage = "receipt",
                communication_outcome = "succeeded",
                execution_outcome = if error.code == ErrorCode::OutcomeUnknown { "unknown" } else { "rejected" },
                recovery_decision = if error.code == ErrorCode::OutcomeUnknown { "same_request" } else { "stop" },
                end_to_end_us,
                code,
            );
        });
    }

    pub(super) fn failed(&self, stage: &'static str, reason: &'static str, error: &Fault) {
        self.emit("failed", stage, reason, error, None);
    }

    pub(super) fn interrupted(
        &self,
        stage: &'static str,
        reason: &'static str,
        error: &Fault,
        durable: Option<bool>,
    ) {
        self.emit("interrupted", stage, reason, error, durable);
    }

    fn emit(
        &self,
        phase: &'static str,
        stage: &'static str,
        reason: &'static str,
        error: &Fault,
        durable: Option<bool>,
    ) {
        if self.state.progress.swap(FINISHED, Ordering::Relaxed) == FINISHED {
            return;
        }
        let duration_us = self.elapsed_us();
        let code = code(error.code);
        let execution_outcome = if error.code == ErrorCode::OutcomeUnknown {
            "unknown"
        } else {
            "not_started"
        };
        let recovery_decision = if execution_outcome == "unknown" {
            "same_request"
        } else {
            "stop"
        };
        tracing::dispatcher::with_default(&self.state.dispatch, || {
            if phase == "failed" {
                tracing::error!(
                    target: "sailry",
                    area = "link",
                    operation = "remote_command",
                    phase,
                    node = self.state.node.as_str(),
                    request = self.state.request.as_str(),
                    stage,
                    reason,
                    duration_us,
                    end_to_end_us = duration_us,
                    communication_outcome = "failed",
                    execution_outcome,
                    recovery_decision,
                    code,
                    durable,
                );
            } else {
                tracing::warn!(
                    target: "sailry",
                    area = "link",
                    operation = "remote_command",
                    phase,
                    node = self.state.node.as_str(),
                    request = self.state.request.as_str(),
                    stage,
                    reason,
                    duration_us,
                    end_to_end_us = duration_us,
                    communication_outcome = "interrupted",
                    execution_outcome,
                    recovery_decision,
                    code,
                    durable,
                );
            }
        });
    }

    fn elapsed_us(&self) -> u64 {
        u64::try_from(self.state.started.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
}

impl Drop for State {
    fn drop(&mut self) {
        let progress = *self.progress.get_mut();
        if progress == FINISHED {
            return;
        }
        let end_to_end_us = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let uncertain = progress >= MAYBE_SENT;
        let durable = match progress {
            ADMITTED => Some(false),
            DURABLE => Some(true),
            _ => None,
        };
        tracing::dispatcher::with_default(&self.dispatch, || {
            // Dropping the controller future or completion observer never proves
            // cancellation of a command that may already have side effects.
            tracing::warn!(
                target: "sailry",
                area = "link", operation = "remote_command", phase = "interrupted",
                node = self.node.as_str(), request = self.request.as_str(),
                stage = if progress >= ADMITTED { "completion_observer" } else { "dispatch" },
                reason = "observation_dropped", communication_outcome = "interrupted",
                execution_outcome = if uncertain { "unknown" } else { "not_started" },
                recovery_decision = if uncertain { "same_request" } else { "stop" },
                code = if uncertain { "outcome_unknown" } else { "cancelled" },
                end_to_end_us, durable,
            );
        });
    }
}

fn code(value: ErrorCode) -> &'static str {
    match value {
        ErrorCode::InvalidRequest => "invalid_request",
        ErrorCode::WrongTarget => "wrong_target",
        ErrorCode::NotFound => "not_found",
        ErrorCode::NotConfigured => "not_configured",
        ErrorCode::Conflict => "conflict",
        ErrorCode::RevisionConflict => "revision_conflict",
        ErrorCode::PermissionDenied => "permission_denied",
        ErrorCode::Expired => "expired",
        ErrorCode::Busy => "busy",
        ErrorCode::Unavailable => "unavailable",
        ErrorCode::Cancelled => "cancelled",
        ErrorCode::OutcomeUnknown => "outcome_unknown",
        ErrorCode::Internal => "internal",
    }
}
