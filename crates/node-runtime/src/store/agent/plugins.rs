//! Host operation tools use the same package revisions captured by ADK admission.
use super::*;
mod references;
pub(in crate::store) use references::{assistant, capture, packages, read};

pub(in crate::store) fn package(
    db: &Connection,
    context: &plugin::Context,
) -> Result<plugin::Info, Fault> {
    let run = calls::active(
        db,
        context
            .turn
            .ok_or_else(|| invalid("plugin turn is required"))?,
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
        .ok_or_else(|| invalid("plugin package is unavailable"))
}

pub(in crate::store) fn check_storage_write(db: &Connection, turn: TurnId) -> Result<(), Fault> {
    if permissions::config(db, turn)?.mode == WorkMode::Plan {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "planning turns cannot write plugin storage",
        ));
    }
    Ok(())
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
