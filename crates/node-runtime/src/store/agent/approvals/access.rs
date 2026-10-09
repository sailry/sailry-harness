use super::{Binding, ToolConfirmationRequest};
use sailry_protocol::{
    Command, ErrorCode, Fault,
    conversation::{Approval, ApprovalChoice, ApprovalSource},
    process::Access,
    tool::Operation,
};

pub(super) fn requested(
    db: &rusqlite::Connection,
    turn: sailry_protocol::TurnId,
    request: &ToolConfirmationRequest,
    binding: Option<&Binding>,
) -> Result<Option<Access>, Fault> {
    let command = binding.is_some_and(|binding| binding.operation == Some(Operation::RunCommand));
    if command {
        return super::super::super::sandbox::effective(db, &request.args).map(Some);
    }
    if binding.is_some() && super::super::plugins::command_flow(db, turn, &request.tool_name)? {
        return super::super::super::sandbox::effective(db, &serde_json::json!({})).map(Some);
    }
    match request.args.get("access") {
        Some(value) if value == "unrestricted" => serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| Fault::invalid(format!("invalid command access: {error}"))),
        _ => Ok(None),
    }
}

pub(super) fn check(approval: &Approval, prepared: Option<&Command>) -> Result<(), Fault> {
    let Some(Command::RunCommand { access, .. }) = prepared else {
        if approval.source == ApprovalSource::Sandbox {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "sandbox approval requires a confined command",
            ));
        }
        return Ok(());
    };
    if approval.access != Some(*access) {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "prepared command access differs from approval",
        ));
    }
    if crate::process::sandbox::SUPPORTED
        && *access == Access::Unrestricted
        && (approval.access != Some(Access::Unrestricted)
            || !(approval.source == ApprovalSource::Full
                || (approval.source == ApprovalSource::User
                    && approval.choice == Some(ApprovalChoice::Once))))
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "prepared command requires unrestricted authority",
        ));
    }
    Ok(())
}
