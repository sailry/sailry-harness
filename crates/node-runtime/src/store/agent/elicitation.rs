//! MCP input lives in canonical notices, outside provider tool-call batches.
use super::*;
use crate::agent::canonical::ToolIdentity;
use crate::agent::canonical::{Kind, Notice, Record};
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEY: &str = "sailry.mcp.elicitation";
const RESULT: &str = "sailry.mcp.elicitation_result";
pub(crate) const TOOL: &str = "mcp_elicitation";

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Input {
    pub parent: ToolIdentity,
    pub request: Value,
    pub spec: question::Spec,
    pub schema: Value,
}

pub(crate) fn event(turn: TurnId, input: &Input) -> Record {
    let mut event = Record::new(
        turn,
        Kind::Notice(Notice::Elicitation {
            parent: input.parent.clone(),
            request: input.request.clone(),
            spec: input.spec.clone(),
            schema: input.schema.clone(),
        }),
    );
    event.provenance.author = TOOL.into();
    event
}

pub(crate) fn result(turn: TurnId, call: String, response: &question::Response) -> Record {
    let mut event = Record::new(
        turn,
        Kind::Notice(Notice::ElicitationResult {
            call,
            response: response.clone(),
        }),
    );
    event.provenance.author = TOOL.into();
    event
}

fn input(event: &Record) -> Result<Option<Input>, Fault> {
    let Kind::Notice(Notice::Elicitation {
        parent,
        request,
        spec,
        schema,
    }) = &event.kind
    else {
        return Ok(None);
    };
    let mut identity = ToolIdentity::new(parent.name.clone(), parent.arguments.clone());
    identity.id = parent.id.clone();
    Ok(Some(Input {
        parent: identity,
        request: request.clone(),
        spec: spec.clone(),
        schema: schema.clone(),
    }))
}

pub(super) async fn validate(db: &Connection, turn: TurnId, event: &Record) -> Result<(), Fault> {
    if let Some(input) = input(event)? {
        calls::active(db, turn).await?;
        if !matches!(
            input.spec.input,
            question::Input::Form { .. } | question::Input::Url { .. }
        ) {
            return Err(sailry_protocol::Fault::invalid(
                "MCP input must not change model content",
            ));
        }
        input.spec.validate()?;
        calls::locate(db, turn, &input.parent).await?;
    }
    Ok(())
}

pub(super) fn project(event: &Record) -> Result<Vec<history::tools::Raw>, Fault> {
    if let Some(input) = input(event)? {
        return Ok(vec![history::tools::Raw::new(
            Some(event.id.clone()),
            TOOL.into(),
            serde_json::to_value(input.spec).map_err(storage_error)?,
        )]);
    }
    if let Kind::Notice(Notice::ElicitationResult { call, response }) = &event.kind {
        let mut raw = history::tools::Raw::new(Some(call.clone()), TOOL.into(), Value::Null);
        raw.result = Some(serde_json::to_value(response).map_err(storage_error)?);
        return Ok(vec![raw]);
    }
    Ok(Vec::new())
}

async fn read(db: &Connection, turn: TurnId, call: &str) -> Result<Input, Fault> {
    let body: Vec<u8> = {
        let mut statement = db
            .prepare("SELECT body FROM agent_events WHERE turn=?1 AND id=?2")
            .await
            .map_err(storage_error)?;
        let row = statement
            .query_row(params![turn.to_string(), call])
            .await
            .map_err(storage_error)?;
        let mapped: turso::Result<_> = row.get(0);
        mapped.map_err(storage_error)?
    };
    input(&serde_json::from_slice(&body).map_err(storage_error)?)?
        .ok_or_else(|| sailry_protocol::Fault::invalid("MCP input has no canonical request"))
}

pub(super) async fn locate(
    db: &Connection,
    turn: TurnId,
    request: &ToolIdentity,
) -> Result<(String, usize), Fault> {
    let call = calls::id(request)?;
    let input = read(db, turn, call).await?;
    if request.name != TOOL
        || serde_json::to_value(&input.spec).map_err(storage_error)? != request.arguments
    {
        return Err(sailry_protocol::Fault::invalid(
            "MCP input arguments differ",
        ));
    }
    calls::locate(db, turn, &input.parent).await?;
    Ok((call.into(), 0))
}

pub(super) fn spec(event: &Record) -> Result<Option<question::Spec>, Fault> {
    Ok(input(event)?.map(|input| input.spec))
}

pub(super) async fn validate_answer(
    db: &Connection,
    question: &question::Question,
    answer: &question::Answer,
) -> Result<(), Fault> {
    let question::Answer::Form(values) = answer else {
        return Ok(());
    };
    let input = read(db, question.turn, &question.entry).await?;
    let validator = jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(&input.schema)
        .map_err(|_| sailry_protocol::Fault::invalid("invalid MCP input schema"))?;
    if !validator.is_valid(&Value::Object(values.clone())) {
        return Err(sailry_protocol::Fault::invalid(
            "answer does not match the requested MCP form",
        ));
    }
    Ok(())
}

impl Ingress {
    pub(crate) async fn cancel_input(&self, turn: TurnId, call: String) {
        let _ = self
            .submit(Job::Agent(Box::new(Operation::CancelInput { turn, call })))
            .await;
    }
}

pub(super) async fn cancel(
    database: &mut Database,
    turn: TurnId,
    call: &str,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let publication = database.publication.clone();
    let mut write = Some(database.write().await?);
    let result: Result<_, Fault> = async {
        let mut transaction = database
            .connection
            .unchecked_transaction()
            .await
            .map_err(storage_error)?;
        transaction.set_drop_behavior(turso::transaction::DropBehavior::Ignore);
        let body: Option<Vec<u8>> = {
            let mut statement = transaction
                .prepare("SELECT body FROM agent_questions WHERE turn=?1 AND call=?2")
                .await
                .map_err(storage_error)?;
            match statement.query_row(params![turn.to_string(), call]).await {
                Ok(row) => {
                    let mapped: turso::Result<_> = row.get(0);
                    Some(mapped.map_err(storage_error)?)
                }
                Err(turso::Error::QueryReturnedNoRows) => None,
                Err(error) => return Err(storage_error(error)),
            }
        };
        let Some(body) = body else { return Ok(()) };
        let mut question: question::Question =
            serde_json::from_slice(&body).map_err(storage_error)?;
        if question.state != question::State::Pending {
            return Ok(());
        }
        question.state = question::State::Cancelled;
        transaction
            .execute(
                "UPDATE agent_questions SET body=?2 WHERE id=?1",
                params![question.id.to_string(), encode(&question)?],
            )
            .await
            .map_err(storage_error)?;
        let envelope = changed(&transaction, database.node, question.session).await?;
        let publication = publication.lock().await;
        crate::store::database::conflict::commit(transaction).await?;
        drop(write.take());
        publication.send(events, envelope);
        feed::command(database, &Output::Question(question)).await;
        Ok(())
    }
    .await;
    if let Some(write) = write {
        if let Err(error) = crate::store::database::conflict::rollback(&database.connection).await {
            database.connection.hold_write(write);
            return Err(error);
        }
        drop(write);
    }
    result
}
