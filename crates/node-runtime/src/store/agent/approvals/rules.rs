use super::*;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Binding {
    pub owner: plugin::Reference,
    pub tool: String,
    pub operation: Option<sailry_protocol::tool::Operation>,
}

pub(super) async fn validate(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    binding: &Binding,
) -> Result<(), Fault> {
    if binding.tool != request.tool_name
        || !super::super::plugins::read(db, turn)
            .await?
            .contains(&binding.owner)
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

pub(super) async fn read(db: &Connection, id: ApprovalId) -> Result<Option<Binding>, Fault> {
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT binding FROM agent_approvals WHERE id=?1")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row([id.to_string()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    serde_json::from_slice(&body).map_err(storage_error)
}

pub(super) async fn matching(
    db: &Connection,
    run: &Run,
    binding: &Binding,
    request: &ToolConfirmationRequest,
) -> Result<Option<(ApprovalSource, ApprovalChoice)>, Fault> {
    let mut query = db.prepare("SELECT choice FROM agent_approval_rules WHERE session=?1 AND worktree=?2 AND binding=?3 ORDER BY rowid").await.map_err(storage_error)?;
    let choices = {
        let mut rows = query
            .query(params![
                run.session.to_string(),
                run.worktree.to_string(),
                encode(binding)?
            ])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    };
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

pub(super) async fn insert(
    db: &Connection,
    run: &Run,
    binding: &Binding,
    choice: &ApprovalChoice,
    id: ApprovalId,
) -> Result<(), Fault> {
    db.execute("INSERT OR IGNORE INTO agent_approval_rules(session,worktree,binding,choice,approval) VALUES(?1,?2,?3,?4,?5)", params![run.session.to_string(),run.worktree.to_string(),encode(binding)?,encode(choice)?,id.to_string()]).await.map_err(storage_error)?;
    Ok(())
}

pub(super) async fn apply_session(
    db: &Connection,
    waiting: &std::sync::Mutex<Waiting>,
    source: &Run,
    binding: &Binding,
) -> Result<(), Fault> {
    let mut query = db.prepare("SELECT body FROM agent_approvals WHERE json_extract(body,'$.session')=?1 AND binding=?2 AND json_extract(body,'$.state')='pending' ORDER BY rowid").await.map_err(storage_error)?;
    let approvals = {
        let mut rows = query
            .query(params![source.session.to_string(), encode(binding)?])
            .await
            .map_err(storage_error)?;
        let mut values = Vec::new();
        while let Some(row) = rows.next().await.map_err(storage_error)? {
            let mapped: turso::Result<_> = (|row: &turso::Row| row.get::<Vec<u8>>(0))(&row);
            values.push(mapped);
        }
        values.into_iter()
    }
    .map(|body| {
        serde_json::from_slice::<Approval>(&body.map_err(storage_error)?).map_err(storage_error)
    })
    .collect::<Result<Vec<_>, Fault>>()?;
    let resource = super::super::permissions::config(db, source.turn)
        .await?
        .resource;
    for mut approval in approvals {
        if !approval.can_remember
            || approval.source != ApprovalSource::User
            || approval.startup.is_some()
            || approval.ssh.is_some()
        {
            continue;
        }
        let run = super::super::runs::get(db, approval.turn).await?;
        if run.kind != RunKind::Task
            || run.status != Status::Running
            || run.session != source.session
            || run.worktree != source.worktree
            || super::super::permissions::config(db, run.turn)
                .await?
                .resource
                != resource
        {
            continue;
        }
        let request = super::request(db, &approval).await?;
        validate(db, approval.turn, &request, binding).await?;
        if matching(db, &run, binding, &request).await?
            != Some((ApprovalSource::SessionRule, ApprovalChoice::Session))
        {
            continue;
        }
        if super::check_waiter(waiting, approval.id).is_err() {
            continue;
        }
        approval.state = ApprovalState::Approved;
        approval.source = ApprovalSource::SessionRule;
        approval.choice = Some(ApprovalChoice::Session);
        super::put(db, &approval).await?;
    }
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
