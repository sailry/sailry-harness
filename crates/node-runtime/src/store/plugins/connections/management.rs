//! Agent management consumes the exact prepared command or explicit host-key consent.
use super::*;
use crate::store::{
    agent::{ApprovalBinding, connections::Prepared},
    database::storage_error,
};
use rusqlite::{OptionalExtension, params};
use sailry_protocol::{
    Request,
    conversation::{Approval, ApprovalState},
    tool::Operation,
};

pub(super) fn authorize(db: &Connection, request: &Request) -> Result<bool, Fault> {
    let context = request.plugin.as_ref().expect("plugin connection context");
    let Some(turn) = context.turn else {
        return Ok(false);
    };
    if context.invocation.is_some() {
        return Err(denied("connection management requires a captured tool"));
    }
    let operation = match request.command {
        Command::SaveDatabase { .. } => Operation::SaveDatabaseConnection,
        Command::SaveSsh { .. } => Operation::SaveSshConnection,
        Command::TrustSsh { .. } => return trust(db, request),
        _ => return Err(denied("connection management requires a controller view")),
    };
    let record: Option<(Vec<u8>, Vec<u8>, Vec<u8>)> = db.query_row(
        "SELECT body,binding,prepared FROM agent_approvals WHERE turn=?1 AND request=?2 AND claimed=1",
        params![turn.to_string(), request.id.to_string()],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).optional().map_err(storage_error)?;
    let (body, binding, prepared) =
        record.ok_or_else(|| denied("connection save has no claimed approval"))?;
    let approval: Approval = serde_json::from_slice(&body).map_err(storage_error)?;
    let binding: Option<ApprovalBinding> =
        serde_json::from_slice(&binding).map_err(storage_error)?;
    let prepared: Option<Prepared> = serde_json::from_slice(&prepared).map_err(storage_error)?;
    if approval.state != ApprovalState::Approved
        || Some(approval.session) != context.session
        || !binding.is_some_and(|binding| {
            binding.owner == context.package && binding.operation == Some(operation)
        })
        || !prepared
            .map(|p| p.matches(&request.command))
            .transpose()?
            .unwrap_or(false)
    {
        return Err(denied("connection save differs from its claimed approval"));
    }
    Ok(matches!(
        request.command,
        Command::SaveDatabase {
            expected_revision: 0,
            ..
        } | Command::SaveSsh {
            expected_revision: 0,
            ..
        }
    ))
}

fn trust(db: &Connection, request: &Request) -> Result<bool, Fault> {
    let context = request.plugin.as_ref().unwrap();
    let turn = context.turn.unwrap();
    let Command::TrustSsh {
        profile,
        expected_revision,
        key,
    } = &request.command
    else {
        unreachable!()
    };
    let mut query = db.prepare("SELECT body,binding FROM agent_approvals WHERE turn=?1 AND claimed=1 AND json_extract(body,'$.state')='approved' AND json_type(body,'$.ssh')='object'").map_err(storage_error)?;
    let records = query
        .query_map([turn.to_string()], |row| {
            Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, Vec<u8>>(1)?))
        })
        .map_err(storage_error)?;
    for record in records {
        let (body, binding) = record.map_err(storage_error)?;
        let approval: Approval = serde_json::from_slice(&body).map_err(storage_error)?;
        let binding: Option<ApprovalBinding> =
            serde_json::from_slice(&binding).map_err(storage_error)?;
        if Some(approval.session) == context.session
            && binding.is_some_and(|b| {
                b.owner == context.package
                    && matches!(
                        b.operation,
                        Some(Operation::RunSsh | Operation::TransferSsh)
                    )
            })
            && approval.ssh.is_some_and(|trust| {
                trust.profile == *profile
                    && trust.expected_revision == *expected_revision
                    && trust.key == *key
            })
        {
            return Ok(false);
        }
    }
    Err(denied("SSH host key requires its exact explicit approval"))
}
