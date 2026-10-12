//! Existing model services retain catalog, native validation and receipt ownership.
use super::{Execution, Worker};
use crate::store::{Ingress, resources};
use sailry_protocol::{
    Command,
    installation::{Phase, Resource},
    models::Kind,
};
use std::sync::Arc;

pub(in crate::store) fn handles(command: &Command) -> bool {
    matches!(
        command,
        Command::InstallOcrModel { .. }
            | Command::DeleteOcrModel { .. }
            | Command::InstallKnowledgeModel { .. }
    )
}

pub(in crate::store) fn submit(
    worker: &mut Worker,
    ingress: Arc<Ingress>,
    execution: resources::Execution,
) -> Result<(), Box<Execution>> {
    let resource = match &execution.request.command {
        Command::InstallOcrModel { id } | Command::DeleteOcrModel { id } => Resource::Model {
            kind: Kind::Ocr,
            id: id.clone(),
        },
        Command::InstallKnowledgeModel { model } => Resource::Model {
            kind: Kind::Embedding,
            id: model.key().into_owned(),
        },
        _ => unreachable!("model installation command expected"),
    };
    let request = execution.request.clone();
    let execution = Execution {
        caller: execution.caller,
        request: execution.request,
        reply: execution.reply,
        operation: true,
        observe: !matches!(request.command, Command::DeleteOcrModel { .. }),
    };
    worker.submit(execution, resource, move |reporter, stop| async move {
        reporter(Phase::Inspecting, None);
        match &request.command {
            Command::InstallOcrModel { .. } | Command::DeleteOcrModel { .. } => {
                ingress.change_ocr(&request, &stop, Some(&reporter)).await
            }
            Command::InstallKnowledgeModel { .. } => {
                ingress
                    .change_knowledge(&request, &stop, Some(&reporter))
                    .await
            }
            _ => unreachable!("model installation command expected"),
        }
    })
}
