//! Host operation tools use the same package revisions captured by Node admission.
use super::*;
mod readonly;
mod references;
pub(in crate::store) use readonly::check_action;
pub(in crate::store) use references::{assistant, capture, configuration, packages, read};

pub(in crate::store) async fn package(
    db: &Connection,
    request: &Request,
) -> Result<plugin::Info, Fault> {
    let context = request
        .plugin
        .as_ref()
        .ok_or_else(|| Fault::new(ErrorCode::PermissionDenied, "plugin provenance is required"))?;
    let run = runs::get(
        db,
        context
            .turn
            .ok_or_else(|| sailry_protocol::Fault::invalid("plugin turn is required"))?,
    )
    .await?;
    // Turn initialization reads its captured context before declaring summary tools.
    // It does not grant compaction permission to invoke those tools or mutate state.
    if run.kind != RunKind::Task
        && !matches!(
            request.command,
            Command::ReadPluginValue { .. }
                | Command::ReadPluginConversationValue { .. }
                | Command::ListPluginKeys { .. }
                | Command::SearchPluginValues(_)
                | Command::ReadPluginSettings { .. }
                | Command::ReadPluginConnection { .. }
                | Command::ReadSession { .. }
                | Command::ReadConversation { .. }
                | Command::ReadTurn { .. }
        )
    {
        return Err(Fault::invalid("context compaction cannot invoke tools"));
    }
    if run.status != Status::Running {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "plugin turn is no longer running",
        ));
    }
    if Some(run.session) != context.session {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plugin turn belongs to another session",
        ));
    }
    let references = read(db, run.turn).await?;
    if !references.contains(&context.package) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plugin version is outside the admitted turn",
        ));
    }
    packages(db, std::slice::from_ref(&context.package))
        .await?
        .pop()
        .ok_or_else(|| sailry_protocol::Fault::invalid("plugin package is unavailable"))
}

pub(super) async fn operation(
    db: &Connection,
    turn: TurnId,
    name: &str,
) -> Result<Option<sailry_protocol::tool::Operation>, Fault> {
    let references = read(db, turn).await?;
    let packages = packages(db, &references).await?;

    Ok(packages
        .iter()
        .find_map(|package| crate::plugins::tools::operation(package, name)))
}

pub(super) async fn command_flow(db: &Connection, turn: TurnId, name: &str) -> Result<bool, Fault> {
    let packages = packages(db, &read(db, turn).await?).await?;
    Ok(packages.iter().any(|package| {
        package.extension.as_ref().is_some_and(|extension| {
            extension.tools.iter().any(|tool| {
                tool.valid()
                    && crate::plugins::tools::alias(&package.summary.name, &tool.name) == name
                    && tool
                        .handler
                        .as_ref()
                        .and_then(|handler| handler.flow.as_ref())
                        .is_some_and(|flow| {
                            flow.operations
                                .contains(&sailry_protocol::tool::Operation::RunCommand)
                        })
            })
        })
    }))
}

pub(super) async fn storage_flow(db: &Connection, turn: TurnId, name: &str) -> Result<bool, Fault> {
    use sailry_protocol::tool::Operation;
    let packages = packages(db, &read(db, turn).await?).await?;
    Ok(packages.iter().any(|package| {
        package.extension.as_ref().is_some_and(|extension| {
            extension.tools.iter().any(|tool| {
                tool.valid()
                    && crate::plugins::tools::alias(&package.summary.name, &tool.name) == name
                    && tool
                        .handler
                        .as_ref()
                        .and_then(|handler| handler.flow.as_ref())
                        .is_some_and(|flow| {
                            !flow.operations.is_empty()
                                && flow.operations.iter().all(|operation| {
                                    matches!(
                                        operation,
                                        Operation::SetValue
                                            | Operation::DeleteValue
                                            | Operation::StorageTransaction
                                    )
                                })
                        })
            })
        })
    }))
}
