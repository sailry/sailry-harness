//! Native clarification owns its deadline and never turns silence into consent.
use super::*;
use adk_core::{AdkError, Tool, ToolConfirmationRequest, ToolContext};
use async_trait::async_trait;
use sailry_protocol::conversation::question::{
    Answer, Input, MAX_TEXT_BYTES, Response, Spec, State,
};
use serde_json::{Value, json};

pub(super) struct Question {
    pub ingress: Arc<Ingress>,
    pub turn: TurnId,
    pub session: String,
    pub stop: CancellationToken,
    pub planning: bool,
}

#[async_trait]
impl Tool for Question {
    fn name(&self) -> &str {
        "ask_user"
    }
    fn description(&self) -> &str {
        "Collect foreseeable missing information and preferences at the start of the task, grouping related decisions into concise questions. Proceed when the available information is sufficient; ask during work only for a newly discovered blocking question. Use text or single/multiple choices, optionally allowing another answer. Put the best-supported recommended option first and label it (Recommended). If the question times out, the tool returns that first option as an automatic recommendation, not a user answer; continue with that recommendation. Without choices, use the available context and state any necessary assumption; never invent missing facts or secrets. Do not use this tool for tool permissions, blanket authorization, or repeated confirmation of work the user already requested. Invoke the actual tool; the runtime handles any required approval separately. Answers do not grant tool permissions. Never request passwords or secrets: questions and answers are stored in conversation history and sent to the model. Cancellation or Stop is not a timeout or consent. Plan acceptance still requires an explicit user response. Permission modes never answer questions automatically."
    }
    fn timeout_override(&self) -> Option<Option<Duration>> {
        // The same ADK budget is settled durably before this tool returns.
        Some(None)
    }
    fn parameters_schema(&self) -> Option<Value> {
        Some(
            json!({"type": "object", "additionalProperties": false, "required": ["prompt", "input"], "properties": {
                "prompt": {"type": "string"},
                "input": {"anyOf": [
                    {"type": "object", "additionalProperties": false, "required": ["kind", "multiline", "max_bytes"], "properties": {
                        "kind": {"type": "string", "enum": ["text"]}, "multiline": {"type": "boolean"}, "max_bytes": {"type": "integer", "minimum": 1, "maximum": MAX_TEXT_BYTES}
                    }},
                    {"type": "object", "additionalProperties": false, "required": ["kind", "options", "multiple"], "properties": {
                        "kind": {"type": "string", "enum": ["choice"]}, "options": {"type": "array", "items": {"type": "string"}, "minItems": 1}, "multiple": {"type": "boolean"}, "allow_other": {"type": "boolean", "default": false}
                    }},
                    {"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {
                        "kind": {"type": "string", "enum": ["plan"]}
                    }}
                ]}
            }}),
        )
    }
    async fn execute(
        &self,
        context: Arc<dyn ToolContext>,
        arguments: Value,
    ) -> adk_core::Result<Value> {
        if context.session_id() != self.session {
            return Err(AdkError::tool("question belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(AdkError::tool("question was cancelled"));
        }
        let result = async {
            let spec: Spec = serde_json::from_value(arguments.clone())
                .map_err(|_| Fault::invalid("invalid question arguments"))?;
            spec.validate()?;
            if matches!(
                spec.input,
                sailry_protocol::conversation::question::Input::Plan
            ) && !self.planning
            {
                return Err(Fault::new(
                    ErrorCode::PermissionDenied,
                    "plan review is unavailable in this turn",
                ));
            }
            let timeout = adk_agent::DEFAULT_TOOL_TIMEOUT;
            #[cfg(any(test, feature = "test-support"))]
            let timeout = self
                .ingress
                .agents
                .question_timeout
                .lock()
                .unwrap()
                .unwrap_or(timeout);
            let waiting = self.ingress.ask(
                self.turn,
                ToolConfirmationRequest {
                    acting: None,
                    origin: None,
                    tool_name: self.name().into(),
                    function_call_id: Some(context.function_call_id().into()),
                    args: arguments,
                },
            );
            tokio::pin!(waiting);
            let response = tokio::select! {
                biased;
                response = &mut waiting => response?,
                _ = tokio::time::sleep(timeout) => {
                    let question = self.ingress.expire_question(
                        self.turn,
                        context.function_call_id().into(),
                        self.stop.clone(),
                    ).await?;
                    if question.state == State::TimedOut {
                        let recommendations = match &spec.input {
                            Input::Choice { options, .. } => vec![json!({"index":0,"option":options[0]})],
                            _ => Vec::new(),
                        };
                        return Ok(json!({
                            "status":"timed_out", "source":"automatic_timeout",
                            "question":question.id, "answers":[], "recommendations":recommendations
                        }));
                    }
                    waiting.await?
                }
            };
            Ok::<_, Fault>(match response {
                Response::Decline => unreachable!("decline requires an MCP request"),
                Response::Answer(Answer::Form(_) | Answer::Opened) => {
                    unreachable!("this input requires an MCP request")
                }
                Response::Cancel => json!({"status": "cancelled"}),
                Response::Answer(Answer::Text(text)) => {
                    if matches!(
                        spec.input,
                        sailry_protocol::conversation::question::Input::Plan
                    ) {
                        let mut actions = context.actions();
                        actions.skip_summarization = true;
                        context.set_actions(actions);
                    }
                    json!({"status": "answered", "answer": text})
                }
                Response::Answer(Answer::Choices { selected, other }) => {
                    let sailry_protocol::conversation::question::Input::Choice { options, .. } =
                        spec.input
                    else {
                        unreachable!("Node validates answer types");
                    };
                    let mut answers: Vec<_> = selected
                        .into_iter()
                        .map(|index| options[index].clone())
                        .collect();
                    answers.extend(other);
                    json!({"status": "answered", "answers": answers})
                }
                Response::Answer(Answer::Plan { turn }) => {
                    let mut actions = context.actions();
                    actions.skip_summarization = true;
                    context.set_actions(actions);
                    json!({"status": "accepted", "coding_turn": turn})
                }
                Response::StartCoding { .. } => {
                    return Err(Fault::new(
                        ErrorCode::Internal,
                        "plan response was not admitted",
                    ));
                }
            })
        };
        tokio::select! {
            biased;
            _ = self.stop.cancelled() => Err(AdkError::tool("question was cancelled")),
            result = result => match result { Ok(value) => Ok(value), Err(error) => Ok(json!({"error": error})) },
        }
    }
}
