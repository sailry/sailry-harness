//! Admitted Plan turns cannot turn a plugin's read-only declaration into write authority.
use super::*;
use sailry_protocol::plugin::Action;

#[cfg(test)]
mod tests;

pub(in crate::store) fn check_action(
    db: &Connection,
    turn: TurnId,
    command: &Command,
    action: Option<&Action>,
) -> Result<(), Fault> {
    if permissions::config(db, turn)?.mode != WorkMode::Plan {
        return Ok(());
    }
    let allowed = action.is_some_and(Action::read_only)
        || (matches!(command, Command::WriteArtifact(_))
            && action == Some(&Action::WriteArtifacts))
        || matches!(
            command,
            Command::ReadInstallations { .. }
                | Command::DismissInstallation { .. }
                | Command::ReadPluginSettings { .. }
                | Command::ListPluginModels
                | Command::GuideTurn { .. }
                | Command::Dispatch {
                    action: sailry_protocol::dispatch::Command::ListHandlers
                        | sailry_protocol::dispatch::Command::ListSchedules
                        | sailry_protocol::dispatch::Command::ListJobs { .. }
                        | sailry_protocol::dispatch::Command::ReadJob { .. }
                        | sailry_protocol::dispatch::Command::ReadResult { .. },
                    ..
                }
        );
    let writes = matches!(
        command,
        Command::QueryDatabase {
            read_only: false,
            ..
        }
    ) || matches!(command, Command::UseBrowser { action, .. } if action.requires_approval());
    if !allowed || writes {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            if action == Some(&Action::WriteStorage) {
                "planning turns cannot write plugin storage"
            } else {
                "planning turns cannot dispatch plugin actions with side effects"
            },
        ));
    }
    Ok(())
}
