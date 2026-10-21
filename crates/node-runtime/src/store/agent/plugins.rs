//! Host operation tools use the same package revisions captured by ADK admission.
use super::*;
mod readonly;
mod references;
pub(in crate::store) use readonly::check_action;
pub(in crate::store) use references::{assistant, capture, packages, read};

pub(in crate::store) fn package(
    db: &Connection,
    context: &plugin::Context,
) -> Result<plugin::Info, Fault> {
    let run = calls::active(
        db,
        context
            .turn
            .ok_or_else(|| sailry_protocol::Fault::invalid("plugin turn is required"))?,
    )?;
    if Some(run.session) != context.session {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plugin turn belongs to another session",
        ));
    }
    let references = read(db, run.turn)?;
    if !references.contains(&context.package) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plugin version is outside the admitted turn",
        ));
    }
    packages(db, std::slice::from_ref(&context.package))?
        .pop()
        .ok_or_else(|| sailry_protocol::Fault::invalid("plugin package is unavailable"))
}

pub(super) fn operation(
    db: &Connection,
    turn: TurnId,
    name: &str,
) -> Result<Option<sailry_protocol::tool::Operation>, Fault> {
    let references = read(db, turn)?;
    let packages = packages(db, &references)?;
    if crate::computer::definition(name).is_some() {
        let config = permissions::config(db, turn)?;
        return Ok(packages.iter().find_map(|package| {
            crate::plugins::tools::computer(package, name).filter(|_| {
                crate::plugins::conversation::selects(
                    config.assistant.as_ref(),
                    &packages,
                    package,
                    name,
                )
            })
        }));
    }
    Ok(packages
        .iter()
        .find_map(|package| crate::plugins::tools::operation(package, name)))
}

pub(super) fn command_flow(db: &Connection, turn: TurnId, name: &str) -> Result<bool, Fault> {
    let packages = packages(db, &read(db, turn)?)?;
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

pub(super) fn storage_flow(db: &Connection, turn: TurnId, name: &str) -> Result<bool, Fault> {
    use sailry_protocol::tool::Operation;
    let packages = packages(db, &read(db, turn)?)?;
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

pub(in crate::store) fn check_computer(
    db: &Connection,
    context: &plugin::Context,
    package: &plugin::Info,
    name: &str,
) -> Result<(), Fault> {
    let Some(turn) = context.turn else {
        return Ok(());
    };
    let config = permissions::config(db, turn)?;
    if config.mode == WorkMode::Plan && crate::computer::read_only(name) != Some(true) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "planning turns cannot control the computer",
        ));
    }
    let packages = packages(db, &read(db, turn)?)?;
    if !crate::plugins::conversation::selects(config.assistant.as_ref(), &packages, package, name) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "computer tool is outside the admitted assistant selection",
        ));
    }
    Ok(())
}
