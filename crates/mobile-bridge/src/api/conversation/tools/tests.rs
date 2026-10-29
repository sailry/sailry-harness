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
    serde_json::from_str(&super::view(view, None, &mut Default::default()).unwrap()).unwrap()
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
    assert_eq!(encoded["calls"][0]["details"], true);
    assert!(encoded["calls"][0].get("output_deferred").is_none());
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
    assert!(encoded["calls"][0].get("texts").is_none());
    assert!(encoded["calls"][0].get("output_deferred").is_none());
    assert_eq!(encoded["calls"][0]["details"], true);
    let page = &view.snapshot.as_ref().unwrap().page;
    let packet = packet(&view.calls[0].with_detail(detail), page).unwrap();
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
    let packet = packet(
        &view.calls[0].with_detail(detail),
        &view.snapshot.as_ref().unwrap().page,
    )
    .unwrap();
    assert_eq!(packet["output"], Value::Null);
    assert_eq!(packet["result"]["error"], "Actual failure");
    assert_eq!(encoded["snapshot"], original);
}

struct Backend {
    reference: sailry_protocol::tool::detail::Reference,
    detail: Arc<Detail>,
    total: Option<usize>,
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
                        total_bytes: self.total.unwrap_or(text.len()),
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
async fn sends_selected_details_once_per_revision() {
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
        total: None,
    };
    let client = Arc::new(Client::new(Arc::new(backend)));
    let (sender, receiver) = watch::channel(view);
    let (older, _requests) = mpsc::channel(1);
    let details = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    let detail_changes = Mutex::new(details.subscribe());
    let updates = Arc::new(ConversationUpdates {
        details,
        detail_changes,
        selected: Default::default(),
        sent: Default::default(),
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
    updates
        .track_tools(json!({"items":[{"entry":"request","index":0}]}).to_string())
        .unwrap();
    let mut value: Value = serde_json::from_str(&pending.await.unwrap().unwrap()).unwrap();
    while value["calls"][0]["detail_state"]["kind"] != "ready" {
        value = serde_json::from_str(&updates.next().await.unwrap()).unwrap();
    }
    let packet = &value["calls"][0]["detail"];
    assert_eq!(
        packet["output"]["preview"],
        "Original output".repeat(10_000)
    );
    assert_eq!(packet["result"], "Original output".repeat(10_000));
    let key = value["calls"][0]["detail_key"].clone();
    let reference: sailry_protocol::tool::detail::Reference =
        serde_json::from_str(key.as_str().unwrap()).unwrap();
    let current = sender.borrow();
    assert_eq!(
        Some(reference),
        current.calls[0].detail_reference(&current.snapshot.as_ref().unwrap().page)
    );
    drop(current);
    let mut current = sender.borrow().clone();
    Arc::make_mut(current.snapshot.as_mut().unwrap()).sequence += 1;
    sender.send_replace(current);
    let repeated: Value = serde_json::from_str(&updates.next().await.unwrap()).unwrap();
    assert_eq!(repeated["calls"][0]["detail_state"]["kind"], "ready");
    assert!(repeated["calls"][0].get("detail").is_none());
    assert_eq!(repeated["calls"][0]["detail_key"], key);
    updates
        .track_tools(json!({"items":[]}).to_string())
        .unwrap();
    let released: Value = serde_json::from_str(&updates.next().await.unwrap()).unwrap();
    assert!(released["calls"][0]["detail"].is_null());
    assert_eq!(released["calls"][0]["detail_key"], key);
    updates
        .track_tools(json!({"items":[{"entry":"request","index":0}]}).to_string())
        .unwrap();
    let mut restored: Value = serde_json::from_str(&updates.next().await.unwrap()).unwrap();
    while restored["calls"][0]["detail_state"]["kind"] != "ready" {
        restored = serde_json::from_str(&updates.next().await.unwrap()).unwrap();
    }
    assert_eq!(
        restored["calls"][0]["detail"]["result"],
        "Original output".repeat(10_000)
    );
    updates.close();
    assert!(updates.next().await.is_err());
    assert!(
        updates
            .track_tools(json!({"items":[]}).to_string())
            .is_err()
    );
}

#[tokio::test]
async fn refreshes_approval_facts_without_resending_details() {
    use sailry_protocol::{
        ApprovalId,
        conversation::{Approval, ApprovalSource, ApprovalState},
        process::Access,
    };
    let (mut current, mut detail) = fixture(Presentation::Details, Value::Null);
    let snapshot = Arc::make_mut(current.snapshot.as_mut().unwrap());
    let page = Arc::make_mut(&mut snapshot.page);
    page.entries.truncate(1);
    let Part::ToolCall { summary, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    summary.revision = "a".repeat(64);
    let call = &mut Arc::make_mut(&mut current.calls)[0];
    call.response = None;
    call.state = State::Waiting;
    let approval = Approval {
        id: ApprovalId::new(),
        session: page.session,
        turn: call.turn,
        entry: call.source.entry.clone(),
        index: call.source.index,
        state: ApprovalState::Pending,
        source: ApprovalSource::User,
        choice: None,
        reason: None,
        prefix: None,
        can_remember: false,
        startup: None,
        ssh: None,
        access: None,
    };
    call.approval = Some(approval.clone());
    page.approvals.push(approval);
    let full = display::Resolved {
        label: None,
        input: display::Input {
            summary: Some(display::Text {
                text: "Exact captured command".into(),
                code: true,
            }),
            context: Some(display::Text {
                text: "/exact/working/directory".into(),
                code: false,
            }),
            ..Default::default()
        },
        output: None,
        approval: vec![],
        status: None,
        command: None,
        diagnostics: vec![],
    };
    Arc::make_mut(&mut detail).resolved = Some(full);
    let reference = call.detail_reference(page).unwrap();
    let client = Arc::new(Client::new(Arc::new(Backend {
        reference: reference.clone(),
        detail,
        total: None,
    })));
    let tracker = Tracker::new(client, tokio::runtime::Handle::current());
    assert!(current.calls[0].approval_pending(&current.snapshot.as_ref().unwrap().page));
    select(
        &current,
        &[Selection {
            entry: "request".into(),
            index: 0,
        }],
        &tracker,
    );
    assert!(matches!(
        tracker.wait(&reference).await,
        sailry_client::conversation::details::State::Ready(_)
    ));
    let mut sent = BTreeSet::new();
    let first: Value =
        serde_json::from_str(&super::view(&current, Some(&tracker), &mut sent).unwrap()).unwrap();
    assert_eq!(first["calls"][0]["approval_view"]["unrestricted"], false);
    assert_eq!(
        first["calls"][0]["approval_view"]["context"],
        "/exact/working/directory"
    );
    assert!(first["calls"][0]["detail"].is_object());
    assert!(first["calls"][0]["detail"].get("approval_view").is_none());
    Arc::make_mut(&mut current.calls)[0]
        .approval
        .as_mut()
        .unwrap()
        .access = Some(Access::Unrestricted);
    let snapshot = Arc::make_mut(current.snapshot.as_mut().unwrap());
    snapshot.sequence += 1;
    Arc::make_mut(&mut snapshot.page).approvals[0].access = Some(Access::Unrestricted);
    assert_eq!(
        current.calls[0].detail_reference(&snapshot.page),
        Some(reference)
    );
    let latest: Value =
        serde_json::from_str(&super::view(&current, Some(&tracker), &mut sent).unwrap()).unwrap();
    assert_eq!(latest["calls"][0]["detail_state"]["kind"], "ready");
    assert!(latest["calls"][0].get("detail").is_none());
    assert_eq!(latest["calls"][0]["approval_view"]["unrestricted"], true);
    assert_eq!(
        latest["calls"][0]["approval_view"]["description"]["label"],
        "Exact captured command"
    );
    assert_eq!(
        latest["calls"][0]["approval_view"]["context"],
        "/exact/working/directory"
    );
}

#[tokio::test]
async fn selection_exposes_limits_and_releases_details() {
    use crate::api::conversation::ConversationUpdates;
    use sailry_link::CancellationToken;
    use tokio::sync::{Mutex, mpsc, watch};
    let (view, detail) = fixture(Presentation::Details, json!("Original output"));
    let reference = view.calls[0]
        .detail_reference(&view.snapshot.as_ref().unwrap().page)
        .unwrap();
    let backend = Backend {
        reference: reference.clone(),
        detail,
        total: Some(sailry_client::conversation::details::MAX_BYTES + 1),
    };
    let client = Arc::new(Client::new(Arc::new(backend)));
    let (_sender, receiver) = watch::channel(view);
    let (older, _requests) = mpsc::channel(1);
    let details = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    let detail_changes = Mutex::new(details.subscribe());
    let updates = ConversationUpdates {
        details,
        detail_changes,
        selected: Default::default(),
        sent: Default::default(),
        current: receiver.clone(),
        receiver: Mutex::new(receiver),
        stop: CancellationToken::new(),
        task: tokio::spawn(std::future::pending()),
        older,
    };
    updates
        .track_tools(json!({"items":[{"entry":"request","index":0}]}).to_string())
        .unwrap();
    assert!(matches!(
        updates.details.wait(&reference).await,
        sailry_client::conversation::details::State::TooLarge(Limit::Transfer)
    ));
    let current = updates.current.borrow().clone();
    let summary = serialized(&current);
    let value: Value = serde_json::from_str(
        &super::view(&current, Some(&updates.details), &mut Default::default()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        value["calls"][0]["detail_state"],
        json!({"kind":"too_large","limit":"transfer"})
    );
    assert_eq!(
        value["calls"][0]["approval_view"],
        summary["calls"][0]["approval_view"]
    );
    assert_eq!(value["detail_errors"].as_array().unwrap().len(), 1);
    updates
        .track_tools(json!({"items":[]}).to_string())
        .unwrap();
    let value: Value = serde_json::from_str(
        &super::view(&current, Some(&updates.details), &mut Default::default()).unwrap(),
    )
    .unwrap();
    assert!(value["calls"][0]["detail_state"].is_null());
    assert!(value["calls"][0]["detail"].is_null());
    updates.close();
}
