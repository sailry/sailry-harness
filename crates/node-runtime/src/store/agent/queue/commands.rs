use super::*;

pub(in crate::store) fn execute(
    db: &Connection,
    caller: NodeId,
    request: &Request,
) -> Result<(Output, Option<Event>), Fault> {
    let mut changed = Vec::new();
    let session = match &request.command {
        Command::ReadQueuedTurn { turn } => {
            let run = runs::get(db, *turn)?;
            let session = super::super::super::commands::session(db, run.session)?;
            if session.delegation.is_some()
                && !super::super::delegation::queued_guidance(db, run.turn)?
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "child queued input is read-only",
                ));
            }
            return Ok((Output::QueuedMessage(message(db, *turn)?), None));
        }
        Command::EditQueuedTurn {
            turn,
            expected_revision,
            message,
        } => {
            validate(message)?;
            let run = check_item(db, *turn, *expected_revision)?;
            let mut session = super::super::super::commands::session(db, run.session)?;
            super::super::super::sessions::writable(&session)?;
            session.config = super::super::permissions::config(db, run.turn)?;
            session.roles =
                super::super::super::sessions::roles::read(db, run.session, run.revision)?;
            super::super::references::validate(
                db,
                &session,
                message,
                Some(&super::super::plugins::read(db, run.turn)?),
            )?;
            if run.kind != RunKind::Task {
                return Err(sailry_protocol::Fault::invalid(
                    "context compaction has no editable input",
                ));
            }
            super::super::super::attachments::turns::bind(db, *turn, &message.attachments)?;
            db.execute(
                "UPDATE agent_pending SET revision=revision+1,request=?2,caller=?3,recipient=NULL WHERE turn=?1",
                params![turn.to_string(), request.id.to_string(), &caller.0[..]],
            )
            .map_err(storage_error)?;
            bump(db, run.session)?;
            run.session
        }
        Command::RemoveQueuedTurn {
            turn,
            expected_revision,
        } => {
            let run = check_item(db, *turn, *expected_revision)?;
            let session = super::super::super::commands::session(db, run.session)?;
            if session.delegation.is_some()
                && !super::super::delegation::queued_guidance(db, run.turn)?
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "child queued input is read-only",
                ));
            }
            changed.push(runs::stop(db, *turn)?);
            run.session
        }
        Command::SendQueuedTurn {
            turn,
            expected_revision,
        } => {
            let run = check_item(db, *turn, *expected_revision)?;
            super::super::super::sessions::writable(&super::super::super::commands::session(
                db,
                run.session,
            )?)?;
            let first = read(db, run.session)?.items.first().map(|item| item.turn);
            reorder(db, run.session, *turn, first)?;
            for active in runs::list(db, run.session, &[])? {
                if matches!(active.status, Status::Running | Status::Stopping) {
                    changed.push(runs::stop(db, active.turn)?);
                }
            }
            pause(db, run.session, false)?;
            changed.push(runs::start(db, *turn)?);
            run.session
        }
        Command::GuideQueuedTurn {
            turn,
            recipient,
            expected_revision,
        } => {
            let (session, runs) =
                guidance::select(db, caller, request, *turn, *recipient, *expected_revision)?;
            changed.extend(runs);
            session
        }
        Command::MoveQueuedTurn {
            session,
            expected_revision,
            turn,
            before,
        } => {
            super::super::super::sessions::writable(&super::super::super::commands::session(
                db, *session,
            )?)?;
            check_revision(read(db, *session)?.revision, *expected_revision)?;
            reorder(db, *session, *turn, *before)?;
            *session
        }
        Command::SetQueuePaused {
            session,
            expected_revision,
            paused,
        } => {
            super::super::super::sessions::writable(&super::super::super::commands::session(
                db, *session,
            )?)?;
            check_revision(read(db, *session)?.revision, *expected_revision)?;
            pause(db, *session, *paused)?;
            if *paused {
                guidance::clear(db, *session, None)?;
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
            queue: read(db, session)?,
            runs: changed,
        }),
        Some(super::super::activity::changed(db, session)?),
    ))
}
