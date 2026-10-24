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
        let _waiting = self
            .agents
            .timeline(turn)
            .map(|timeline| timeline.span(crate::agent::observation::Phase::Excluded));
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

pub(super) async fn begin(
    database: &mut Database,
    turn: TurnId,
    request: &ToolConfirmationRequest,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<Question, Fault> {
    if !matches!(
        request.tool_name.as_str(),
        "ask_user" | elicitation::TOOL | review::TOOL
    ) {
        return Err(sailry_protocol::Fault::invalid(
            "tool does not accept user input",
        ));
    }
    let spec: Spec = serde_json::from_value(request.args.clone())
        .map_err(|_| sailry_protocol::Fault::invalid("invalid question arguments"))?;
    spec.validate()?;
    if request.tool_name == "ask_user" && spec.prompt.is_empty() {
        return Err(sailry_protocol::Fault::invalid("question prompt is empty"));
    }
    let transaction = database
        .connection
        .unchecked_transaction()
        .await
        .map_err(storage_error)?;
    let run = calls::active(&transaction, turn).await?;
    if matches!(spec.input, question::Input::Plan) {
        planning::validate(&transaction, turn, run.session).await?;
    }
    let (entry, index) = if request.tool_name == review::TOOL {
        review::locate(&transaction, turn, request).await?
    } else if request.tool_name == elicitation::TOOL {
        elicitation::locate(&transaction, turn, request).await?
    } else {
        if matches!(
            spec.input,
            question::Input::Form { .. } | question::Input::Url { .. }
        ) {
            return Err(sailry_protocol::Fault::invalid(
                "this input requires an MCP request",
            ));
        }
        calls::locate(&transaction, turn, request).await?
    };
    let call = calls::id(request)?;
    let question = create(&transaction, &run, call, entry, index).await?;
    let envelope = changed(&transaction, database.node, run.session).await?;
    transaction.commit().await.map_err(storage_error)?;
    let _ = events.send(envelope);
    database.feeds.publish(
        database.node,
        run.session,
        Change::Question(question.clone()),
    );
    Ok(question)
}

pub(super) async fn create(
    db: &Connection,
    run: &Run,
    call: &str,
    entry: String,
    index: usize,
) -> Result<Question, Fault> {
    let turn = run.turn;
    let exists: bool = {
        let mut statement = db
            .prepare("SELECT EXISTS(SELECT 1 FROM agent_questions WHERE turn=?1 AND call=?2)")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![turn.to_string(), call])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
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
    db.execute(
        "INSERT INTO agent_questions(id,turn,call,body) VALUES(?1,?2,?3,?4)",
        params![
            question.id.to_string(),
            turn.to_string(),
            call,
            encode(&question)?
        ],
    )
    .await
    .map_err(storage_error)?;
    Ok(question)
}

pub(in crate::store) async fn resolve(
    db: &Connection,
    session: SessionId,
    id: QuestionId,
    response: &Response,
) -> Result<Question, Fault> {
    let mut question = get(db, id).await?;
    if question.session != session {
        return Err(Fault::new(
            ErrorCode::WrongTarget,
            "question belongs to another session",
        ));
    }
    let state = match response {
        Response::Decline => {
            if !matches!(
                spec(db, &question).await?.input,
                question::Input::Form { .. } | question::Input::Url { .. }
            ) {
                return Err(sailry_protocol::Fault::invalid(
                    "decline requires an MCP request",
                ));
            }
            State::Declined
        }
        Response::Answer(answer) => State::Answered(answer.clone()),
        Response::Cancel => State::Cancelled,
        Response::StartCoding { .. } => {
            return Err(sailry_protocol::Fault::invalid(
                "plan acceptance requires turn admission",
            ));
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
    if !review::completed(db, &question).await? {
        calls::active(db, question.turn).await?;
    }
    if let Response::Answer(answer) = response {
        spec(db, &question).await?.validate_answer(answer)?;
        elicitation::validate_answer(db, &question, answer).await?;
    }
    question.state = state;
    put(db, &question).await?;
    Ok(question)
}

async fn spec(db: &Connection, question: &Question) -> Result<Spec, Fault> {
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![question.turn.to_string(), question.entry.as_str()])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
        mapped.map_err(storage_error)?
    };
    let event: AdkEvent = serde_json::from_slice(&body).map_err(storage_error)?;
    if question.index == 0
        && let Some(spec) = elicitation::spec(&event)?
    {
        return Ok(spec);
    }
    if question.index == 0
        && let Some(spec) = review::spec(&event)
    {
        return Ok(spec);
    }
    let Some(adk_core::Part::FunctionCall { name, args, .. }) = event
        .content()
        .and_then(|content| content.parts.get(question.index))
    else {
        return Err(sailry_protocol::Fault::invalid(
            "question does not reference a canonical tool call",
        ));
    };
    if name != "ask_user" {
        return Err(sailry_protocol::Fault::invalid(
            "question references another tool",
        ));
    }
    serde_json::from_value(args.clone()).map_err(storage_error)
}

pub(super) async fn close(
    db: &Connection,
    turn: TurnId,
    state: State,
) -> Result<Vec<Question>, Fault> {
    let mut query = db.prepare("SELECT body FROM agent_questions WHERE turn=?1 AND json_extract(body,'$.state.kind')='pending'").await.map_err(storage_error)?;
    let mut questions = {
        let mut rows = query
            .query([turn.to_string()])
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
        serde_json::from_slice::<Question>(&body.map_err(storage_error)?).map_err(storage_error)
    })
    .collect::<Result<Vec<_>, Fault>>()?;
    for question in &mut questions {
        question.state = state.clone();
        put(db, question).await?;
    }
    Ok(questions)
}

pub(super) async fn list(
    db: &Connection,
    session: SessionId,
    entries: &[Entry],
) -> Result<Vec<Question>, Fault> {
    let ids: Vec<_> = entries.iter().map(|entry| &entry.id).collect();
    let mut query = db.prepare("SELECT q.body FROM agent_questions q JOIN conversation_turns h ON h.turn=q.turn WHERE h.session=?1 AND (json_extract(q.body,'$.state.kind')='pending' OR json_extract(q.body,'$.entry') IN (SELECT value FROM json_each(?2))) ORDER BY q.rowid").await.map_err(storage_error)?;
    {
        let mut rows = query
            .query(params![
                session.to_string(),
                serde_json::to_string(&ids).map_err(storage_error)?
            ])
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
        let mut question: Question =
            serde_json::from_slice(&body.map_err(storage_error)?).map_err(storage_error)?;
        question.session = session;
        Ok(question)
    })
    .collect()
}

async fn get(db: &Connection, id: QuestionId) -> Result<Question, Fault> {
    let body: Option<Vec<u8>> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_questions WHERE id=?1")
            .await
            .map_err(storage_error)?;
        match statement.query_row([id.to_string()]).await {
            Ok(row) => {
                let mapped: turso::Result<_> = (|row: &turso::Row| row.get(0))(&row);
                Some(mapped.map_err(storage_error)?)
            }
            Err(turso::Error::QueryReturnedNoRows) => None,
            Err(error) => return Err(storage_error(error).into()),
        }
    };
    serde_json::from_slice(
        &body.ok_or_else(|| Fault::new(ErrorCode::NotFound, "question does not exist"))?,
    )
    .map_err(storage_error)
}

pub(super) async fn put(db: &Connection, question: &Question) -> Result<(), Fault> {
    db.execute(
        "UPDATE agent_questions SET body=?2 WHERE id=?1",
        params![question.id.to_string(), encode(question)?],
    )
    .await
    .map_err(storage_error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
