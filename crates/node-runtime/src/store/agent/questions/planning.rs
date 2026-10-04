use super::*;
use crate::store::commands;
use question::{AcceptedPlan, Answer, Input};

pub(super) fn validate(db: &Connection, turn: TurnId, session: SessionId) -> Result<(), Fault> {
    if super::super::permissions::config(db, turn)?.mode != WorkMode::Plan {
        return Err(Fault::new(
            ErrorCode::PermissionDenied,
            "plan review requires a planning turn",
        ));
    }
    crate::store::sessions::writable(&commands::session(db, session)?)
}

pub(in crate::store) fn accept(
    db: &Connection,
    caller: NodeId,
    request: &Request,
) -> Result<AcceptedPlan, Fault> {
    let Command::ResolveQuestion {
        session: id,
        question: question_id,
        response:
            Response::StartCoding {
                expected_revision,
                message,
            },
    } = &request.command
    else {
        return Err(invalid("request does not accept a plan"));
    };
    let mut question = get(db, *question_id)?;
    if question.session != *id {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "plan belongs to another session",
        ));
    }
    if question.state != State::Pending {
        return Err(Fault::new(ErrorCode::Conflict, "plan has already closed"));
    }
    calls::active(db, question.turn)?;
    if !matches!(spec(db, &question)?.input, Input::Plan) {
        return Err(invalid("question does not contain a plan"));
    }
    let mut session = commands::session(db, *id)?;
    crate::store::sessions::writable(&session)?;
    commands::check_revision(session.revision, *expected_revision)?;
    if session.config.mode != WorkMode::Code {
        session.config.mode = WorkMode::Code;
        let (Output::Session(updated), _) = commands::revise_session(db, session)? else {
            unreachable!("session revision returns the updated session");
        };
        session = updated;
    }
    let turn = super::super::queue::create(db, caller, request, session.clone(), message, true)?;
    super::super::queue::prioritize(db, &turn)?;
    question.state = State::Answered(Answer::Plan { turn: turn.id });
    put(db, &question)?;
    session.activity = super::super::activity::read(db, session.id)?;
    Ok(AcceptedPlan {
        question,
        session,
        turn,
    })
}
