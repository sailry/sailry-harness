//! Models share durable installation ordering and the Node's verified registry.
use super::{Execution, Worker};
use crate::store::{Ingress, database::Database, resources};
use sailry_protocol::{
    Command, EventEnvelope,
    installation::{Phase, Resource},
};
use std::sync::Arc;
use tokio::sync::broadcast;

pub(in crate::store) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::InstallLocalModel { .. } | Command::DeleteLocalModel { .. }
    )
}

pub(in crate::store) async fn submit(
    worker: &mut Worker,
    database: &mut Database,
    events: &broadcast::Sender<EventEnvelope>,
    ingress: Arc<Ingress>,
    execution: resources::Execution,
) -> Result<(), Box<Execution>> {
    let id = match &execution.request.command {
        Command::InstallLocalModel { id } | Command::DeleteLocalModel { id } => id,
        _ => unreachable!("model installation command expected"),
    };
    let definition = match crate::store::local_models::definition(&database.connection, id).await {
        Ok(definition) => definition,
        Err(error) => {
            let result = database
                .finish_external(execution.caller, &execution.request, Err(error), events)
                .await;
            let _ = execution.reply.send(result);
            return Ok(());
        }
    };
    let resource = Resource::Model {
        kind: definition.format.kind(),
        id: id.clone(),
    };
    let removing =
        matches!(execution.request.command, Command::DeleteLocalModel { .. }).then(|| id.clone());
    let models = ingress.local_models.clone();
    let request = execution.request.clone();
    let execution = Execution {
        caller: execution.caller,
        request: execution.request,
        reply: execution.reply,
        operation: true,
        observe: !matches!(request.command, Command::DeleteLocalModel { .. }),
    };
    let result = worker.submit(execution, resource, move |reporter, stop| async move {
        reporter(Phase::Inspecting, None);
        ingress
            .change_local_model(&request, &stop, Some(&reporter))
            .await
    });
    if result.is_ok()
        && let Some(id) = removing
    {
        // Ordered removal must cancel an installation before waiting for its completion.
        models.cancel(&id);
    }
    result
}
