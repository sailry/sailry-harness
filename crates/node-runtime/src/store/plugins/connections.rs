//! Connection grants retain the captured resource and the core sharing decision.
use rusqlite::Connection;
use sailry_protocol::{
    Command, ErrorCode, Fault,
    connection::Resource,
    plugin::{Action, Context},
};

use crate::store::{commands, connections, databases, ssh, terminals, worktrees};
mod management;

pub(super) fn action(
    db: &Connection,
    context: &Context,
    request: &sailry_protocol::Request,
) -> Result<Option<Action>, Fault> {
    let command = &request.command;
    let mut target = None;
    let mut management = false;
    let action = match command {
        Command::ListDatabases => Action::ReadDatabases,
        Command::ListSsh => Action::ReadSsh,
        Command::SaveDatabase { profile, .. } | Command::TestDatabase { profile, .. } => {
            management = true;
            target = Some(Resource::Database(profile.id));
            Action::ManageDatabases
        }
        Command::RemoveDatabase { profile, .. } => {
            management = true;
            target = Some(Resource::Database(*profile));
            Action::ManageDatabases
        }
        Command::BrowseDatabase { profile, .. } | Command::CheckDatabase { profile, .. } => {
            target = Some(Resource::Database(*profile));
            Action::ReadDatabases
        }
        Command::QueryDatabase {
            profile, read_only, ..
        } => {
            target = Some(Resource::Database(*profile));
            if *read_only {
                Action::ReadDatabases
            } else {
                Action::ControlDatabases
            }
        }
        // The worker checks the original active request's package context before cancelling.
        Command::CancelDatabase { .. } => Action::ReadDatabases,
        Command::SaveSsh { profile, .. } => {
            management = true;
            target = Some(Resource::Ssh(profile.id));
            Action::ManageSsh
        }
        Command::RemoveSsh { profile, .. }
        | Command::TrustSsh { profile, .. }
        | Command::InstallHost { profile, .. } => {
            management = true;
            target = Some(Resource::Ssh(*profile));
            Action::ManageSsh
        }
        Command::ReadHostInstall { .. } => {
            management = true;
            Action::ManageSsh
        }
        Command::CheckSsh { profile, .. }
        | Command::ListSshTerminals { profile }
        | Command::BrowseSshDirectory { profile, .. }
        | Command::DownloadSshFile { profile, .. } => {
            target = Some(Resource::Ssh(*profile));
            Action::ReadSsh
        }
        Command::RunSsh { profile, .. }
        | Command::OpenSshTerminal { profile, .. }
        | Command::ModifySshFile { profile, .. }
        | Command::FinishSshUpload { profile, .. } => {
            target = Some(Resource::Ssh(*profile));
            Action::ControlSsh
        }
        Command::TransferSsh {
            profile, transfer, ..
        } => {
            if context.worktree.is_none()
                && context.session.is_none()
                && context.turn.is_none()
                && context.invocation.is_none()
            {
                if !worktrees::list(db)?
                    .iter()
                    .any(|tree| tree.id == transfer.worktree && tree.project.is_some())
                {
                    return Err(denied(
                        "SSH transfer requires a registered project worktree",
                    ));
                }
                return Ok(Some(Action::ManageSsh));
            }
            if context.worktree != Some(transfer.worktree) {
                return Err(denied("SSH transfer is outside the captured worktree"));
            }
            target = Some(Resource::Ssh(*profile));
            Action::ControlSsh
        }
        Command::StageSshUpload(spec) => {
            target = Some(Resource::Ssh(spec.profile));
            Action::ControlSsh
        }
        Command::CloseSshTerminal { terminal } => {
            let terminal = terminals::required(db, *terminal)?;
            let profile = terminal
                .ssh
                .ok_or_else(|| denied("terminal is not an SSH terminal"))?;
            target = Some(Resource::Ssh(profile));
            Action::ControlSsh
        }
        Command::CancelSsh { .. } => Action::ControlSsh,
        Command::OpenTerminal { terminal, .. }
        | Command::InspectTerminal { terminal }
        | Command::ClaimTerminal { terminal, .. }
        | Command::InputTerminal { terminal, .. }
        | Command::ResizeTerminal { terminal, .. }
        | Command::SetTerminalAppearance { terminal, .. } => {
            let terminal = terminals::required(db, *terminal)?;
            let Some(profile) = terminal.ssh else {
                return Ok(None);
            };
            target = Some(Resource::Ssh(profile));
            if matches!(command, Command::InspectTerminal { .. }) {
                Action::ReadSsh
            } else {
                Action::ControlSsh
            }
        }
        _ => return Ok(None),
    };
    let creation = if management && (context.turn.is_some() || context.invocation.is_some()) {
        management::authorize(db, request)?
    } else {
        false
    };
    if creation {
        if crate::store::agent::connections::bound(db, context.turn.expect("checked turn"))?
            .is_some()
        {
            return Err(denied(
                "bound conversations cannot create another connection",
            ));
        }
    } else if let Some(target) = target {
        check_target(db, context, target)?;
    } else if context.turn.is_some() && context.session.is_none() {
        return Err(denied("connection tool requires a captured session"));
    }
    Ok(Some(action))
}

pub(in crate::store) fn check_target(
    db: &Connection,
    context: &Context,
    target: Resource,
) -> Result<(), Fault> {
    let (bound, project) = if let Some(session) = context.session {
        let session = commands::session(db, session)?;
        if Some(session.worktree) != context.worktree {
            return Err(denied("connection session belongs to another worktree"));
        }
        (session.config.resource, session.project)
    } else if let Some(worktree) = context.worktree {
        let worktree = worktrees::list(db)?
            .into_iter()
            .find(|candidate| candidate.id == worktree)
            .ok_or_else(|| denied("captured worktree is unavailable"))?;
        (None, worktree.project)
    } else if context.turn.is_none() && context.invocation.is_none() {
        // Global connection management is an explicit controller surface.
        return Ok(());
    } else {
        return Err(denied("connection operation requires a captured resource"));
    };
    if let Some(turn) = context.turn {
        let run = crate::store::agent::connections::owner(db, turn)?;
        if Some(run.session) != context.session || Some(run.worktree) != context.worktree {
            return Err(denied("connection turn belongs to another resource"));
        }
        if crate::store::agent::connections::created(db, turn, &context.package, target)? {
            return Ok(());
        }
    }
    let sharing = match target {
        Resource::Database(id) => databases::read(db, id)?.and_then(|profile| profile.sharing),
        Resource::Ssh(id) => ssh::read(db, id)?.and_then(|profile| profile.sharing),
    };
    if connections::permits(bound, project, target, sharing.as_ref()) {
        Ok(())
    } else {
        Err(denied(
            "connection is not available to this captured resource",
        ))
    }
}

fn denied(message: &str) -> Fault {
    Fault::new(ErrorCode::PermissionDenied, message)
}
