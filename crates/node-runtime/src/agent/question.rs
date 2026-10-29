//! Native clarification owns its deadline and never turns silence into consent.
use super::*;
use crate::agent::catalog::CallContext;
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

impl Question {
    pub(super) fn registration(self) -> super::catalog::Registration {
        let provider = Arc::new(self);
        super::catalog::Registration::managed(provider.descriptor(), provider)
    }
    fn is_read_only(&self) -> bool {
        false
    }
    fn name(&self) -> &str {
        "ask_user"
    }
    fn description(&self) -> &str {
        "Ask the user a text or single/multiple-choice question, optionally allowing another answer. Bundle related decisions; put the best-supported option first and label it (Recommended). On timeout the tool returns that option as an automatic recommendation, not a user answer. Do not ask for tool permissions or confirmation of requested work; the runtime handles approvals. Answers do not grant tool permissions. Never ask for passwords or secrets: questions and answers are stored in history. Cancellation is not consent; plan acceptance requires an explicit user response."
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
    async fn execute(&self, context: Arc<CallContext>, arguments: Value) -> Result<Value, Fault> {
        if context.session_id() != self.session {
            return Err(Fault::invalid("question belongs to another session"));
        }
        if context.is_cancelled() || self.stop.is_cancelled() {
            return Err(Fault::invalid("question was cancelled"));
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
            let timeout = Duration::from_secs(300);
            #[cfg(any(test, feature = "test-support"))]
            let timeout = self
                .ingress
                .agents
                .question_timeout
                .lock()
                .unwrap()
                .unwrap_or(timeout);
            let waiting = self.ingress.ask(self.turn, context.request());
            tokio::pin!(waiting);
            let response = if matches!(spec.input, Input::Plan) {
                waiting.await?
            } else {
                tokio::select! {
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
                }
            };
            Ok::<_, Fault>(match response {
                Response::Decline => unreachable!("decline requires an MCP request"),
                Response::Answer(Answer::Form(_) | Answer::Opened) => {
                    unreachable!("this input requires an MCP request")
                }
                Response::Cancel => json!({"status": "cancelled"}),
                Response::Answer(Answer::Text(text)) => {
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
            _ = self.stop.cancelled() => Err(Fault::invalid("question was cancelled")),
            result = result => match result { Ok(value) => Ok(value), Err(error) => Ok(json!({"error": error})) },
        }
    }
}

crate::agent::catalog::provider!(Question);
