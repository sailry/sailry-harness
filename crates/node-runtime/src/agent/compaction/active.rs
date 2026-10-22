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
    manual_applied: AtomicBool,
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
            manual_applied: AtomicBool::new(false),
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
        invocation: &str,
    ) -> adk_core::Result<Option<usize>> {
        self.summarizer.prefix(
            &history.events,
            self.candidates(history, keep).into_iter().filter(|cut| {
                tail(request, &history.events[*cut..], &self.preamble, invocation).is_some()
            }),
        )
    }

    fn saved(
        &self,
        request: &mut LlmRequest,
        history: &[Event],
        summary: &Event,
        offset: usize,
        invocation: &str,
    ) -> adk_core::Result<()> {
        let index = tail(request, history, &self.preamble, invocation)
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
        invocation: &str,
        request: LlmRequest,
    ) -> adk_core::Result<BeforeModelResult> {
        let result = self.before_inner(invocation, request).await;
        if let Err(error) = &result
            && !self.summarizer.stop.is_cancelled()
        {
            self.notice(
                error,
                if self.manual_applied.load(Ordering::Relaxed) {
                    SummaryMode::Manual
                } else {
                    SummaryMode::Automatic
                },
                false,
            )
            .await?;
        }
        result
    }

    async fn before_inner(
        &self,
        invocation: &str,
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
            self.saved(&mut request, &history.events[1..], summary, 0, invocation)?;
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
        self.manual_applied.store(false, Ordering::Relaxed);
        let history = self.history().await?;
        let keep = self.retention.load(Ordering::Relaxed);
        let Some(cut) = self.cut(&history, &request, keep, invocation)? else {
            let candidates = self.candidates(&history, keep);
            let mismatch = !candidates.is_empty()
                && !candidates.iter().any(|cut| {
                    tail(
                        &request,
                        &history.events[*cut..],
                        &self.preamble,
                        invocation,
                    )
                    .is_some()
                });
            if full && mismatch {
                return Err(invalid(
                    "model context does not match the persisted summary",
                ));
            }
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
                    self.notice(
                        &invalid(if mismatch {
                            "model context does not match the persisted summary"
                        } else {
                            "no complete prefix fits the context summary budget"
                        }),
                        SummaryMode::Automatic,
                        true,
                    )
                    .await?;
                }
            }
            return Ok(BeforeModelResult::Continue(request));
        };
        // The helper model owns bounded transient retries. Never repeat a
        // completed invalid summary or a failed save through another loop.
        let result = self
            .summarize(&history.events[..cut], true, history.frontier.as_deref())
            .await;
        let result = match result {
            Ok(Some(summary)) if !self.summarizer.stop.is_cancelled() => {
                // Validate the resulting window before committing its boundary.
                let index = tail(&request, &history.events[cut..], &self.preamble, invocation)
                    .ok_or_else(|| invalid("model context does not match the persisted summary"))?;
                let mut projected = request.clone();
                projected.contents.splice(
                    1..index,
                    [summary
                        .actions
                        .compaction
                        .as_ref()
                        .ok_or_else(|| invalid("persisted context summary is missing"))?
                        .compacted_content
                        .clone()],
                );
                if full
                    && self.summarizer.context.is_some_and(|context| {
                        estimate::request(&projected).is_ok_and(|tokens| tokens >= context)
                    })
                {
                    return Err(invalid(
                        "context remains over the estimated model budget after compaction",
                    ));
                }
                match self
                    .service
                    .append_event(&self.session.to_string(), summary.clone())
                    .await
                {
                    Ok(()) => {
                        self.saved(
                            &mut request,
                            &history.events[cut..],
                            &summary,
                            offset,
                            invocation,
                        )?;
                        return Ok(BeforeModelResult::Continue(request));
                    }
                    Err(error) => {
                        self.summarizer
                            .failure(SummaryMode::Automatic, "persistence", &error);
                        Err(save_error(error))
                    }
                }
            }
            other => other,
        };
        let failure = result.err();
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        if full {
            return Err(failure.unwrap_or_else(|| {
                invalid("context exceeds the model limit and no summary was applied")
            }));
        }
        if let Some(error) = failure {
            self.notice(&error, SummaryMode::Automatic, true).await?;
        }
        Ok(BeforeModelResult::Continue(request))
    }

    async fn notice(
        &self,
        error: &adk_core::AdkError,
        mode: SummaryMode,
        continued: bool,
    ) -> adk_core::Result<()> {
        failure::record(
            self.service.as_ref(),
            &self.session.to_string(),
            self.summarizer.turn,
            mode,
            error,
            continued,
        )
        .await
    }
}

#[async_trait]
impl BaseEventsSummarizer for Context {
    async fn summarize_events(&self, window: &[Event]) -> adk_core::Result<Option<Event>> {
        let result = self.after(window).await;
        if let Err(error) = &result
            && !self.summarizer.stop.is_cancelled()
        {
            self.notice(error, SummaryMode::Automatic, true).await?;
        }
        result
    }
}

impl Context {
    async fn after(&self, window: &[Event]) -> adk_core::Result<Option<Event>> {
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
                        self.notice(&save_error(error), SummaryMode::Automatic, true)
                            .await?;
                    }
                }
                // The event is already persisted; do not ask Runner to append it twice.
                Ok(None)
            }
            Err(error) => {
                self.notice(&error, SummaryMode::Automatic, true).await?;
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
        let mut augmented = content.clone();
        augmented.parts.push(Part::Thinking {
            thinking: "Retained reasoning".into(),
            signature: Some("retained-signature".into()),
        });
        event.set_content(augmented.clone());
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
        let index = tail(&request, &[event.clone()], &preamble, "current").unwrap();
        assert_eq!(index, 2);
        request
            .contents
            .splice(1..index, [Content::new("model").with_text("Summary")]);
        assert_eq!(request.contents[2].parts, augmented.parts);
        assert!(
            tail(
                &request,
                &[event.clone()],
                &Content::new("system").with_text("Changed instructions"),
                "current",
            )
            .is_none()
        );
        request.contents[2].parts.reverse();
        assert!(tail(&request, &[event], &preamble, "current").is_none());
    }
}
