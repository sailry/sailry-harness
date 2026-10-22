//! Observe provider-adapted requests without retaining their contents.
use super::*;
use adk_core::{LlmResponse, model_observation as wire};
use sailry_protocol::conversation::{
    Usage,
    observation::{Attempt, Outcome, Request},
};
use std::{collections::BTreeMap, sync::Mutex, time::Instant};
mod categories;

pub(super) const KEY: &str = "sailry_model_observation";

pub(in crate::agent) struct Capture {
    request: Request,
    active: Option<u64>,
    starts: BTreeMap<u64, Instant>,
    offset: u64,
    persisted: Option<String>,
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
        timeline: Option<Arc<crate::agent::observation::Timeline>>,
        sources: Arc<Mutex<Vec<Source>>>,
    ) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            request: Request {
                id: uuid::Uuid::new_v4().to_string(),
                turn,
                model,
                context_limit: None,
                timestamp_ms: chrono::Utc::now().timestamp_millis(),
                boundary: None,
                attempts: Vec::new(),
            },
            active: None,
            starts: BTreeMap::new(),
            offset: 0,
            persisted: None,
            timeline,
            sources,
        }))
    }

    pub(super) fn empty(&self) -> bool {
        self.request.attempts.is_empty()
    }

    pub(super) fn observer(capture: &Arc<Mutex<Self>>) -> wire::Observer {
        let capture = capture.clone();
        {
            let mut value = capture.lock().unwrap_or_else(|poison| poison.into_inner());
            value.offset = value.request.attempts.len() as u64;
            value.active = None;
        }
        Arc::new(move |event| {
            let mut value = capture.lock().unwrap_or_else(|poison| poison.into_inner());
            value.observe(event);
        })
    }

    fn observe(&mut self, event: wire::Event) {
        let index = self.offset.saturating_add(event.attempt);
        if let wire::Kind::Started { transport, reused } = event.kind {
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
            wire::Kind::Finished { elapsed, outcome } => {
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
            }
            wire::Kind::Started { .. } => {}
        }
    }

    pub(super) fn response(&mut self, response: &LlmResponse) {
        let Some(index) = self.active else {
            return;
        };
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
        ingress: &Ingress,
        session: SessionId,
    ) -> adk_core::Result<()> {
        let request = capture
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .request
            .clone();
        let snapshot = serde_json::to_string(&request).expect("request observation serializes");
        if capture
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .persisted
            .as_ref()
            == Some(&snapshot)
        {
            return Ok(());
        }
        let mut event = adk_core::Event::new(request.turn.to_string());
        event.author = "system".into();
        event.provider_metadata.insert(KEY.into(), snapshot.clone());
        ingress
            .sessions(request.turn)
            .append_event(&session.to_string(), event)
            .await?;
        capture
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .persisted = Some(snapshot);
        Ok(())
    }
}

fn micros(duration: std::time::Duration) -> u64 {
    duration.as_micros().try_into().unwrap_or(u64::MAX)
}
