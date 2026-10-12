//! Canonical history is the only receipt inventory. No presentation database exists.
use super::*;
use sailry_protocol::{
    NodeId, Request,
    plugin::{card, desktop::Surface},
};
use std::collections::BTreeSet;

#[cfg(test)]
mod tests;

fn denied() -> Fault {
    Fault::new(ErrorCode::PermissionDenied, "card is no longer available")
}

pub(in crate::store) fn captured(
    db: &Connection,
    receipt: card::Receipt,
) -> Result<(Info, card::Summary), Fault> {
    let session = super::super::commands::session(db, receipt.session)?;
    if session.delegation.is_some() {
        return Err(denied());
    }
    let run = super::super::agent::visible_run(db, receipt.session, receipt.turn)?;
    if run.origin.is_some()
        || run.session != receipt.session
        || !matches!(
            run.status,
            sailry_protocol::conversation::Status::Running
                | sailry_protocol::conversation::Status::Completed
                | sailry_protocol::conversation::Status::Failed
        )
    {
        return Err(denied());
    }
    let latest: Option<String> = db.query_row(
        "SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1",
        [receipt.session.to_string()], |row| row.get(0),
    ).optional().map_err(storage_error)?;
    if latest.as_deref() != Some(receipt.turn.to_string().as_str()) {
        return Err(denied());
    }
    let references = super::super::agent::plugins::read(db, receipt.turn)?;
    let packages = super::super::agent::plugins::packages(db, &references)?;
    let mut query = db.prepare(
        "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn=?2 ORDER BY e.sequence",
    ).map_err(storage_error)?;
    let mut calls = BTreeSet::new();
    for row in query
        .query_map(
            params![receipt.session.to_string(), receipt.turn.to_string()],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?
    {
        let event: adk_core::Event =
            serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
        for part in event
            .content()
            .into_iter()
            .flat_map(|content| &content.parts)
        {
            match part {
                adk_core::Part::FunctionCall {
                    id: Some(id), name, ..
                } => {
                    calls.insert((event.branch.clone(), id.clone(), name.clone()));
                }
                adk_core::Part::FunctionResponse {
                    id: Some(id),
                    function_response,
                    ..
                } => {
                    let name = &function_response.name;
                    if !calls.contains(&(event.branch.clone(), id.clone(), name.clone())) {
                        continue;
                    }
                    let Some(content) = sailry_protocol::tool::Presentation::Content
                        .content(&function_response.response)
                    else {
                        continue;
                    };
                    let Some(sailry_protocol::tool::Block::Card(summary)) =
                        content.blocks.get(usize::from(receipt.block))
                    else {
                        continue;
                    };
                    if summary.receipt != receipt || summary.worktree != run.worktree {
                        continue;
                    }
                    let Some(package) = packages
                        .iter()
                        .find(|package| package.summary.reference() == summary.package)
                    else {
                        continue;
                    };
                    let Some(extension) = package.extension.as_ref() else {
                        continue;
                    };
                    if !extension.tools.iter().any(|tool| {
                        tool.handler.is_some()
                            && crate::plugins::tools::alias(&package.summary.name, &tool.name)
                                == *name
                    }) || crate::plugins::cards::identity(
                        receipt.turn,
                        &summary.package,
                        name,
                        id,
                    )? != receipt.call
                        || !extension.cards.iter().any(|declaration| {
                            declaration.key == summary.key
                                && crate::plugins::cards::props(declaration, &summary.props)
                        })
                    {
                        continue;
                    }
                    let current = required(db, &summary.package.name)?;
                    if !current.summary.enabled || current.summary.digest != summary.package.digest
                    {
                        return Err(denied());
                    }
                    return Ok((current, summary.clone()));
                }
                _ => {}
            }
        }
    }
    Err(denied())
}

pub(in crate::store) fn check(
    db: &Connection,
    node: NodeId,
    caller: NodeId,
    request: &Request,
) -> Result<(), Fault> {
    let context = request.plugin.as_ref().ok_or_else(denied)?;
    let Surface::Card(receipt) = context.surface else {
        return Err(denied());
    };
    if context.turn.is_some()
        || context.invocation.is_some()
        || context.session != Some(receipt.session)
    {
        return Err(denied());
    }
    let (info, summary) = captured(db, receipt)?;
    if summary.node != node
        || context.worktree != Some(summary.worktree)
        || context.package != info.summary.reference()
    {
        return Err(denied());
    }
    let declaration = info
        .extension
        .as_ref()
        .and_then(|extension| extension.cards.iter().find(|card| card.key == summary.key))
        .ok_or_else(denied)?;
    let server = declaration.mcp_server.as_deref().ok_or_else(denied)?;
    if !info
        .mcp
        .iter()
        .any(|entry| entry.name == server && entry.transport != plugin::McpTransport::Stdio)
    {
        return Err(denied());
    }
    match &request.command {
        Command::ReadMcpAuthorization {
            package,
            server: named,
        }
        | Command::BeginMcpLogin {
            package,
            server: named,
            client_id: None,
            ..
        } if *package == context.package && named == server => Ok(()),
        Command::CompleteMcpLogin { attempt, .. } | Command::CancelMcpLogin { attempt } => {
            let body: Option<Vec<u8>> = db
                .query_row(
                    "SELECT body FROM requests WHERE caller=?1 AND id=?2",
                    params![&caller.0[..], attempt.to_string()],
                    |row| row.get(0),
                )
                .optional()
                .map_err(storage_error)?;
            let original: Request =
                serde_json::from_slice(&body.ok_or_else(denied)?).map_err(storage_error)?;
            if original.plugin.as_ref().is_none_or(|original| {
                original.surface != context.surface
                    || original.session != context.session
                    || original.worktree != context.worktree
                    || original.package.name != context.package.name
                    || original.package.digest != context.package.digest
            }) || !matches!(original.command, Command::BeginMcpLogin { server: named, .. } if named == server)
            {
                return Err(denied());
            }
            Ok(())
        }
        _ => Err(denied()),
    }
}
