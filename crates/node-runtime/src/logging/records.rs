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
    Turn,
    Logging,
}
impl Area {
    fn name(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::Command => "command",
            Self::Run => "run",
            Self::Model => "model",
            Self::Plugin => "plugin",
            Self::Image => "image",
            Self::Artifact => "artifact",
            Self::Desktop => "desktop",
            Self::Turn => "turn",
            Self::Logging => "logging",
        }
    }
}
#[derive(Clone, Copy)]
pub enum Phase {
    Begin,
    Admitted,
    Completed,
    Continued,
    Failed,
    Cancelled,
    Interrupted,
    FirstToken,
}
impl Phase {
    fn name(self) -> &'static str {
        match self {
            Self::Begin => "begin",
            Self::Admitted => "admitted",
            Self::Completed => "completed",
            Self::Continued => "continued",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
            Self::FirstToken => "first_token",
        }
    }
}

#[derive(Clone, Default)]
pub struct Context {
    pub node: Option<NodeId>,
    pub caller: Option<NodeId>,
    pub request: Option<RequestId>,
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
    pub source_kind: Option<&'static str>,
    pub reason: Option<&'static str>,
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
            source_kind: None,
            reason: None,
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
            node = id(context.node.and_then(|value| serde_json::to_string(&value).ok())), caller = id(context.caller.and_then(|value| serde_json::to_string(&value).ok())),
            request = id(context.request.map(|value| value.to_string())), session = id(context.session.map(|value| value.to_string())),
            turn = id(context.turn.map(|value| value.to_string())), worktree = id(context.worktree.map(|value| value.to_string())),
            provider = id(context.provider.map(|value| value.to_string())), provider_name = context.provider_name.as_deref().unwrap_or_default(), model = context.model.as_deref().unwrap_or_default(),
            package = package.map_or("", |value| value.name.as_str()), digest = package.map_or("", |value| value.digest.as_str()), settings_revision = package.map_or(0, |value| value.settings_revision),
            handler = context.handler.as_deref().unwrap_or_default(), invocation = id(context.invocation.map(|value| value.to_string())),
            call = id(context.call.map(|value| value.to_string())),
            artifact_id = id(context.artifact.map(|value| value.to_string())), revision = self.revision,
            command = self.command.unwrap_or_default(), source_kind = self.source_kind.unwrap_or_default(), reason = self.reason.unwrap_or_default(),
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
        self.entry.emit();
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
