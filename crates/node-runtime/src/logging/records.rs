use sailry_link::CancellationToken;
use sailry_protocol::{
    ArtifactId, ErrorCode, NodeId, ProviderId, RequestId, SessionId, TurnId, WorktreeId,
    conversation::Status,
};
use std::time::Instant;

#[derive(Clone, Copy)]
pub enum Area {
    Node,
    Command,
    Run,
    Model,
    Plugin,
    Image,
    Artifact,
    Desktop,
    Link,
    Client,
    Turn,
    Logging,
}
impl Area {
    #[cfg(any(target_os = "macos", test))]
    pub(super) const ALL: [Self; 12] = [
        Self::Node,
        Self::Command,
        Self::Run,
        Self::Model,
        Self::Plugin,
        Self::Image,
        Self::Artifact,
        Self::Desktop,
        Self::Link,
        Self::Client,
        Self::Turn,
        Self::Logging,
    ];

    pub(super) fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Command => "command",
            Self::Run => "run",
            Self::Model => "model",
            Self::Plugin => "plugin",
            Self::Image => "image",
            Self::Artifact => "artifact",
            Self::Desktop => "desktop",
            Self::Link => "link",
            Self::Client => "client",
            Self::Turn => "turn",
            Self::Logging => "logging",
        }
    }
}
#[derive(Clone, Copy)]
pub enum Phase {
    Begin,
    Admitted,
    Retrying,
    Selected,
    Completed,
    Continued,
    Failed,
    Cancelled,
    Interrupted,
}
impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Admitted => "admitted",
            Self::Retrying => "retrying",
            Self::Selected => "selected",
            Self::Completed => "completed",
            Self::Continued => "continued",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Clone, Default)]
pub struct Context {
    pub node: Option<NodeId>,
    pub caller: Option<NodeId>,
    pub request: Option<RequestId>,
    pub incident: Option<RequestId>,
    pub session: Option<SessionId>,
    pub turn: Option<TurnId>,
    pub worktree: Option<WorktreeId>,
    pub provider: Option<ProviderId>,
    pub provider_name: Option<String>,
    pub model: Option<String>,
    pub package: Option<sailry_protocol::plugin::Reference>,
    pub handler: Option<String>,
    pub invocation: Option<RequestId>,
    pub call: Option<RequestId>,
    pub artifact: Option<ArtifactId>,
}

/// This allowlist deliberately has no message, input, output, URL or path field.
#[derive(Clone)]
pub struct Entry {
    pub area: Area,
    pub phase: Phase,
    pub context: Context,
    pub command: Option<&'static str>,
    pub operation: Option<&'static str>,
    pub stage: Option<&'static str>,
    pub source_kind: Option<&'static str>,
    pub reason: Option<&'static str>,
    pub cause_code: Option<&'static str>,
    pub cause_component: Option<&'static str>,
    pub cause_category: Option<&'static str>,
    pub upstream_status: Option<u64>,
    pub attempt: Option<u64>,
    pub limit: Option<u64>,
    pub request_mode: Option<&'static str>,
    pub transport: Option<&'static str>,
    pub connection_reused: Option<bool>,
    pub communication_outcome: Option<&'static str>,
    pub execution_outcome: Option<&'static str>,
    pub recovery_decision: Option<&'static str>,
    pub wait_ms: Option<u64>,
    pub end_to_end_us: Option<u64>,
    pub rtt_ms: Option<u64>,
    pub generation: Option<u64>,
    pub sequence: Option<u64>,
    pub fallback_reason: Option<&'static str>,
    pub continuation: Option<&'static str>,
    pub stream: Option<bool>,
    pub budget: Option<i64>,
    pub parse_layer: Option<&'static str>,
    pub parse_category: Option<&'static str>,
    pub parse_cause: Option<&'static str>,
    pub parse_field: Option<&'static str>,
    pub parse_event: Option<String>,
    pub parse_line: Option<u64>,
    pub parse_column: Option<u64>,
    pub code: Option<ErrorCode>,
    pub status: Option<Status>,
    pub duration_us: Option<u64>,
    pub first_token_us: Option<u64>,
    pub bytes: Option<u64>,
    pub count: Option<u64>,
    pub durable: Option<bool>,
    pub elapsed_us: Option<u64>,
    pub last_content_us: Option<u64>,
    pub revision: Option<u64>,
}
impl Entry {
    pub fn new(area: Area, phase: Phase) -> Self {
        Self {
            area,
            phase,
            context: Context::default(),
            command: None,
            operation: None,
            stage: None,
            source_kind: None,
            reason: None,
            cause_code: None,
            cause_component: None,
            cause_category: None,
            upstream_status: None,
            attempt: None,
            limit: None,
            request_mode: None,
            transport: None,
            connection_reused: None,
            communication_outcome: None,
            execution_outcome: None,
            recovery_decision: None,
            wait_ms: None,
            end_to_end_us: None,
            rtt_ms: None,
            generation: None,
            sequence: None,
            fallback_reason: None,
            continuation: None,
            stream: None,
            budget: None,
            parse_layer: None,
            parse_category: None,
            parse_cause: None,
            parse_field: None,
            parse_event: None,
            parse_line: None,
            parse_column: None,
            code: None,
            status: None,
            duration_us: None,
            first_token_us: None,
            bytes: None,
            count: None,
            durable: None,
            elapsed_us: None,
            last_content_us: None,
            revision: None,
        }
    }
    pub fn emit(&self) {
        self.write(None);
    }
    pub fn emit_debug(&self) {
        self.write(Some(tracing::Level::DEBUG));
    }
    pub(crate) fn emit_warn(&self) {
        self.write(Some(tracing::Level::WARN));
    }
    fn write(&self, level: Option<tracing::Level>) {
        let id = |value: Option<String>| value.unwrap_or_default();
        let context = &self.context;
        let package = context.package.as_ref();
        let code = self
            .code
            .and_then(|value| serde_json::to_value(value).ok())
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        let status = self
            .status
            .and_then(|value| serde_json::to_value(value).ok())
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default();
        macro_rules! emit { ($level:expr) => { tracing::event!(target: "sailry", $level,
            area = self.area.name(), phase = self.phase.name(),
            node = id(context.node.map(|value| value.to_string())), caller = id(context.caller.map(|value| value.to_string())),
            incident = id(context.incident.map(|value| value.to_string())),
            request = id(context.request.map(|value| value.to_string())), session = id(context.session.map(|value| value.to_string())),
            turn = id(context.turn.map(|value| value.to_string())), worktree = id(context.worktree.map(|value| value.to_string())),
            provider = id(context.provider.map(|value| value.to_string())), provider_name = context.provider_name.as_deref().unwrap_or_default(), model = context.model.as_deref().unwrap_or_default(),
            package = package.map_or("", |value| value.name.as_str()), digest = package.map_or("", |value| value.digest.as_str()), settings_revision = package.map(|value| value.settings_revision),
            handler = context.handler.as_deref().unwrap_or_default(), invocation = id(context.invocation.map(|value| value.to_string())),
            call = id(context.call.map(|value| value.to_string())),
            artifact_id = id(context.artifact.map(|value| value.to_string())), revision = self.revision,
            command = self.command.unwrap_or_default(), operation = self.operation.unwrap_or_default(), stage = self.stage.unwrap_or_default(), source_kind = self.source_kind.unwrap_or_default(), reason = self.reason.unwrap_or_default(),
            cause_code = self.cause_code.unwrap_or_default(), cause_component = self.cause_component.unwrap_or_default(), cause_category = self.cause_category.unwrap_or_default(), upstream_status = self.upstream_status,
            attempt = self.attempt, limit = self.limit, request_mode = self.request_mode.unwrap_or_default(), stream = self.stream, budget = self.budget,
            transport = self.transport.unwrap_or_default(), connection_reused = self.connection_reused, fallback_reason = self.fallback_reason.unwrap_or_default(), continuation = self.continuation.unwrap_or_default(),
            communication_outcome = self.communication_outcome.unwrap_or_default(), execution_outcome = self.execution_outcome.unwrap_or_default(), recovery_decision = self.recovery_decision.unwrap_or_default(),
            wait_ms = self.wait_ms, end_to_end_us = self.end_to_end_us, rtt_ms = self.rtt_ms, generation = self.generation, sequence = self.sequence,
            parse_layer = self.parse_layer.unwrap_or_default(), parse_category = self.parse_category.unwrap_or_default(), parse_cause = self.parse_cause.unwrap_or_default(), parse_field = self.parse_field.unwrap_or_default(), parse_event = self.parse_event.as_deref().unwrap_or_default(), parse_line = self.parse_line, parse_column = self.parse_column,
            code = code.as_str(), status = status.as_str(), duration_us = self.duration_us, first_token_us = self.first_token_us, bytes = self.bytes, count = self.count, durable = self.durable,
            elapsed_us = self.elapsed_us, last_content_us = self.last_content_us,
        ); } }
        if level == Some(tracing::Level::DEBUG) {
            emit!(tracing::Level::DEBUG);
        } else if level == Some(tracing::Level::WARN) {
            emit!(tracing::Level::WARN);
        } else if matches!(self.phase, Phase::Failed) {
            emit!(tracing::Level::ERROR);
        } else if matches!(self.phase, Phase::Interrupted)
            || self.code == Some(ErrorCode::OutcomeUnknown)
        {
            emit!(tracing::Level::WARN);
        } else {
            emit!(tracing::Level::INFO);
        }
    }
}

/// Observation only: dropping a span never cancels or replays the work it describes.
pub struct Span {
    pub entry: Entry,
    started: Instant,
    finished: bool,
    stop: Option<CancellationToken>,
    routine: bool,
}
impl Span {
    pub fn new(entry: Entry) -> Self {
        Self::begin(entry, Instant::now())
    }
    pub fn begin(entry: Entry, started: Instant) -> Self {
        entry.emit();
        Self {
            entry,
            started,
            finished: false,
            stop: None,
            routine: false,
        }
    }
    pub(crate) fn query(entry: Entry) -> Self {
        Self {
            entry,
            started: Instant::now(),
            finished: false,
            stop: None,
            routine: true,
        }
    }
    pub fn cancel_when(mut self, stop: CancellationToken) -> Self {
        self.stop = Some(stop);
        self
    }
    pub fn elapsed_us(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_micros()).unwrap_or(u64::MAX)
    }
    pub fn record(&self, phase: Phase) {
        if self.routine {
            return;
        }
        let mut entry = self.entry.clone();
        entry.phase = phase;
        entry.duration_us = Some(self.elapsed_us());
        entry.emit();
    }
    pub fn finish(&mut self, phase: Phase, code: Option<ErrorCode>) {
        if self.finished {
            return;
        }
        self.entry.phase = phase;
        self.entry.code = code;
        self.entry.duration_us = Some(self.elapsed_us());
        // Ordinary successful reads carry no new diagnostic fact. Keep slow
        // completions, every failure and uncertain/cancelled outcomes visible.
        if !self.routine || !matches!(phase, Phase::Completed) || self.elapsed_us() >= 100_000 {
            self.entry.emit();
        }
        self.finished = true;
    }
}
impl Drop for Span {
    fn drop(&mut self) {
        if !self.finished {
            let cancelled = self
                .stop
                .as_ref()
                .is_some_and(CancellationToken::is_cancelled);
            self.finish(
                if cancelled {
                    Phase::Cancelled
                } else {
                    Phase::Interrupted
                },
                Some(if cancelled {
                    ErrorCode::Cancelled
                } else {
                    ErrorCode::OutcomeUnknown
                }),
            );
        }
    }
}
