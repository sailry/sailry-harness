//! Compact complete canonical prefixes through ADK's per-model callback.
use super::*;
use crate::store::agent::{ContextUsage, compaction::History};
use adk_core::BeforeModelResult;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

mod matching;
mod native;
use matching::{Checkpoint, tail};

pub(super) struct Context {
    ingress: Arc<Ingress>,
    service: Arc<dyn SessionService>,
    session: sailry_protocol::SessionId,
    summarizer: Summarizer,
    preamble: Content,
    applied: AtomicBool,
    retention: AtomicUsize,
    attempted: Mutex<Option<Attempt>>,
    checkpoint: Mutex<Option<Arc<Checkpoint>>>,
}

struct Attempt {
    end: String,
    length: usize,
    // A larger retention window alone is not newly completed work.
    frontier: Option<String>,
}

impl Attempt {
    fn advances(&self, next: &Self) -> bool {
        self.end != next.end && next.length > self.length && self.frontier != next.frontier
    }
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
            retention: AtomicUsize::new(1),
            attempted: Mutex::new(None),
            checkpoint: Mutex::new(None),
        }
    }

    async fn summarize(
        &self,
        events: &[Event],
        automatic: bool,
        frontier: Option<&str>,
    ) -> adk_core::Result<Option<Event>> {
        let last = events
            .iter()
            .rev()
            .find(|event| event.content().is_some())
            .map(|event| event.id.clone());
        if automatic && let Some(last) = last {
            let mut attempted = self
                .attempted
                .lock()
                .map_err(|_| invalid("context attempt state is unavailable"))?;
            let next = Attempt {
                end: last,
                length: events.len(),
                frontier: frontier.map(str::to_owned),
            };
            if attempted
                .as_ref()
                .is_some_and(|previous| !previous.advances(&next))
            {
                return Ok(None);
            }
            *attempted = Some(next);
        }
        self.summarizer
            .summarize_with(
                events,
                if automatic {
                    SummaryMode::Automatic
                } else {
                    SummaryMode::Manual
                },
            )
            .await
    }

    async fn history(&self) -> adk_core::Result<History> {
        self.ingress
            .context_history(self.session, self.summarizer.turn)
            .await
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
        usage
            .tokens
            .saturating_add(estimate::tokens(&usage.pending))
    }

    fn candidates(&self, history: &History, keep: usize) -> Vec<usize> {
        let mut exchange = crate::store::agent::compaction::Exchange::default();
        let complete = history.events[history.current..]
            .iter()
            .map(|event| exchange.push(event))
            .collect::<Vec<_>>();
        let mut cuts = history
            .events
            .iter()
            .enumerate()
            .skip(history.current)
            .filter(|(_, event)| {
                event.content().is_some_and(|content| {
                    content
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::FunctionCall { .. }))
                })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .skip(keep)
            .filter(|cut| *cut > history.current && complete[*cut - history.current - 1])
            .collect::<Vec<_>>();
        let recent = history
            .events
            .iter()
            .enumerate()
            .filter(|(_, event)| {
                event.content().is_some_and(|content| {
                    content
                        .parts
                        .iter()
                        .any(|part| matches!(part, Part::FunctionCall { .. }))
                })
            })
            .map(|(index, _)| index)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .nth(keep);
        cuts.extend(history.completed.iter().rev().copied().filter(|cut| {
            *cut <= history.current
                && recent.is_none_or(|recent| *cut <= recent)
                && history.recent.is_none_or(|recent| *cut <= recent)
        }));
        cuts
    }

    fn cut(
        &self,
        history: &History,
        request: &LlmRequest,
        keep: usize,
    ) -> adk_core::Result<Option<usize>> {
        self.summarizer.prefix(
            &history.events,
            self.candidates(history, keep)
                .into_iter()
                .filter(|cut| tail(request, &history.events[*cut..], &self.preamble).is_some()),
        )
    }

    fn saved(
        &self,
        request: &mut LlmRequest,
        history: &[Event],
        summary: &Event,
        offset: usize,
    ) -> adk_core::Result<()> {
        let index = tail(request, history, &self.preamble)
            .ok_or_else(|| invalid("model context does not match the persisted summary"))?;
        let content = summary
            .actions
            .compaction
            .as_ref()
            .map(|value| &value.compacted_content)
            .or_else(|| summary.content())
            .ok_or_else(|| invalid("persisted context summary is missing"))?;
        let checkpoint = Checkpoint::new(request, index, content.clone(), offset);
        request.contents.splice(1..index, [content.clone()]);
        request.previous_response_id = None;
        *self
            .checkpoint
            .lock()
            .map_err(|_| invalid("context checkpoint is unavailable"))? =
            Some(Arc::new(checkpoint));
        *self
            .attempted
            .lock()
            .map_err(|_| invalid("context attempt state is unavailable"))? = None;
        self.applied.store(true, Ordering::Relaxed);
        Ok(())
    }

    pub(super) async fn before(
        &self,
        _invocation: &str,
        mut request: LlmRequest,
    ) -> adk_core::Result<BeforeModelResult> {
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        let checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| invalid("context checkpoint is unavailable"))?
            .clone();
        let mut offset = if let Some(checkpoint) = &checkpoint {
            checkpoint.apply(&mut request, &self.preamble)?;
            checkpoint.offset()
        } else {
            0
        };
        if self.applied.load(Ordering::Relaxed) && checkpoint.is_none() {
            let history = self.history().await?;
            let summary = history
                .events
                .first()
                .filter(|event| {
                    event
                        .provider_metadata
                        .contains_key(crate::store::agent::compaction::END_EVENT)
                })
                .ok_or_else(|| invalid("persisted context summary is missing"))?;
            self.saved(&mut request, &history.events[1..], summary, 0)?;
            offset = self
                .checkpoint
                .lock()
                .map_err(|_| invalid("context checkpoint is unavailable"))?
                .as_ref()
                .unwrap()
                .offset();
        }
        let usage = self.usage().await?;
        // Estimates trigger compaction only after a summary invalidates the
        // provider observation. Do not rescan a measured request every batch.
        let projected = match usage.as_ref() {
            Some(usage) if usage.observed => Some(Self::projected(usage)),
            Some(_) => Some(estimate::request(&request)?),
            None => None,
        };
        let compact = self.summarizer.needs_compaction(projected);
        let full = self
            .summarizer
            .context
            .zip(
                usage
                    .as_ref()
                    .filter(|usage| usage.observed)
                    .map(|usage| usage.tokens),
            )
            .is_some_and(|(context, tokens)| tokens >= context);
        if !compact {
            return Ok(BeforeModelResult::Continue(request));
        }
        let history = self.history().await?;
        let keep = self.retention.load(Ordering::Relaxed);
        let Some(cut) = self.cut(&history, &request, keep)? else {
            if full {
                return Err(invalid(
                    "context exceeds the model limit and no complete prefix fits the summary budget",
                ));
            }
            if !self.candidates(&history, keep).is_empty() {
                let key = self
                    .candidates(&history, keep)
                    .first()
                    .and_then(|cut| {
                        history.events[..*cut]
                            .iter()
                            .rev()
                            .find(|event| event.content().is_some())
                    })
                    .map(|event| format!("fit:{}", event.id));
                let warn = {
                    let mut attempted = self
                        .attempted
                        .lock()
                        .map_err(|_| invalid("context attempt state is unavailable"))?;
                    let length = self
                        .candidates(&history, keep)
                        .first()
                        .copied()
                        .unwrap_or(0);
                    let next = key.map(|end| Attempt {
                        end,
                        length,
                        frontier: history.frontier.clone(),
                    });
                    let warn = next.as_ref().is_some_and(|next| {
                        attempted
                            .as_ref()
                            .is_none_or(|previous| previous.advances(next))
                    });
                    if warn {
                        *attempted = next;
                    }
                    warn
                };
                if warn {
                    self.warning().await?;
                }
            }
            return Ok(BeforeModelResult::Continue(request));
        };
        let mut failure = None;
        for attempt in 0..if full { 2 } else { 1 } {
            let result = if !full && attempt == 0 {
                self.summarize(&history.events[..cut], true, history.frontier.as_deref())
                    .await
            } else {
                self.summarizer
                    .summarize_with(&history.events[..cut], SummaryMode::Automatic)
                    .await
            };
            let result = match result {
                Ok(Some(summary)) if !self.summarizer.stop.is_cancelled() => {
                    match self
                        .service
                        .append_event(&self.session.to_string(), summary.clone())
                        .await
                    {
                        Ok(()) => {
                            self.saved(&mut request, &history.events[cut..], &summary, offset)?;
                            // A completed summary can still be too large with the retained input.
                            // This remains an estimate, never a claim of provider-measured overflow.
                            if full
                                && self.summarizer.context.is_some_and(|context| {
                                    estimate::request(&request)
                                        .is_ok_and(|tokens| tokens >= context)
                                })
                            {
                                return Err(invalid(
                                    "context remains over the estimated model budget after compaction",
                                ));
                            }
                            return Ok(BeforeModelResult::Continue(request));
                        }
                        Err(error) => {
                            self.summarizer
                                .failure(SummaryMode::Automatic, "persistence", &error);
                            Err(error)
                        }
                    }
                }
                other => other,
            };
            match result {
                Err(error) => failure = Some(error),
                Ok(_) => break,
            }
            if self.summarizer.stop.is_cancelled() {
                return Err(invalid("context compaction was cancelled"));
            }
        }
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        if full {
            return Err(failure.unwrap_or_else(|| {
                invalid("context exceeds the model limit and no summary was applied")
            }));
        }
        if failure.is_some() {
            self.warning().await?;
        }
        Ok(BeforeModelResult::Continue(request))
    }

    async fn warning(&self) -> adk_core::Result<()> {
        let mut event = Event::new(self.summarizer.turn.to_string());
        event.author = "system".into();
        event.provider_metadata.insert(
            "sailry_operation_warning".into(),
            "context_summary_failed".into(),
        );
        self.service
            .append_event(&self.session.to_string(), event)
            .await
    }
}

#[async_trait]
impl BaseEventsSummarizer for Context {
    async fn summarize_events(&self, window: &[Event]) -> adk_core::Result<Option<Event>> {
        let usage = self.usage().await?;
        let tokens = usage.as_ref().map(Self::projected);
        if !self.summarizer.needs_compaction(tokens) || self.summarizer.stop.is_cancelled() {
            return Ok(None);
        }
        let Some(last) = window.last() else {
            return Ok(None);
        };
        let history = self.history().await?;
        let Some(end) = history.events.iter().position(|event| event.id == last.id) else {
            return Ok(None);
        };
        let candidates = history
            .completed
            .iter()
            .rev()
            .copied()
            .filter(|cut| *cut <= end + 1);
        let Some(cut) = self.summarizer.prefix(&history.events, candidates)? else {
            return Ok(None);
        };
        match self
            .summarize(&history.events[..cut], true, history.frontier.as_deref())
            .await
        {
            Ok(Some(summary)) if !self.summarizer.stop.is_cancelled() => {
                // Own persistence through the same canonical service so a rejected
                // checkpoint is observable, rather than ADK logging it silently.
                match self
                    .service
                    .append_event(&self.session.to_string(), summary)
                    .await
                {
                    Ok(()) => {
                        *self
                            .checkpoint
                            .lock()
                            .map_err(|_| invalid("context checkpoint is unavailable"))? = None;
                        *self
                            .attempted
                            .lock()
                            .map_err(|_| invalid("context attempt state is unavailable"))? = None;
                        self.applied.store(true, Ordering::Relaxed);
                    }
                    Err(error) => {
                        self.summarizer
                            .failure(SummaryMode::Automatic, "persistence", &error);
                        self.warning().await?;
                    }
                }
                // The event is already persisted; do not ask Runner to append it twice.
                Ok(None)
            }
            Err(_) => {
                self.warning().await?;
                Ok(None)
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retries_only_after_completed_work() {
        let previous = Attempt {
            end: "first".into(),
            length: 4,
            frontier: Some("turn:third".into()),
        };
        let retained = Attempt {
            end: "second".into(),
            length: 6,
            frontier: Some("turn:third".into()),
        };
        assert!(!previous.advances(&retained));
        let metadata = Attempt {
            end: "retry".into(),
            length: 12,
            frontier: Some("turn:third".into()),
        };
        assert!(!previous.advances(&metadata));
        let exchange = Attempt {
            end: "next".into(),
            length: 8,
            frontier: Some("exchange:result".into()),
        };
        assert!(previous.advances(&exchange));
        assert!(!previous.advances(&Attempt {
            end: previous.end.clone(),
            ..exchange
        }));
        assert!(!previous.advances(&Attempt {
            end: "older".into(),
            length: 3,
            frontier: Some("exchange:new".into())
        }));
    }

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
