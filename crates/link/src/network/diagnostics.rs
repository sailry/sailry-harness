use std::time::Instant;

use iroh::endpoint::{ConnectionError, ReadError, ReadExactError, WriteError};
use sailry_protocol::{ErrorCode, Fault};

#[derive(Debug, PartialEq, Eq)]
struct Cause {
    category: &'static str,
    code: Option<u64>,
    quiet: bool,
}

impl Cause {
    fn new(category: &'static str, code: Option<u64>, quiet: bool) -> Self {
        Self {
            category,
            code,
            quiet,
        }
    }

    fn connection(error: &ConnectionError) -> Self {
        match error {
            ConnectionError::VersionMismatch => Self::new("version_mismatch", None, false),
            ConnectionError::TransportError(error) => {
                Self::new("transport_error", Some(error.code.into()), false)
            }
            ConnectionError::ConnectionClosed(error) => {
                Self::new("connection_closed", Some(error.error_code.into()), false)
            }
            ConnectionError::ApplicationClosed(error) => Self::new(
                "application_closed",
                Some(error.error_code.into_inner()),
                false,
            ),
            ConnectionError::Reset => Self::new("connection_reset", None, false),
            ConnectionError::TimedOut => Self::new("connection_timeout", None, false),
            ConnectionError::LocallyClosed => Self::new("locally_closed", None, true),
            ConnectionError::CidsExhausted => Self::new("cids_exhausted", None, false),
        }
    }

    fn read(error: &ReadError) -> Self {
        match error {
            ReadError::Reset(code) => Self::new("stream_reset", Some(code.into_inner()), false),
            ReadError::ConnectionLost(error) => Self::connection(error),
            ReadError::ClosedStream => Self::new("stream_closed", None, true),
            ReadError::ZeroRttRejected => Self::new("zero_rtt_rejected", None, false),
        }
    }

    fn write(error: &WriteError) -> Self {
        match error {
            WriteError::Stopped(code) => Self::new("stream_stopped", Some(code.into_inner()), true),
            WriteError::ConnectionLost(error) => Self::connection(error),
            WriteError::ClosedStream => Self::new("stream_closed", None, true),
            WriteError::ZeroRttRejected => Self::new("zero_rtt_rejected", None, false),
        }
    }

    fn emit(self, stage: &'static str, started: Instant, bytes: usize, limit: usize) {
        // Progress counts completed I/O only. A failed write_all/read_exact may
        // have transferred more bytes than its last confirmed boundary.
        let duration_us = elapsed(started);
        if self.quiet {
            tracing::debug!(
                target: "sailry", area = "link", operation = "frame_io", phase = "interrupted",
                stage, bytes = bytes as u64, limit = limit as u64, duration_us,
                code = "unavailable", cause_category = self.category, cause_code = self.code,
            );
        } else {
            tracing::warn!(
                target: "sailry", area = "link", operation = "frame_io", phase = "failed",
                stage, bytes = bytes as u64, limit = limit as u64, duration_us,
                code = "unavailable", cause_category = self.category, cause_code = self.code,
            );
        }
    }
}

pub(super) fn read(
    error: ReadError,
    stage: &'static str,
    started: Instant,
    bytes: usize,
    limit: usize,
) -> Fault {
    Cause::read(&error).emit(stage, started, bytes, limit);
    super::frame::network(error)
}

pub(super) fn exact(
    error: ReadExactError,
    stage: &'static str,
    started: Instant,
    bytes: usize,
    limit: usize,
) -> Fault {
    let (cause, bytes) = match &error {
        ReadExactError::FinishedEarly(count) => {
            (Cause::new("unexpected_eof", None, false), bytes + count)
        }
        ReadExactError::ReadError(error) => (Cause::read(error), bytes),
    };
    cause.emit(stage, started, bytes, limit);
    super::frame::network(error)
}

pub(super) fn eof(stage: &'static str, started: Instant, bytes: usize, limit: usize) -> Fault {
    Cause::new("unexpected_eof", None, false).emit(stage, started, bytes, limit);
    super::frame::network("unexpected EOF")
}

pub(super) fn write(
    error: WriteError,
    stage: &'static str,
    started: Instant,
    bytes: usize,
    limit: usize,
) -> Fault {
    Cause::write(&error).emit(stage, started, bytes, limit);
    super::frame::network(error)
}

pub(super) fn timeout(started: Instant) {
    tracing::warn!(
        target: "sailry", area = "link", operation = "frame_io", phase = "failed",
        stage = "wait", reason = "io_timeout", code = "unavailable",
        cause_category = "io_timeout", duration_us = elapsed(started),
        wait_ms = super::IO_TIMEOUT.as_millis() as u64,
    );
}

pub(super) fn request(error: &Fault, started: Instant) {
    if error.code == ErrorCode::Cancelled {
        return;
    }
    // Actual I/O errors and deadlines have their own warning above. A departed
    // observer can also interrupt a write, so the enclosing request stays debug.
    let code = match error.code {
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
    };
    tracing::debug!(
        target: "sailry", area = "link", operation = "server_request", phase = "failed",
        stage = "request", code, duration_us = elapsed(started),
    );
}

fn elapsed(started: Instant) -> u64 {
    started.elapsed().as_micros().min(u64::MAX as u128) as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use iroh::endpoint::{ApplicationClose, TransportError, TransportErrorCode};

    #[test]
    fn classifies_io_errors() {
        assert_eq!(
            Cause::read(&ReadError::Reset(7u32.into())),
            Cause::new("stream_reset", Some(7), false)
        );
        assert_eq!(
            Cause::write(&WriteError::ConnectionLost(ConnectionError::TimedOut)),
            Cause::new("connection_timeout", None, false)
        );
        assert_eq!(
            Cause::read(&ReadError::ZeroRttRejected),
            Cause::new("zero_rtt_rejected", None, false)
        );
    }

    #[test]
    fn keeps_cancellation_quiet() {
        assert!(Cause::write(&WriteError::Stopped(0u32.into())).quiet);
        assert!(Cause::write(&WriteError::ClosedStream).quiet);
        assert!(Cause::read(&ReadError::ClosedStream).quiet);
        assert!(Cause::read(&ReadError::ConnectionLost(ConnectionError::LocallyClosed)).quiet);
    }

    #[test]
    fn omits_private_close_reasons() {
        let closed = ConnectionError::ApplicationClosed(ApplicationClose {
            error_code: 9u32.into(),
            reason: "PRIVATE_CLOSE_REASON".into(),
        });
        let transport = ConnectionError::TransportError(TransportError::new(
            TransportErrorCode::INTERNAL_ERROR,
            "PRIVATE_TRANSPORT_REASON".into(),
        ));
        for (error, category, code) in [
            (closed, "application_closed", 9),
            (
                transport,
                "transport_error",
                TransportErrorCode::INTERNAL_ERROR.into(),
            ),
        ] {
            let cause = Cause::read(&ReadError::ConnectionLost(error));
            assert_eq!(cause, Cause::new(category, Some(code), false));
            assert!(!format!("{cause:?}").contains("PRIVATE_"));
        }
    }
}
