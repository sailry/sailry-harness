//! Dropped operations retain one content-free observation under their original consent.
use super::{Model, Observation, RequestResult, StatusClass, Tokens, Tool, ToolResult};
use sailry_link::CancellationToken;
use sailry_protocol::ErrorCode;
use std::{path::Path, time::Instant};

pub(crate) struct Call {
    capture: crate::telemetry::Capture,
    tool: Option<Tool>,
    stop: CancellationToken,
    started: Instant,
}

impl Call {
    pub(crate) fn new(
        profile: Option<&Path>,
        tool: Option<Tool>,
        stop: &CancellationToken,
    ) -> Option<Self> {
        Some(Self {
            capture: crate::telemetry::capture(profile)?,
            tool: Some(tool?),
            stop: stop.clone(),
            started: Instant::now(),
        })
    }

    pub(crate) fn finish(mut self, result: ToolResult, error: Option<ErrorCode>) {
        let (result, error) = if self.stop.is_cancelled() {
            (ToolResult::Cancelled, Some(ErrorCode::Cancelled))
        } else {
            (result, error)
        };
        self.emit(result, error);
    }

    fn emit(&mut self, result: ToolResult, error: Option<ErrorCode>) {
        let Some(tool) = self.tool.take() else {
            return;
        };
        self.capture.record(Observation::Tool {
            tool,
            result,
            error,
            elapsed: self.started.elapsed(),
        });
    }
}

impl Drop for Call {
    fn drop(&mut self) {
        self.emit(ToolResult::Cancelled, Some(ErrorCode::Cancelled));
    }
}

pub(crate) struct Request {
    capture: crate::telemetry::Capture,
    model: Option<Model>,
    status: StatusClass,
    tokens: Tokens,
}

impl Request {
    /// Native search sends one direct HTTP request; no private retry loop runs here.
    pub(crate) fn direct(
        profile: Option<&Path>,
        provider: &sailry_protocol::conversation::Provider,
        model: &str,
    ) -> Option<Self> {
        Some(Self {
            capture: crate::telemetry::capture(profile)?,
            model: Some(Model::capture(provider, model)),
            status: StatusClass::Unknown,
            tokens: Tokens::default(),
        })
    }
    pub(crate) fn status(&mut self, status: u16) {
        self.status = StatusClass::from(Some(status));
    }
    pub(crate) fn tokens(&mut self, tokens: Tokens) {
        self.tokens = tokens;
    }
    pub(crate) fn finish(mut self, result: RequestResult, error: Option<ErrorCode>) {
        self.emit(result, error);
    }
    fn emit(&mut self, result: RequestResult, error: Option<ErrorCode>) {
        let Some(model) = self.model.take() else {
            return;
        };
        self.capture.record(Observation::Model {
            model,
            result,
            error,
            status: self.status,
            retries: Some(0),
            tokens: self.tokens,
        });
    }
}

impl Drop for Request {
    fn drop(&mut self) {
        self.emit(RequestResult::Cancelled, Some(ErrorCode::Cancelled));
    }
}

pub(crate) struct Search {
    capture: Option<crate::telemetry::Capture>,
    driver: Option<sailry_protocol::search::Driver>,
    started: Instant,
}

impl Search {
    pub(crate) fn new(
        profile: Option<&Path>,
        selection: Option<sailry_protocol::search::Selection>,
    ) -> Option<Self> {
        Some(Self {
            capture: Some(crate::telemetry::capture(profile)?),
            driver: match selection {
                Some(sailry_protocol::search::Selection::Driver(driver)) => Some(driver),
                _ => None,
            },
            started: Instant::now(),
        })
    }

    pub(crate) fn finish(
        mut self,
        result: &Result<sailry_protocol::search::Page, sailry_protocol::Fault>,
    ) {
        let error = result.as_ref().err().map(|error| error.code);
        if let Ok(page) = result {
            self.driver = Some(page.driver);
        }
        self.emit(
            match error {
                Some(ErrorCode::Cancelled) => RequestResult::Cancelled,
                Some(_) => RequestResult::Error,
                None => RequestResult::Success,
            },
            error,
        );
    }

    fn emit(&mut self, result: RequestResult, error: Option<ErrorCode>) {
        let Some(capture) = self.capture.take() else {
            return;
        };
        capture.record(Observation::Search {
            driver: self.driver,
            result,
            error,
            elapsed: self.started.elapsed(),
        });
    }
}

impl Drop for Search {
    fn drop(&mut self) {
        self.emit(RequestResult::Cancelled, Some(ErrorCode::Cancelled));
    }
}

#[cfg(test)]
mod tests;
