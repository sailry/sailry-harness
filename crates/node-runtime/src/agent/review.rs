//! The Node offers review after a plain planning answer without another model call.
use super::*;
use crate::store::agent::review;
use adk_core::ToolConfirmationRequest;
use adk_session::SessionService;
use sailry_protocol::conversation::question::{Input, Spec};

pub(super) fn requested(event: &adk_core::Event) -> bool {
    !event.llm_response.partial
        && event.content().is_some_and(|content| {
            content.parts.iter().any(|part| {
                matches!(part, adk_core::Part::FunctionCall { name, args, .. }
                    if name == "ask_user"
                    && serde_json::from_value::<Spec>(args.clone()).is_ok_and(|spec| {
                        matches!(spec.input, Input::Plan) && !spec.prompt.is_empty() && spec.validate().is_ok()
                    }))
            })
        })
}

pub(super) async fn wait(
    ingress: &Arc<Ingress>,
    invocation: &Invocation,
    stop: &CancellationToken,
    source: String,
) -> Result<(), Fault> {
    let turn = invocation.turn.id;
    let session = invocation.turn.session.to_string();
    let sessions = ingress.sessions(turn);
    let event = review::event(turn, source);
    let call = event.id.clone();
    let work = async {
        sessions
            .append_event(&session, event)
            .await
            .map_err(model::error)?;
        let response = ingress
            .ask(
                turn,
                ToolConfirmationRequest {
                    tool_name: review::TOOL.into(),
                    function_call_id: Some(call.clone()),
                    args: serde_json::json!({"prompt": "", "input": {"kind": "plan"}}),
                },
            )
            .await?;
        sessions
            .append_event(&session, review::result(turn, call, response))
            .await
            .map_err(model::error)
    };
    tokio::select! {
        biased;
        _ = stop.cancelled() => Ok(()),
        result = work => result,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn excludes_invalid_model_requests() {
        for prompt in ["", " ", "A concrete plan"] {
            let mut event = adk_core::Event::new("fixture");
            event.set_content(adk_core::Content {
                role: "model".into(),
                parts: vec![adk_core::Part::FunctionCall {
                    id: Some("review".into()),
                    name: "ask_user".into(),
                    args: serde_json::json!({"prompt":prompt,"input":{"kind":"plan"}}),
                    thought_signature: None,
                }],
            });
            assert_eq!(requested(&event), !prompt.trim().is_empty());
            event.llm_response.partial = true;
            assert!(!requested(&event));
        }
    }
}
