use super::*;
use goose_agent::events::AgentEvent;
use goose_provider_types::conversation::message::ActionRequiredData;
use rmcp::model::{CallToolRequestParams, ErrorData};
use sailry_link::CancellationToken;
use sailry_protocol::TurnId;
use serde_json::json;
use tokio::sync::mpsc;

fn session(messages: impl IntoIterator<Item = Message>) -> Session {
    let turn = TurnId::new();
    super::super::tests::session(
        messages
            .into_iter()
            .map(|message| (turn, message.with_generated_id()))
            .collect(),
    )
}

async fn evaluate(
    approval: &Approval,
    session: &Session,
) -> (OperationResult<Effect>, mpsc::UnboundedReceiver<AgentEvent>) {
    let (sender, receiver) = mpsc::unbounded_channel();
    let emit = Emitter::new(sender, CancellationToken::new());
    let result = approval
        .run(session, &session.conversation, &emit)
        .await
        .unwrap();
    (result, receiver)
}

#[tokio::test]
async fn marks_allowed_calls_before_requesting_approval() {
    let session = session([
        Message::user().with_text("use tools"),
        Message::assistant()
            .with_tool_request("read-1", Ok(CallToolRequestParams::new("read")))
            .with_tool_request("write-1", Ok(CallToolRequestParams::new("write")))
            .with_tool_request("read-2", Ok(CallToolRequestParams::new("read")))
            .with_tool_request("write-2", Ok(CallToolRequestParams::new("write"))),
    ]);
    let before = session.conversation.clone();
    for (required, expected) in [
        (
            BTreeSet::new(),
            vec!["read-1", "write-1", "read-2", "write-2"],
        ),
        (["write".into()].into(), vec!["read-1", "read-2"]),
    ] {
        let approval = Approval {
            required,
            bindings: BTreeMap::new(),
        };
        let (result, mut events) = evaluate(&approval, &session).await;
        let OperationResult::Applied(result) = result else {
            panic!("allowed calls should be marked");
        };
        assert_eq!(result.status, RunStatus::Continuing);
        let ids = result
            .effects
            .into_iter()
            .map(|effect| {
                let Effect::Conversation(ConversationEffect::PatchToolRequestMeta {
                    tool_call_id,
                    patch,
                }) = effect
                else {
                    panic!("only executable metadata should be patched");
                };
                assert_eq!(patch, json!({EXECUTABLE_KEY: true}));
                tool_call_id
            })
            .collect::<Vec<_>>();
        assert_eq!(ids, expected);
        assert!(events.try_recv().is_err());
        assert_eq!(session.conversation, before);
    }
}

#[tokio::test]
async fn keeps_required_calls_in_order() {
    for executable in [None, Some(false)] {
        let arguments = json!({"path": "requested.txt", "text": "requested"});
        let session = session([
            Message::user().with_text("use tools"),
            Message::assistant()
                .with_tool_request_with_metadata(
                    "read",
                    Ok(CallToolRequestParams::new("read")),
                    None,
                    Some(json!({EXECUTABLE_KEY: true})),
                )
                .with_tool_request_with_metadata(
                    "approved",
                    Ok(CallToolRequestParams::new("write")),
                    None,
                    Some(json!({EXECUTABLE_KEY: true})),
                )
                .with_tool_request_with_metadata(
                    "write-1",
                    Ok(CallToolRequestParams::new("write")
                        .with_arguments(arguments.as_object().unwrap().clone())),
                    None,
                    executable.map(|value| json!({EXECUTABLE_KEY: value})),
                )
                .with_tool_request("write-2", Ok(CallToolRequestParams::new("write"))),
        ]);
        let approval = Approval {
            required: ["write".into()].into(),
            bindings: BTreeMap::new(),
        };
        let (result, mut events) = evaluate(&approval, &session).await;
        let OperationResult::Applied(result) = result else {
            panic!("the first unapproved call should require approval");
        };
        assert_eq!(result.status, RunStatus::Yielded);
        assert_eq!(result.effects.len(), 1);
        let Effect::Approval { request, binding } = &result.effects[0] else {
            panic!("the existing approval owner should receive the call");
        };
        assert_eq!(request.id.as_deref(), Some("write-1"));
        assert_eq!(request.name, "write");
        assert_eq!(request.arguments, arguments);
        assert!(binding.is_none());
        let AgentEvent::Message(message) = events.try_recv().unwrap() else {
            panic!("approval should emit its action notice");
        };
        assert!(message.is_user_visible());
        assert!(!message.is_agent_visible());
        assert!(matches!(
            &message.content[0],
            MessageContent::ActionRequired(action)
                if matches!(&action.data, ActionRequiredData::ToolConfirmation { id, .. } if id == "write-1")
        ));
        assert!(events.try_recv().is_err());
    }
}

#[tokio::test]
async fn skips_closed_external_and_previous_calls() {
    let session = session([
        Message::user().with_text("previous turn"),
        Message::assistant()
            .with_tool_request("old-read", Ok(CallToolRequestParams::new("read")))
            .with_tool_request("old-write", Ok(CallToolRequestParams::new("write"))),
        Message::user().with_text("current turn"),
        Message::assistant()
            .with_tool_request("denied-read", Ok(CallToolRequestParams::new("read")))
            .with_tool_request("denied-write", Ok(CallToolRequestParams::new("write")))
            .with_tool_request_with_metadata(
                "external-read",
                Ok(CallToolRequestParams::new("read")),
                None,
                Some(json!({"goose.external_dispatch": true})),
            )
            .with_tool_request_with_metadata(
                "external-write",
                Ok(CallToolRequestParams::new("write")),
                None,
                Some(json!({"goose.external_dispatch": true})),
            )
            .with_tool_request_with_metadata(
                "allowed-read",
                Ok(CallToolRequestParams::new("read")),
                None,
                Some(json!({EXECUTABLE_KEY: true, "retained": "metadata"})),
            )
            .with_tool_request_with_metadata(
                "approved-write",
                Ok(CallToolRequestParams::new("write")),
                None,
                Some(json!({EXECUTABLE_KEY: true})),
            )
            .with_tool_request(
                "malformed",
                Err(ErrorData::invalid_params("malformed arguments", None)),
            ),
        Message::user()
            .with_tool_response(
                "denied-read",
                Ok(CallToolResult::error(vec![ContentBlock::text(
                    "Tool execution was denied.",
                )])),
            )
            .with_tool_response(
                "denied-write",
                Ok(CallToolResult::error(vec![ContentBlock::text(
                    "Tool execution was denied.",
                )])),
            ),
    ]);
    let before = session.conversation.clone();
    let approval = Approval {
        required: ["write".into()].into(),
        bindings: BTreeMap::new(),
    };
    let (result, mut events) = evaluate(&approval, &session).await;
    assert!(matches!(result, OperationResult::NotApplicable));
    assert!(events.try_recv().is_err());
    assert_eq!(session.conversation, before);
}
