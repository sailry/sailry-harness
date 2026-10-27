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

struct Backend(
    AtomicUsize,
    std::sync::Mutex<std::collections::BTreeSet<String>>,
    Option<String>,
);

impl Backend {
    fn new() -> Arc<Self> {
        Arc::new(Self(AtomicUsize::new(0), Default::default(), None))
    }

    fn large(text: String) -> Arc<Self> {
        Arc::new(Self(AtomicUsize::new(0), Default::default(), Some(text)))
    }
}

impl Transport for Backend {
    fn target(&self) -> NodeId {
        NodeId([53; 32])
    }
    fn subscribe(&self, _: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        Box::pin(async { Err(Fault::new(ErrorCode::Unavailable, "unused subscription")) })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            assert!(tokio::runtime::Handle::try_current().is_ok());
            let Command::ReadToolDetail {
                session,
                turn,
                entry,
                index,
                call,
                expected_revision,
                cursor,
                limit,
                ..
            } = request.command
            else {
                panic!("detail read expected")
            };
            self.0.fetch_add(1, Ordering::Relaxed);
            if self.1.lock().unwrap().remove(&entry) {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected tool detail failure",
                ));
            }
            let small = serde_json::to_string(&Detail {
                arguments: json!({"path":"fixture.txt"}),
                result: Some(json!({"text":"A small result"})),
                resolved: None,
                content: None,
                images: vec![],
            })
            .unwrap();
            let full = self.2.as_ref().unwrap_or(&small);
            let offset = cursor.unwrap_or(0);
            let end = (offset + limit).min(full.len());
            let text = full[offset..end].to_owned();
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
                    offset,
                    total_bytes: full.len(),
                    next_offset: (end < full.len()).then_some(end),
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
    let backend = Backend::new();
    let mut observation = Observation {
        history: history(sailry_protocol::SessionId::new(), TurnId::new()),
        client: Arc::new(Client::new(backend.clone())),
        runtime: tokio::runtime::Handle::current(),
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
    let backend = Backend::new();
    let mut observation = Observation {
        history: history(sailry_protocol::SessionId::new(), TurnId::new()),
        client: Arc::new(Client::new(backend.clone())),
        runtime: tokio::runtime::Handle::current(),
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
fn retains_compact_values_over_parsed_budget() {
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
    cache.insert("oversized".into(), stamp, &value).unwrap();
    assert_eq!(cache.values.len(), 1);
    assert!(cache.bytes < 16 * 1024);
}

#[tokio::test]
async fn a_failed_call_does_not_hide_peers() {
    let backend = Backend::new();
    backend.1.lock().unwrap().insert("entry-0".into());
    let observation = Observation {
        history: history(sailry_protocol::SessionId::new(), TurnId::new()),
        client: Arc::new(Client::new(backend.clone())),
        runtime: tokio::runtime::Handle::current(),
        prefix: "plugin_fixture_".into(),
        cache: Default::default(),
    };
    let partial = snapshot(&observation).await.unwrap();
    assert_eq!(partial["calls"].as_array().unwrap().len(), 64);
    assert!(
        partial["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|call| call["timestamp_ms"] != 0)
    );
    let recovered = snapshot(&observation).await.unwrap();
    assert_eq!(recovered["calls"].as_array().unwrap().len(), 65);
    assert_eq!(backend.0.load(Ordering::Relaxed), 66);
}

#[core::prelude::v1::test]
fn dispatches_on_the_binding_runtime() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let backend = Backend::new();
    let observation = Observation {
        history: history(sailry_protocol::SessionId::new(), TurnId::new()),
        client: Arc::new(Client::new(backend)),
        runtime: runtime.handle().clone(),
        prefix: "plugin_fixture_".into(),
        cache: Default::default(),
    };
    assert!(tokio::runtime::Handle::try_current().is_err());
    let value = futures::executor::block_on(read(observation, CancellationToken::new())).unwrap();
    assert_eq!(value["calls"].as_array().unwrap().len(), 65);
}

#[tokio::test]
async fn compact_snapshots_reuse_large_parsed_values() {
    let detail = Detail {
        arguments: json!({"path":"fixture.txt"}),
        result: Some(Value::Array(vec![Value::Null; 600_000])),
        resolved: None,
        content: None,
        images: Vec::new(),
    };
    let text = serde_json::to_string(&detail).unwrap();
    let parsed: Detail = serde_json::from_str(&text).unwrap();
    assert!(
        2 * sailry_client::conversation::details::retained_bytes(parsed.result.as_ref().unwrap())
            > MAX_CAPTURE_BYTES
    );
    drop(parsed);
    drop(detail);
    let backend = Backend::large(text);
    let mut history = history(sailry_protocol::SessionId::new(), TurnId::new());
    Arc::make_mut(&mut history.calls).truncate(2);
    let observation = Observation {
        history,
        client: Arc::new(Client::new(backend.clone())),
        runtime: tokio::runtime::Handle::current(),
        prefix: "plugin_fixture_".into(),
        cache: Default::default(),
    };
    let first = snapshot(&observation).await.unwrap();
    assert_eq!(first["calls"].as_array().unwrap().len(), 2);
    let reads = backend.0.load(Ordering::Relaxed);
    assert!(reads > 2);
    assert_eq!(snapshot(&observation).await.unwrap(), first);
    assert_eq!(backend.0.load(Ordering::Relaxed), reads);
    assert!(observation.cache.lock().await.bytes < 8 * 1024 * 1024);
}

#[tokio::test]
async fn over_capacity_snapshots_do_not_refetch() {
    let backend = Backend::new();
    let mut history = history(sailry_protocol::SessionId::new(), TurnId::new());
    Arc::make_mut(&mut history.calls).truncate(2);
    let page = &history.snapshot.as_ref().unwrap().page;
    let mut cache = Cache::default();
    let value = Value::String("x".repeat(MAX_CAPTURE_BYTES / 2));
    for (index, call) in history.calls.iter().enumerate() {
        let stamp = Stamp {
            tool: call.stamp(page).unwrap(),
            timestamp_ms: index as i64,
            sequence: Some(index as u64 + 1),
        };
        let key = format!("{}:{}", call.turn, call.source.key());
        assert_eq!(cache.insert(key, stamp, &value).is_ok(), index == 0);
    }
    drop(value);
    assert!(
        cache
            .values
            .iter()
            .all(|(_, captured)| captured.value.is_none())
    );
    let mut observation = Observation {
        history,
        client: Arc::new(Client::new(backend.clone())),
        runtime: tokio::runtime::Handle::current(),
        prefix: "plugin_fixture_".into(),
        cache: Arc::new(tokio::sync::Mutex::new(cache)),
    };
    assert!(snapshot(&observation).await.is_err());
    assert!(snapshot(&observation).await.is_err());
    assert_eq!(backend.0.load(Ordering::Relaxed), 0);
    Arc::make_mut(&mut observation.history.calls).truncate(1);
    assert_eq!(
        snapshot(&observation).await.unwrap()["calls"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(backend.0.load(Ordering::Relaxed), 1);
}
