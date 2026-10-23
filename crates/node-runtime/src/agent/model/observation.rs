//! Observe provider-adapted requests without retaining their contents.
use super::*;
use adk_core::{LlmResponse, model_observation as wire};
use sailry_protocol::conversation::{
    Usage,
    observation::{Attempt, Outcome, Request},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Mutex,
    time::Instant,
};
mod categories;

pub(super) const KEY: &str = "sailry_model_observation";

pub(in crate::agent) struct Capture {
    request: Request,
    context: crate::logging::Context,
    active: Option<u64>,
    starts: BTreeMap<u64, Instant>,
    diagnostics: Vec<crate::logging::Entry>,
    announced: bool,
    persisted: BTreeSet<u64>,
    timeline: Option<Arc<crate::agent::observation::Timeline>>,
    sources: Arc<Mutex<Vec<Source>>>,
}

#[derive(Clone)]
pub(in crate::agent) struct Source {
    pub kind: sailry_protocol::conversation::observation::CategoryKind,
    pub text: String,
}

impl Capture {
    pub(super) fn new(
        turn: TurnId,
        model: String,
        context: crate::logging::Context,
        timeline: Option<Arc<crate::agent::observation::Timeline>>,
        sources: Arc<Mutex<Vec<Source>>>,
    ) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            request: Request {
                id: context.call.expect("model call identity").to_string(),
                turn,
                model,
                context_limit: None,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                boundary: None,
                attempts: Vec::new(),
            },
            context,
            active: None,
            starts: BTreeMap::new(),
            diagnostics: Vec::new(),
            announced: false,
            persisted: BTreeSet::new(),
            timeline,
            sources,
        }))
    }

    pub(super) fn empty(&self) -> bool {
        self.request.attempts.is_empty()
    }

    pub(super) fn observer(capture: &Arc<Mutex<Self>>) -> wire::Observer {
        let capture = capture.clone();
        let offset = {
            let mut value = capture.lock().unwrap_or_else(|poison| poison.into_inner());
            value.active = None;
            value
                .request
                .attempts
                .last()
                .map_or(0, |attempt| attempt.index)
        };
        Arc::new(move |event| {
            let mut value = capture.lock().unwrap_or_else(|poison| poison.into_inner());
            value.observe(event, offset);
        })
    }

    fn observe(&mut self, event: wire::Event, offset: u64) {
        let index = offset.saturating_add(event.attempt);
        if self.persisted.contains(&index) {
            return;
        }
        if let wire::Kind::Started { transport, reused } = event.kind {
            if self
                .request
                .attempts
                .iter()
                .any(|attempt| attempt.index == index)
            {
                return;
            }
            self.active = Some(index);
            self.starts.insert(index, Instant::now());
            self.request.attempts.push(Attempt {
                index,
                transport: match transport {
                    wire::Transport::Http => "http",
                    wire::Transport::WebSocket => "web_socket",
                }
                .into(),
                reused,
                headers_us: None,
                first_text_us: None,
                elapsed_us: None,
                status: None,
                outcome: Outcome::Cancelled,
                categories: Vec::new(),
                usage: None,
                complete_context: false,
                cache_reported: false,
                reasoning_reported: false,
            });
            self.diagnostics.push(diagnostic(
                &self.context,
                self.request.attempts.last().unwrap(),
                crate::logging::Phase::Begin,
                None,
            ));
            return;
        }
        let Some(attempt) = self
            .request
            .attempts
            .iter_mut()
            .find(|value| value.index == index)
        else {
            return;
        };
        // A terminal response may still supply usage and transport metadata
        // before the enclosing scope returns. Only its end is idempotent.
        if attempt.elapsed_us.is_some() && matches!(event.kind, wire::Kind::Finished { .. }) {
            return;
        }
        match event.kind {
            wire::Kind::Prepared { body } => {
                attempt.complete_context = body
                    .get("previous_response_id")
                    .is_none_or(serde_json::Value::is_null)
                    && body
                        .get("previous_interaction_id")
                        .is_none_or(serde_json::Value::is_null);
                attempt.categories = categories::estimate(&body, &self.sources.lock().unwrap());
            }
            wire::Kind::Connection { reused } => attempt.reused = Some(reused),
            wire::Kind::Headers {
                status,
                elapsed,
                reused,
            } => {
                attempt.status = Some(status);
                attempt.headers_us = Some(micros(elapsed));
                attempt.reused = reused;
            }
            wire::Kind::Finished {
                elapsed,
                outcome,
                failure,
            } => {
                if let Some(timeline) = &self.timeline
                    && let Some(start) = self.starts.get(&index)
                {
                    use crate::agent::observation::Phase;
                    let waiting = attempt
                        .first_text_us
                        .map(std::time::Duration::from_micros)
                        .unwrap_or(elapsed)
                        .min(elapsed);
                    timeline.record(*start, waiting, Phase::Waiting);
                    if waiting < elapsed {
                        timeline.record(*start + waiting, elapsed - waiting, Phase::Output);
                    }
                }
                attempt.elapsed_us = Some(micros(elapsed));
                attempt.outcome = match outcome {
                    wire::Outcome::Succeeded => Outcome::Succeeded,
                    wire::Outcome::Failed => Outcome::Failed,
                    wire::Outcome::Cancelled => Outcome::Cancelled,
                };
                let phase = match outcome {
                    wire::Outcome::Succeeded => crate::logging::Phase::Completed,
                    wire::Outcome::Failed => crate::logging::Phase::Failed,
                    wire::Outcome::Cancelled => crate::logging::Phase::Cancelled,
                };
                self.diagnostics
                    .push(diagnostic(&self.context, attempt, phase, failure));
            }
            wire::Kind::Started { .. } => {}
        }
    }

    pub(super) fn response(&mut self, response: &LlmResponse) {
        let Some(index) = self.active else {
            return;
        };
        if self.persisted.contains(&index) {
            return;
        }
        let Some(attempt) = self
            .request
            .attempts
            .iter_mut()
            .find(|value| value.index == index)
        else {
            return;
        };
        if attempt.first_text_us.is_none()
            && response.content.as_ref().is_some_and(|content| {
                content
                    .parts
                    .iter()
                    .any(|part| matches!(part, adk_core::Part::Text { text } if !text.is_empty()))
            })
        {
            attempt.first_text_us = self
                .starts
                .get(&index)
                .map(|started| micros(started.elapsed()));
        }
        if let Some(usage) = response.usage_metadata.as_ref() {
            attempt.usage = Some(Usage {
                input: usage.prompt_token_count.max(0) as u64,
                output: usage.candidates_token_count.max(0) as u64,
                cached_input: usage
                    .cache_read_input_token_count
                    .unwrap_or_default()
                    .max(0) as u64,
                reasoning: usage.thinking_token_count.unwrap_or_default().max(0) as u64,
            });
            attempt.cache_reported = usage.cache_read_input_token_count.is_some();
            attempt.reasoning_reported = usage.thinking_token_count.is_some();
        }
    }

    pub(in crate::agent) async fn persist(
        capture: &Arc<Mutex<Self>>,
        ingress: &Arc<Ingress>,
        session: SessionId,
    ) -> adk_core::Result<()> {
        // Call after response consumption and its observation scope have ended,
        // so terminal callbacks and final response usage have both arrived.
        let (pending, diagnostics) = {
            let mut capture = capture.lock().unwrap_or_else(|poison| poison.into_inner());
            (capture.pending(), capture.diagnostics())
        };
        // ADK callbacks only project into memory. Native diagnostics may perform
        // I/O, so emit outside the callback, lock and model observation scope.
        for entry in diagnostics {
            entry.emit();
        }
        for request in pending {
            let event = event(&request);
            // Stable event IDs let the canonical history owner deduplicate an
            // overlapping finalizer or an uncertain append acknowledgement.
            ingress
                .sessions(request.turn)
                .append_event(&session.to_string(), event)
                .await?;
            let mut capture = capture.lock().unwrap_or_else(|poison| poison.into_inner());
            capture.announced = true;
            capture
                .persisted
                .extend(request.attempts.iter().map(|attempt| attempt.index));
        }
        Ok(())
    }

    fn pending(&self) -> Vec<Request> {
        let mut records = Vec::new();
        let record = |attempts| Request {
            id: self.request.id.clone(),
            turn: self.request.turn,
            model: self.request.model.clone(),
            context_limit: self.request.context_limit,
            timestamp_ms: self.request.timestamp_ms,
            boundary: self.request.boundary,
            attempts,
        };
        if !self.announced {
            records.push(record(Vec::new()));
        }
        for attempt in &self.request.attempts {
            if attempt.elapsed_us.is_none() || self.persisted.contains(&attempt.index) {
                continue;
            }
            records.push(record(vec![attempt.clone()]));
        }
        records
    }

    fn diagnostics(&mut self) -> Vec<crate::logging::Entry> {
        let mut records = std::mem::take(&mut self.diagnostics);
        for entry in &mut records {
            if matches!(entry.phase, crate::logging::Phase::Begin) {
                continue;
            }
            if let Some(attempt) = self
                .request
                .attempts
                .iter()
                .find(|attempt| Some(attempt.index) == entry.attempt)
            {
                entry.connection_reused = attempt.reused;
                entry.upstream_status = entry.upstream_status.or(attempt.status.map(u64::from));
                entry.duration_us = attempt.elapsed_us;
            }
        }
        records
    }
}

fn diagnostic(
    context: &crate::logging::Context,
    attempt: &Attempt,
    phase: crate::logging::Phase,
    failure: Option<wire::Failure>,
) -> crate::logging::Entry {
    let mut entry = crate::logging::Entry::new(crate::logging::Area::Model, phase);
    entry.context = context.clone();
    entry.operation = Some("model_attempt");
    entry.source_kind = Some("deferred_observation");
    entry.stage = Some("transport");
    entry.attempt = Some(attempt.index);
    entry.transport = Some(if attempt.transport == "http" {
        "http"
    } else {
        "websocket"
    });
    entry.connection_reused = attempt.reused;
    entry.upstream_status = attempt.status.map(u64::from);
    entry.duration_us = attempt.elapsed_us;
    if attempt.elapsed_us.is_some() && attempt.outcome == Outcome::Cancelled {
        entry.code = Some(ErrorCode::Cancelled);
    }
    if let Some(failure) = failure {
        super::diagnostics::failure(&mut entry, failure);
    }
    entry
}

fn event(request: &Request) -> adk_core::Event {
    let mut event = adk_core::Event::new(request.turn.to_string());
    // Retried appends must have identical content as well as a stable ID.
    event.timestamp = chrono::DateTime::from_timestamp_millis(request.timestamp_ms)
        .expect("capture timestamp is valid");
    let suffix = request
        .attempts
        .first()
        .map_or_else(|| "request".into(), |attempt| attempt.index.to_string());
    event.id = format!("model:{}:{suffix}", request.id);
    event.author = "system".into();
    event.provider_metadata.insert(
        KEY.into(),
        serde_json::to_string(request).expect("request observation serializes"),
    );
    event
}

fn micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}

#[cfg(test)]
mod tests;
