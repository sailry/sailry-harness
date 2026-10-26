use super::*;

pub(in crate::store) async fn execute(
    db: &Connection,
    caller: NodeId,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    let mut changed = Vec::new();
    let session = match &request.command {
        Command::ReadQueuedTurn { turn } => {
            let run = runs::get(db, *turn).await?;
            let session = super::super::super::commands::session(db, run.session).await?;
            if session.delegation.is_some()
                && !super::super::delegation::queued_guidance(db, run.turn).await?
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "child queued input is read-only",
                ));
            }
            return Ok((Output::QueuedMessage(message(db, *turn).await?), None));
        }
        Command::EditQueuedTurn {
            turn,
            expected_revision,
            message,
        } => {
            validate(message)?;
            let run = check_item(db, *turn, *expected_revision).await?;
            let mut session = super::super::super::commands::session(db, run.session).await?;
            super::super::super::sessions::writable(&session)?;
            session.config = super::super::permissions::config(db, run.turn).await?;
            session.roles = super::super::super::sessions::roles::for_turn(db, run.turn).await?;
            super::super::references::validate(
                db,
                &session,
                message,
                Some(&super::super::plugins::read(db, run.turn).await?),
            )
            .await?;
            if run.notice {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "background notice input is read-only",
                ));
            }
            if run.kind != RunKind::Task {
                return Err(sailry_protocol::Fault::invalid(
                    "context compaction has no editable input",
                ));
            }
            super::super::super::attachments::turns::bind(db, *turn, &message.attachments).await?;
            db.execute(
                "UPDATE agent_pending SET revision=revision+1,request=?2,caller=?3,recipient=NULL WHERE turn=?1",
                params![turn.to_string(), request.id.to_string(), &caller.0[..]],
            ).await
            .map_err(storage_error)?;
            bump(db, run.session).await?;
            changed.push(run.clone());
            run.session
        }
        Command::RemoveQueuedTurn {
            turn,
            expected_revision,
        } => {
            let run = check_item(db, *turn, *expected_revision).await?;
            let session = super::super::super::commands::session(db, run.session).await?;
            if session.delegation.is_some()
                && !super::super::delegation::queued_guidance(db, run.turn).await?
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "child queued input is read-only",
                ));
            }
            changed.push(runs::stop(db, *turn).await?);
            run.session
        }
        Command::SendQueuedTurn {
            turn,
            expected_revision,
        } => {
            let run = check_item(db, *turn, *expected_revision).await?;
            super::super::super::sessions::writable(
                &super::super::super::commands::session(db, run.session).await?,
            )?;
            let first = read(db, run.session)
                .await?
                .items
                .first()
                .map(|item| item.turn);
            reorder(db, run.session, *turn, first).await?;
            for active in runs::list(db, run.session, &[]).await? {
                if matches!(active.status, Status::Running | Status::Stopping) {
                    changed.push(runs::stop(db, active.turn).await?);
                }
            }
            pause(db, run.session, false).await?;
            changed.push(runs::start(db, *turn).await?);
            run.session
        }
        Command::GuideQueuedTurn {
            turn,
            recipient,
            expected_revision,
        } => {
            let (session, runs) =
                guidance::select(db, caller, request, *turn, *recipient, *expected_revision)
                    .await?;
            changed.extend(runs);
            session
        }
        Command::WithdrawGuidance {
            turn,
            expected_revision,
        } => {
            let run = check_item(db, *turn, *expected_revision).await?;
            let session = super::super::super::commands::session(db, run.session).await?;
            if session.delegation.is_some()
                && !super::super::delegation::queued_guidance(db, run.turn).await?
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "child queued input is read-only",
                ));
            }
            let affected = db
                .execute(
                    "UPDATE agent_pending SET recipient=NULL,revision=revision+1 WHERE turn=?1 AND recipient IS NOT NULL",
                    [turn.to_string()],
                )
                .await
                .map_err(storage_error)?;
            if affected == 0 {
                return Err(Fault::new(
                    ErrorCode::Conflict,
                    "queued input has no pending guidance",
                ));
            }
            db.execute(
                "UPDATE agent_runs SET ready=0 WHERE turn=?1",
                [turn.to_string()],
            )
            .await
            .map_err(storage_error)?;
            bump(db, run.session).await?;
            changed.push(run.clone());
            run.session
        }
        Command::MoveQueuedTurn {
            session,
            expected_revision,
            turn,
            before,
        } => {
            super::super::super::sessions::writable(
                &super::super::super::commands::session(db, *session).await?,
            )?;
            check_revision(read(db, *session).await?.revision, *expected_revision)?;
            reorder(db, *session, *turn, *before).await?;
            *session
        }
        Command::SetQueuePaused {
            session,
            expected_revision,
            paused,
        } => {
            super::super::super::sessions::writable(
                &super::super::super::commands::session(db, *session).await?,
            )?;
            check_revision(read(db, *session).await?.revision, *expected_revision)?;
            pause(db, *session, *paused).await?;
            if *paused {
                guidance::clear(db, *session, None).await?;
            }
            *session
        }
        _ => {
            return Err(sailry_protocol::Fault::invalid(
                "command does not operate on a queue",
            ));
        }
    };
    Ok((
        Output::Queue(QueueUpdate {
            session,
            queue: read(db, session).await?,
            runs: changed,
        }),
        Some(super::super::activity::changed(db, session).await?),
    ))
}
