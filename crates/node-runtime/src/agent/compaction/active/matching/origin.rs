//! Assert historical projection against the pinned Runner itself.
use super::*;
use adk_core::Session as _;
use adk_session::{CreateRequest, InMemorySessionService};

#[tokio::test]
async fn follows_runner_ownership_rules() {
    for origin in [
        serde_json::Value::Null,
        json!({"provider":"responses", "model":"", "scope":"owner"}),
        json!({"provider":"responses", "model":"fixture", "scope":"owner"}),
    ] {
        let service = InMemorySessionService::new();
        let session = service
            .create(CreateRequest {
                app_name: "fixture".into(),
                user_id: "fixture".into(),
                session_id: Some("fixture".into()),
                state: Default::default(),
            })
            .await
            .unwrap();
        let runner = adk_runner::MutableSession::new(Arc::from(session));
        let mut event = Event::new("previous");
        event.author = "assistant".into();
        event.set_content(Content {
            role: "assistant".into(),
            parts: vec![
                Part::Thinking {
                    thinking: "Verified reasoning".into(),
                    signature: Some("signed".into()),
                },
                Part::Text {
                    text: "Visible result".into(),
                },
            ],
        });
        event.llm_response.provider_metadata = Some(json!({"provider_origin":origin,
            "openai":{"conversation_history_parts":[{"server_tool_call":{
                "type":"reasoning", "id":"reasoning-1", "summary":[], "encrypted_content":"opaque"}}]}}));
        runner.append_event(event.clone());
        let preamble = Content::new("system").with_text("Instructions");
        let projected = runner.conversation_history_with_context("assistant", "");
        let mut contents = vec![
            preamble.clone(),
            Content::new("user").with_text("Old prefix"),
        ];
        contents.extend(projected);
        let mut request = LlmRequest::new("fixture", contents);
        assert_eq!(
            tail(&request, std::slice::from_ref(&event), &preamble, "current"),
            Some(2)
        );
        for extra in [
            Part::Text {
                text: "Unrecorded suffix".into(),
            },
            Part::Thinking {
                thinking: "Unrecorded reasoning".into(),
                signature: Some("another signature".into()),
            },
            Part::FunctionCall {
                name: "write_file".into(),
                args: json!({"path":"unrecorded.txt"}),
                id: Some("extra-call".into()),
                thought_signature: None,
            },
        ] {
            request.contents[2].parts.push(extra);
            assert_eq!(
                tail(&request, std::slice::from_ref(&event), &preamble, "current"),
                None
            );
            request.contents[2].parts.pop();
        }
        request.contents[2] = adk_core::history::with_provider_parts(
            event.content().unwrap(),
            event.llm_response.provider_metadata.as_ref(),
        );
        request.contents[2].role = "model".into();
        // LlmAgent's live path projects originless response metadata too. It
        // must remain a match only inside that exact current invocation.
        assert_eq!(
            tail(
                &request,
                std::slice::from_ref(&event),
                &preamble,
                "previous"
            ),
            Some(2)
        );
        let has_origin =
            adk_core::history::provider_origin(event.llm_response.provider_metadata.as_ref())
                .is_some();
        assert_eq!(
            tail(&request, &[event], &preamble, "current"),
            has_origin.then_some(2)
        );
    }
}
