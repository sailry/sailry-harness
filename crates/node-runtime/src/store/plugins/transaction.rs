//! A single existing command transaction owns rollback, receipts and publication.
use super::super::{commands, database::Database};
use rusqlite::Connection;
use sailry_protocol::{
    Command, ErrorCode, Event, Fault, NodeId, Output, Request, plugin::transaction,
};
use std::path::Path;

pub(in crate::store) fn check(
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
        db.check_plugin(caller, &child)?;
    }
    Ok(())
}

pub(in crate::store) fn execute(
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
    let checkpoint = super::storage::conversation::sequence(db)?;
    let mut admitted = None;
    for operation in operations {
        let mut child = request.clone();
        child.command = operation.command(package);
        let (output, event) = commands::execute(db, node, caller, profile, &child)?;
        if let Output::QueuedTurn(turn) = &output {
            admitted = Some((turn.session, turn.id));
        }
        outputs.push(output);
        events.extend(event);
    }
    if let Some((session, turn)) = admitted {
        super::storage::conversation::reanchor(db, session, checkpoint, turn)?;
    }
    Ok((Output::PluginTransaction(outputs), events))
}

fn denied() -> Fault {
    Fault::new(ErrorCode::PermissionDenied, "plugin provenance is required")
}
