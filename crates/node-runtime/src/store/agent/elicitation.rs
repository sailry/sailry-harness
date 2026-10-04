//! MCP input lives in canonical ADK metadata, outside provider tool-call batches.
use super::*;
use adk_core::ToolConfirmationRequest;
use serde::{Deserialize, Serialize};
use serde_json::Value;

const KEY: &str = "sailry.mcp.elicitation";
const RESULT: &str = "sailry.mcp.elicitation_result";
pub(crate) const TOOL: &str = "mcp_elicitation";

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Input {
    pub parent: ToolConfirmationRequest,
    pub request: Value,
    pub spec: question::Spec,
    pub schema: Value,
}

pub(crate) fn event(turn: TurnId, input: &Input) -> AdkEvent {
    let mut event = AdkEvent::new(turn.to_string());
    event.author = TOOL.into();
    event.provider_metadata.insert(
        KEY.into(),
        serde_json::to_string(input).expect("MCP input is serializable"),
    );
    event
}

pub(crate) fn result(turn: TurnId, call: String, response: &question::Response) -> AdkEvent {
    let mut event = AdkEvent::new(turn.to_string());
    event.author = TOOL.into();
    event.provider_metadata.insert(
        RESULT.into(),
        serde_json::json!({"id":call,"response":response}).to_string(),
    );
    event
}

fn input(event: &AdkEvent) -> Result<Option<Input>, Fault> {
    event
        .provider_metadata
        .get(KEY)
        .map(|value| serde_json::from_str(value).map_err(|_| invalid("invalid MCP input metadata")))
        .transpose()
}

pub(super) fn validate(db: &Connection, turn: TurnId, event: &AdkEvent) -> Result<(), Fault> {
    if let Some(input) = input(event)? {
        calls::active(db, turn)?;
        if event.content().is_some()
            || !matches!(
                input.spec.input,
                question::Input::Form { .. } | question::Input::Url { .. }
            )
        {
            return Err(invalid("MCP input must not change model content"));
        }
        input.spec.validate()?;
        calls::locate(db, turn, &input.parent)?;
    }
    Ok(())
}

pub(super) fn project(event: &AdkEvent) -> Result<Vec<Part>, Fault> {
    if let Some(input) = input(event)? {
        return Ok(vec![Part::ToolCall {
            display: None,
            presentation: Default::default(),
            grouping: Default::default(),
            id: Some(event.id.clone()),
            name: TOOL.into(),
            arguments: serde_json::to_value(input.spec).map_err(storage_error)?,
        }]);
    }
    if let Some(result) = event.provider_metadata.get(RESULT) {
        let result: Value = serde_json::from_str(result).map_err(storage_error)?;
        return Ok(vec![Part::ToolResult {
            id: result["id"].as_str().map(str::to_owned),
            name: TOOL.into(),
            result: result["response"].clone(),
            images: Vec::new(),
        }]);
    }
    Ok(Vec::new())
}

fn read(db: &Connection, turn: TurnId, call: &str) -> Result<Input, Fault> {
    let body: Vec<u8> = db
        .query_row(
            "SELECT body FROM agent_events WHERE turn=?1 AND id=?2",
            params![turn.to_string(), call],
            |row| row.get(0),
        )
        .map_err(storage_error)?;
    input(&serde_json::from_slice(&body).map_err(storage_error)?)?
        .ok_or_else(|| invalid("MCP input has no canonical request"))
}

pub(super) fn locate(
    db: &Connection,
    turn: TurnId,
    request: &ToolConfirmationRequest,
) -> Result<(String, usize), Fault> {
    let call = calls::id(request)?;
    let input = read(db, turn, call)?;
    if request.tool_name != TOOL
        || serde_json::to_value(&input.spec).map_err(storage_error)? != request.args
    {
        return Err(invalid("MCP input arguments differ"));
    }
    calls::locate(db, turn, &input.parent)?;
    Ok((call.into(), 0))
}

pub(super) fn spec(event: &AdkEvent) -> Result<Option<question::Spec>, Fault> {
    Ok(input(event)?.map(|input| input.spec))
}

pub(super) fn validate_answer(
    db: &Connection,
    question: &question::Question,
    answer: &question::Answer,
) -> Result<(), Fault> {
    let question::Answer::Form(values) = answer else {
        return Ok(());
    };
    let input = read(db, question.turn, &question.entry)?;
    let validator = jsonschema::draft202012::options()
        .should_validate_formats(true)
        .build(&input.schema)
        .map_err(|_| invalid("invalid MCP input schema"))?;
    if !validator.is_valid(&Value::Object(values.clone())) {
        return Err(invalid("answer does not match the requested MCP form"));
    }
    Ok(())
}

impl Ingress {
    pub(crate) async fn cancel_input(&self, turn: TurnId, call: String) {
        let _ = self
            .sender
            .send(Job::Agent(Box::new(Operation::CancelInput { turn, call })))
            .await;
    }
}

pub(super) fn cancel(
    database: &mut Database,
    turn: TurnId,
    call: &str,
    events: &broadcast::Sender<EventEnvelope>,
) -> Result<(), Fault> {
    let transaction = database.connection.transaction().map_err(storage_error)?;
    let body: Option<Vec<u8>> = transaction
        .query_row(
            "SELECT body FROM agent_questions WHERE turn=?1 AND call=?2",
            params![turn.to_string(), call],
            |row| row.get(0),
        )
        .optional()
        .map_err(storage_error)?;
    let Some(body) = body else { return Ok(()) };
    let mut question: question::Question = serde_json::from_slice(&body).map_err(storage_error)?;
    if question.state != question::State::Pending {
        return Ok(());
    }
    question.state = question::State::Cancelled;
    transaction
        .execute(
            "UPDATE agent_questions SET body=?2 WHERE id=?1",
            params![question.id.to_string(), encode(&question)?],
        )
        .map_err(storage_error)?;
    let envelope = changed(&transaction, database.node, question.session)?;
    transaction.commit().map_err(storage_error)?;
    let _ = events.send(envelope);
    feed::command(database, &Output::Question(question));
    Ok(())
}
