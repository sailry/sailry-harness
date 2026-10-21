//! Node-owned local retrieval; configuration and grants remain in the primary store.
mod chunks;
mod citations;
mod index;
pub(crate) mod models;
mod rank;
pub(crate) mod source;
mod storage;
pub(crate) mod text;
mod vector;
mod worker;

use crate::store::{Ingress, knowledge::Stored};
use sailry_link::CancellationToken;
use sailry_protocol::{ErrorCode, Fault, Request, knowledge::*};
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak, mpsc},
};

pub(crate) struct Service {
    pub(crate) models: Arc<models::Models>,
    path: PathBuf,
    ingress: Weak<Ingress>,
    wake: mpsc::SyncSender<()>,
    cancel: Mutex<CancellationToken>,
    stop: CancellationToken,
    worker: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl Service {
    pub(crate) async fn open(
        profile: &Path,
        registry: Arc<crate::models::registry::Registry>,
        ingress: Weak<Ingress>,
        stop: CancellationToken,
    ) -> Result<Arc<Self>, Fault> {
        let models = models::Models::new(registry);
        let path = profile.join("storage/knowledge.db");
        let target = path.clone();
        let index = tokio::task::spawn_blocking(move || {
            let parent = target.parent().expect("knowledge cache has a parent");
            let directory = crate::files::path::root(parent)?;
            match directory.symlink_metadata("knowledge.db") {
                Ok(metadata) if metadata.is_symlink() || !metadata.is_file() => {
                    return Err(Fault::invalid("knowledge cache must be a regular file"));
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(crate::files::io_error(error)),
                Ok(_) => {}
            }
            index::Index::open(&target)
        })
        .await
        .map_err(|_| unavailable())??;
        let (wake, receiver) = mpsc::sync_channel(1);
        let owner = Arc::new(Self {
            models,
            path,
            ingress,
            wake,
            cancel: Mutex::new(stop.child_token()),
            stop,
            worker: Mutex::new(None),
        });
        let handle = tokio::runtime::Handle::current();
        let weak = Arc::downgrade(&owner);
        let thread = std::thread::Builder::new()
            .name("sailry-knowledge".into())
            .spawn(move || worker::run(weak, receiver, index, handle))
            .map_err(|error| Fault::new(ErrorCode::Internal, error.to_string()))?;
        *owner
            .worker
            .lock()
            .expect("knowledge worker lock is healthy") = Some(thread);
        owner.refresh();
        Ok(owner)
    }

    pub(crate) fn refresh(&self) {
        let _ = self.wake.try_send(());
    }

    pub(crate) fn invalidate(&self) {
        let mut token = self
            .cancel
            .lock()
            .expect("knowledge cancellation lock is healthy");
        token.cancel();
        *token = self.stop.child_token();
        self.refresh();
    }

    pub(crate) async fn query(
        &self,
        request: &Request,
        query: &Query,
        cancel: &CancellationToken,
    ) -> Result<Page, Fault> {
        let prepared = self.prepare(request, query, cancel).await?;
        let Prepared {
            bindings,
            embeddings,
            indexing,
        } = prepared;
        let path = self.path.clone();
        let query = query.clone();
        let cancel = cancel.clone();
        tokio::task::spawn_blocking(move || {
            index::Index::reader(&path)?.search(&query, &bindings, &embeddings, indexing, &cancel)
        })
        .await
        .map_err(|_| unavailable())?
    }

    async fn prepare(
        &self,
        request: &Request,
        query: &Query,
        cancel: &CancellationToken,
    ) -> Result<Prepared, Fault> {
        query.validate()?;
        let ingress = self.ingress.upgrade().ok_or_else(unavailable)?;
        let allowed = ingress.knowledge_allowed(request).await?;
        let choice = self
            .models
            .registry
            .settings()
            .choice(sailry_protocol::models::Kind::Embedding);
        let bindings = allowed
            .iter()
            .filter(|stored| {
                query.collections.is_empty() || query.collections.contains(&stored.collection.id)
            })
            .map(|stored| self.models.binding(stored, &choice))
            .collect::<Vec<_>>();
        let indexing = allowed
            .iter()
            .filter(|stored| {
                query.collections.is_empty() || query.collections.contains(&stored.collection.id)
            })
            .any(|stored| {
                matches!(
                    stored.collection.progress.status,
                    Status::Pending | Status::Indexing
                )
            });
        let mut embeddings = Vec::new();
        if query.mode != Mode::Keyword {
            for model in bindings
                .iter()
                .filter(|binding| {
                    allowed.iter().any(|stored| {
                        stored.collection.id == binding.id && !stored.collection.paused
                    })
                })
                .filter_map(|binding| binding.model.as_ref().map(|(model, _)| model.clone()))
                .collect::<BTreeSet<_>>()
            {
                match self
                    .models
                    .encode(
                        &model,
                        &choice,
                        vec![query.query.clone()],
                        sailry_embedding::Input::Query,
                        models::Lane::Interactive,
                        cancel,
                    )
                    .await
                {
                    Ok(mut vectors) => {
                        if vectors.len() != 1 {
                            return Err(Fault::new(
                                ErrorCode::Internal,
                                "embedding query count differs",
                            ));
                        }
                        let vector = vectors.pop().expect("one query embedding was checked");
                        if vector.iter().any(|value| *value != 0.) {
                            embeddings.push((model, vector));
                        }
                    }
                    Err(error) if error.code == ErrorCode::NotConfigured => {}
                    Err(error) => return Err(error),
                }
            }
        }
        Ok(Prepared {
            bindings,
            embeddings,
            indexing,
        })
    }

    pub(crate) async fn shutdown(&self) {
        self.stop.cancel();
        self.cancel
            .lock()
            .expect("knowledge cancellation lock is healthy")
            .cancel();
        let _ = self.wake.try_send(());
        let worker = self
            .worker
            .lock()
            .expect("knowledge worker lock is healthy")
            .take();
        if let Some(worker) = worker {
            let _ = tokio::task::spawn_blocking(move || worker.join()).await;
        }
    }
}

struct Prepared {
    bindings: Vec<index::Binding>,
    embeddings: Vec<(String, Vec<f32>)>,
    indexing: bool,
}

fn binding(stored: &Stored, model: Option<(String, usize)>) -> index::Binding {
    index::Binding {
        id: stored.collection.id,
        generation: stored.collection.progress.generation,
        scope: *blake3::hash(
            &serde_json::to_vec(&(
                &stored.collection.owner,
                &stored.collection.definition.source,
                &stored.collection.definition.globs,
                &stored.collection.definition.tags,
                stored.capture.package.as_ref().map(|package| &package.name),
                stored.capture.worktree,
                stored.capture.session,
                stored.automatic,
            ))
            .expect("knowledge source capture is serializable"),
        )
        .as_bytes(),
        rebuild: stored.collection.rebuild,
        paused: stored.collection.paused,
        model,
    }
}

fn unavailable() -> Fault {
    Fault::new(ErrorCode::Unavailable, "knowledge service is unavailable")
}
