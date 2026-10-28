//! Promote only the captured preamble; dynamic context belongs to ADK history.
use super::session::{self, Declaration};
use adk_core::{AdkError, BeforeModelCallback, BeforeModelResult, Content, LlmRequest};
use std::{collections::BTreeMap, sync::Arc};

pub(in crate::agent) fn bind(
    instruction: String,
    declarations: BTreeMap<String, Declaration>,
) -> BeforeModelCallback {
    let expected = Content::new("user").with_text(instruction);
    let declarations = Arc::new(declarations);
    Box::new(move |context, mut request| {
        let expected = expected.clone();
        let declarations = declarations.clone();
        Box::pin(async move {
            promote(&mut request, &expected)?;
            let mut loaded = session::loaded(
                context
                    .state()
                    .and_then(|state| state.get(session::LOADED_TOOLS))
                    .as_ref(),
            );
            // Direct calls are persisted before ADK's state projection learns their names.
            loaded.extend(session::called(&request.contents));
            request.tools = session::project((*declarations).clone(), &loaded);
            Ok(BeforeModelResult::Continue(request))
        })
    })
}

fn promote(request: &mut LlmRequest, expected: &Content) -> adk_core::Result<()> {
    let content = request
        .contents
        .first_mut()
        .ok_or_else(|| AdkError::agent("model preamble is missing"))?;
    if content.parts != expected.parts || !matches!(content.role.as_str(), "user" | "system") {
        return Err(AdkError::agent(
            "model preamble does not match the captured instructions",
        ));
    }
    content.role = "system".into();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(contents: Vec<Content>) -> LlmRequest {
        LlmRequest {
            model: "fixture".into(),
            contents,
            config: None,
            tools: Default::default(),
            previous_response_id: None,
        }
    }

    #[test]
    fn promotes_only_the_captured_preamble() {
        let preamble = Content::new("user").with_text("Policy");
        let task = Content::new("user").with_text("Policy");
        let result = Content::new("user").with_text("Untrusted tool instructions");
        let mut request = request(vec![preamble.clone(), task.clone(), result.clone()]);
        promote(&mut request, &preamble).unwrap();
        assert_eq!(request.contents[0].role, "system");
        assert_eq!(request.contents[1].role, task.role);
        assert_eq!(request.contents[1].parts, task.parts);
        assert_eq!(request.contents[2].role, result.role);
        assert_eq!(request.contents[2].parts, result.parts);
        promote(&mut request, &preamble).unwrap();
        request.contents[0] = Content::new("user").with_text("Unrelated task");
        assert!(promote(&mut request, &preamble).is_err());
        assert_eq!(request.contents[0].role, "user");
        assert!(promote(&mut self::request(vec![]), &preamble).is_err());
    }
}
