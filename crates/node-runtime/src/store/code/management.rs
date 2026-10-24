//! Bind lifecycle admission to the original claimed conversation call, never callback authority.
use super::*;
use sailry_protocol::{conversation::ApprovalState, tool::Operation};

pub(super) async fn authorize(database: &Database, request: &Request) -> Result<(), Fault> {
    let Some(context) = &request.plugin else {
        return Ok(());
    };
    if context.controller_settings() {
        // The shared action gate has already checked the captured package and grant.
        return Ok(());
    }
    let denied = || {
        Fault::new(
            ErrorCode::PermissionDenied,
            "language management requires its exact claimed conversation approval",
        )
    };
    let turn = context.turn.ok_or_else(denied)?;
    if context.invocation.is_some() {
        return Err(denied());
    }
    let record: Option<(Vec<u8>, Vec<u8>)> = {
        let mut statement = database
        .connection.prepare("SELECT body,binding FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1").await.map_err(storage_error)?;
        match statement
            .query_row(params![turn.to_string(), request.id.to_string()])
            .await
        {
            Ok(row) => {
                let mapped: turso::Result<_> =
                    (|row: &turso::Row| Ok((row.get(0)?, row.get(1)?)))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    let (body, binding) = record.ok_or_else(denied)?;
    let approval: sailry_protocol::conversation::Approval =
        serde_json::from_slice(&body).map_err(storage_error)?;
    let binding: Option<super::super::agent::ApprovalBinding> =
        serde_json::from_slice(&binding).map_err(storage_error)?;
    let operation = match request.command {
        Command::InstallCodeServer { .. } => Operation::InstallCodeServer,
        Command::UninstallCodeServer { .. } => Operation::UninstallCodeServer,
        Command::StartCodeServer { .. } => Operation::StartCodeServer,
        Command::StopCodeServer { .. } => Operation::StopCodeServer,
        _ => return Err(denied()),
    };
    let binding = binding
        .filter(|binding| binding.owner == context.package && binding.operation == Some(operation))
        .ok_or_else(denied)?;
    if approval.session != context.session.ok_or_else(denied)?
        || approval.state != ApprovalState::Approved
    {
        return Err(denied());
    }
    let body: Vec<u8> = {
        let mut statement = database
            .connection
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![turn.to_string(), approval.entry])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    let event: adk_core::Event = serde_json::from_slice(&body).map_err(storage_error)?;
    let Some(adk_core::Part::FunctionCall { name, args, .. }) = event
        .content()
        .and_then(|content| content.parts.get(approval.index))
    else {
        return Err(denied());
    };
    if name != &binding.tool
        || crate::code::management::command(operation, args.clone(), context)? != request.command
    {
        return Err(denied());
    }
    Ok(())
}
