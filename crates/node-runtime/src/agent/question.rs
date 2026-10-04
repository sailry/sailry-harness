//! ADK suspends this tool until a controller answers through the shared Node.
use super::*;
use adk_core::{AdkError, Tool, ToolConfirmationRequest, ToolContext};
use async_trait::async_trait;
use sailry_protocol::conversation::question::{Answer, MAX_TEXT_BYTES, Response, Spec};
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
        "Ask one focused question to collect missing information, clarify requirements, or learn a preference, and wait for the answer. Use text or single/multiple choices, optionally allowing another answer. Do not use this tool for tool permissions, blanket authorization, or repeated confirmation of work the user already requested. Invoke the actual tool; the runtime handles any required approval separately. Answers do not grant tool permissions. Never request passwords or secrets: questions and answers are stored in conversation history and sent to the model. Cancellation is not consent. Permission modes never answer questions automatically."
    }
    fn parameters_schema(&self) -> Option<Value> {
        let mut schema = json!({"type": "object", "additionalProperties": false, "required": ["prompt", "input"], "properties": {
            "prompt": {"type": "string"},
            "input": {"anyOf": [
                {"type": "object", "additionalProperties": false, "required": ["kind", "multiline", "max_bytes"], "properties": {
                    "kind": {"type": "string", "enum": ["text"]}, "multiline": {"type": "boolean"}, "max_bytes": {"type": "integer", "minimum": 1, "maximum": MAX_TEXT_BYTES}
                }},
                {"type": "object", "additionalProperties": false, "required": ["kind", "options", "multiple", "allow_other"], "properties": {
                    "kind": {"type": "string", "enum": ["choice"]}, "options": {"type": "array", "items": {"type": "string"}, "minItems": 1, "maxItems": 64}, "multiple": {"type": "boolean"}, "allow_other": {"type": "boolean"}
                }}
            ]}
        }});
        if self.planning {
            schema["properties"]["input"]["anyOf"].as_array_mut().unwrap().push(
                json!({"type": "object", "additionalProperties": false, "required": ["kind"], "properties": {
                    "kind": {"type": "string", "enum": ["plan"]}
                }}),
            );
        }
        Some(schema)
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
                .map_err(|_| Fault::new(ErrorCode::InvalidRequest, "invalid question arguments"))?;
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
            let response = self
                .ingress
                .ask(
                    self.turn,
                    ToolConfirmationRequest {
                        tool_name: self.name().into(),
                        function_call_id: Some(context.function_call_id().into()),
                        args: arguments,
                    },
                )
                .await?;
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
