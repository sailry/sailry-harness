//! Session authority uses the admitted revision; command access captures the Node's current policy.
use super::*;
use adk_core::ToolConfirmationRequest;

pub(super) fn config(db: &Connection, turn: TurnId) -> Result<SessionConfig, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT r.config FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}

pub(super) fn source(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
) -> Result<ApprovalSource, Fault> {
    let config = config(db, turn)?;
    if config.mode == sailry_protocol::WorkMode::Plan {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "planning turns cannot authorize tools with side effects",
        ));
    }
    let operation = super::plugins::operation(db, turn, &request.tool_name)?;
    if operation == Some(sailry_protocol::tool::Operation::RunCommand) {
        let access = super::super::sandbox::effective(db, &request.args)?;
        return Ok(
            if access == sailry_protocol::process::Access::Workspace
                && crate::process::sandbox::SUPPORTED
            {
                ApprovalSource::Sandbox
            } else if config.permission == Permission::Full {
                ApprovalSource::Full
            } else {
                ApprovalSource::User
            },
        );
    }
    let command_flow =
        operation.is_none() && super::plugins::command_flow(db, turn, &request.tool_name)?;
    if command_flow && !super::super::sandbox::read(db)?.enabled {
        return Ok(if config.permission == Permission::Full {
            ApprovalSource::Full
        } else {
            ApprovalSource::User
        });
    }
    if !command_flow
        && crate::process::sandbox::SUPPORTED
        && request
            .args
            .get("access")
            .is_some_and(|value| value == "unrestricted")
    {
        return Ok(ApprovalSource::User);
    }
    if matches!(
        operation,
        Some(
            sailry_protocol::tool::Operation::SetValue
                | sailry_protocol::tool::Operation::DeleteValue
                | sailry_protocol::tool::Operation::StorageTransaction
        )
    ) {
        // The operation and grant belong to the admitted package. Admission still
        // checks its live availability, scope and exact private KV mutations.
        return Ok(ApprovalSource::Storage);
    }
    Ok(match (config.permission, request.tool_name.as_str()) {
        (
            Permission::Full,
            "create_worktree" | "register_worktree" | "remove_worktree" | "install_skill"
            | "update_skill" | "uninstall_skill",
        ) => ApprovalSource::Full,
        (Permission::Full, name)
            if name.starts_with("mcp_")
                || name.starts_with("plugin_")
                || matches!(
                    operation,
                    Some(
                        sailry_protocol::tool::Operation::ReadComputer
                            | sailry_protocol::tool::Operation::ControlComputer
                    )
                ) =>
        {
            ApprovalSource::Full
        }
        (Permission::Project, _)
            if matches!(
                operation,
                Some(
                    sailry_protocol::tool::Operation::WriteFile
                        | sailry_protocol::tool::Operation::EditFile
                        | sailry_protocol::tool::Operation::EditNotebook
                        | sailry_protocol::tool::Operation::DeleteFile
                        | sailry_protocol::tool::Operation::MoveFile
                        | sailry_protocol::tool::Operation::CreateDirectory
                )
            ) =>
        {
            ApprovalSource::Project
        }
        _ => ApprovalSource::User,
    })
}

pub(super) fn code_source(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    startup: &sailry_protocol::code::Startup,
) -> Result<ApprovalSource, Fault> {
    let run = super::calls::active(db, turn)?;
    if run.worktree != startup.worktree
        || !super::plugins::operation(db, turn, &request.tool_name)?
            .is_some_and(crate::code::handles)
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "language startup is outside the admitted read tool",
        ));
    }
    crate::files::path::components(&startup.root, true)?;
    if super::super::code::approved(db, startup)? {
        return Ok(ApprovalSource::CodeRule);
    }
    Ok(match config(db, turn)?.permission {
        Permission::Ask => ApprovalSource::User,
        Permission::Project => ApprovalSource::Project,
        Permission::Full => ApprovalSource::Full,
    })
}
