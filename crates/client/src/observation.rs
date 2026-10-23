//! Metadata-only subscription diagnostics. No event payload enters this module.
use sailry_protocol::{ErrorCode, Fault, NodeId, RequestId, SessionId};
use std::time::{Duration, Instant};

pub(crate) struct Observation {
    node: String,
    call: String,
    session: Option<String>,
    operation: &'static str,
    generation: u64,
    stage: &'static str,
    started: Instant,
    finished: bool,
    dispatch: tracing::Dispatch,
}

impl Observation {
    pub(crate) fn new(node: NodeId, session: Option<SessionId>) -> Self {
        let observation = Self {
            node: node.to_string(),
            call: RequestId::new().to_string(),
            session: session.map(|session| session.to_string()),
            operation: if session.is_some() {
                "conversation_subscription"
            } else {
                "node_subscription"
            },
            generation: 0,
            stage: "subscribe",
            started: Instant::now(),
            finished: false,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        };
        observation.emit("begin", "subscribe", "none", None, None, None);
        observation
    }

    pub(crate) fn call(&self) -> &str {
        &self.call
    }

    pub(crate) fn connecting(&mut self, generation: u64) {
        self.generation = generation;
        self.stage = "subscribe";
        if generation > 1 {
            self.emit("reconnecting", "resubscribe", "reconnect", None, None, None);
        }
    }

    pub(crate) fn connected(&mut self) {
        self.stage = "stream";
    }

    pub(crate) fn ready(&self, sequence: u64) {
        self.emit("ready", "snapshot", "none", None, None, Some(sequence));
    }

    pub(crate) fn recovering(&mut self, stage: &'static str, reason: &'static str) {
        self.stage = stage;
        self.emit(
            "recovering",
            reason,
            if self.session.is_some() {
                "resubscribe"
            } else {
                "snapshot"
            },
            None,
            None,
            None,
        );
    }

    pub(crate) fn issue(&mut self, stage: &'static str, error: &Fault) {
        let previous = std::mem::replace(&mut self.stage, stage);
        self.emit(
            "interrupted",
            stage,
            "retain_snapshot",
            Some(error.code),
            None,
            None,
        );
        self.stage = previous;
    }

    pub(crate) fn disconnected(&self, error: &Fault, delay: Duration) {
        let retry = matches!(error.code, ErrorCode::Unavailable | ErrorCode::Busy);
        self.emit(
            "interrupted",
            self.stage,
            if retry { "reconnect" } else { "stop" },
            Some(error.code),
            retry.then_some(delay.as_millis().min(u64::MAX as u128) as u64),
            None,
        );
    }

    pub(crate) fn finish(&mut self, error: Option<&Fault>) {
        self.finished = true;
        self.emit(
            if error.is_some() {
                "failed"
            } else {
                "completed"
            },
            self.stage,
            "stop",
            error.map(|error| error.code),
            None,
            None,
        );
    }

    fn emit(
        &self,
        phase: &'static str,
        reason: &'static str,
        recovery_decision: &'static str,
        error: Option<ErrorCode>,
        wait_ms: Option<u64>,
        sequence: Option<u64>,
    ) {
        let duration_us = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let code = error.map(code);
        tracing::dispatcher::with_default(&self.dispatch, || {
            if matches!(phase, "interrupted" | "recovering" | "failed") {
                tracing::warn!(
                    target: "sailry",
                    area = "client", operation = self.operation, phase,
                    node = self.node.as_str(), call = self.call.as_str(),
                    session = self.session.as_deref(), generation = self.generation,
                    stage = self.stage, reason, recovery_decision, code,
                    duration_us, wait_ms, sequence,
                );
            } else {
                tracing::info!(
                    target: "sailry",
                    area = "client", operation = self.operation, phase,
                    node = self.node.as_str(), call = self.call.as_str(),
                    session = self.session.as_deref(), generation = self.generation,
                    stage = self.stage, reason, recovery_decision, code,
                    duration_us, wait_ms, sequence,
                );
            }
        });
    }
}

impl Drop for Observation {
    fn drop(&mut self) {
        if !self.finished {
            // A controller dropping an observer does not cancel Node work.
            self.emit("interrupted", "observer_dropped", "stop", None, None, None);
        }
    }
}

pub(crate) struct Recovery {
    node: String,
    request: String,
    call: Option<String>,
    generation: u64,
    started: Instant,
    finished: bool,
    dispatch: tracing::Dispatch,
}

impl Recovery {
    pub(crate) fn new(
        node: NodeId,
        request: RequestId,
        generation: u64,
        call: Option<&str>,
    ) -> Self {
        let recovery = Self {
            node: node.to_string(),
            request: request.to_string(),
            call: call.map(str::to_owned),
            generation,
            started: Instant::now(),
            finished: false,
            dispatch: tracing::dispatcher::get_default(Clone::clone),
        };
        recovery.emit("begin", None);
        recovery
    }

    pub(crate) fn finish(&mut self, error: Option<&Fault>) {
        self.finished = true;
        self.emit(
            if error.is_some() {
                "failed"
            } else {
                "completed"
            },
            error.map(|error| error.code),
        );
    }

    fn emit(&self, phase: &'static str, error: Option<ErrorCode>) {
        let duration_us = self.started.elapsed().as_micros().min(u64::MAX as u128) as u64;
        let code = error.map(code);
        let recovery_decision = match phase {
            "begin" => "snapshot",
            "completed" => "none",
            "failed" => "caller_decides",
            _ => "stop",
        };
        tracing::dispatcher::with_default(&self.dispatch, || {
            if matches!(phase, "failed" | "interrupted") {
                tracing::warn!(
                    target: "sailry",
                    area = "client", operation = "snapshot_recovery", phase,
                    node = self.node.as_str(), request = self.request.as_str(),
                    call = self.call.as_deref(), generation = self.generation,
                    stage = "snapshot", recovery_decision, code, duration_us,
                );
            } else {
                tracing::info!(
                    target: "sailry",
                    area = "client", operation = "snapshot_recovery", phase,
                    node = self.node.as_str(), request = self.request.as_str(),
                    call = self.call.as_deref(), generation = self.generation,
                    stage = "snapshot", recovery_decision, code, duration_us,
                );
            }
        });
    }
}

impl Drop for Recovery {
    fn drop(&mut self) {
        if !self.finished {
            self.emit("interrupted", None);
        }
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
