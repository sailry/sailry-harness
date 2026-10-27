//! Prepare immutable prefixes in the background and apply at a safe model boundary.
use super::*;
use crate::store::agent::{ContextUsage, compaction::History};
use adk_core::BeforeModelResult;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

mod background;
mod matching;
mod native;
mod recovery;
mod state;
#[cfg(test)]
mod tests;
use background::Candidate;
use matching::{Checkpoint, tail};
pub(in crate::agent) use state::State;

const PREPARE_PERCENT: u64 = 85;
const PROTECTED_OUTPUT: u64 = 50_000;
const MINIMUM_SAVING: u64 = 30_000;
pub(crate) const MASKS: &str = "sailry.context.masked_outputs";
pub(crate) const PROJECTION: &str = "sailry_context_projection";

pub(in crate::agent) struct Context {
    ingress: Arc<Ingress>,
    service: Arc<dyn SessionService>,
    session: sailry_protocol::SessionId,
    summarizer: Summarizer,
    applied: AtomicBool,
    rebuilt: AtomicBool,
    restored: AtomicBool,
    retention: AtomicUsize,
    state: Arc<State>,
    checkpoint: Mutex<Option<Arc<Checkpoint>>>,
    candidate: Mutex<Option<Candidate>>,
    gate: tokio::sync::Mutex<()>,
}

impl Context {
    pub(super) fn new(
        ingress: Arc<Ingress>,
        service: Arc<dyn SessionService>,
        session: sailry_protocol::SessionId,
        summarizer: Summarizer,
    ) -> Self {
        let state = ingress.agents.compaction(summarizer.turn);
        Self {
            ingress,
            service,
            session,
            summarizer,
            applied: AtomicBool::new(false),
            rebuilt: AtomicBool::new(false),
            restored: AtomicBool::new(false),
            retention: AtomicUsize::new(1),
            state,
            checkpoint: Mutex::new(None),
            candidate: Mutex::new(None),
            gate: Default::default(),
        }
    }

    pub(in crate::agent) fn model(self: &Arc<Self>, model: Arc<dyn Llm>) -> Arc<dyn Llm> {
        recovery::wrap(self.clone(), model)
    }

    fn preamble(&self) -> &Content {
        self.summarizer
            .preamble
            .as_ref()
            .expect("active context captures its instructions")
    }

    fn window(&self) -> Option<model::history::Window> {
        Some(model::history::Window {
            context: self.summarizer.context?,
            output: self.summarizer.output.unwrap_or(4096).max(0) as u64,
        })
    }

    async fn summarize(
        &self,
        events: &[Event],
        automatic: bool,
        tools: &std::collections::HashMap<String, serde_json::Value>,
    ) -> adk_core::Result<Option<Event>> {
        let mut summarizer = self.summarizer.clone();
        summarizer.tools = Some(tools.clone());
        summarizer.masks = Some(self.state.masked()?);
        summarizer
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

    /// A provider observation is authoritative for the content it measured; only
    /// later canonical content and pending input are estimated. Without one, the
    /// projected request is estimated.
    async fn tokens(&self, request: &LlmRequest, pending: &[Content]) -> adk_core::Result<u64> {
        Ok(match self.usage().await?.filter(|usage| usage.observed) {
            Some(usage) => Self::projected(&usage).saturating_add(
                pending
                    .iter()
                    .map(|content| 4 + content.parts.iter().map(estimate::part).sum::<u64>())
                    .sum::<u64>(),
            ),
            None => estimate::request(request)?,
        })
    }

    fn mask(
        &self,
        request: &mut LlmRequest,
        protect: u64,
        minimum: u64,
        revisions: &model::history::Revisions,
    ) -> adk_core::Result<u64> {
        let saved = model::history::mask(request, protect, minimum, revisions)?;
        if saved > 0 {
            self.state.mask(model::history::masked(request))?;
        }
        Ok(saved)
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
        invocation: &str,
        pending: bool,
    ) -> adk_core::Result<Option<usize>> {
        self.summarizer.prefix(
            &history.events,
            self.candidates(history, self.retention.load(Ordering::Relaxed))
                .into_iter()
                .filter(|cut| {
                    pending
                        || tail(
                            request,
                            &history.events[*cut..],
                            self.preamble(),
                            invocation,
                        )
                        .is_some()
                }),
            self.summarizer.recent(),
        )
    }

    fn rebuild(
        &self,
        request: &mut LlmRequest,
        history: &History,
        invocation: &str,
        pending: &[Content],
    ) -> adk_core::Result<()> {
        *self
            .checkpoint
            .lock()
            .map_err(|_| invalid("context checkpoint is unavailable"))? = None;
        match matching::rebuild(request, &history.events, self.preamble(), invocation) {
            Ok(()) => {
                request.contents.extend_from_slice(pending);
                Ok(())
            }
            // A changed preamble keeps the supplied request uncompacted; the
            // saved summary still applies to a fresh runner.
            Err(error)
                if failure::classify(&error) == (Stage::History, Reason::HistoryMismatch) =>
            {
                self.summarizer
                    .failure(SummaryMode::Automatic, "rebuild", &error);
                Ok(())
            }
            Err(error) => Err(error),
        }
    }

    fn saved(
        &self,
        request: &mut LlmRequest,
        history: &[Event],
        summary: &Event,
        offset: usize,
        invocation: &str,
    ) -> adk_core::Result<()> {
        let index = tail(request, history, self.preamble(), invocation).ok_or_else(|| {
            invalid_at(
                Stage::History,
                Reason::HistoryMismatch,
                "model context does not match the persisted summary",
            )
        })?;
        let content = summary
            .actions
            .compaction
            .as_ref()
            .map(|value| &value.compacted_content)
            .or_else(|| summary.content())
            .ok_or_else(|| {
                invalid_at(
                    Stage::History,
                    Reason::HistoryMismatch,
                    "persisted context summary is missing",
                )
            })?;
        let checkpoint = Checkpoint::new(request, index, content.clone(), offset);
        request.contents.splice(1..index, [content.clone()]);
        request.previous_response_id = None;
        if !self.rebuilt.load(Ordering::Relaxed) {
            *self
                .checkpoint
                .lock()
                .map_err(|_| invalid("context checkpoint is unavailable"))? =
                Some(Arc::new(checkpoint));
        }
        self.applied.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn reconcile(
        &self,
        request: &mut LlmRequest,
        invocation: &str,
    ) -> adk_core::Result<usize> {
        if self.rebuilt.load(Ordering::Relaxed) {
            self.rebuild(request, &self.history().await?, invocation, &[])?;
            return Ok(0);
        }
        let checkpoint = self
            .checkpoint
            .lock()
            .map_err(|_| invalid("context checkpoint is unavailable"))?
            .clone();
        if let Some(checkpoint) = checkpoint {
            if checkpoint.apply(request, self.preamble()).is_ok() {
                return Ok(checkpoint.offset());
            }
            // A stale live checkpoint is disposable. Rebuild through the same
            // canonical SessionService window that a fresh Runner receives.
            self.rebuilt.store(true, Ordering::Relaxed);
            self.rebuild(request, &self.history().await?, invocation, &[])?;
        } else if self.applied.load(Ordering::Relaxed) {
            let history = self.history().await?;
            let summary = history.events.first().filter(|event| {
                event
                    .provider_metadata
                    .contains_key(crate::store::agent::compaction::END_EVENT)
            });
            if let Some(summary) = summary {
                if self
                    .saved(request, &history.events[1..], summary, 0, invocation)
                    .is_ok()
                {
                    return Ok(self
                        .checkpoint
                        .lock()
                        .map_err(|_| invalid("context checkpoint is unavailable"))?
                        .as_ref()
                        .expect("saved summary has a checkpoint")
                        .offset());
                }
            }
            self.rebuilt.store(true, Ordering::Relaxed);
            self.rebuild(request, &history, invocation, &[])?;
        }
        Ok(0)
    }

    fn discard_candidate(&self) -> adk_core::Result<()> {
        self.candidate
            .lock()
            .map_err(|_| invalid("prepared context is unavailable"))?
            .take();
        Ok(())
    }

    fn prepare_background(
        &self,
        history: &History,
        tokens: u64,
        tools: &std::collections::HashMap<String, serde_json::Value>,
    ) -> adk_core::Result<()> {
        let mut candidate = self
            .candidate
            .lock()
            .map_err(|_| invalid("prepared context is unavailable"))?;
        if candidate
            .as_ref()
            .is_some_and(|candidate| !candidate.matches(history, self.preamble()))
        {
            candidate.take();
        }
        if candidate.is_some() {
            return Ok(());
        }
        // Only an attempted foreground or applied summary advances the growth gate.
        if !self.state.allows(tokens, false)? {
            return Ok(());
        }
        let Some(cut) = self.summarizer.prefix(
            &history.events,
            self.candidates(history, self.retention.load(Ordering::Relaxed))
                .into_iter(),
            self.summarizer.recent(),
        )?
        else {
            return Ok(());
        };
        let mut summarizer = self.summarizer.clone();
        summarizer.tools = Some(tools.clone());
        summarizer.masks = Some(self.state.masked()?);
        *candidate = Some(Candidate::start(
            history.events[..cut].to_vec(),
            self.preamble().clone(),
            summarizer,
        ));
        Ok(())
    }

    async fn prepared(
        &self,
        history: &History,
    ) -> adk_core::Result<Option<(usize, Option<Event>)>> {
        let candidate = {
            let mut candidate = self
                .candidate
                .lock()
                .map_err(|_| invalid("prepared context is unavailable"))?;
            if candidate
                .as_ref()
                .is_some_and(|candidate| !candidate.matches(history, self.preamble()))
            {
                candidate.take();
            }
            candidate.take()
        };
        match candidate {
            Some(candidate) => candidate.finish().await.map(Some),
            None => Ok(None),
        }
    }

    async fn restore_masks(&self) -> adk_core::Result<()> {
        if self.restored.load(Ordering::Relaxed) {
            return Ok(());
        }
        let session = self
            .service
            .get(adk_session::GetRequest {
                app_name: APP.into(),
                user_id: USER.into(),
                session_id: self.session.to_string(),
                num_recent_events: Some(0),
                after: None,
            })
            .await?;
        if let Some(value) = session.state().get(MASKS) {
            self.state.mask(
                serde_json::from_value(value)
                    .map_err(|_| invalid("saved output masks are invalid"))?,
            )?;
        }
        self.restored.store(true, Ordering::Relaxed);
        Ok(())
    }

    async fn invalidate(&self, request: &LlmRequest, pending: &[Content]) -> adk_core::Result<()> {
        self.state.mask(model::history::masked(request))?;
        let mut canonical = request.clone();
        canonical
            .contents
            .truncate(canonical.contents.len().saturating_sub(pending.len()));
        let tokens = estimate::request(&canonical)?;
        let mut event = Event::new(self.summarizer.turn.to_string());
        event.author = "system".into();
        event
            .actions
            .state_delta
            .insert(MASKS.into(), json!(self.state.masked()?));
        event
            .provider_metadata
            .insert(PROJECTION.into(), json!({"tokens":tokens}).to_string());
        self.service
            .append_event(&self.session.to_string(), event)
            .await
    }

    async fn apply(
        &self,
        request: &mut LlmRequest,
        history: &History,
        cut: usize,
        summary: Event,
        offset: usize,
        invocation: &str,
        pending: &[Content],
    ) -> adk_core::Result<()> {
        let current = self.history().await?;
        if current.events.len() < cut
            || !history.events[..cut]
                .iter()
                .zip(&current.events)
                .all(|(captured, current)| captured.id == current.id)
        {
            return Err(invalid_at(
                Stage::History,
                Reason::HistoryMismatch,
                "prepared context no longer matches canonical history",
            ));
        }
        if pending.is_empty()
            && tail(request, &history.events[cut..], self.preamble(), invocation).is_none()
        {
            return Err(invalid_at(
                Stage::History,
                Reason::HistoryMismatch,
                "model context does not match the prepared summary",
            ));
        }
        self.service
            .append_event(&self.session.to_string(), summary.clone())
            .await
            .map_err(save_error)?;
        // The canonical read path rehydrates owner context for both live
        // application and a fresh Runner. Persisted handoffs contain no copies.
        let materialized = self.history().await?;
        let summary = materialized
            .events
            .first()
            .filter(|event| event.id == summary.id)
            .ok_or_else(|| {
                invalid_at(
                    Stage::History,
                    Reason::HistoryMismatch,
                    "saved summary is no longer the active context boundary",
                )
            })?;
        self.applied.store(true, Ordering::Relaxed);
        if !pending.is_empty() {
            self.rebuild(request, &materialized, invocation, pending)?;
        } else if self
            .saved(request, &history.events[cut..], summary, offset, invocation)
            .is_err()
        {
            self.rebuilt.store(true, Ordering::Relaxed);
            self.rebuild(request, &materialized, invocation, pending)?;
        }
        Ok(())
    }

    async fn transform(
        &self,
        invocation: &str,
        mut request: LlmRequest,
        pending: &[Content],
        forced: bool,
    ) -> adk_core::Result<LlmRequest> {
        let _boundary = self.gate.lock().await;
        if self.summarizer.stop.is_cancelled() {
            return Err(invalid("context compaction was cancelled"));
        }
        let forced_history = if forced {
            let history = self.history().await?;
            // The provider has already projected this request. Recreate the
            // canonical ADK view before matching boundaries or checkpoints.
            matching::rebuild(&mut request, &history.events, self.preamble(), invocation)?;
            *self
                .checkpoint
                .lock()
                .map_err(|_| invalid("context checkpoint is unavailable"))? = None;
            self.rebuilt.store(true, Ordering::Relaxed);
            Some(history)
        } else {
            None
        };
        let offset = if forced {
            0
        } else if pending.is_empty() {
            self.reconcile(&mut request, invocation).await?
        } else {
            0
        };
        self.restore_masks().await?;
        let masks = self.state.masked()?;
        let mut projected = request.clone();
        // Raw ADK results identify their canonical revision by their own hash.
        // Saved masks alone do not require reading the full canonical history.
        model::history::remask(&mut projected, &masks, &Default::default());
        let tokens = self.tokens(&projected, pending).await?;
        if !forced
            && self
                .summarizer
                .budget()
                .is_none_or(|budget| tokens < budget * PREPARE_PERCENT / 100)
        {
            // Ordinary requests already have ADK's history. Canonical boundaries
            // and result revisions are needed only for a changed projection.
            if matches!(
                model::history::limit(&mut projected, self.window(), &Default::default()),
                Ok(None)
            ) {
                return Ok(projected);
            }
            projected = request.clone();
        }
        let history = match forced_history {
            Some(history) => history,
            None => self.history().await?,
        };
        let revisions = model::history::revisions(&history.events);
        // Summary boundaries match the unmasked request; masks are reapplied last.
        model::history::remask(&mut projected, &masks, &revisions);
        let above = |tokens| self.summarizer.needs_compaction(Some(tokens));
        if !forced && !above(tokens) {
            if self
                .summarizer
                .budget()
                .is_some_and(|budget| tokens >= budget * PREPARE_PERCENT / 100)
            {
                self.prepare_background(&history, tokens, &request.tools)?;
            }
            let projection = model::history::limit(&mut projected, self.window(), &revisions)?;
            if self.state.project(projection)? {
                self.invalidate(&projected, pending).await?;
            }
            return Ok(projected);
        }
        // Compare with the normal request before adding masks. Ordinary output
        // formatting alone must not count as recovery from a failed summary.
        let mut baseline = projected.clone();
        if let Err(error) = model::history::limit(&mut baseline, self.window(), &revisions)
            && !model::context_overflow(&error)
        {
            return Err(error);
        }
        let initial = estimate::request(&baseline)?;
        drop(baseline);
        let mut saved = 0;
        if !forced {
            // Masking older results usually suffices; a prepared summary stays
            // available for a later boundary.
            saved = self.mask(&mut projected, PROTECTED_OUTPUT, MINIMUM_SAVING, &revisions)?;
            if saved > 0 && !above(tokens.saturating_sub(saved)) {
                let projection = model::history::limit(&mut projected, self.window(), &revisions)?;
                self.state.project(projection)?;
                self.invalidate(&projected, pending).await?;
                return Ok(projected);
            }
        }
        drop(projected);
        let prepared = match self.prepared(&history).await {
            Ok(Some(prepared)) => Ok(Some(prepared)),
            Ok(None) => match self.cut(&history, &request, invocation, !pending.is_empty())? {
                Some(cut) if self.state.allows(tokens, forced)? => self
                    .summarize(&history.events[..cut], true, &request.tools)
                    .await
                    .map(|summary| Some((cut, summary))),
                // A gated attempt or no eligible completed prefix is not a
                // failed summary; immutable input still has to fit the window.
                _ => Ok(None),
            },
            Err(error) => Err(error),
        };
        let mut applied = false;
        let result = match prepared {
            Ok(Some((cut, Some(summary)))) => {
                let result = self
                    .apply(
                        &mut request,
                        &history,
                        cut,
                        summary,
                        offset,
                        invocation,
                        pending,
                    )
                    .await;
                applied = result.is_ok();
                result
            }
            Ok(_) => Ok(()),
            Err(error) => Err(error),
        };
        let failure = match result {
            Err(error)
                if failure::classify(&error) == (Stage::History, Reason::HistoryMismatch) =>
            {
                self.rebuild(&mut request, &self.history().await?, invocation, pending)?;
                if pending.is_empty() {
                    self.rebuilt.store(true, Ordering::Relaxed);
                }
                None
            }
            Err(error) if failure::projectable(&error) || failure::transient(&error) => Some(error),
            Err(error) => return Err(error),
            Ok(()) => None,
        };
        model::history::remask(&mut request, &self.state.masked()?, &revisions);
        saved += self.mask(&mut request, PROTECTED_OUTPUT, MINIMUM_SAVING, &revisions)?;
        if applied && above(estimate::request(&request)?) {
            // Retained results that keep the new window above the trigger are
            // masked now rather than summarized again at the next step.
            saved += self.mask(&mut request, 0, 0, &revisions)?;
        }
        let projection = model::history::limit(&mut request, self.window(), &revisions)?;
        let changed = self.state.project(projection)?;
        let after = estimate::request(&request)?;
        let failure = match failure {
            // Exhausted transient retries continue with the retained context.
            Some(error) if !failure::transient(&error) && after >= initial => return Err(error),
            failure => failure,
        };
        if applied {
            self.state.record(after)?;
        } else if failure.is_some() {
            self.state.record(tokens)?;
        }
        if applied || saved > 0 || changed {
            self.invalidate(&request, pending).await?;
        }
        if let Some(error) = failure {
            self.notice(&error, SummaryMode::Automatic, true).await?;
        }
        Ok(request)
    }

    pub(in crate::agent) async fn preflight(
        &self,
        pending: &[Content],
        tools: std::collections::HashMap<String, serde_json::Value>,
    ) -> adk_core::Result<()> {
        let history = self.history().await?;
        let mut request =
            LlmRequest::new(self.summarizer.model.name(), vec![self.preamble().clone()]);
        request.contents.extend(
            history
                .events
                .iter()
                .filter_map(|event| super::history::project(event, "")),
        );
        request.contents.extend_from_slice(pending);
        request.tools = tools;
        let result = self
            .transform("", request, pending, false)
            .await
            .map(|_| ())
            .map_err(|error| {
                if model::context_overflow(&error) {
                    request_error(error)
                } else {
                    error
                }
            });
        if let Err(error) = &result
            && !self.summarizer.stop.is_cancelled()
        {
            self.halt(error, SummaryMode::Automatic).await?;
        }
        result
    }

    pub(super) async fn before(
        &self,
        invocation: &str,
        request: LlmRequest,
    ) -> adk_core::Result<BeforeModelResult> {
        let result = self
            .transform(invocation, request, &[], false)
            .await
            .map_err(|error| {
                if model::context_overflow(&error) {
                    request_error(error)
                } else {
                    error
                }
            });
        if let Err(error) = &result
            && !self.summarizer.stop.is_cancelled()
        {
            self.halt(error, SummaryMode::Automatic).await?;
        }
        result.map(BeforeModelResult::Continue)
    }

    async fn halt(&self, error: &adk_core::AdkError, mode: SummaryMode) -> adk_core::Result<()> {
        self.discard_candidate()?;
        self.ingress
            .agents
            .halt(self.summarizer.turn, model::error(error));
        self.notice(error, mode, false).await
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
