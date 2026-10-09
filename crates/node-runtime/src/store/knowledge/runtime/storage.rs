//! Semantic registrations commit beside their raw storage writes and canonical events.
use super::*;

pub(in crate::store) fn changes(command: &Command) -> bool {
    if let Command::PluginTransaction { operations } = command {
        return operations.iter().any(|operation| {
            matches!(
                operation,
                sailry_protocol::plugin::transaction::Operation::Write { .. }
                    | sailry_protocol::plugin::transaction::Operation::Index { .. }
                    | sailry_protocol::plugin::transaction::Operation::Remove { .. }
                    | sailry_protocol::plugin::transaction::Operation::ConversationWrite { .. }
                    | sailry_protocol::plugin::transaction::Operation::ConversationRemove { .. }
                    | sailry_protocol::plugin::transaction::Operation::Continue { .. }
            )
        });
    }
    matches!(
        command,
        Command::WritePluginValue { .. }
            | Command::WriteIndexedPluginValue { .. }
            | Command::RemovePluginValue { .. }
            | Command::WritePluginConversationValue { .. }
            | Command::RemovePluginConversationValue { .. }
            | Command::ContinueTurn { .. }
    )
}

pub(in crate::store) fn prepare(
    database: &mut Database,
    request: &Request,
    stop: &CancellationToken,
    _events: &broadcast::Sender<EventEnvelope>,
) -> Result<super::super::semantic::Plan, Fault> {
    super::super::semantic::prepare(&database.connection, request, stop)
}

pub(in crate::store) fn register(
    database: &mut Database,
    stop: &CancellationToken,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let changed = super::super::semantic::startup(&transaction, stop)?;
    let changes = changed.then_some(Event::KnowledgeCollectionsChanged);
    let envelopes = record(&transaction, database.node, changes.into_iter().collect())?;
    transaction.commit().map_err(storage_error)?;
    for envelope in envelopes {
        let _ = events.send(envelope);
    }
    Ok(())
}

impl Ingress {
    pub(super) async fn knowledge_register(&self, stop: &CancellationToken) -> Result<(), Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeRegister {
            stop: stop.clone(),
            reply,
        })
        .await?;
        response.await.map_err(|_| unavailable())?
    }

    pub(crate) async fn semantic_plan(
        &self,
        request: &Request,
        stop: &CancellationToken,
    ) -> Result<super::super::semantic::Plan, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeSemanticPlan {
            request: Box::new(request.clone()),
            stop: stop.clone(),
            reply,
        })
        .await?;
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled,"semantic storage search was cancelled")),
            result = response => result.map_err(|_| unavailable())?,
        }
    }

    pub(crate) async fn semantic_page(
        &self,
        request: &Request,
        plan: super::super::semantic::Plan,
        selection: super::super::semantic::Selection,
        stop: &CancellationToken,
    ) -> Result<super::super::semantic::Materialized, Fault> {
        let (reply, response) = oneshot::channel();
        self.send_internal(Job::KnowledgeSemanticPage {
            request: Box::new(request.clone()),
            plan: Box::new(plan),
            selection: Box::new(selection),
            stop: stop.clone(),
            reply,
        })
        .await?;
        tokio::select! {
            biased;
            _ = stop.cancelled() => Err(Fault::new(ErrorCode::Cancelled,"semantic storage search was cancelled")),
            result = response => result.map_err(|_| unavailable())?,
        }
    }

    pub(in crate::store) async fn read_storage_search(
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
        let stop = self.closed.child_token();
        let _guard = stop.clone().drop_guard();
        let output = Output::PluginSearch(service.search_storage(&request, &stop).await?);
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
