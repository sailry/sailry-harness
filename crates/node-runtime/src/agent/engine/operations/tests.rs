use super::*;
use crate::agent::canonical::EXTERNAL_DISPATCH_KEY;
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
) -> (Outcome, mpsc::UnboundedReceiver<Message>) {
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
        let Outcome::Applied(result) = result else {
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
        let Outcome::Applied(result) = result else {
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
                Some(json!({EXTERNAL_DISPATCH_KEY: true})),
            )
            .with_tool_request_with_metadata(
                "external-write",
                Ok(CallToolRequestParams::new("write")),
                None,
                Some(json!({EXTERNAL_DISPATCH_KEY: true})),
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
    assert!(matches!(result, Outcome::NotApplicable));
    assert!(events.try_recv().is_err());
    assert_eq!(session.conversation, before);
}

#[tokio::test]
async fn records_unhandled_calls_without_changing_results() {
    use crate::agent::{model::Content, observation::Timeline};
    let metadata = json!({"provider":"fixture"}).as_object().unwrap().clone();
    let session = session([
        Message::user().with_text("use tools"),
        Message::assistant().with_tool_request_with_metadata(
            "unknown",
            Ok(CallToolRequestParams::new("unregistered")),
            Some(metadata.clone()),
            Some(json!({EXECUTABLE_KEY: true})),
        ),
    ]);
    let before = session.conversation.clone();
    let timeline = Timeline::new();
    let remaining = Remaining {
        timeline: Some(timeline.clone()),
    };
    let (sender, mut receiver) = mpsc::unbounded_channel();
    let emit = Emitter::new(sender, CancellationToken::new());
    let Outcome::Applied(transition) = remaining
        .run(&session, &session.conversation, &emit)
        .await
        .unwrap()
    else {
        panic!("unhandled calls must produce effects")
    };
    assert_eq!(transition.status, RunStatus::Continuing);
    let [Effect::Conversation(ConversationEffect::AppendMessage(message))] =
        transition.effects.as_slice()
    else {
        panic!("a response message is required")
    };
    let [Content::ToolResult(response)] = message.content.as_slice() else {
        panic!("a tool result is required")
    };
    assert_eq!(response.id, "unknown");
    assert_eq!(response.metadata, Some(metadata));
    assert_eq!(
        response.result,
        Ok(CallToolResult::error(vec![ContentBlock::text(
            "This tool call was not handled."
        )]))
    );
    let observed = timeline
        .tool_failure("", "unknown", "unregistered")
        .unwrap();
    assert_eq!(
        observed.failure,
        sailry_protocol::tool::Failure::runtime(None)
    );
    assert!(observed.message.is_none());
    assert_eq!(receiver.try_recv().unwrap(), *message);
    assert_eq!(session.conversation, before);
}
