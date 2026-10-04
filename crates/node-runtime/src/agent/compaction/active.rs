//! Compact completed tool exchanges through ADK's per-model callback.
use super::*;
use adk_core::BeforeModelResult;
use std::sync::atomic::{AtomicBool, Ordering};

mod native;

pub(super) struct Context {
    ingress: Arc<Ingress>,
    service: Arc<dyn SessionService>,
    session: sailry_protocol::SessionId,
    summarizer: Summarizer,
    preamble: Content,
    applied: AtomicBool,
}

impl Context {
    pub(super) fn new(
        ingress: Arc<Ingress>,
        service: Arc<dyn SessionService>,
        session: sailry_protocol::SessionId,
        summarizer: Summarizer,
        instruction: String,
    ) -> Self {
        Self {
            ingress,
            service,
            session,
            summarizer,
            preamble: Content::new("user").with_text(instruction),
            applied: AtomicBool::new(false),
        }
    }

    async fn events(&self) -> adk_core::Result<Vec<Event>> {
        Ok(self
            .service
            .get(adk_session::GetRequest {
                app_name: APP.into(),
                user_id: USER.into(),
                session_id: self.session.to_string(),
                num_recent_events: None,
                after: None,
            })
            .await?
            .events()
            .all())
    }

    async fn needs_compaction(&self) -> adk_core::Result<bool> {
        let usage = self
            .ingress
            .context_usage(self.session)
            .await
            .map_err(|error| {
                adk_core::AdkError::session("context usage could not be read").with_source(error)
            })?;
        // Only estimate content added after the latest provider observation;
        // its output is already included. Estimates never enter usage totals.
        let tokens = usage.map(|usage| {
            usage
                .tokens
                .saturating_add(estimate::tokens(&usage.pending))
        });
        Ok(self.summarizer.needs_compaction(tokens))
    }

    pub(super) async fn before(
        &self,
        invocation: &str,
        mut request: LlmRequest,
    ) -> adk_core::Result<BeforeModelResult> {
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        let compact = self.needs_compaction().await?;
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        if !compact && !self.applied.load(Ordering::Relaxed) {
            return Ok(BeforeModelResult::Continue(request));
        }
        let mut events = self.events().await?;
        // Preserve the latest two complete tool exchanges. The callback runs
        // after ADK has yielded and persisted every result in the previous batch.
        let cut = events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                event.invocation_id == invocation
                    && event.content().is_some_and(|content| {
                        content
                            .parts
                            .iter()
                            .any(|part| matches!(part, Part::FunctionCall { .. }))
                    })
            })
            .rev()
            .nth(1)
            .map(|(index, _)| index);
        if compact
            && let Some(cut) = cut
            && crate::store::agent::compaction::complete_exchange(
                &events[..cut]
                    .iter()
                    .filter(|event| event.invocation_id == invocation)
                    .cloned()
                    .collect::<Vec<_>>(),
            )
            && tail(&request, &events[cut..], &self.preamble).is_some()
        {
            match self.summarizer.summarize_events(&events[..cut]).await {
                Ok(Some(summary)) if !self.summarizer.stop.is_cancelled() => {
                    self.service
                        .append_event(&self.session.to_string(), summary)
                        .await?;
                    self.applied.store(true, Ordering::Relaxed);
                    events = self.events().await?;
                }
                Ok(_) => {}
                Err(error) => eprintln!("Context compaction failed: {error}"),
            }
        }
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        if self.applied.load(Ordering::Relaxed) {
            let summary = events
                .first()
                .and_then(Event::content)
                .ok_or_else(|| invalid("persisted context summary is missing"))?;
            let index = tail(&request, &events[1..], &self.preamble)
                .ok_or_else(|| invalid("model context does not match the persisted summary"))?;
            request.contents.splice(1..index, [summary.clone()]);
            request.previous_response_id = None;
        }
        Ok(BeforeModelResult::Continue(request))
    }
}

// Keep provider-added parts in the actual request. Match the complete retained
// suffix before replacing its prefix, rather than rebuilding ADK's prompt.
fn tail(request: &LlmRequest, events: &[Event], preamble: &Content) -> Option<usize> {
    if !request
        .contents
        .first()
        .is_some_and(|content| content.role == preamble.role && content.parts == preamble.parts)
    {
        return None;
    }
    let retained: Vec<_> = events.iter().filter_map(Event::content).collect();
    if retained.is_empty() {
        return None;
    }
    let index = request.contents.len().checked_sub(retained.len())?;
    (index > 0
        && request.contents[index..]
            .iter()
            .zip(retained)
            .all(|(actual, stored)| actual.parts.starts_with(&stored.parts)))
    .then_some(index)
}

#[async_trait]
impl BaseEventsSummarizer for Context {
    async fn summarize_events(&self, window: &[Event]) -> adk_core::Result<Option<Event>> {
        if !self.needs_compaction().await? || self.summarizer.stop.is_cancelled() {
            return Ok(None);
        }
        let Some(last) = window.last() else {
            return Ok(None);
        };
        let canonical = self.events().await?;
        // A summary produced inside this invocation can supersede ADK's
        // original post-invocation window. Do not summarize the stale prefix.
        let Some(end) = canonical.iter().position(|event| event.id == last.id) else {
            return Ok(None);
        };
        self.summarizer.summarize_events(&canonical[..=end]).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_retained_provider_parts() {
        let preamble = Content::new("user").with_text("Instructions");
        let content = Content::new("model").with_text("Repeated response");
        let mut event = Event::new("invocation");
        event.set_content(content.clone());
        let augmented = content.clone().with_text("Provider continuation");
        let mut request = LlmRequest {
            model: "fixture".into(),
            contents: vec![preamble.clone(), content, augmented.clone()],
            config: None,
            tools: Default::default(),
            previous_response_id: None,
        };
        let index = tail(&request, &[event.clone()], &preamble).unwrap();
        assert_eq!(index, 2);
        request
            .contents
            .splice(1..index, [Content::new("model").with_text("Summary")]);
        assert_eq!(request.contents[2].parts, augmented.parts);
        assert!(
            tail(
                &request,
                &[event.clone()],
                &Content::new("user").with_text("Changed instructions")
            )
            .is_none()
        );
        request.contents[2].parts.reverse();
        assert!(tail(&request, &[event], &preamble).is_none());
    }
}
