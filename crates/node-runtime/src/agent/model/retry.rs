use super::types::{Error, ErrorKind, Event, Model, ModelStream, Request};
use futures::StreamExt;
use std::{collections::VecDeque, sync::Arc, time::Duration};
use tokio_util::sync::{CancellationToken, DropGuard};

pub(super) struct Policy {
    pub retries: usize,
    pub delay: Duration,
    pub buffered: bool,
}

impl Policy {
    pub(super) fn new(retries: usize, delay: Duration, buffered: bool) -> Self {
        #[cfg(any(test, feature = "test-support"))]
        let delay =
            if std::env::var_os("SAILRY_MODEL_SKIP_BACKOFF").is_some_and(|value| value == "true") {
                Duration::ZERO
            } else {
                delay
            };
        Self {
            retries,
            delay,
            buffered,
        }
    }
}

pub(super) fn stream(
    model: Arc<dyn Model>,
    request: Request,
    policy: Policy,
    cancel: CancellationToken,
    stop: CancellationToken,
) -> ModelStream {
    let cancel = cancel.child_token();
    let state = State {
        model,
        request,
        policy,
        _guard: cancel.clone().drop_guard(),
        cancel,
        stop,
        stream: None,
        pending: VecDeque::new(),
        attempt: 0,
        emitted: false,
        done: false,
    };
    Box::pin(futures::stream::unfold(state, |mut state| async move {
        loop {
            if let Some(event) = state.pending.pop_front() {
                state.emitted = true;
                return Some((event, state));
            }
            if state.done {
                return None;
            }
            if state.stream.is_none() {
                let result = tokio::select! {
                    biased;
                    _ = state.cancel.cancelled() => Err(cancelled()),
                    _ = state.stop.cancelled() => Err(cancelled()),
                    result = state.model.call(state.request.clone(), state.cancel.clone()) => result,
                };
                match result {
                    Ok(stream) => state.stream = Some(stream),
                    Err(error) => {
                        if state.retry(&error).await {
                            continue;
                        }
                        state.done = true;
                        let error = state.failure(error);
                        return Some((Err(error), state));
                    }
                }
            }
            let mut buffered = VecDeque::new();
            loop {
                let stream = state.stream.as_mut().expect("active model attempt");
                let next = tokio::select! {
                    biased;
                    _ = state.cancel.cancelled() => Some(Err(cancelled())),
                    _ = state.stop.cancelled() => Some(Err(cancelled())),
                    next = stream.next() => next,
                };
                match next {
                    Some(Ok(event)) if state.policy.buffered => buffered.push_back(Ok(event)),
                    Some(Ok(event)) => {
                        state.emitted = true;
                        return Some((Ok(event), state));
                    }
                    Some(Err(error)) => {
                        state.stream = None;
                        if state.retry(&error).await {
                            break;
                        }
                        state.done = true;
                        let error = state.failure(error);
                        return Some((Err(error), state));
                    }
                    None => {
                        state.stream = None;
                        state.pending = buffered;
                        state.done = true;
                        break;
                    }
                }
            }
        }
    }))
}

struct State {
    model: Arc<dyn Model>,
    request: Request,
    policy: Policy,
    _guard: DropGuard,
    cancel: CancellationToken,
    stop: CancellationToken,
    stream: Option<ModelStream>,
    pending: VecDeque<Result<Event, Error>>,
    attempt: usize,
    emitted: bool,
    done: bool,
}

impl State {
    async fn retry(&mut self, error: &Error) -> bool {
        if self.emitted
            || !error.retryable()
            || self.attempt >= self.policy.retries
            || self.cancel.is_cancelled()
            || self.stop.is_cancelled()
        {
            return false;
        }
        self.attempt += 1;
        let delay = self.policy.delay;
        tokio::select! {
            biased;
            _ = self.cancel.cancelled() => false,
            _ = self.stop.cancelled() => false,
            _ = tokio::time::sleep(delay) => true,
        }
    }

    fn failure(&self, error: Error) -> Error {
        if self.cancel.is_cancelled() || self.stop.is_cancelled() {
            self.cancel.cancel();
            cancelled()
        } else {
            error
        }
    }
}

fn cancelled() -> Error {
    Error::new(ErrorKind::Cancelled, "model request was cancelled")
}

#[cfg(test)]
mod tests;
