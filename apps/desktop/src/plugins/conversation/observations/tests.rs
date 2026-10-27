use super::*;
use sailry_client::{
    Client,
    conversation::tools::{Call, Reference as CallReference, State},
};
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{
    Command, ErrorCode, Fault, NodeId, Output, Receipt, Request, Topic, TurnId,
    conversation::{Page, Snapshot},
    tool::detail::{Detail, Page as DetailPage, Reference},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

struct Backend(AtomicUsize);

impl Transport for Backend {
    fn target(&self) -> NodeId {
        NodeId([53; 32])
    }
    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { Err(Fault::new(ErrorCode::Unavailable, "unused subscription")) })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let Command::ReadToolDetail {
                session,
                turn,
                entry,
                index,
                call,
                expected_revision,
                cursor,
                ..
            } = request.command
            else {
                panic!("detail read expected")
            };
            assert!(cursor.is_none());
            self.0.fetch_add(1, Ordering::Relaxed);
            let text = serde_json::to_string(&Detail {
                arguments: json!({"path":"fixture.txt"}),
                result: Some(json!({"text":"A small result"})),
                resolved: None,
                content: None,
                images: vec![],
            })
            .unwrap();
            let reference = Reference {
                session,
                turn,
                entry,
                index,
                call,
                revision: expected_revision,
            };
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender
                .send(Ok(Output::ToolDetail(DetailPage {
                    reference,
                    offset: 0,
                    total_bytes: text.len(),
                    next_offset: None,
                    text,
                })))
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

fn history(session: sailry_protocol::SessionId, turn: TurnId) -> sailry_client::conversation::View {
    let entries = (0..65).map(|index| json!({
        "sequence":index+1, "id":format!("entry-{index}"), "turn":turn,
        "author":"assistant", "branch":"", "timestamp_ms":index,
        "parts":[
            sailry_protocol::conversation::Part::ToolCall {
                id:Some(format!("call-{index}")),name:"plugin_fixture_read".into(),presentation:Default::default(),grouping:Default::default(),
                summary:Box::new(sailry_protocol::tool::detail::Summary {revision:"a".repeat(64),..Default::default()})
            },
            sailry_protocol::conversation::Part::ToolResult {
                id:Some(format!("call-{index}")),name:"plugin_fixture_read".into(),images:vec![],
                summary:Box::new(sailry_protocol::tool::detail::Summary {revision:"a".repeat(64),..Default::default()})
            }
        ], "citations":[], "search_suggestions":null, "usage":null
    })).collect::<Vec<_>>();
    let page:Page = serde_json::from_value(json!({
        "session":session,"revision":1,"entries":entries,"runs":[],"queue":sailry_protocol::conversation::Queue::default(),
        "approvals":[],"questions":[],"children":[],"background":[],"next_before":null
    })).unwrap();
    let calls = Arc::new(
        page.entries
            .iter()
            .enumerate()
            .map(|(index, entry)| Call {
                turn,
                id: Some(format!("call-{index}")),
                name: "plugin_fixture_read".into(),
                terminal_interaction: false,
                presentation: Default::default(),
                grouping: Default::default(),
                source: CallReference {
                    entry: entry.id.clone(),
                    index: 0,
                },
                response: Some(CallReference {
                    entry: entry.id.clone(),
                    index: 1,
                }),
                state: State::Returned,
                failure: None,
                approval: None,
                question: None,
                progress: None,
                resolved: None,
                detail: None,
            })
            .collect(),
    );
    sailry_client::conversation::View {
        connected: true,
        calls,
        snapshot: Some(Arc::new(Snapshot {
            node: NodeId([53; 32]),
            sequence: 65,
            page: Arc::new(page),
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
        })),
        ..Default::default()
    }
}

#[tokio::test]
async fn reads_only_changed_calls() {
    let backend = Arc::new(Backend(AtomicUsize::new(0)));
    let mut observation = Observation {
        history: history(sailry_protocol::SessionId::new(), TurnId::new()),
        client: Arc::new(Client::new(backend.clone())),
        prefix: "plugin_fixture_".into(),
        cache: Default::default(),
    };
    let first = snapshot(&observation).await.unwrap();
    assert_eq!(first["calls"].as_array().unwrap().len(), 65);
    assert_eq!(backend.0.load(Ordering::Relaxed), 65);
    assert_eq!(snapshot(&observation).await.unwrap(), first);
    assert_eq!(backend.0.load(Ordering::Relaxed), 65);
    let view = observation.history.snapshot.as_mut().unwrap();
    let page = &mut Arc::make_mut(view).page;
    let page = Arc::make_mut(page);
    let sailry_protocol::conversation::Part::ToolResult { summary, .. } =
        &mut page.entries[0].parts[1]
    else {
        unreachable!()
    };
    summary.revision = "b".repeat(64);
    snapshot(&observation).await.unwrap();
    assert_eq!(backend.0.load(Ordering::Relaxed), 66);
    observation.history.snapshot = None;
    snapshot(&observation).await.unwrap();
    let cache = observation.cache.lock().await;
    assert!(cache.values.is_empty());
    assert_eq!(cache.bytes, 0);
}

#[tokio::test]
async fn metadata_updates_reuse_captured_details() {
    let backend = Arc::new(Backend(AtomicUsize::new(0)));
    let mut observation = Observation {
        history: history(sailry_protocol::SessionId::new(), TurnId::new()),
        client: Arc::new(Client::new(backend.clone())),
        prefix: "plugin_fixture_".into(),
        cache: Default::default(),
    };
    snapshot(&observation).await.unwrap();
    let view = Arc::make_mut(observation.history.snapshot.as_mut().unwrap());
    Arc::make_mut(&mut view.page).entries[0].timestamp_ms = 1234;
    let changed = snapshot(&observation).await.unwrap();
    assert_eq!(changed["calls"][0]["timestamp_ms"], 1234);
    assert_eq!(backend.0.load(Ordering::Relaxed), 65);
}

#[core::prelude::v1::test]
fn rejects_values_over_retained_budget() {
    let history = history(sailry_protocol::SessionId::new(), TurnId::new());
    let page = &history.snapshot.as_ref().unwrap().page;
    let stamp = Stamp {
        tool: history.calls[0].stamp(page).unwrap(),
        timestamp_ms: 0,
        sequence: None,
    };
    let mut text = String::with_capacity(MAX_CAPTURE_BYTES);
    text.push('x');
    let value = Value::String(text);
    assert!(sailry_client::conversation::details::retained_bytes(&value) > MAX_CAPTURE_BYTES);
    let mut cache = Cache::default();
    cache.insert("oversized".into(), stamp, value);
    assert!(cache.values.is_empty());
    assert_eq!(cache.bytes, 0);
}
