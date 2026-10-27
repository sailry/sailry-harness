use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{
    NodeId, Receipt, Request, Topic, TurnId,
    conversation::{Entry, Part},
    tool::detail::{Page as DetailPage, Summary},
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Backend {
    reference: Reference,
    text: String,
    calls: AtomicUsize,
    failures: AtomicUsize,
    total: Option<usize>,
    fail_at: Mutex<Option<usize>>,
    panic_at: Mutex<Option<usize>>,
    blocked: Mutex<Option<(Arc<tokio::sync::Notify>, Arc<tokio::sync::Notify>)>>,
}

impl Backend {
    fn new() -> Arc<Self> {
        let detail = Detail {
            arguments: json!({"body":"Original argument".repeat(10_000)}),
            result: Some(json!({"text":"中文🙂\n".repeat(30_000)})),
            resolved: None,
            content: None,
            images: Vec::new(),
        };
        Arc::new(Self {
            reference: Reference {
                session: SessionId::new(),
                turn: TurnId::new(),
                entry: "result".into(),
                index: 0,
                call: Some("call".into()),
                revision: "a".repeat(64),
            },
            text: serde_json::to_string(&detail).unwrap(),
            calls: AtomicUsize::new(0),
            failures: AtomicUsize::new(0),
            total: None,
            fail_at: Mutex::new(None),
            panic_at: Mutex::new(None),
            blocked: Mutex::new(None),
        })
    }

    fn page(&self, revision: u64) -> Page {
        Page {
            session: self.reference.session,
            revision,
            entries: vec![Entry {
                sequence: 1,
                turn: self.reference.turn,
                id: self.reference.entry.clone(),
                author: "assistant".into(),
                branch: String::new(),
                timestamp_ms: 1,
                usage: None,
                citations: vec![],
                search_suggestions: None,
                commentary: Default::default(),
                parts: vec![Part::ToolResult {
                    id: self.reference.call.clone(),
                    name: "fixture".into(),
                    images: vec![],
                    summary: Box::new(Summary {
                        revision: self.reference.revision.clone(),
                        ..Default::default()
                    }),
                }],
            }],
            runs: vec![],
            queue: Default::default(),
            approvals: vec![],
            questions: vec![],
            children: vec![],
            background: vec![],
            next_before: None,
        }
    }
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
            let Command::ReadToolDetail {
                session,
                turn,
                entry,
                index,
                call,
                cursor,
                limit,
                expected_revision,
                ..
            } = &request.command
            else {
                panic!("detail read expected");
            };
            let count = self.calls.fetch_add(1, Ordering::SeqCst);
            let panic = *self.panic_at.lock().unwrap();
            assert_ne!(panic, Some(count), "injected detail panic");
            let blocked = (entry == &self.reference.entry && cursor.is_none())
                .then(|| self.blocked.lock().unwrap().clone())
                .flatten();
            if let Some((started, release)) = blocked {
                started.notify_one();
                release.notified().await;
            }
            if self
                .failures
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                })
                .is_ok()
            {
                return Err(Fault::new(
                    ErrorCode::Unavailable,
                    "injected detail failure",
                ));
            }
            if self
                .fail_at
                .lock()
                .unwrap()
                .is_some_and(|failure| failure == count)
            {
                return Err(Fault::new(ErrorCode::Unavailable, "injected page failure"));
            }
            let offset = cursor.unwrap_or(0);
            let mut end = offset.saturating_add(*limit).min(self.text.len());
            while !self.text.is_char_boundary(end) {
                end -= 1;
            }
            let reference = Reference {
                session: *session,
                turn: *turn,
                entry: entry.clone(),
                index: *index,
                call: call.clone(),
                revision: expected_revision.clone(),
            };
            let output = Output::ToolDetail(DetailPage {
                reference,
                offset,
                total_bytes: self.total.unwrap_or(self.text.len()),
                next_offset: (end < self.text.len()).then_some(end),
                text: self.text[offset..end].to_owned(),
            });
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(Ok(output)).unwrap();
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
async fn coalesces_and_caches_complete_unicode_details() {
    let backend = Backend::new();
    let client = Client::new(backend.clone());
    client.details.reconcile(&backend.page(1));
    let (left, right) = tokio::join!(
        client.tool_detail(backend.reference.clone()),
        client.tool_detail(backend.reference.clone()),
    );
    let left = left.unwrap();
    assert!(Arc::ptr_eq(&left, &right.unwrap()));
    assert_eq!(
        left.result.as_ref().unwrap()["text"],
        "中文🙂\n".repeat(30_000)
    );
    let count = backend.calls.load(Ordering::SeqCst);
    assert!(count > 1);
    client.details.reconcile(&backend.page(1));
    assert!(Arc::ptr_eq(
        &left,
        &client.tool_detail(backend.reference.clone()).await.unwrap()
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), count);
    client.details.reconcile(&backend.page(2));
    assert!(client.cached_tool_detail(&backend.reference).is_none());
    assert_eq!(
        client
            .tool_detail(backend.reference.clone())
            .await
            .unwrap()
            .result,
        left.result
    );
    assert!(backend.calls.load(Ordering::SeqCst) > count);
}

#[tokio::test]
async fn failed_continuation_preserves_existing_clipboard() {
    let backend = Backend::new();
    *backend.fail_at.lock().unwrap() = Some(1);
    let client = Client::new(backend.clone());
    let mut clipboard = "Existing clipboard".to_owned();
    if let Ok(detail) = client.tool_detail(backend.reference.clone()).await {
        clipboard = detail.result.as_ref().unwrap().to_string();
    }
    assert_eq!(clipboard, "Existing clipboard");
    assert!(client.cached_tool_detail(&backend.reference).is_none());
    *backend.fail_at.lock().unwrap() = None;
    assert!(client.tool_detail(backend.reference.clone()).await.is_ok());
    assert!(client.cached_tool_detail(&backend.reference).is_some());
}

#[test]
fn invalidates_changed_parts_during_a_read() {
    let backend = Backend::new();
    let cache = Cache::default();
    cache.reconcile(&backend.page(1));
    let future = futures::future::pending().boxed().shared();
    cache.values.lock().unwrap().loading.insert(
        backend.reference.clone(),
        Reading {
            id: 1,
            invalidated: false,
            future: future.downgrade().unwrap(),
        },
    );
    let mut page = backend.page(1);
    let Part::ToolResult { summary, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    summary.revision = "b".repeat(64);
    cache.reconcile(&page);
    let detail = Arc::new(serde_json::from_str(&backend.text).unwrap());
    assert!(!cache.finish(&backend.reference, 1, Some((detail, backend.text.len()))));
    assert!(cache.get(&backend.reference).is_none());
}

#[tokio::test]
async fn independent_reads_do_not_block_approvals() {
    let backend = Backend::new();
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((started.clone(), release.clone()));
    let client = Arc::new(Client::new(backend.clone()));
    let reading = client.clone();
    let reference = backend.reference.clone();
    let slow = tokio::spawn(async move { reading.tool_detail(reference).await });
    started.notified().await;
    let mut approval = backend.reference.clone();
    approval.entry = "approval".into();
    let detail = tokio::time::timeout(
        std::time::Duration::from_secs(1),
        client.tool_detail(approval),
    )
    .await;
    assert!(detail.unwrap().is_ok());
    release.notify_one();
    assert!(slow.await.unwrap().is_ok());
}

#[tokio::test]
async fn shares_failures_without_caching_them() {
    let backend = Backend::new();
    *backend.fail_at.lock().unwrap() = Some(0);
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((started.clone(), release.clone()));
    let client = Client::new(backend.clone());
    let reads = async {
        tokio::join!(
            client.tool_detail(backend.reference.clone()),
            client.tool_detail(backend.reference.clone())
        )
    };
    tokio::pin!(reads);
    tokio::select! {
        _ = started.notified() => {},
        _ = &mut reads => panic!("detail read should wait"),
    }
    release.notify_one();
    let (left, right) = reads.await;
    assert_eq!(left.unwrap_err().code, ErrorCode::Unavailable);
    assert_eq!(right.unwrap_err().code, ErrorCode::Unavailable);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    *backend.blocked.lock().unwrap() = None;
    assert!(client.tool_detail(backend.reference.clone()).await.is_ok());
}

#[tokio::test]
async fn rejects_results_from_changed_generation() {
    let backend = Backend::new();
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((started.clone(), release.clone()));
    let client = Arc::new(Client::new(backend.clone()));
    client.details.reconcile(&backend.page(1));
    let reading = client.clone();
    let reference = backend.reference.clone();
    let pending = tokio::spawn(async move { reading.detail(reference).await });
    started.notified().await;
    client.details.reconcile(&backend.page(2));
    release.notify_one();
    assert!(matches!(pending.await.unwrap().unwrap(), Read::Superseded));
    assert!(client.cached_tool_detail(&backend.reference).is_none());
    assert!(client.details.values.lock().unwrap().loading.is_empty());
}

#[test]
fn bounds_revision_metadata_without_cached_sessions() {
    let backend = Backend::new();
    let cache = Cache::default();
    for _ in 0..1000 {
        let mut page = backend.page(1);
        page.session = SessionId::new();
        cache.reconcile(&page);
    }
    assert_eq!(cache.values.lock().unwrap().revisions.len(), 1);
}

#[test]
fn accounts_for_parsed_container_allocations() {
    let value = serde_json::Value::Array(vec![serde_json::Value::Null; 4096]);
    let wire_bytes = serde_json::to_vec(&value).unwrap().len();
    assert!(retained_bytes(&value) >= 4096 * std::mem::size_of::<serde_json::Value>());
    assert!(retained_bytes(&value) > wire_bytes * 4);
    let backend = Backend::new();
    let cache = Cache::default();
    cache.values.lock().unwrap().loading.insert(
        backend.reference.clone(),
        Reading {
            id: 1,
            invalidated: false,
            future: futures::future::pending::<Result<Read, Fault>>()
                .boxed()
                .shared()
                .downgrade()
                .unwrap(),
        },
    );
    let detail = Arc::new(serde_json::from_str(&backend.text).unwrap());
    assert!(cache.finish(&backend.reference, 1, Some((detail, MAX_BYTES + 1))));
    assert!(cache.get(&backend.reference).is_none());
}

#[test]
fn reads_without_a_tokio_caller() {
    let backend = Backend::new();
    let client = Client::new(backend.clone());
    let detail =
        futures::executor::block_on(client.tool_detail(backend.reference.clone())).unwrap();
    assert_eq!(detail.arguments["body"], "Original argument".repeat(10_000));
    assert!(client.cached_tool_detail(&backend.reference).is_some());
}

#[test]
fn panic_cleans_the_inflight_reference() {
    let backend = Backend::new();
    *backend.panic_at.lock().unwrap() = Some(0);
    let client = Client::new(backend.clone());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        futures::executor::block_on(client.tool_detail(backend.reference.clone()))
    }));
    assert!(result.is_err());
    assert!(client.details.values.lock().unwrap().loading.is_empty());
    assert!(futures::executor::block_on(client.tool_detail(backend.reference.clone())).is_ok());
}

#[tokio::test]
async fn cancellation_cleans_the_last_waiter() {
    let backend = Backend::new();
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((started.clone(), release));
    let client = Arc::new(Client::new(backend.clone()));
    let reader = client.clone();
    let reference = backend.reference.clone();
    let pending = tokio::spawn(async move { reader.tool_detail(reference).await });
    started.notified().await;
    pending.abort();
    assert!(pending.await.unwrap_err().is_cancelled());
    assert!(client.details.values.lock().unwrap().loading.is_empty());
    *backend.blocked.lock().unwrap() = None;
    assert!(client.tool_detail(backend.reference.clone()).await.is_ok());
}

#[tokio::test]
async fn replaces_invalidated_reads_without_erasing_successors() {
    let backend = Backend::new();
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((started.clone(), release.clone()));
    let client = Arc::new(Client::new(backend.clone()));
    client.details.reconcile(&backend.page(1));
    let reader = client.clone();
    let reference = backend.reference.clone();
    let old = tokio::spawn(async move { reader.detail(reference).await });
    started.notified().await;
    client.details.reconcile(&backend.page(2));
    let next_started = Arc::new(tokio::sync::Notify::new());
    let next_release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((next_started.clone(), next_release.clone()));
    let reader = client.clone();
    let reference = backend.reference.clone();
    let next = tokio::spawn(async move { reader.tool_detail(reference).await });
    next_started.notified().await;
    let id = client.details.values.lock().unwrap().loading[&backend.reference].id;
    release.notify_one();
    assert!(matches!(old.await.unwrap().unwrap(), Read::Superseded));
    assert_eq!(
        client.details.values.lock().unwrap().loading[&backend.reference].id,
        id
    );
    next_release.notify_one();
    let detail = next.await.unwrap().unwrap();
    assert!(Arc::ptr_eq(
        &detail,
        &client.cached_tool_detail(&backend.reference).unwrap()
    ));
}

#[tokio::test]
async fn classifies_parsed_allocation_limits() {
    let mut backend = Arc::try_unwrap(Backend::new()).ok().unwrap();
    backend.text = serde_json::to_string(&Detail {
        arguments: serde_json::Value::Array(vec![serde_json::Value::Null; 1_100_000]),
        result: None,
        resolved: None,
        content: None,
        images: vec![],
    })
    .unwrap();
    assert!(backend.text.len() < MAX_BYTES);
    let backend = Arc::new(backend);
    let client = Client::new(backend.clone());
    assert!(matches!(
        client.detail(backend.reference.clone()).await.unwrap(),
        Read::TooLarge(Limit::Memory)
    ));
    assert!(client.cached_tool_detail(&backend.reference).is_none());
}

#[tokio::test]
async fn classifies_transfer_limits_before_reading_all_pages() {
    let mut backend = Arc::try_unwrap(Backend::new()).ok().unwrap();
    backend.total = Some(MAX_BYTES + 1);
    let backend = Arc::new(backend);
    let client = Client::new(backend.clone());
    assert!(matches!(
        client.detail(backend.reference.clone()).await.unwrap(),
        Read::TooLarge(Limit::Transfer)
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1);
    assert!(client.cached_tool_detail(&backend.reference).is_none());
}

#[tokio::test(start_paused = true)]
async fn retries_then_requires_an_explicit_attempt() {
    let backend = Backend::new();
    backend.failures.store(4, Ordering::SeqCst);
    let client = Arc::new(Client::new(backend.clone()));
    let tracker = Tracker::new(client, tokio::runtime::Handle::current());
    tracker.set([backend.reference.clone()]);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Failed { retry_at: None, .. }
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 4);
    tracker.set([backend.reference.clone()]);
    tokio::time::advance(std::time::Duration::from_secs(60)).await;
    assert_eq!(backend.calls.load(Ordering::SeqCst), 4);
    assert_eq!(tracker.failures().len(), 1);
    assert!(tracker.failures().is_empty());
    tracker.retry(&backend.reference);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    assert!(tracker.failures().is_empty());
}

#[test]
fn tracks_without_a_tokio_caller() {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let backend = Backend::new();
    let client = Arc::new(Client::new(backend.clone()));
    let tracker = Tracker::new(client, runtime.handle().clone());
    assert!(tokio::runtime::Handle::try_current().is_err());
    tracker.set([backend.reference.clone()]);
    let state = futures::executor::block_on(tracker.wait(&backend.reference));
    assert!(matches!(state, State::Ready(_)));
}

#[tokio::test]
async fn pins_visible_details_and_releases_their_allocations() {
    let backend = Backend::new();
    let client = Arc::new(Client::new(backend.clone()));
    let mut references = vec![backend.reference.clone(); 2];
    references[1].entry = "second".into();
    let details = references
        .iter()
        .map(|reference| {
            let detail = Arc::new(Detail {
                arguments: serde_json::Value::String("x".repeat(24 * 1024 * 1024)),
                result: None,
                resolved: None,
                content: None,
                images: vec![],
            });
            client
                .details
                .values
                .lock()
                .unwrap()
                .entries
                .push_back(Cached {
                    reference: reference.clone(),
                    bytes: allocation(&detail, reference),
                    detail: detail.clone(),
                });
            Arc::downgrade(&detail)
        })
        .collect::<Vec<_>>();
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    tracker.set(references.clone());
    for reference in &references {
        assert!(matches!(tracker.wait(reference).await, State::Ready(_)));
    }
    for index in 0..=MAX_ENTRIES {
        let mut reference = backend.reference.clone();
        reference.entry = format!("other-{index}");
        client.tool_detail(reference).await.unwrap();
    }
    for (reference, expected) in references.iter().zip(&details) {
        let State::Ready(detail) = tracker.state(reference) else {
            panic!("visible detail must remain ready");
        };
        assert!(std::ptr::eq(Arc::as_ptr(&detail), expected.as_ptr()));
    }
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            <= MAX_BYTES
    );
    tracker.set([]);
    for index in 0..=MAX_ENTRIES {
        let mut reference = backend.reference.clone();
        reference.entry = format!("release-{index}");
        client.tool_detail(reference).await.unwrap();
    }
    assert!(details.iter().all(|detail| detail.upgrade().is_none()));
    assert!(client.details.values.lock().unwrap().pins.is_empty());
}

#[tokio::test]
async fn observes_eviction_without_reloading_unchanged_history() {
    let backend = Backend::new();
    let client = Arc::new(Client::new(backend.clone()));
    let tracker = Tracker::observe(client.clone(), tokio::runtime::Handle::current());
    tracker.set([backend.reference.clone()]);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    for index in 0..=MAX_ENTRIES {
        let mut reference = backend.reference.clone();
        reference.entry = format!("other-{index}");
        client.tool_detail(reference).await.unwrap();
    }
    let count = backend.calls.load(Ordering::SeqCst);
    tracker.set([backend.reference.clone()]);
    assert!(matches!(
        tracker.state(&backend.reference),
        State::Superseded
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), count);
    tracker.retry(&backend.reference);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    assert!(backend.calls.load(Ordering::SeqCst) > count);
}

#[tokio::test]
async fn drops_superseded_results_without_feedback() {
    let backend = Backend::new();
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((started.clone(), release.clone()));
    let client = Arc::new(Client::new(backend.clone()));
    client.details.reconcile(&backend.page(1));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    tracker.set([backend.reference.clone()]);
    started.notified().await;
    client.details.reconcile(&backend.page(2));
    release.notify_one();
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Superseded
    ));
    assert!(tracker.failures().is_empty());
    let mut peer = backend.page(1);
    peer.session = SessionId::new();
    client.details.reconcile(&peer);
    assert_eq!(client.details.revision(backend.reference.session), Some(2));
    *backend.blocked.lock().unwrap() = None;
    tracker.set([backend.reference.clone()]);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    tracker.set([]);
    assert!(client.details.values.lock().unwrap().pins.is_empty());
    assert!(client.details.values.lock().unwrap().tracked.is_empty());
}

#[tokio::test]
async fn limits_simultaneous_pins_and_allows_later_retry() {
    let mut backend = Arc::try_unwrap(Backend::new()).ok().unwrap();
    backend.text = serde_json::to_string(&Detail {
        arguments: serde_json::Value::String("x".repeat(34 * 1024 * 1024)),
        result: None,
        resolved: None,
        content: None,
        images: vec![],
    })
    .unwrap();
    let parsed: Detail = serde_json::from_str(&backend.text).unwrap();
    let bytes = allocation(&parsed, &backend.reference);
    assert!(bytes < MAX_BYTES);
    assert!(2 * bytes > MAX_BYTES);
    drop(parsed);
    let backend = Arc::new(backend);
    let client = Arc::new(Client::new(backend.clone()));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    let first = backend.reference.clone();
    let mut second = first.clone();
    second.entry = "second".into();
    tracker.set([first.clone(), second.clone()]);
    let states = [tracker.wait(&first).await, tracker.wait(&second).await];
    assert_eq!(
        states
            .iter()
            .filter(|state| matches!(state, State::Ready(_)))
            .count(),
        1
    );
    assert_eq!(
        states
            .iter()
            .filter(|state| matches!(state, State::TooLarge(Limit::Memory)))
            .count(),
        1
    );
    let retry = if matches!(states[0], State::TooLarge(_)) {
        first
    } else {
        second
    };
    drop(states);
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .map(|entry| entry.bytes)
            .sum::<usize>()
            <= MAX_BYTES
    );
    tracker.set([retry.clone()]);
    tracker.retry(&retry);
    assert!(matches!(tracker.wait(&retry).await, State::Ready(_)));
    tracker.set([]);
    assert!(client.details.values.lock().unwrap().pins.is_empty());
}
