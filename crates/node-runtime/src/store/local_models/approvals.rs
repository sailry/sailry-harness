//! Model removal consumes the original claimed command approval.
use crate::store::{
    agent::{ApprovalBinding, approvals::Prepared},
    database::storage_error,
};
use rusqlite::{Connection, OptionalExtension, params};
use sailry_protocol::{
    Command, ErrorCode, Fault, Request,
    conversation::{Approval, ApprovalState},
    tool::Operation,
};

pub(in crate::store) fn check(db: &Connection, request: &Request) -> Result<(), Fault> {
    let Some(context) = &request.plugin else {
        return Ok(());
    };
    // Existing granted SDK installations keep their admitted authority.
    if matches!(request.command, Command::InstallLocalModel { .. }) {
        return Ok(());
    }
    let denied = || {
        Fault::new(
            ErrorCode::PermissionDenied,
            "model change requires its exact captured approval",
        )
    };
    let turn = context.turn.ok_or_else(denied)?;
    if context.invocation.is_some() {
        return Err(denied());
    }
    let run = crate::store::agent::connections::owner(db, turn)?;
    if context.session != Some(run.session) || context.worktree != Some(run.worktree) {
        return Err(denied());
    }
    if !matches!(request.command, Command::DeleteLocalModel { .. }) {
        return Err(denied());
    }
    let record:Option<(Vec<u8>,Vec<u8>,Vec<u8>)> = db.query_row(
        "SELECT body,binding,prepared FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1",
        params![turn.to_string(),request.id.to_string()],|row|Ok((row.get(0)?,row.get(1)?,row.get(2)?)),
    ).optional().map_err(storage_error)?;
    let (body, binding, prepared) = record.ok_or_else(denied)?;
    let approval: Approval = serde_json::from_slice(&body).map_err(storage_error)?;
    let binding: Option<ApprovalBinding> =
        serde_json::from_slice(&binding).map_err(storage_error)?;
    let prepared: Option<Prepared> = serde_json::from_slice(&prepared).map_err(storage_error)?;
    let canonical = crate::store::agent::approvals::request(db, &approval)?;
    if approval.state != ApprovalState::Approved
        || approval.session != run.session
        || !binding.is_some_and(|binding| {
            binding.owner == context.package
                && binding.operation == Some(Operation::RemoveModel)
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
