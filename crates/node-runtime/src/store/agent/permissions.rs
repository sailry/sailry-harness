//! Session authority uses the admitted revision; command access captures the Node's current policy.
use super::*;
use crate::agent::canonical::ToolIdentity;

pub(super) async fn config(db: &Connection, turn: TurnId) -> Result<SessionConfig, Fault> {
    let body: Vec<u8> = {
        let mut statement = db.prepare("SELECT r.config FROM turns t JOIN session_revisions r ON r.session=t.session AND r.revision=t.revision WHERE t.id=?1").await.map_err(storage_error)?;
        let row = statement
            .query_row([turn.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    serde_json::from_slice(&body).map_err(storage_error)
}

pub(super) async fn source(
    db: &Connection,
    turn: TurnId,
    request: &ToolIdentity,
) -> Result<ApprovalSource, Fault> {
    let config = config(db, turn).await?;
    let operation = super::plugins::operation(db, turn, &request.name).await?;
    if operation == Some(sailry_protocol::tool::Operation::WriteArtifact) {
        let run = super::calls::active(db, turn).await?;
        if super::super::commands::session(db, run.session)
            .await?
            .delegation
            .is_some()
        {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "delegated sessions cannot write artifacts",
            ));
        }
        return Ok(ApprovalSource::Storage);
    }
    if config.mode == sailry_protocol::WorkMode::Plan {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "planning turns cannot authorize tools with side effects",
        ));
    }
    if operation == Some(sailry_protocol::tool::Operation::RunCommand) {
        let access = super::super::sandbox::effective(db, &request.arguments).await?;
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
        operation.is_none() && super::plugins::command_flow(db, turn, &request.name).await?;
    if command_flow && !super::super::sandbox::read(db).await?.enabled {
        return Ok(if config.permission == Permission::Full {
            ApprovalSource::Full
        } else {
            ApprovalSource::User
        });
    }
    if !command_flow
        && crate::process::sandbox::SUPPORTED
        && request
            .arguments
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
    ) || operation.is_none() && super::plugins::storage_flow(db, turn, &request.name).await?
    {
        // The operation and grant belong to the admitted package. Admission still
        // checks its live availability, scope and exact private KV mutations.
        return Ok(ApprovalSource::Storage);
    }
    Ok(match (config.permission, request.name.as_str()) {
        (
            Permission::Full,
            "create_worktree" | "register_worktree" | "remove_worktree" | "manage_skills"
            | "manage_mcp" | "manage_plugins",
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
