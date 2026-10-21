//! Typed embedding work uses the Node's shared model scheduler and receipts.
pub(super) use crate::models::runtime::Lane;
use crate::models::{
    registry::Registry,
    runtime::{Engine, embedding_error},
};
use sailry_link::CancellationToken;
use sailry_protocol::{
    Fault,
    models::{Choice, Kind},
};
use std::sync::Arc;

const BATCH: usize = 4;

pub(crate) struct Models {
    pub(crate) registry: Arc<Registry>,
    #[cfg(any(test, feature = "test-support"))]
    indexed: std::sync::atomic::AtomicU64,
}

impl Models {
    pub(crate) fn new(registry: Arc<Registry>) -> Arc<Self> {
        Arc::new(Self {
            registry,
            #[cfg(any(test, feature = "test-support"))]
            indexed: std::sync::atomic::AtomicU64::new(0),
        })
    }

    pub(super) fn binding(&self, stored: &super::Stored, choice: &Choice) -> super::index::Binding {
        let model = stored
            .collection
            .definition
            .model
            .as_deref()
            .or_else(|| choice.enabled.then_some(choice.model.as_deref()).flatten())
            .and_then(|id| {
                let definition = self.registry.definition(id).ok()?;
                Some((id.to_owned(), usize::from(definition.format.dimension()?)))
            });
        super::binding(stored, model)
    }

    pub(super) async fn encode(
        &self,
        model: &str,
        choice: &Choice,
        texts: Vec<String>,
        purpose: sailry_embedding::Input,
        lane: Lane,
        cancel: &CancellationToken,
    ) -> Result<Vec<Vec<f32>>, Fault> {
        if texts.len() > BATCH {
            return Err(Fault::invalid("embedding batch exceeds four inputs"));
        }
        let resource = self
            .registry
            .resource(Kind::Embedding, Some(model), Some(choice))?;
        let vectors = self
            .registry
            .execute(resource, lane, cancel, move |engine, cancel| {
                let Engine::Embedding(engine) = engine else {
                    return Err(Fault::invalid("embedding engine required"));
                };
                if cancel.is_cancelled() {
                    return Err(crate::models::registry::cancelled());
                }
                engine
                    .encode_for(
                        purpose,
                        &texts.iter().map(String::as_str).collect::<Vec<_>>(),
                    )
                    .map_err(embedding_error)
            })
            .await?;
        #[cfg(any(test, feature = "test-support"))]
        if lane == Lane::Background {
            self.indexed
                .fetch_add(vectors.len() as u64, std::sync::atomic::Ordering::Relaxed);
        }
        Ok(vectors)
    }

    pub(super) async fn offsets(
        &self,
        model: &str,
        choice: &Choice,
        text: String,
        cancel: &CancellationToken,
    ) -> Result<Vec<(usize, usize)>, Fault> {
        let resource = self
            .registry
            .resource(Kind::Embedding, Some(model), Some(choice))?;
        self.registry
            .execute(resource, Lane::Background, cancel, move |engine, _| {
                let Engine::Embedding(engine) = engine else {
                    return Err(Fault::invalid("embedding engine required"));
                };
                engine.offsets(&text).map_err(embedding_error)
            })
            .await
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn indexed_inputs(&self) -> u64 {
        self.indexed.load(std::sync::atomic::Ordering::Relaxed)
    }
}
