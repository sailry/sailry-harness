//! A single existing command transaction owns rollback, receipts and publication.
use super::super::{commands, database::Database};
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, NodeId, Output, Request, plugin::transaction,
};
use std::path::Path;
use turso::Connection;

pub(in crate::store) async fn check(
    db: &Database,
    caller: NodeId,
    request: &Request,
) -> Result<(), Fault> {
    let Command::PluginTransaction { operations } = &request.command else {
        unreachable!()
    };
    transaction::validate(operations)?;
    let package = &request.plugin.as_ref().ok_or_else(denied)?.package;
    for operation in operations {
        let mut child = request.clone();
        child.command = operation.command(package);
        Box::pin(db.check_plugin(caller, &child)).await?;
    }
    Ok(())
}

pub(in crate::store) async fn execute(
    db: &Connection,
    node: NodeId,
    caller: NodeId,
    profile: Option<&Path>,
    request: &Request,
) -> Result<(Output, Vec<Event>), Fault> {
    let Command::PluginTransaction { operations } = &request.command else {
        unreachable!()
    };
    transaction::validate(operations)?;
    let package = &request.plugin.as_ref().ok_or_else(denied)?.package;
    let mut outputs = Vec::with_capacity(operations.len());
    let mut events = Vec::new();
    let checkpoint = super::storage::conversation::sequence(db).await?;
    let mut admitted = None;
    for operation in operations {
        let mut child = request.clone();
        child.command = operation.command(package);
        let (output, event) =
            Box::pin(commands::execute(db, node, caller, profile, &child)).await?;
        if let Output::QueuedTurn(turn) = &output {
            admitted = Some((turn.session, turn.id));
        }
        outputs.push(output);
        events.extend(event);
    }
    if let Some((session, turn)) = admitted {
        super::storage::conversation::reanchor(db, session, checkpoint, turn).await?;
    }
    Ok((Output::PluginTransaction(outputs), events))
}

fn denied() -> Fault {
    Fault::new(ErrorCode::PermissionDenied, "plugin provenance is required")
}
