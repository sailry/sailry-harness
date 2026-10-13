//! The Node offers review after a plain planning answer without another model call.
use super::*;
use crate::store::agent::review;
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

pub(super) fn completion(turn: TurnId, source: String) -> adk_core::Event {
    review::event(turn, source)
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
