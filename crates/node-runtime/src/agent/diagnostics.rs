//! Opt-in turn timing with an explicit, content-free field allowlist.

use sailry_protocol::TurnId;
use std::time::{Duration, Instant};

pub(super) struct Diagnostics {
    turn: TurnId,
    started: Option<Instant>,
}

#[derive(Clone, Copy)]
pub(super) enum Phase {
    Turn,
    Runner,
    PlanReview,
    BrowserCleanup,
    McpSettle,
    McpCache,
    McpClose,
    ChildSettlement,
    CompletionHooks,
    OperationDrain,
    FinishRun,
}

impl Phase {
    fn label(self) -> &'static str {
        match self {
            Self::Turn => "turn",
            Self::Runner => "runner",
            Self::PlanReview => "plan_review",
            Self::BrowserCleanup => "browser_cleanup",
            Self::McpSettle => "mcp_settle",
            Self::McpCache => "mcp_cache",
            Self::McpClose => "mcp_close",
            Self::ChildSettlement => "child_settlement",
            Self::CompletionHooks => "completion_hooks",
            Self::OperationDrain => "operation_drain",
            Self::FinishRun => "finish_run",
        }
    }
}

pub(super) struct Span {
    turn: TurnId,
    phase: Phase,
    origin: Option<Instant>,
    started: Option<Instant>,
    operations: Option<usize>,
    last_content: Option<Duration>,
}

#[derive(Clone, Copy)]
enum State {
    Begin,
    End,
}

fn enabled(value: Option<&str>) -> bool {
    value == Some("1")
}

impl Diagnostics {
    pub fn new(turn: TurnId) -> Self {
        Self::configured(
            turn,
            enabled(std::env::var("SAILRY_TURN_DIAGNOSTICS").ok().as_deref()),
        )
    }

    fn configured(turn: TurnId, enabled: bool) -> Self {
        Self {
            turn,
            started: enabled.then(Instant::now),
        }
    }

    pub fn enabled(&self) -> bool {
        self.started.is_some()
    }

    pub fn span(&self, phase: Phase, operations: Option<usize>) -> Span {
        let span = Span {
            turn: self.turn,
            phase,
            origin: self.started,
            started: self.started.map(|_| Instant::now()),
            operations,
            last_content: None,
        };
        if let Some(started) = span.started {
            eprintln!("{}", span.line(State::Begin, started));
        }
        span
    }
}

impl Span {
    pub fn content(&mut self) {
        if let Some(origin) = self.origin {
            self.last_content = Some(origin.elapsed());
        }
    }

    fn line(&self, state: State, now: Instant) -> String {
        let elapsed = self
            .origin
            .map_or(0, |origin| now.duration_since(origin).as_millis());
        let duration = self
            .started
            .map_or(0, |started| now.duration_since(started).as_millis());
        let state_label = match state {
            State::Begin => "begin",
            State::End => "end",
        };
        let mut line = format!(
            "sailry_turn turn={} phase={} state={state_label} elapsed_ms={elapsed} duration_ms={duration}",
            self.turn,
            self.phase.label(),
        );
        if let Some(operations) = self.operations {
            line.push_str(&format!(" operations={operations}"));
        }
        if matches!(state, State::End)
            && let Some(last_content) = self.last_content
        {
            line.push_str(&format!(" last_content_ms={}", last_content.as_millis()));
        }
        line
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if self.started.is_some() {
            eprintln!("{}", self.line(State::End, Instant::now()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn requires_explicit_opt_in() {
        for value in [None, Some(""), Some("0"), Some("true"), Some(" 1 ")] {
            assert!(!enabled(value));
        }
        assert!(enabled(Some("1")));
        let diagnostics = Diagnostics::configured(TurnId::new(), false);
        assert!(!diagnostics.enabled());
        let mut span = diagnostics.span(Phase::Runner, None);
        span.content();
        assert!(span.started.is_none());
        assert!(span.origin.is_none());
        assert!(span.last_content.is_none());
    }

    #[test]
    fn formats_only_allowed_fields() {
        let origin = Instant::now();
        let turn = TurnId::new();
        let span = Span {
            turn,
            phase: Phase::Runner,
            origin: Some(origin),
            started: Some(origin + Duration::from_millis(10)),
            operations: None,
            last_content: Some(Duration::from_millis(25)),
        };
        let now = origin + Duration::from_millis(40);
        assert_eq!(
            span.line(State::Begin, now),
            format!(
                "sailry_turn turn={turn} phase=runner state=begin elapsed_ms=40 duration_ms=30"
            ),
        );
        assert_eq!(
            span.line(State::End, now),
            format!(
                "sailry_turn turn={turn} phase=runner state=end elapsed_ms=40 duration_ms=30 last_content_ms=25"
            ),
        );
    }

    #[test]
    fn includes_operation_count_without_details() {
        let span = Span {
            turn: TurnId::new(),
            phase: Phase::OperationDrain,
            origin: None,
            started: None,
            operations: Some(3),
            last_content: None,
        };
        assert!(
            span.line(State::Begin, Instant::now())
                .ends_with(" operations=3")
        );
    }
}
