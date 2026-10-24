//! Catalog mutations consume one exact captured native operation approval.
use super::*;
use crate::store::agent::{ApprovalBinding, approvals::Prepared};
use sailry_protocol::{
    Command, Request,
    conversation::{Approval, ApprovalState},
    tool::Operation,
};

pub(in crate::store) async fn check(db: &Connection, request: &Request) -> Result<(), Fault> {
    let Some(context) = &request.plugin else {
        return Ok(());
    };
    let Some(turn) = context.turn else {
        return Ok(());
    };
    let denied = || {
        Fault::new(
            ErrorCode::PermissionDenied,
            "role change requires its exact captured approval",
        )
    };
    if context.invocation.is_some() {
        return Err(denied());
    }
    let run = crate::store::agent::connections::owner(db, turn).await?;
    if context.session != Some(run.session) || context.worktree != Some(run.worktree) {
        return Err(denied());
    }
    let operation = match &request.command {
        Command::PutRole { .. } => Operation::SaveRole,
        Command::RemoveRole { .. } => Operation::RemoveRole,
        Command::SetSessionRoles { session, .. } if *session == run.session => {
            Operation::ApplyRoles
        }
        _ => return Err(denied()),
    };
    let mut statement = db.prepare(
        "SELECT body,binding,prepared FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1",
    ).await.map_err(storage_error)?;
    let row = match statement
        .query_row(params![turn.to_string(), request.id.to_string()])
        .await
    {
        Ok(row) => row,
        Err(turso::Error::QueryReturnedNoRows) => return Err(denied()),
        Err(error) => return Err(storage_error(error)),
    };
    let body: Vec<u8> = row.get(0).map_err(storage_error)?;
    let binding: Vec<u8> = row.get(1).map_err(storage_error)?;
    let prepared: Vec<u8> = row.get(2).map_err(storage_error)?;
    drop(statement);
    let approval: Approval = serde_json::from_slice(&body).map_err(storage_error)?;
    let binding: Option<ApprovalBinding> =
        serde_json::from_slice(&binding).map_err(storage_error)?;
    let prepared: Option<Prepared> = serde_json::from_slice(&prepared).map_err(storage_error)?;
    let canonical = crate::store::agent::approvals::request(db, &approval).await?;
    if approval.state != ApprovalState::Approved
        || approval.session != run.session
        || !binding.is_some_and(|binding| {
            binding.owner == context.package
                && binding.operation == Some(operation)
                && binding.tool == canonical.tool_name
        })
        || !prepared
            .map(|prepared| prepared.matches(&request.command))
            .transpose()?
            .unwrap_or(false)
    {
        return Err(denied());
    }
    Ok(())
}
