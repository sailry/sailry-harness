//! Aggregate adapted payloads in request scope; never retain the logger's input.
use sailry_protocol::conversation::observation::{Category, CategoryKind};
use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::Instant,
};

mod categories;
mod context;
mod fingerprints;
pub(in crate::agent) use context::Context;
#[cfg(test)]
mod tests;

#[derive(Clone)]
pub(in crate::agent) struct Source {
    pub kind: CategoryKind,
    pub text: String,
}

/// This belongs to a turn's wrapper, never the cached provider or HTTP client.
#[derive(Clone, Default)]
pub(in crate::agent) struct Sources(Arc<Mutex<Option<Inventory>>>);

#[derive(Clone)]
struct Inventory {
    system: Vec<Source>,
    contexts: Vec<Context>,
}

impl Sources {
    pub(in crate::agent) fn replace(&self, sources: Vec<Source>) {
        self.replace_with_contexts(sources, Vec::new());
    }

    pub(in crate::agent) fn replace_with_contexts(
        &self,
        sources: Vec<Source>,
        contexts: Vec<Context>,
    ) {
        let mut unique = std::collections::BTreeMap::new();
        for source in sources {
            if !source.text.is_empty() {
                unique.insert((source.kind, source.text.clone()), source);
            }
        }
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = Some(Inventory {
            system: unique.into_values().collect(),
            contexts,
        });
    }

    pub(super) fn capture(&self, system: &str, started: Instant) -> Payload {
        Payload(Arc::new(Mutex::new(State {
            sources: self
                .0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .clone(),
            system: blake3::hash(system.as_bytes()),
            categories: categories::estimate(&serde_json::Value::Null, None, &[]),
            complete: false,
            started,
            http: None,
            first_output_us: None,
            diagnostic: None,
        })))
    }
}

#[derive(Clone)]
pub(super) struct Payload(Arc<Mutex<State>>);

struct State {
    sources: Option<Inventory>,
    system: blake3::Hash,
    categories: Vec<Category>,
    complete: bool,
    started: Instant,
    http: Option<Http>,
    /// The first decoded output delta, before any block buffering.
    first_output_us: Option<u64>,
    diagnostic: Option<(crate::logging::Context, u64)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Http {
    pub status: u16,
    pub headers_us: u64,
}

tokio::task_local! {
    static CURRENT: Payload;
}

impl Payload {
    pub(super) fn with_identity(self, identity: crate::logging::Context) -> Self {
        if std::env::var_os("SAILRY_DEBUG_REQUEST_FINGERPRINTS").is_some_and(|value| value == "1") {
            self.0
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .diagnostic = Some((identity, 0));
        }
        self
    }

    pub(super) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub(super) fn snapshot(&self) -> (Vec<Category>, bool, Option<Http>) {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        (state.categories.clone(), state.complete, state.http)
    }

    /// The earliest raw output delta of the latest prepared request, if a
    /// Sailry transport observed one.
    pub(super) fn first_output(&self) -> Option<u64> {
        self.0
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .first_output_us
    }

    /// Records raw output that arrived at `at`, before the stream buffers it
    /// into complete reasoning or tool-call blocks.
    pub(super) fn output(&self, at: Instant) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if state.first_output_us.is_none() {
            state.first_output_us = Some(
                u64::try_from(at.saturating_duration_since(state.started).as_micros())
                    .unwrap_or(u64::MAX),
            );
        }
    }

    fn prepared(&self, body: &serde_json::Value) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        if let Some((identity, attempt)) = &mut state.diagnostic {
            *attempt = attempt.saturating_add(1);
            fingerprints::record(body, identity, *attempt);
        }
        // A later prepared request must not inherit a previous retry's receipt.
        state.http = None;
        state.first_output_us = None;
        let sources = if categories::retains_system(body, state.system) {
            state.sources.as_ref()
        } else {
            None
        };
        let categories = categories::estimate(
            body,
            sources.map(|inventory| inventory.system.as_slice()),
            sources.map_or(&[], |inventory| inventory.contexts.as_slice()),
        );
        state.complete = categories.iter().any(|category| category.tokens.is_some())
            && body
                .get("previous_response_id")
                .is_none_or(serde_json::Value::is_null)
            && body
                .get("previous_interaction_id")
                .is_none_or(serde_json::Value::is_null);
        state.categories = categories;
    }
}

/// Package preparation has already selected its wire body, before protected auth.
pub(super) fn prepared(body: Option<&str>) {
    let _ = CURRENT.try_with(|payload| {
        let body = body.and_then(|body| serde_json::from_str(body).ok());
        payload.prepared(body.as_ref().unwrap_or(&serde_json::Value::Null));
    });
}

/// The request scope, for transport work that continues on a spawned task.
pub(super) fn current() -> Option<Payload> {
    CURRENT.try_with(Clone::clone).ok()
}

/// Only Sailry's package transport exposes actual response headers here.
pub(super) fn headers(status: u16) {
    let _ = CURRENT.try_with(|payload| {
        let mut state = payload.0.lock().unwrap_or_else(|error| error.into_inner());
        state.http = Some(Http {
            status,
            headers_us: u64::try_from(state.started.elapsed().as_micros()).unwrap_or(u64::MAX),
        });
    });
}
