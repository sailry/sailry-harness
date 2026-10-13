//! Successful synthetic reviews remain questions, not active tool invocations.
use super::*;
use question::{Question, State};

pub(in crate::store) type Changes = Vec<(SessionId, Change)>;

pub(in crate::store::agent) fn completed(
    db: &Connection,
    question: &Question,
) -> Result<bool, Fault> {
    if question.index != 0 {
        return Ok(false);
    }
    let run = runs::get(db, question.turn)?;
    if run.session != question.session
        || run.kind != RunKind::Task
        || run.status != Status::Completed
        || run.error.is_some()
    {
        return Ok(false);
    }
    let body: Option<Vec<u8>> = db.query_row(
        "SELECT e.body FROM agent_events e JOIN conversation_events h ON h.sequence=e.sequence WHERE h.session=?1 AND e.turn=?2 AND e.id=?3",
        params![question.session.to_string(),question.turn.to_string(),question.entry],|row| row.get(0),
    ).optional().map_err(storage_error)?;
    let Some(body) = body else {
        return Ok(false);
    };
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    if spec(&event).is_none() {
        return Ok(false);
    }
    validate(db, question.turn, &event)?;
    Ok(true)
}

pub(in crate::store::agent) fn publish(
    db: &Connection,
    run: &Run,
    event: AdkEvent,
) -> Result<Changes, Fault> {
    if run.status != Status::Completed || run.error.is_some() || spec(&event).is_none() {
        return Err(sailry_protocol::Fault::invalid(
            "review requires successful plan completion",
        ));
    }
    let call = event.id.clone();
    let entry = super::super::history::insert(db, run.session, run.turn, event)?
        .ok_or_else(|| Fault::new(ErrorCode::Conflict, "completion review already exists"))?;
    let question = super::super::questions::create(db, run, &call, call.clone(), 0)?;
    if !completed(db, &question)? {
        return Err(sailry_protocol::Fault::invalid(
            "completion review is not eligible",
        ));
    }
    Ok(vec![
        (run.session, Change::Entry(entry)),
        (run.session, Change::Question(question)),
    ])
}

fn resolved(db: &Connection, question: &Question) -> Result<Changes, Fault> {
    if !completed(db, question)? {
        return Ok(Vec::new());
    }
    let response = match &question.state {
        State::Answered(answer) => Response::Answer(answer.clone()),
        State::Cancelled => Response::Cancel,
        _ => {
            return Err(sailry_protocol::Fault::invalid(
                "completion review is not resolved",
            ));
        }
    };
    let id = format!("plan-review-result:{}", question.entry);
    let previous: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![question.turn.to_string(), id],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    if let Some(previous) = previous {
        let previous: AdkEvent = serde_json::from_slice(&previous).map_err(storage_error)?;
        let recorded: Recorded = serde_json::from_str(
            previous
                .provider_metadata
                .get(RESULT)
                .ok_or_else(|| Fault::invalid("review result metadata is missing"))?,
        )
        .map_err(storage_error)?;
        if previous.author != TOOL
            || recorded.call != question.entry
            || recorded.response != response
        {
            return Err(Fault::new(
                ErrorCode::Conflict,
                "review result already differs",
            ));
        }
        return Ok(Vec::new());
    }
    let mut event = result(question.turn, question.entry.clone(), response);
    // Separate successful command IDs answering the same question must not append twice.
    event.id = id;
    Ok(
        super::super::history::insert(db, question.session, question.turn, event)?
            .into_iter()
            .map(|entry| (question.session, Change::Entry(entry)))
            .collect(),
    )
}

pub(in crate::store::agent) fn supersede(
    db: &Connection,
    session: SessionId,
) -> Result<Changes, Fault> {
    let mut query = db.prepare("SELECT q.body FROM agent_questions q JOIN conversation_turns h ON h.turn=q.turn WHERE h.session=?1 AND json_extract(q.body,'$.state.kind')='pending'").map_err(storage_error)?;
    let questions = query
        .query_map([session.to_string()], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|row| {
            serde_json::from_slice::<Question>(&row.map_err(storage_error)?).map_err(storage_error)
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    let mut changes = Vec::new();
    for mut question in questions {
        if question.session != session || !completed(db, &question)? {
            continue;
        }
        question.state = State::Cancelled;
        super::super::questions::put(db, &question)?;
        changes.extend(resolved(db, &question)?);
        changes.push((session, Change::Question(question)));
    }
    Ok(changes)
}

pub(in crate::store) fn command(
    db: &Connection,
    request: &Request,
    output: &Output,
) -> Result<Changes, Fault> {
    match (&request.command, output) {
        (
            Command::SubmitTurn { .. } | Command::QueueTurn { .. } | Command::SubmitTask { .. },
            Output::QueuedTurn(turn),
        ) => supersede(db, turn.session),
        (Command::ResolveQuestion { .. }, Output::Question(question)) => resolved(db, question),
        (Command::ResolveQuestion { .. }, Output::PlanAccepted(accepted)) => {
            resolved(db, &accepted.question)
        }
        (Command::PluginTransaction { operations }, Output::PluginTransaction(outputs)) => {
            let mut changes = Vec::new();
            for (operation, output) in operations.iter().zip(outputs) {
                if let (
                    sailry_protocol::plugin::transaction::Operation::Submit { .. },
                    Output::QueuedTurn(turn),
                ) = (operation, output)
                {
                    changes.extend(supersede(db, turn.session)?);
                }
            }
            Ok(changes)
        }
        _ => Ok(Vec::new()),
    }
}
