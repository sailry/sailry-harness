//! Compact completed tool exchanges through ADK's per-model callback.
use super::*;
use crate::store::agent::ContextUsage;
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
    failed: AtomicBool,
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
            preamble: Content::new("system").with_text(instruction),
            applied: AtomicBool::new(false),
            failed: AtomicBool::new(false),
        }
    }

    async fn summarize(
        &self,
        events: &[Event],
        automatic: bool,
    ) -> adk_core::Result<Option<Event>> {
        // A failed automatic request must not stall every remaining tool batch
        // or repeat again after the invocation. Explicit requests can retry;
        // a new turn has its own Context and keeps the original history.
        if automatic && self.failed.load(Ordering::Relaxed) {
            return Ok(None);
        }
        let result = self
            .summarizer
            .summarize_with(events, SummaryMode::Automatic)
            .await;
        match &result {
            Err(_) => self.failed.store(true, Ordering::Relaxed),
            Ok(Some(_)) => self.failed.store(false, Ordering::Relaxed),
            Ok(None) => {}
        }
        result
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

    async fn usage(&self) -> adk_core::Result<Option<ContextUsage>> {
        self.ingress
            .context_usage(self.session)
            .await
            .map_err(|error| {
                adk_core::AdkError::session("context usage could not be read").with_source(error)
            })
    }

    fn projected(usage: &ContextUsage) -> u64 {
        // Pending text estimates trigger early compaction, not a tokenizer-based
        // rejection. Only the provider observation establishes a known full window.
        usage
            .tokens
            .saturating_add(estimate::tokens(&usage.pending))
    }

    async fn needs_compaction(&self) -> adk_core::Result<bool> {
        Ok(self
            .summarizer
            .needs_compaction(self.usage().await?.as_ref().map(Self::projected)))
    }

    fn cut(
        &self,
        events: &[Event],
        invocation: &str,
        request: &LlmRequest,
    ) -> adk_core::Result<Option<usize>> {
        // Retain the current invocation's latest two complete tool exchanges.
        let exchanges = events
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
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        for cut in exchanges.into_iter().rev().skip(1) {
            let current = events[..cut]
                .iter()
                .filter(|event| event.invocation_id == invocation)
                .cloned()
                .collect::<Vec<_>>();
            if crate::store::agent::compaction::complete_exchange(&current)
                && tail(request, &events[cut..], &self.preamble).is_some()
                && self.summarizer.fits(&events[..cut])?
            {
                return Ok(Some(cut));
            }
        }
        // Prior turns are already terminal. Summarize only a complete prefix,
        // retaining all current input, attachments and provider-added suffix parts.
        let current = events
            .iter()
            .position(|event| event.invocation_id == invocation)
            .unwrap_or(0);
        for cut in (1..=current).rev() {
            if (cut == current || events[cut - 1].invocation_id != events[cut].invocation_id)
                && tail(request, &events[cut..], &self.preamble).is_some()
                && self.summarizer.fits(&events[..cut])?
            {
                return Ok(Some(cut));
            }
        }
        Ok(None)
    }

    pub(super) async fn before(
        &self,
        invocation: &str,
        mut request: LlmRequest,
    ) -> adk_core::Result<BeforeModelResult> {
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        let usage = self.usage().await?;
        let compact = self
            .summarizer
            .needs_compaction(usage.as_ref().map(Self::projected));
        let full = self
            .summarizer
            .context
            .zip(usage.as_ref().map(|usage| usage.tokens))
            .is_some_and(|(context, tokens)| tokens >= context);
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        if !compact && !self.applied.load(Ordering::Relaxed) {
            return Ok(BeforeModelResult::Continue(request));
        }
        let mut events = self.events().await?;
        if compact {
            let cut = self.cut(&events, invocation, &request)?;
            if let Some(cut) = cut {
                match self.summarize(&events[..cut], true).await {
                    Ok(Some(summary)) if !self.summarizer.stop.is_cancelled() => {
                        self.service
                            .append_event(&self.session.to_string(), summary)
                            .await
                            .inspect_err(|error| {
                                self.summarizer.failure(
                                    SummaryMode::Automatic,
                                    "persistence",
                                    error,
                                )
                            })?;
                        self.applied.store(true, Ordering::Relaxed);
                        events = self.events().await?;
                    }
                    Err(error) if full => return Err(error),
                    _ if full => {
                        return Err(invalid(
                            "context exceeds the model limit and no summary was applied",
                        ));
                    }
                    _ => {}
                }
            } else if full {
                return Err(invalid(
                    "context exceeds the model limit and no complete prefix fits the summary budget",
                ));
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
    if !request.contents.first().is_some_and(|content| {
        content.role == preamble.role && content.parts.starts_with(&preamble.parts)
    }) {
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
        self.summarize(&canonical[..=end], true).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_retained_provider_parts() {
        let preamble = Content::new("system").with_text("Instructions");
        let content = Content::new("model").with_text("Repeated response");
        let mut event = Event::new("invocation");
        event.set_content(content.clone());
        let augmented = content.clone().with_text("Provider continuation");
        let mut request = LlmRequest {
            model: "fixture".into(),
            contents: vec![
                preamble.clone().with_text("Scoped project instructions"),
                content,
                augmented.clone(),
            ],
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
                &Content::new("system").with_text("Changed instructions")
            )
            .is_none()
        );
        request.contents[2].parts.reverse();
        assert!(tail(&request, &[event], &preamble).is_none());
    }
}
