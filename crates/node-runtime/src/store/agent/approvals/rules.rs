use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub owner: plugin::Reference,
    pub tool: String,
    pub operation: Option<sailry_protocol::tool::Operation>,
}

pub(super) fn validate(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    binding: &Binding,
) -> Result<(), Fault> {
    if binding.tool != request.tool_name
        || !super::super::plugins::read(db, turn)?.contains(&binding.owner)
    {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "approval owner is outside the admitted tool catalog",
        ));
    }
    Ok(())
}

pub(super) fn command<'request>(
    binding: &Binding,
    request: &'request ToolConfirmationRequest,
) -> Option<&'request str> {
    (binding.operation == Some(sailry_protocol::tool::Operation::RunCommand))
        .then(|| request.args.get("command")?.as_str())
        .flatten()
}

pub(super) fn read(db: &Connection, id: ApprovalId) -> Result<Option<Binding>, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT binding FROM agent_approvals WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    serde_json::from_slice(&body).map_err(storage_error)
}

pub(super) fn matching(
    db: &Connection,
    run: &Run,
    binding: &Binding,
    request: &ToolConfirmationRequest,
) -> Result<Option<(ApprovalSource, ApprovalChoice)>, Fault> {
    let mut query = db.prepare("SELECT choice FROM agent_approval_rules WHERE session=?1 AND worktree=?2 AND binding=?3 ORDER BY rowid").map_err(storage_error)?;
    let choices = query
        .query_map(
            params![
                run.session.to_string(),
                run.worktree.to_string(),
                encode(binding)?
            ],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?;
    let mut matched = None;
    for choice in choices {
        let choice: ApprovalChoice =
            serde_json::from_slice(&choice.map_err(storage_error)?).map_err(storage_error)?;
        match &choice {
            ApprovalChoice::Session => return Ok(Some((ApprovalSource::SessionRule, choice))),
            ApprovalChoice::Prefix(prefix)
                if command(binding, request)
                    .is_some_and(|command| super::prefix::matches(command, prefix)) =>
            {
                matched = Some((ApprovalSource::PrefixRule, choice))
            }
            _ => {}
        }
    }
    Ok(matched)
}

pub(super) fn insert(
    db: &Connection,
    run: &Run,
    binding: &Binding,
    choice: &ApprovalChoice,
    id: ApprovalId,
) -> Result<(), Fault> {
    db.execute("INSERT OR IGNORE INTO agent_approval_rules(session,worktree,binding,choice,approval) VALUES(?1,?2,?3,?4,?5)", params![run.session.to_string(),run.worktree.to_string(),encode(binding)?,encode(choice)?,id.to_string()]).map_err(storage_error)?;
    Ok(())
}

pub(super) fn check_prepared(approval: &Approval, prepared: Option<&Command>) -> Result<(), Fault> {
    if let Some(ApprovalChoice::Prefix(prefix)) = &approval.choice {
        let Some(Command::RunCommand { command, .. }) = prepared else {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "prepared command is outside the approved prefix",
            ));
        };
        if !super::prefix::matches(command, prefix) {
            return Err(Fault::new(
                ErrorCode::PermissionDenied,
                "prepared command is outside the approved prefix",
            ));
        }
    }
    Ok(())
}
