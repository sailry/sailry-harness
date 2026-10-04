//! Input metadata and accepted answers reference the single ADK history.
use super::*;
use adk_core::ToolConfirmationRequest;
use question::{Question, Response, Spec, State};
use std::collections::BTreeMap;

mod planning;
pub(in crate::store) use planning::accept;

pub(in crate::store) type Waiting =
    BTreeMap<QuestionId, (TurnId, oneshot::Sender<Result<Response, Fault>>)>;

impl Ingress {
    pub(crate) async fn ask(
        &self,
        turn: TurnId,
        request: ToolConfirmationRequest,
    ) -> Result<Response, Fault> {
        let (reply, response) = oneshot::channel();
        self.sender
            .send(Job::Agent(Box::new(Operation::Ask {
                turn,
                request,
                reply,
            })))
            .await
            .map_err(|_| super::super::unavailable())?;
        response.await.map_err(|_| super::super::unavailable())?
    }
}

pub(super) fn begin(
    database: &mut Database,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Question, Fault> {
    if !matches!(request.tool_name.as_str(), "ask_user" | elicitation::TOOL) {
        return Err(invalid("tool does not accept user input"));
    }
    let spec: Spec = serde_json::from_value(request.args.clone())
        .map_err(|_| invalid("invalid question arguments"))?;
    spec.validate()?;
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let run = calls::active(&transaction, turn)?;
    if matches!(spec.input, question::Input::Plan) {
        planning::validate(&transaction, turn, run.session)?;
    }
    let (entry, index) = if request.tool_name == elicitation::TOOL {
        elicitation::locate(&transaction, turn, request)?
    } else {
        if matches!(
            spec.input,
            question::Input::Form { .. } | question::Input::Url { .. }
        ) {
            return Err(invalid("this input requires an MCP request"));
        }
        calls::locate(&transaction, turn, request)?
    };
    let call = calls::id(request)?;
    let exists: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_questions WHERE turn=?1 AND call=?2)",
            params![turn.to_string(), call],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    if exists {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "tool call already requested user input",
        ));
    }
    let question = Question {
        id: QuestionId::new(),
        session: run.session,
        turn,
        entry,
        index,
        state: State::Pending,
    };
    transaction
        .execute(
            "INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,?3,?4)",
            params![
                question.id.to_string(),
                turn.to_string(),
                call,
                encode(&question)?
            ],
        )
        .map_err(storage_error)?;
    let envelope = changed(&transaction, database.node, run.session)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    database.feeds.publish(
        database.node,
        run.session,
        Change::Question(question.clone()),
    );
    Ok(question)
}

pub(in crate::store) fn resolve(
    db: &Connection,
    session: SessionId,
    id: QuestionId,
    response: &Response,
) -> Result<Question, Fault> {
    let mut question = get(db, id)?;
    if question.session != session {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "question belongs to another session",
        ));
    }
    let state = match response {
        Response::Decline => {
            if !matches!(
                spec(db, &question)?.input,
                question::Input::Form { .. } | question::Input::Url { .. }
            ) {
                return Err(invalid("decline requires an MCP request"));
            }
            State::Declined
        }
        Response::Answer(answer) => State::Answered(answer.clone()),
        Response::Cancel => State::Cancelled,
        Response::StartCoding { .. } => {
            return Err(invalid("plan acceptance requires turn admission"));
        }
    };
    if question.state == state {
        return Ok(question);
    }
    if question.state != State::Pending {
        return Err(Fault::new(
            ErrorCode::Conflict,
            "question has already closed",
        ));
    }
    calls::active(db, question.turn)?;
    if let Response::Answer(answer) = response {
        spec(db, &question)?.validate_answer(answer)?;
        elicitation::validate_answer(db, &question, answer)?;
    }
    question.state = state;
    put(db, &question)?;
    Ok(question)
}

fn spec(db: &Connection, question: &Question) -> Result<Spec, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![question.turn.to_string(), question.entry],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    if question.index == 0
        && let Some(spec) = elicitation::spec(&event)?
    {
        return Ok(spec);
    }
    let Some(adk_core::Part::FunctionCall { name, args, .. }) = event
        .content()
        .and_then(|content| content.parts.get(question.index))
    else {
        return Err(invalid("question does not reference a canonical tool call"));
    };
    if name != "ask_user" {
        return Err(invalid("question references another tool"));
    }
    serde_json::from_value(args.clone()).map_err(storage_error)
}

pub(super) fn close(db: &Connection, turn: TurnId, state: State) -> Result<Vec<Question>, Fault> {
    let mut query = db.prepare("SELECT body FROM agent_questions WHERE turn=?1 AND json_extract(body,'$.state.kind')='pending'").map_err(storage_error)?;
    let mut questions = query
        .query_map([turn.to_string()], |row| row.get::<_, Vec<u8>>(0))
        .map_err(storage_error)?
        .map(|body| {
            serde_json::from_slice::<Question>(&body.map_err(storage_error)?).map_err(storage_error)
        })
        .collect::<Result<Vec<_>, Fault>>()?;
    for question in &mut questions {
        question.state = state.clone();
        put(db, question)?;
    }
    Ok(questions)
}

pub(super) fn list(
    db: &Connection,
    session: SessionId,
    entries: &[Entry],
) -> Result<Vec<Question>, Fault> {
    let ids: Vec<_> = entries.iter().map(|entry| &entry.id).collect();
    let mut query = db.prepare("SELECT q.body FROM agent_questions q JOIN conversation_turns h ON h.turn=q.turn WHERE h.session=?1 AND (json_extract(q.body,'$.state.kind')='pending' OR json_extract(q.body,'$.entry') IN (SELECT value FROM json_each(?2))) ORDER BY q.rowid").map_err(storage_error)?;
    query
        .query_map(
            params![
                session.to_string(),
                serde_json::to_string(&ids).map_err(storage_error)?
            ],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .map_err(storage_error)?
        .map(|body| {
            let mut question: Question =
                serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
            question.session = session;
            Ok(question)
        })
        .collect()
}

fn get(db: &Connection, id: QuestionId) -> Result<Question, Fault> {
    let body: Option<Vec<u8>> = db
        .query_row(
            "SELECT body FROM agent_questions WHERE id=?1",
            [id.to_string()],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "question does not exist"))?,
    )
    .map_err(storage_error)
}

fn put(db: &Connection, question: &Question) -> Result<(), Fault> {
    db.execute(
        "UPDATE agent_questions SET body=?2 WHERE id=?1",
        params![question.id.to_string(), encode(question)?],
    )
    .map_err(storage_error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
