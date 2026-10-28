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
    Option<Arc<tokio::sync::Barrier>>,
);

impl Backend {
    fn new() -> Arc<Self> {
        Arc::new(Self(AtomicUsize::new(0), Default::default(), None, None))
    }

    fn large(text: String) -> Arc<Self> {
        Arc::new(Self(
            AtomicUsize::new(0),
            Default::default(),
            Some(text),
            None,
        ))
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
            if let Some(barrier) = &self.3 {
                barrier.wait().await;
            }
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
            let (failed, permanent) = {
                let mut failures = self.1.lock().unwrap();
                let permanent = failures.contains(&format!("always:{entry}"));
                (permanent || failures.remove(&entry), permanent)
            };
            if failed {
                return Err(Fault::new(
                    if permanent {
                        ErrorCode::NotFound
                    } else {
                        ErrorCode::Unavailable
                    },
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
            let full = self
                .2
                .as_ref()
                .filter(|_| entry != "entry-1")
                .unwrap_or(&small);
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
    let entries = (0..3).map(|index| json!({
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
            sequence: 3,
            page: Arc::new(page),
            missing: vec![],
            drafts: vec![],
            statistics: Default::default(),
            artifacts: Default::default(),
        })),
        ..Default::default()
    }
}

fn observation(
    history: sailry_client::conversation::View,
    client: Arc<Client>,
    runtime: tokio::runtime::Handle,
) -> Observation {
    let details =
        sailry_client::conversation::details::Tracker::new(client.clone(), runtime.clone());
    Observation {
        history,
        runtime,
        client,
        prefix: "plugin_fixture_".into(),
        details,
    }
}

#[tokio::test]
async fn reads_only_changed_calls() {
    let backend = Backend::new();
    let mut observation = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend.clone())),
        tokio::runtime::Handle::current(),
    );
    let first = snapshot(&observation).await.unwrap();
    assert_eq!(first["calls"].as_array().unwrap().len(), 3);
    assert_eq!(backend.0.load(Ordering::Relaxed), 3);
    assert_eq!(snapshot(&observation).await.unwrap(), first);
    assert_eq!(backend.0.load(Ordering::Relaxed), 3);
    let view = observation.history.snapshot.as_mut().unwrap();
    let page = Arc::make_mut(&mut Arc::make_mut(view).page);
    let sailry_protocol::conversation::Part::ToolResult { summary, .. } =
        &mut page.entries[0].parts[1]
    else {
        unreachable!()
    };
    summary.revision = "b".repeat(64);
    snapshot(&observation).await.unwrap();
    assert_eq!(backend.0.load(Ordering::Relaxed), 4);
    observation.history.snapshot = None;
    assert!(
        snapshot(&observation).await.unwrap()["calls"]
            .as_array()
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn metadata_updates_reuse_existing_details() {
    let backend = Backend::new();
    let mut observation = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend.clone())),
        tokio::runtime::Handle::current(),
    );
    snapshot(&observation).await.unwrap();
    let view = Arc::make_mut(observation.history.snapshot.as_mut().unwrap());
    Arc::make_mut(&mut view.page).entries[0].timestamp_ms = 1234;
    let changed = snapshot(&observation).await.unwrap();
    assert_eq!(changed["calls"][0]["timestamp_ms"], 1234);
    assert_eq!(backend.0.load(Ordering::Relaxed), 3);
}

#[tokio::test]
async fn concurrent_snapshots_keep_their_selected_calls() {
    let backend = Backend::new();
    let first = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend.clone())),
        tokio::runtime::Handle::current(),
    );
    let mut second = first.clone();
    Arc::make_mut(&mut second.history.calls).reverse();
    let (first, second) = tokio::join!(snapshot(&first), snapshot(&second));
    for (value, expected) in [(first.unwrap(), [0, 1, 2]), (second.unwrap(), [2, 1, 0])] {
        assert_eq!(
            value["calls"]
                .as_array()
                .unwrap()
                .iter()
                .map(|call| call["timestamp_ms"].as_i64().unwrap())
                .collect::<Vec<_>>(),
            expected
        );
        assert!(value["errors"].as_array().unwrap().is_empty());
        assert!(value["calls"].as_array().unwrap().iter().all(|call| {
            call["arguments"]["path"] == "fixture.txt" && call["result"]["text"] == "A small result"
        }));
    }
    assert_eq!(backend.0.load(Ordering::Relaxed), 3);
}

#[tokio::test(start_paused = true)]
async fn recovers_without_new_history() {
    let backend = Backend::new();
    backend.1.lock().unwrap().insert("entry-0".into());
    let observation = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend.clone())),
        tokio::runtime::Handle::current(),
    );
    let (_sender, receiver) = watch::channel(BTreeMap::from([("fixture".to_owned(), observation)]));
    let stop = CancellationToken::new();
    let first = next(receiver.clone(), "", stop.clone()).await.unwrap();
    let partial = &first["conversations"]["fixture"];
    assert_eq!(partial["calls"].as_array().unwrap().len(), 2);
    assert_eq!(partial["errors"].as_array().unwrap().len(), 1);
    let mut seen = first["cursor"].as_str().unwrap().to_owned();
    let recovered = tokio::time::timeout(std::time::Duration::from_secs(1), async {
        loop {
            let update = next(receiver.clone(), &seen, stop.clone()).await.unwrap();
            seen = update["cursor"].as_str().unwrap().to_owned();
            if update["conversations"]["fixture"]["calls"]
                .as_array()
                .unwrap()
                .len()
                == 3
            {
                break update;
            }
        }
    })
    .await
    .expect("tracker recovery must publish without a history change");
    assert!(
        recovered["conversations"]["fixture"]["errors"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!receiver.has_changed().unwrap());
    assert_eq!(backend.0.load(Ordering::Relaxed), 4);
}

#[tokio::test(start_paused = true)]
async fn failure_allows_later_calls() {
    let backend = Backend::new();
    backend.1.lock().unwrap().insert("always:entry-0".into());
    let observation = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend.clone())),
        tokio::runtime::Handle::current(),
    );
    let reference = selected(&observation)[0].2.clone();
    let (sender, receiver) = watch::channel(BTreeMap::from([("fixture".to_owned(), observation)]));
    let stop = CancellationToken::new();
    let first = next(receiver.clone(), "", stop.clone()).await.unwrap();
    let partial = &first["conversations"]["fixture"];
    assert_eq!(partial["calls"].as_array().unwrap().len(), 2);
    assert!(
        partial["calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|call| call["timestamp_ms"] != 0)
    );
    assert_eq!(partial["errors"].as_array().unwrap().len(), 1);
    let tracker = sender.borrow()["fixture"].details.clone();
    assert!(matches!(
        tracker.wait(&reference).await,
        sailry_client::conversation::details::State::Failed { retry_at: None, .. }
    ));
    let settled = next(
        receiver.clone(),
        first["cursor"].as_str().unwrap(),
        stop.clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        settled["conversations"]["fixture"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let reads = backend.0.load(Ordering::Relaxed);
    assert_eq!(reads, 7, "a permanent failure has only bounded retries");
    sender.send_modify(|observed| {
        let observation = observed.get_mut("fixture").unwrap();
        let snapshot = Arc::make_mut(observation.history.snapshot.as_mut().unwrap());
        snapshot.sequence += 1;
        let page = Arc::make_mut(&mut snapshot.page);
        page.revision += 1;
        let mut entry = page.entries[2].clone();
        entry.id = "entry-3".into();
        entry.sequence = 4;
        entry.timestamp_ms = 3;
        for part in &mut entry.parts {
            match part {
                sailry_protocol::conversation::Part::ToolCall { id, .. }
                | sailry_protocol::conversation::Part::ToolResult { id, .. } => {
                    *id = Some("call-3".into());
                }
                _ => unreachable!(),
            }
        }
        page.entries.push(entry);
        let calls = Arc::make_mut(&mut observation.history.calls);
        let mut call = calls[2].clone();
        call.id = Some("call-3".into());
        call.source.entry = "entry-3".into();
        call.response.as_mut().unwrap().entry = "entry-3".into();
        calls.push(call);
    });
    let update = next(receiver, settled["cursor"].as_str().unwrap(), stop)
        .await
        .unwrap();
    let calls = update["conversations"]["fixture"]["calls"]
        .as_array()
        .unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(calls.last().unwrap()["timestamp_ms"], 3);
    assert_eq!(
        update["conversations"]["fixture"]["errors"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(backend.0.load(Ordering::Relaxed), reads + 1);
}

#[tokio::test(start_paused = true)]
async fn reads_calls_concurrently_without_reordering() {
    let backend = Arc::new(Backend(
        AtomicUsize::new(0),
        Default::default(),
        None,
        Some(Arc::new(tokio::sync::Barrier::new(3))),
    ));
    let observation = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend.clone())),
        tokio::runtime::Handle::current(),
    );
    let value = tokio::time::timeout(std::time::Duration::from_secs(1), snapshot(&observation))
        .await
        .expect("three reads must reach the barrier together")
        .unwrap();
    assert_eq!(backend.0.load(Ordering::Relaxed), 3);
    assert_eq!(
        value["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|call| call["timestamp_ms"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
}

#[core::prelude::v1::test]
fn dispatches_on_the_binding_runtime() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let backend = Backend::new();
    let observation = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend)),
        runtime.handle().clone(),
    );
    assert!(tokio::runtime::Handle::try_current().is_err());
    let value = futures::executor::block_on(read(observation, CancellationToken::new())).unwrap();
    assert_eq!(value["calls"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn eviction_keeps_other_sessions_live() {
    let detail = Detail {
        arguments: Value::Null,
        result: Some(Value::String("x".repeat(34 * 1024 * 1024))),
        resolved: None,
        content: None,
        images: vec![],
    };
    let text = serde_json::to_string(&detail).unwrap();
    drop(detail);
    let backend = Backend::large(text);
    let client = Arc::new(Client::new(backend.clone()));
    let current = history(sailry_protocol::SessionId::new(), TurnId::new());
    let mut initial = current.clone();
    Arc::make_mut(&mut initial.calls).truncate(1);
    let initial_page = Arc::make_mut(initial.snapshot.as_mut().unwrap());
    initial_page.sequence = 1;
    Arc::make_mut(&mut initial_page.page).entries.truncate(1);
    let oldest = initial.calls[0]
        .detail_reference(&initial.snapshot.as_ref().unwrap().page)
        .unwrap();
    let mut observed = observation(initial, client.clone(), tokio::runtime::Handle::current());
    let initial = snapshot(&observed).await.unwrap();
    assert_eq!(initial["calls"].as_array().unwrap().len(), 1);
    assert_eq!(initial["calls"][0]["timestamp_ms"], 0);
    drop(initial);
    let sailry_client::conversation::details::Read::Ready(detail) =
        client.detail(oldest).await.unwrap()
    else {
        panic!("complete detail expected");
    };
    assert!(detail.bytes < sailry_client::conversation::details::MAX_BYTES);
    assert!(2 * detail.bytes > sailry_client::conversation::details::MAX_BYTES);
    let released = Arc::downgrade(&detail.detail);
    drop(detail);
    observed.history = current;
    let first = snapshot(&observed).await.unwrap();
    let calls = first["calls"].as_array().unwrap();
    assert_eq!(calls.len(), 3);
    assert_eq!(
        calls
            .iter()
            .map(|call| call["timestamp_ms"].as_i64().unwrap())
            .collect::<Vec<_>>(),
        vec![0, 1, 2]
    );
    assert!(released.upgrade().is_none());
    assert_eq!(calls[0]["result"].as_str().unwrap().len(), 34 * 1024 * 1024);
    assert_eq!(calls[2]["result"], calls[0]["result"]);
    assert!(first["errors"].as_array().unwrap().is_empty());
    let reads = backend.0.load(Ordering::Relaxed);
    let mut peer = history(sailry_protocol::SessionId::new(), TurnId::new());
    Arc::make_mut(&mut peer.calls).retain(|call| call.source.entry == "entry-1");
    let peer = observation(peer, client, tokio::runtime::Handle::current());
    assert_eq!(
        snapshot(&peer).await.unwrap()["calls"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(backend.0.load(Ordering::Relaxed), reads + 1);
    let recovered = snapshot(&observed).await.unwrap();
    assert_eq!(recovered["calls"].as_array().unwrap().len(), 3);
}

#[tokio::test]
async fn reports_individually_oversized_values() {
    let text = serde_json::to_string(&Detail {
        arguments: Value::Array(vec![Value::Null; 1_100_000]),
        result: None,
        resolved: None,
        content: None,
        images: vec![],
    })
    .unwrap();
    let backend = Backend::large(text);
    let observation = observation(
        history(sailry_protocol::SessionId::new(), TurnId::new()),
        Arc::new(Client::new(backend)),
        tokio::runtime::Handle::current(),
    );
    let value = snapshot(&observation).await.unwrap();
    assert_eq!(value["calls"].as_array().unwrap().len(), 1);
    assert_eq!(value["errors"].as_array().unwrap().len(), 2);
    assert!(
        value["errors"]
            .as_array()
            .unwrap()
            .iter()
            .all(|error| { error["message"] == "tool details exceed the display memory budget" })
    );
}

#[tokio::test]
async fn cursor_ignores_drafts_and_tracks_tool_changes() {
    let backend = Backend::new();
    let mut observed: BTreeMap<_, _> = [(
        "fixture".to_owned(),
        observation(
            history(sailry_protocol::SessionId::new(), TurnId::new()),
            Arc::new(Client::new(backend.clone())),
            tokio::runtime::Handle::current(),
        ),
    )]
    .into();
    let original = cursor(&observed).unwrap();
    let current = observed.get_mut("fixture").unwrap();
    Arc::make_mut(current.history.snapshot.as_mut().unwrap()).sequence += 100;
    assert_eq!(cursor(&observed).unwrap(), original);
    let current = observed.get_mut("fixture").unwrap();
    let page = Arc::make_mut(&mut Arc::make_mut(current.history.snapshot.as_mut().unwrap()).page);
    let sailry_protocol::conversation::Part::ToolResult { summary, .. } =
        &mut page.entries[0].parts[1]
    else {
        unreachable!()
    };
    summary.revision = "b".repeat(64);
    assert_ne!(cursor(&observed).unwrap(), original);
    let revised = cursor(&observed).unwrap();
    let current = observed.get_mut("fixture").unwrap();
    let page = Arc::make_mut(&mut Arc::make_mut(current.history.snapshot.as_mut().unwrap()).page);
    page.entries[0].timestamp_ms = 100;
    assert_ne!(cursor(&observed).unwrap(), revised);
    assert_eq!(backend.0.load(Ordering::Relaxed), 0);
}
