use super::*;
use sailry_client::conversation::tools::{Call, Reference, State, display};
use sailry_protocol::{
    ErrorCode, NodeId, SessionId, TurnId,
    conversation::{Entry, Page, Part, Snapshot},
    tool::{Failure, Presentation},
};
use std::sync::Arc;

fn fixture(mode: Presentation, result: Value) -> View {
    let turn = TurnId::new();
    let call = Call {
        turn,
        id: Some("call".into()),
        name: "fixture".into(),
        terminal_interaction: false,
        presentation: mode,
        grouping: Default::default(),
        source: Reference {
            entry: "request".into(),
            index: 0,
        },
        response: Some(Reference {
            entry: "response".into(),
            index: 0,
        }),
        state: State::Returned,
        failure: None,
        approval: None,
        question: None,
        progress: None,
        content: mode.content(&result),
        resolved: None,
    };
    let entry = |id: &str, sequence, part| Entry {
        sequence,
        id: id.into(),
        turn,
        author: "assistant".into(),
        branch: String::new(),
        timestamp_ms: 1,
        parts: vec![part],
        citations: vec![],
        search_suggestions: None,
        usage: None,
    };
    let page = Page {
        session: SessionId::new(),
        revision: 1,
        next_before: None,
        queue: Default::default(),
        approvals: vec![],
        questions: vec![],
        children: vec![],
        background: vec![],
        runs: vec![],
        entries: vec![
            entry(
                "request",
                1,
                Part::ToolCall {
                    id: Some("call".into()),
                    name: "fixture".into(),
                    arguments: json!({}),
                    presentation: mode,
                    grouping: Default::default(),
                    display: None,
                },
            ),
            entry(
                "response",
                2,
                Part::ToolResult {
                    id: Some("call".into()),
                    name: "fixture".into(),
                    result,
                    images: vec![],
                },
            ),
        ],
    };
    View {
        connected: true,
        calls: Arc::new(vec![call]),
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([1; 32]),
            sequence: 2,
            page: Arc::new(page),
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
        })),
        ..Default::default()
    }
}

fn serialized(view: &View) -> Value {
    serde_json::from_str(&super::view(view).unwrap()).unwrap()
}

#[test]
fn preserves_facts_and_business_data() {
    let result = json!({"output":{"isError":true,"error":"domain row"}});
    let mut view = fixture(Presentation::Hidden, result);
    let original = serde_json::to_value(&view.snapshot).unwrap();
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["hidden"], true);
    assert_eq!(encoded["calls"][0]["details"], false);
    assert_eq!(encoded["snapshot"], original);
    Arc::make_mut(&mut view.calls)[0].failure = Some(Failure {
        code: Some(ErrorCode::Unavailable),
    });
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["hidden"], false);
    assert_eq!(encoded["calls"][0]["details"], true);
    assert_eq!(encoded["snapshot"], original);
}

#[test]
fn resolves_literal_content_once() {
    let result = json!({"output":{
        "isError":true,"error":"domain row",
        "escaped/key":{"~value":["Original\ntext 🙂"]},
        "sailry_content":{"version":1,"blocks":[
            {"kind":"text","path":"/escaped~1key/~0value/0"},
            {"kind":"notice","message":{"label":"Kept"}}
        ]}
    }});
    let view = fixture(Presentation::Content, result);
    let original = serde_json::to_value(&view.snapshot).unwrap();
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["details"], true);
    assert_eq!(
        encoded["calls"][0]["texts"],
        json!(["Original\ntext 🙂", null])
    );
    assert_eq!(encoded["calls"][0]["failure"], Value::Null);
    assert_eq!(encoded["snapshot"], original);
}

#[test]
fn failure_does_not_select_success_preview() {
    let mut view = fixture(Presentation::Hidden, json!({"error":"Actual failure"}));
    let call = &mut Arc::make_mut(&mut view.calls)[0];
    call.failure = Some(Failure { code: None });
    call.resolved = Some(display::Resolved {
        label: None,
        input: Default::default(),
        output: Some(display::Output {
            preview: "Claimed success".into(),
            preview_deferred: false,
            notices: vec![],
            paths: vec![],
            body: None,
            table: None,
        }),
        approval: vec![],
        status: None,
        command: None,
        diagnostics: vec![],
    });
    let original = serde_json::to_value(&view.snapshot).unwrap();
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["hidden"], false);
    assert_eq!(encoded["calls"][0]["details"], true);
    assert_eq!(encoded["calls"][0]["output"], Value::Null);
    assert_eq!(
        encoded["calls"][0]["resolved"]["output"]["preview"],
        "Claimed success"
    );
    assert_eq!(super::output(&view, "request", 0).unwrap(), "null");
    assert_eq!(encoded["snapshot"], original);
}

#[test]
fn defers_json_until_requested() {
    let mut view = fixture(
        Presentation::Details,
        json!({"payload":{"isError":true,"error":"domain row"}}),
    );
    let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
    let page = Arc::make_mut(&mut snapshot.page);
    let Part::ToolCall { display, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    *display = Some(
        serde_json::from_value(json!({
            "label":"Inspect",
            "output":{"preview":{"parts":[{"value":{
                "source":"result","path":"/payload"
            },"format":"json"}]}}
        }))
        .unwrap(),
    );
    Arc::make_mut(&mut view.calls)[0].resolved = Some(display::Resolved {
        label: None,
        input: Default::default(),
        output: Some(display::Output {
            preview: String::new(),
            preview_deferred: true,
            notices: vec![],
            paths: vec![],
            body: None,
            table: None,
        }),
        approval: vec![],
        status: None,
        command: None,
        diagnostics: vec![],
    });
    let original = serde_json::to_value(&view.snapshot).unwrap();
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["resolved"]["output"]["preview"], "");
    assert_eq!(
        encoded["calls"][0]["resolved"]["output"]["preview_deferred"],
        true
    );
    assert_eq!(encoded["calls"][0]["output"], Value::Null);
    assert_eq!(encoded["calls"][0]["output_deferred"], true);
    let output: Value = serde_json::from_str(&super::output(&view, "request", 0).unwrap()).unwrap();
    let preview: Value = serde_json::from_str(output["preview"].as_str().unwrap()).unwrap();
    assert_eq!(preview, json!({"isError":true,"error":"domain row"}));
    assert_eq!(output["preview_deferred"], Value::Null);
    assert_eq!(serde_json::to_value(&view.snapshot).unwrap(), original);
    assert!(super::output(&view, "different", 0).is_err());
}

#[tokio::test]
async fn output_does_not_wait_for_updates() {
    use crate::api::conversation::ConversationUpdates;
    use sailry_link::CancellationToken;
    use tokio::sync::{Mutex, mpsc, watch};
    let view = fixture(Presentation::Details, json!("Original output"));
    let (_sender, receiver) = watch::channel(view);
    let (older, _requests) = mpsc::channel(1);
    let updates = Arc::new(ConversationUpdates {
        current: receiver.clone(),
        receiver: Mutex::new(receiver),
        stop: CancellationToken::new(),
        task: tokio::spawn(std::future::pending()),
        older,
    });
    let pending = {
        let updates = updates.clone();
        tokio::spawn(async move { updates.next().await })
    };
    tokio::task::yield_now().await;
    assert!(updates.receiver.try_lock().is_err());
    let output: Value =
        serde_json::from_str(&updates.tool_output("request".into(), 0).unwrap()).unwrap();
    assert_eq!(output["preview"], "Original output");
    updates.close();
    assert!(pending.await.unwrap().is_err());
    assert!(updates.tool_output("request".into(), 0).is_err());
}
