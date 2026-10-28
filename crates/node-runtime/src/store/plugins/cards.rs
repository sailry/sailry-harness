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

pub(in crate::store) async fn captured(
    db: &Connection,
    receipt: card::Receipt,
) -> Result<(Info, card::Summary), Fault> {
    let session = super::super::commands::session(db, receipt.session).await?;
    if session.delegation.is_some() {
        return Err(denied());
    }
    let run = super::super::agent::visible_run(db, receipt.session, receipt.turn).await?;
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
    let latest: Option<String> = {
        let mut statement = db.prepare("SELECT h.turn FROM conversation_turns h JOIN turns t ON t.id=h.turn WHERE h.session=?1 ORDER BY t.rowid DESC LIMIT 1").await.map_err(storage_error)?;
        match statement.query_row([receipt.session.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = row.get(0);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error)),
        }
    };
    if latest.as_deref() != Some(receipt.turn.to_string().as_str()) {
        return Err(denied());
    }
    let references = super::super::agent::plugins::read(db, receipt.turn).await?;
    let packages = super::super::agent::plugins::packages(db, &references).await?;
    let mut query = db.prepare(
        "SELECT e.body FROM conversation_events h JOIN agent_events e ON e.sequence=h.sequence WHERE h.session=?1 AND e.turn=?2 ORDER BY e.sequence",
    ).await.map_err(storage_error)?;
    let mut calls = BTreeSet::new();
    {
        let mut rows = query
            .query(params![
                receipt.session.to_string(),
                receipt.turn.to_string()
            ])
            .await
            .map_err(storage_error)?;
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let row: turso::Result<_> = row.get::<Vec<u8>>(0);

            let event: crate::agent::canonical::Record =
                serde_json::from_slice(&row.map_err(storage_error)?).map_err(storage_error)?;
            for block in event
                .message()
                .into_iter()
                .flat_map(|message| &message.content)
            {
                match &block.kind {
                    crate::agent::canonical::BlockKind::ToolRequest(call) if call.id.is_some() => {
                        calls.insert((
                            event.provenance.branch.clone(),
                            call.id.clone().unwrap(),
                            call.name.clone(),
                        ));
                    }
                    crate::agent::canonical::BlockKind::ToolResponse(result)
                        if result.id.is_some() =>
                    {
                        let id = result.id.as_ref().unwrap();
                        let name = &result.name;
                        if !calls.contains(&(
                            event.provenance.branch.clone(),
                            id.clone(),
                            name.clone(),
                        )) {
                            continue;
                        }
                        let response =
                            super::super::agent::blobs::hydrate(db, &result.result).await?;
                        let Some(content) =
                            sailry_protocol::tool::Presentation::Content.content(&response)
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
                        let current = required(db, &summary.package.name).await?;
                        if !current.summary.available()
                            || current.summary.digest != summary.package.digest
                        {
                            return Err(denied());
                        }
                        return Ok((current, summary.clone()));
                    }
                    _ => {}
                }
            }
        }
    }
    Err(denied())
}

pub(in crate::store) async fn check(
    db: &Connection,
    node: NodeId,
    _caller: NodeId,
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
    let (info, summary) = captured(db, receipt).await?;
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
        Command::ReadPluginConnection { package, card }
            if *package == context.package && card == &declaration.key =>
        {
            Ok(())
        }
        Command::ListConnections | Command::ListConnectorTypes => Ok(()),
        Command::StartConnectorLogin {
            package,
            connector,
            method: sailry_protocol::plugin::connector::Kind::McpOauth,
            input,
            connection,
            ..
        } if *package == context.package
            && input.secrets.is_empty()
            && input.fields.is_empty()
            && !input.registration
            && declaration.mcp_authorization() =>
        {
            let connector=super::connectors::declarations(&info).into_iter().find(|item|item.id==*connector
                && item.auth.iter().any(|auth|matches!(auth,plugin::connector::Auth::McpOauth{server:named,..} if named==server))).ok_or_else(denied)?;
            if let Some(id) = connection {
                let record = super::connectors::read(db, *id).await?;
                if record.plugin != info.summary.name || record.connector != connector.id {
                    return Err(denied());
                }
            }
            Ok(())
        }
        Command::SubmitConnectorInput { .. } | Command::CancelConnectorLogin { .. } => Ok(()),
        _ => Err(denied()),
    }
}
