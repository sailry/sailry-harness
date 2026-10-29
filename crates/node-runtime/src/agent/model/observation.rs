//! Aggregate adapted payloads in request scope; never retain the logger's input.
use goose_provider_types::request_log::{self, RequestLogHandle, RequestLogger};
use sailry_protocol::conversation::observation::{Category, CategoryKind};
use serde::Deserialize;
use std::{
    future::Future,
    sync::{Arc, Mutex, Once},
    time::Instant,
};

mod categories;
mod context;
pub(in crate::agent) use context::Context;
#[cfg(test)]
mod tests;

type LogError = Box<dyn std::error::Error + Send + Sync>;

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
    context: Option<Context>,
}

impl Sources {
    pub(in crate::agent) fn replace(&self, sources: Vec<Source>) {
        self.replace_with_context(sources, None);
    }

    pub(in crate::agent) fn replace_with_context(
        &self,
        sources: Vec<Source>,
        context: Option<Context>,
    ) {
        let mut unique = std::collections::BTreeMap::new();
        for source in sources {
            if !source.text.is_empty() {
                unique.insert((source.kind, source.text.clone()), source);
            }
        }
        *self.0.lock().unwrap_or_else(|error| error.into_inner()) = Some(Inventory {
            system: unique.into_values().collect(),
            context,
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
            categories: categories::estimate(&serde_json::Value::Null, None, None),
            complete: false,
            started,
            http: None,
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
    pub(super) async fn scope<F: Future>(&self, future: F) -> F::Output {
        CURRENT.scope(self.clone(), future).await
    }

    pub(super) fn snapshot(&self) -> (Vec<Category>, bool, Option<Http>) {
        let state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        (state.categories.clone(), state.complete, state.http)
    }

    fn prepared(&self, body: &serde_json::Value) {
        let mut state = self.0.lock().unwrap_or_else(|error| error.into_inner());
        // A later prepared request must not inherit a previous retry's receipt.
        state.http = None;
        let sources = if categories::retains_system(body, state.system) {
            state.sources.as_ref()
        } else {
            None
        };
        let categories = categories::estimate(
            body,
            sources.map(|inventory| inventory.system.as_slice()),
            sources.and_then(|inventory| inventory.context.as_ref()),
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

pub(super) fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        if request_log::install_logger(Logger).is_err() {
            // The GDK hook cannot be replaced. Another owner's logger stays intact.
            tracing::warn!("GDK request payload observation is unavailable: a request logger is already installed");
        }
    });
}

/// Package preparation has already selected its wire body, before protected auth.
pub(super) fn prepared(body: Option<&str>) {
    let _ = CURRENT.try_with(|payload| {
        let body = body.and_then(|body| serde_json::from_str(body).ok());
        payload.prepared(body.as_ref().unwrap_or(&serde_json::Value::Null));
    });
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

struct Logger;
struct Handle {
    payload: Option<Payload>,
}

impl RequestLogger for Logger {
    fn start(&self) -> Result<Box<dyn RequestLogHandle>, LogError> {
        Ok(Box::new(Handle {
            payload: CURRENT.try_with(Clone::clone).ok(),
        }))
    }
}

impl RequestLogHandle for Handle {
    fn write(&mut self, input: &str) -> Result<(), LogError> {
        let Some(payload) = self.payload.take() else {
            return Ok(());
        };
        // GDK also serializes config (including possible auth headers) in this
        // envelope. Deserialize only input, then discard it after aggregation.
        #[derive(Deserialize)]
        struct Prepared {
            input: serde_json::Value,
        }
        if let Ok(body) = serde_json::from_str::<Prepared>(input) {
            payload.prepared(&body.input);
        }
        // Later writes contain response content or errors, never request input.
        // Observation must not fail generation even if an input is unrecognized.
        Ok(())
    }
}
