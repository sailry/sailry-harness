use super::*;
use sailry_client::{
    Client,
    conversation::tools::{Call, Reference, State, display},
};
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, Output, Receipt, Request, SessionId, Topic, TurnId,
    conversation::{Entry, Page, Part, Snapshot},
    tool::{
        Failure, Presentation,
        detail::{Detail, Summary},
    },
};
use std::sync::Arc;

fn fixture(mode: Presentation, result: Value) -> (View, Arc<Detail>) {
    let turn = TurnId::new();
    let content = mode.content(&result);
    let detail = Arc::new(Detail {
        arguments: json!({}),
        result: Some(result),
        resolved: None,
        content: content.clone(),
        images: vec![],
    });
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
        content: None,
        resolved: None,
        detail: None,
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
        commentary: Default::default(),
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
                    summary: Default::default(),
                    presentation: mode,
                    grouping: Default::default(),
                },
            ),
            entry(
                "response",
                2,
                Part::ToolResult {
                    id: Some("call".into()),
                    name: "fixture".into(),
                    images: vec![],
                    summary: Box::new(Summary {
                        revision: "a".repeat(64),
                        has_details: content.is_some() || mode != Presentation::Hidden,
                        ..Default::default()
                    }),
                },
            ),
        ],
    };
    (
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
        },
        detail,
    )
}

fn serialized(view: &View) -> Value {
    serde_json::from_str(&super::view(view).unwrap()).unwrap()
}

fn summary(view: &mut View) -> &mut Summary {
    let snapshot = Arc::make_mut(view.snapshot.as_mut().unwrap());
    let Part::ToolResult { summary, .. } =
        &mut Arc::make_mut(&mut snapshot.page).entries[1].parts[0]
    else {
        unreachable!()
    };
    summary
}

#[test]
fn preserves_bounded_facts_without_raw_values() {
    let (mut view, detail) = fixture(
        Presentation::Hidden,
        json!({"output":{"isError":true,"error":"domain row","body":"中文 🙂".repeat(50_000)}}),
    );
    let original = serde_json::to_value(&view.snapshot).unwrap();
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["hidden"], true);
    assert_eq!(encoded["calls"][0]["details"], false);
    assert_eq!(encoded["snapshot"], original);
    assert!(!encoded.to_string().contains("domain row"));
    assert!(serde_json::to_string(&detail).unwrap().len() > 100_000);
    assert!(encoded.to_string().len() < 10_000);
    Arc::make_mut(&mut view.calls)[0].failure = Some(Failure {
        code: Some(ErrorCode::Unavailable),
    });
    summary(&mut view).diagnostics = true;
    summary(&mut view).has_details = true;
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["hidden"], false);
    assert_eq!(encoded["calls"][0]["row"]["failed"], true);
    assert_eq!(encoded["calls"][0]["output_deferred"], true);
}

#[test]
fn resolves_literal_content_only_in_detail_packet() {
    let (view, detail) = fixture(
        Presentation::Content,
        json!({"output":{
            "escaped/key":{"~value":["Original\ntext 🙂"]},
            "sailry_content":{"version":1,"blocks":[
                {"kind":"text","path":"/escaped~1key/~0value/0"},
                {"kind":"notice","message":{"label":"Kept"}}
            ]}
        }}),
    );
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["texts"], json!([]));
    assert_eq!(encoded["calls"][0]["output_deferred"], true);
    let page = &view.snapshot.as_ref().unwrap().page;
    let packet: Value =
        serde_json::from_str(&packet(&view.calls[0].with_detail(detail), page).unwrap()).unwrap();
    assert_eq!(packet["texts"], json!(["Original\ntext 🙂", null]));
    assert_eq!(
        packet["result"]["output"]["escaped/key"]["~value"][0],
        "Original\ntext 🙂"
    );
}

#[test]
fn failure_does_not_select_success_preview() {
    let (mut view, mut detail) = fixture(Presentation::Hidden, json!({"error":"Actual failure"}));
    let resolved = display::Resolved {
        label: None,
        input: Default::default(),
        output: Some(display::Output {
            preview: "Claimed success".into(),
            preview_deferred: false,
            notices: vec![],
            paths: vec![],
            path_count: 0,
            body: None,
            table: None,
        }),
        approval: vec![],
        status: None,
        command: None,
        diagnostics: vec![],
    };
    Arc::make_mut(&mut view.calls)[0].failure = Some(Failure { code: None });
    summary(&mut view).diagnostics = true;
    summary(&mut view).has_details = true;
    summary(&mut view).resolved = Some(resolved.clone());
    Arc::make_mut(&mut view.calls)[0].resolved = Some(resolved.clone());
    Arc::make_mut(&mut detail).resolved = Some(resolved);
    let original = serde_json::to_value(&view.snapshot).unwrap();
    let encoded = serialized(&view);
    assert_eq!(encoded["calls"][0]["output"], Value::Null);
    assert_eq!(
        encoded["calls"][0]["resolved"]["output"]["preview"],
        "Claimed success"
    );
    let packet: Value = serde_json::from_str(
        &packet(
            &view.calls[0].with_detail(detail),
            &view.snapshot.as_ref().unwrap().page,
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(packet["output"], Value::Null);
    assert_eq!(packet["result"]["error"], "Actual failure");
    assert_eq!(encoded["snapshot"], original);
}

struct Backend {
    reference: sailry_protocol::tool::detail::Reference,
    detail: Arc<Detail>,
}

impl Transport for Backend {
    fn target(&self) -> NodeId {
        NodeId([1; 32])
    }
    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { Err(Fault::new(ErrorCode::Unavailable, "unused subscription")) })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let Command::ReadToolDetail { cursor, limit, .. } = request.command else {
                panic!("tool detail request expected")
            };
            let text = serde_json::to_string(&self.detail).unwrap();
            let offset = cursor.unwrap_or(0);
            let mut end = offset.saturating_add(limit).min(text.len());
            while !text.is_char_boundary(end) {
                end -= 1;
            }
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender
                .send(Ok(Output::ToolDetail(
                    sailry_protocol::tool::detail::Page {
                        reference: self.reference.clone(),
                        offset,
                        total_bytes: text.len(),
                        next_offset: (end < text.len()).then_some(end),
                        text: text[offset..end].into(),
                    },
                )))
                .unwrap();
            Ok(Admission {
                receipt: Receipt {
                    id: request.id,
                    durable: false,
                },
                completion,
            })
        })
    }
}

#[tokio::test]
async fn output_does_not_wait_for_updates() {
    use crate::api::conversation::ConversationUpdates;
    use sailry_link::CancellationToken;
    use tokio::sync::{Mutex, mpsc, watch};
    let (view, detail) = fixture(
        Presentation::Details,
        json!("Original output".repeat(10_000)),
    );
    let backend = Backend {
        reference: view.calls[0]
            .detail_reference(&view.snapshot.as_ref().unwrap().page)
            .unwrap(),
        detail,
    };
    let client = Arc::new(Client::new(Arc::new(backend)));
    let (_sender, receiver) = watch::channel(view);
    let (older, _requests) = mpsc::channel(1);
    let updates = Arc::new(ConversationUpdates {
        client,
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
    let packet: Value =
        serde_json::from_str(&updates.tool_output("request".into(), 0).await.unwrap()).unwrap();
    assert_eq!(
        packet["output"]["preview"],
        "Original output".repeat(10_000)
    );
    assert_eq!(packet["result"], "Original output".repeat(10_000));
    assert!(updates.tool_output("different".into(), 0).await.is_err());
    updates.close();
    assert!(pending.await.unwrap().is_err());
    assert!(updates.tool_output("request".into(), 0).await.is_err());
}
