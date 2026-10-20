//! Retrieval and model work stay outside the durable Store queue.
pub(in crate::store) mod storage;
use super::{Catalog, Stored};
use crate::store::{
    Ingress, Job,
    database::{Database, record, storage_error},
    unavailable,
};
use sailry_link::{Admission, CancellationToken};
use sailry_protocol::{
    Command, ErrorCode, Event, EventEnvelope, Fault, KnowledgeId, Output, Request,
    knowledge::Progress,
};
use std::sync::Arc;
use tokio::sync::{broadcast, oneshot};

pub(in crate::store) async fn start(ingress: &Arc<Ingress>) -> Result<(), Fault> {
    let Some(profile) = ingress.profile.as_deref() else {
        return Ok(());
    };
    ingress.knowledge_register(&ingress.closed).await?;
    let service = crate::knowledge::Service::open(
        profile,
        ingress.local_models.clone(),
        Arc::downgrade(ingress),
        ingress.closed.child_token(),
    )
    .await?;
    if ingress.knowledge.set(service.clone()).is_err() {
        service.shutdown().await;
        return Err(Fault::new(
            ErrorCode::Internal,
            "knowledge service is already initialized",
        ));
    }
    Ok(())
}

pub(in crate::store) fn changes(command: &Command) -> bool {
    storage::changes(command)
        || invalidates(command)
        || matches!(
            command,
            Command::InstallPlugin { .. }
                | Command::InstallPluginSource { .. }
                | Command::InstallPluginUpload { .. }
                | Command::SetPluginEnabled { .. }
                | Command::SavePluginSettings { .. }
        )
}

pub(in crate::store) fn invalidates(command: &Command) -> bool {
    matches!(
        command,
        Command::CreateKnowledgeCollection { .. }
            | Command::UpdateKnowledgeCollection { .. }
            | Command::DeleteKnowledgeCollection { .. }
            | Command::RebuildKnowledgeCollection { .. }
            | Command::PauseKnowledgeCollection { .. }
    )
}

pub(in crate::store) fn observe(
    ingress: &Ingress,
    mut admission: Admission,
    invalidates: bool,
) -> Admission {
    let service = ingress.knowledge.clone();
    let (reply, completion) = oneshot::channel();
    let original = std::mem::replace(&mut admission.completion, completion);
    tokio::spawn(async move {
        if let Ok(result) = original.await {
            if result.is_ok()
                && let Some(service) = service.get()
            {
                if invalidates {
                    service.invalidate();
                } else {
                    service.refresh();
                }
            }
            let _ = reply.send(result);
        }
    });
    admission
}

pub(in crate::store) fn progress(
    database: &mut Database,
    id: KnowledgeId,
    generation: u64,
    progress: &Progress,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let changed = super::progress(&transaction, id, generation, progress)?;
    let envelopes = record(&transaction, database.node, changed.into_iter().collect())?;
    transaction.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(())
}

impl Ingress {
    pub(crate) async fn knowledge_catalog(&self) -> Result<Catalog, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeCatalog { reply }).await?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn knowledge_allowed(&self, request: &Request) -> Result<Vec<Stored>, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeAllowed {
            request: Box::new(request.clone()),
            reply,
        })
        .await?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn knowledge_progress(
        &self,
        id: KnowledgeId,
        generation: u64,
        progress: &Progress,
    ) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeProgress {
            id,
            generation,
            progress: progress.clone(),
            reply,
        })
        .await?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn knowledge_sources(
        &self,
        stored: &Stored,
        after: Option<String>,
        limit: u16,
        stop: &CancellationToken,
    ) -> Result<super::storage::PageRaw, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeSources {
            stored: Box::new(stored.clone()),
            after,
            limit,
            stop: stop.clone(),
            reply,
        })
        .await?;
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "knowledge source read was cancelled")),
            result = response => result.map_err(|_| unavailable())?,
        }
    }

    pub(crate) async fn knowledge_source_current(
        &self,
        stored: &Stored,
        record: &super::storage::Record,
        stop: &CancellationToken,
    ) -> Result<bool, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeSourceCurrent {
            stored: Box::new(stored.clone()),
            record: Box::new(record.clone()),
            stop: stop.clone(),
            reply,
        })
        .await?;
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "knowledge source read was cancelled")),
            result = response => result.map_err(|_| unavailable())?,
        }
    }

    pub(crate) async fn knowledge_source(
        &self,
        stored: &Stored,
        key: &str,
        stop: &CancellationToken,
    ) -> Result<Option<super::storage::Record>, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeSource {
            stored: Box::new(stored.clone()),
            key: key.into(),
            stop: stop.clone(),
            reply,
        })
        .await?;
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled, "knowledge source read was cancelled")),
            result = response => result.map_err(|_| unavailable())?,
        }
    }

    pub(in crate::store) async fn read_knowledge(
        &self,
        request: Request,
    ) -> Result<Admission, Fault> {
        if request.target != self.node || request.version != sailry_protocol::VERSION {
            return Err(Fault::new(
                ErrorCode::WrongTarget,
                "Node or protocol version mismatch",
            ));
        }
        let service = self.knowledge.get().ok_or_else(unavailable)?;
        let output = match &request.command {
            Command::QueryKnowledge(query) => {
                let stop = self.closed.child_token();
                let _guard = stop.clone().drop_guard();
                Output::KnowledgeSearch(service.query(&request, query, &stop).await?)
            }
            Command::ReadKnowledgeCitation(input) => {
                let stop = self.closed.child_token();
                let _guard = stop.clone().drop_guard();
                Output::KnowledgeCitation(service.citation(&request, input, &stop).await?)
            }
            _ => return Err(Fault::invalid("knowledge read expected")),
        };
        let (reply, completion) = oneshot::channel();
        let _ = reply.send(Ok(output));
        Ok(Admission {
            receipt: sailry_protocol::Receipt {
                id: request.id,
                durable: false,
            },
            completion,
        })
    }
}
