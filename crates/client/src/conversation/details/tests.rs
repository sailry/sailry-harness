use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{
    NodeId, Receipt, Request, Topic, TurnId,
    conversation::{Entry, Part},
    tool::detail::{Page as DetailPage, Summary},
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};

// Test fixtures extract complete values while product callers use Read directly.
impl Client {
    fn cached_tool_detail(&self, reference: &Reference) -> Option<Arc<Detail>> {
        self.details
            .values
            .lock()
            .unwrap()
            .get(reference)
            .map(|value| value.detail)
    }
    /// Complete explicit reads share the same canonical cache as presentation trackers.
    async fn tool_detail(&self, reference: Reference) -> Result<Arc<Detail>, Fault> {
        match self.detail(reference).await? {
            Read::Ready(value) => Ok(value.detail),
            Read::TooLarge(limit) => Err(Fault::new(ErrorCode::InvalidRequest, limit.message())),
            Read::Superseded => Err(Fault::new(
                ErrorCode::Cancelled,
                "tool detail read was superseded",
            )),
        }
    }
}

fn assert_contended(state: State) {
    assert!(matches!(
        &state,
        State::Failed { fault, retry_at: None } if fault.code == ErrorCode::Busy
    ));
    assert!(!crate::conversation::tools::decision_ready(
        &sailry_protocol::conversation::Decision::Approve,
        Some(&state),
    ));
}

async fn wait_contended(tracker: &Tracker, reference: &Reference) {
    let mut changed = tracker.subscribe();
    loop {
        let state = tracker.state(reference);
        if !matches!(state, State::Loading) {
            assert_contended(state);
            return;
        }
        changed.changed().await.unwrap();
    }
}

struct Backend {
    reference: Reference,
    text: String,
    texts: BTreeMap<String, String>,
    calls: AtomicUsize,
    failures: AtomicUsize,
    failure_code: ErrorCode,
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
            texts: Default::default(),
            calls: AtomicUsize::new(0),
            failures: AtomicUsize::new(0),
            failure_code: ErrorCode::Unavailable,
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
                return Err(Fault::new(self.failure_code, "injected detail failure"));
            }
            if self
                .fail_at
                .lock()
                .unwrap()
                .is_some_and(|failure| failure == count)
            {
                return Err(Fault::new(ErrorCode::Unavailable, "injected page failure"));
            }
            let text = self.texts.get(entry).unwrap_or(&self.text);
            let offset = cursor.unwrap_or(0);
            let mut end = offset.saturating_add(*limit).min(text.len());
            while !text.is_char_boundary(end) {
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
                total_bytes: self.total.unwrap_or(text.len()),
                next_offset: (end < text.len()).then_some(end),
                text: text[offset..end].to_owned(),
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
    assert!(!cache.finish(
        &backend.reference,
        1,
        Some(Loaded {
            detail,
            bytes: backend.text.len()
        })
    ));
    assert!(
        cache
            .values
            .lock()
            .unwrap()
            .get(&backend.reference)
            .is_none()
    );
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
    assert!(cache.finish(
        &backend.reference,
        1,
        Some(Loaded {
            detail,
            bytes: MAX_BYTES + 1
        })
    ));
    assert!(
        cache
            .values
            .lock()
            .unwrap()
            .get(&backend.reference)
            .is_none()
    );
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
                    value: Loaded {
                        bytes: allocation(&detail, reference),
                        detail: detail.clone(),
                    },
                    pins: 0,
                    valid: true,
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
        assert!(std::ptr::eq(Arc::as_ptr(&detail.detail), expected.as_ptr()));
    }
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .map(|entry| entry.value.bytes)
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
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pins == 0)
    );
}

#[tokio::test]
async fn charges_invalidated_values_until_their_owner_releases() {
    let backend = Backend::new();
    let client = Arc::new(Client::new(backend.clone()));
    client.details.reconcile(&backend.page(1));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    tracker.set([backend.reference.clone()]);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    client.details.reconcile(&backend.page(2));
    {
        let values = client.details.values.lock().unwrap();
        assert_eq!(values.entries.len(), 1);
        assert_eq!(values.entries[0].pins, 1);
        assert!(!values.entries[0].valid);
    }
    assert!(matches!(
        tracker.state(&backend.reference),
        State::Superseded
    ));
    assert!(client.details.values.lock().unwrap().entries.is_empty());
    tracker.retry(&backend.reference);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    tracker.set([]);
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pins == 0)
    );
}

#[tokio::test]
async fn refreshes_selected_reads_after_history_changes() {
    let backend = Backend::new();
    let pages = backend.text.len().div_ceil(DETAIL_PAGE_BYTES);
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    *backend.blocked.lock().unwrap() = Some((started.clone(), release));
    let client = Arc::new(Client::new(backend.clone()));
    client.details.reconcile(&backend.page(1));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    tracker.include(backend.reference.clone());
    started.notified().await;
    client.details.reconcile(&backend.page(2));
    *backend.blocked.lock().unwrap() = None;
    tracker.retain(|_| true);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1 + pages);
    client.details.reconcile(&backend.page(3));
    tracker.include(backend.reference.clone());
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    assert_eq!(backend.calls.load(Ordering::SeqCst), 1 + 2 * pages);
    assert!(tracker.failures().is_empty());
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
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pins == 0)
    );
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
    tracker.set([first.clone()]);
    let State::Ready(detail) = tracker.wait(&first).await else {
        panic!("first detail must be ready");
    };
    let released = Arc::downgrade(&detail.detail);
    drop(detail);
    tracker.include(second.clone());
    assert!(matches!(tracker.wait(&second).await, State::Ready(_)));
    assert_contended(tracker.state(&first));
    assert!(released.upgrade().is_none());
    assert!(tracker.failures().is_empty(), "eviction is silent");
    {
        let values = client.details.values.lock().unwrap();
        assert_eq!(values.entries.len(), 1);
        assert_eq!(values.entries[0].reference, second);
        assert_eq!(values.entries[0].pins, 1);
        assert!(
            values
                .entries
                .iter()
                .map(|entry| entry.value.bytes)
                .sum::<usize>()
                <= MAX_BYTES
        );
    }
    let calls = backend.calls.load(Ordering::SeqCst);
    tracker.set([first.clone(), second.clone()]);
    assert_contended(tracker.state(&first));
    for _ in 0..4 {
        tracker.include(first.clone());
        tracker.include(second.clone());
        tokio::task::yield_now().await;
    }
    assert_eq!(backend.calls.load(Ordering::SeqCst), calls);
    tracker.retry(&first);
    assert!(matches!(tracker.wait(&first).await, State::Ready(_)));
    assert_contended(tracker.state(&second));
    assert!(tracker.failures().is_empty(), "eviction is silent");
    let peer = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    peer.set([first.clone()]);
    assert!(matches!(peer.wait(&first).await, State::Ready(_)));
    peer.include(second.clone());
    wait_contended(&peer, &second).await;
    assert!(matches!(peer.state(&first), State::Ready(_)));
    assert!(matches!(tracker.state(&first), State::Ready(_)));
    let failures = peer.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, second);
    assert!(peer.failures().is_empty());
    assert!(tracker.failures().is_empty());
    {
        let values = client.details.values.lock().unwrap();
        assert_eq!(values.entries.len(), 1);
        assert_eq!(values.entries[0].reference, first);
        assert_eq!(values.entries[0].pins, 2);
    }
    tracker.set([]);
    peer.retry(&second);
    assert!(matches!(peer.wait(&second).await, State::Ready(_)));
    assert_contended(peer.state(&first));
    assert!(peer.failures().is_empty(), "eviction is silent");
    peer.set([]);
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pins == 0)
    );
}

#[tokio::test]
async fn protects_pending_arguments_from_later_results() {
    let detail = |size: usize| {
        serde_json::to_string(&Detail {
            arguments: serde_json::Value::String("x".repeat(size * 1024 * 1024)),
            result: None,
            resolved: None,
            content: None,
            images: vec![],
        })
        .unwrap()
    };
    let mut backend = Arc::try_unwrap(Backend::new()).ok().unwrap();
    backend.text = detail(40);
    backend
        .texts
        .insert(backend.reference.entry.clone(), detail(30));
    let pending = backend.reference.clone();
    let mut expanded = pending.clone();
    expanded.entry = "expanded".into();
    let backend = Arc::new(backend);
    let client = Arc::new(Client::new(backend));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    tracker.protect([pending.clone()]);
    let State::Ready(original) = tracker.wait(&pending).await else {
        panic!("pending arguments must be ready");
    };
    tracker.remove(&pending);
    tracker.include(pending.clone());
    let State::Ready(visible) = tracker.wait(&pending).await else {
        panic!("collapsing a group must preserve pending arguments");
    };
    assert!(Arc::ptr_eq(&original.detail, &visible.detail));
    drop(visible);
    let Read::Ready(transient) = client.detail(pending.clone()).await.unwrap() else {
        panic!("explicit reads reuse the pending value");
    };
    assert!(Arc::ptr_eq(&original.detail, &transient.detail));
    drop(transient);
    assert_eq!(client.details.values.lock().unwrap().entries[0].pins, 1);
    tracker.include(expanded.clone());
    wait_contended(&tracker, &expanded).await;
    let State::Ready(retained) = tracker.state(&pending) else {
        panic!("later expansion must not evict pending arguments");
    };
    assert!(Arc::ptr_eq(&original.detail, &retained.detail));
    assert_eq!(retained.arguments.as_str().unwrap().len(), 30 * 1024 * 1024);
    let failures = tracker.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, expanded);
    assert!(tracker.failures().is_empty());
    drop((original, retained));
    tracker.protect([]);
    tracker.remove(&pending);
    assert!(matches!(tracker.state(&pending), State::Superseded));
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pins == 0)
    );
    tracker.retry(&expanded);
    assert!(matches!(tracker.wait(&expanded).await, State::Ready(_)));
    assert!(matches!(tracker.state(&pending), State::Superseded));
    tracker.set([]);
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pins == 0)
    );
}

#[tokio::test]
async fn explicit_reads_do_not_keep_display_pins() {
    let backend = Backend::new();
    let client = Arc::new(Client::new(backend.clone()));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    tracker.protect([backend.reference.clone()]);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
    let mut reference = backend.reference.clone();
    reference.entry = "copy".into();
    let Read::Ready(value) = client.detail(reference.clone()).await.unwrap() else {
        panic!("complete copy value expected");
    };
    drop(value);
    let values = client.details.values.lock().unwrap();
    assert_eq!(
        values
            .entries
            .iter()
            .find(|entry| entry.reference == reference)
            .unwrap()
            .pins,
        0
    );
    assert_eq!(
        values
            .entries
            .iter()
            .find(|entry| entry.reference == backend.reference)
            .unwrap()
            .pins,
        1
    );
    drop(values);
    let mut selected = Vec::new();
    tracker.retain(|reference| {
        selected.push(reference.clone());
        true
    });
    assert_eq!(selected, vec![backend.reference.clone()]);
}

#[tokio::test(start_paused = true)]
async fn preserves_ready_values_when_peers_block_admission() {
    let detail = |size: usize| {
        serde_json::to_string(&Detail {
            arguments: serde_json::Value::String("x".repeat(size * 1024 * 1024)),
            result: None,
            resolved: None,
            content: None,
            images: vec![],
        })
        .unwrap()
    };
    let mut backend = Arc::try_unwrap(Backend::new()).ok().unwrap();
    backend.text = detail(34);
    backend
        .texts
        .insert(backend.reference.entry.clone(), detail(8));
    backend.texts.insert("peer".into(), detail(32));
    let first = backend.reference.clone();
    let mut candidate = first.clone();
    candidate.entry = "candidate".into();
    let mut held = first.clone();
    held.entry = "peer".into();
    let backend = Arc::new(backend);
    let client = Arc::new(Client::new(backend.clone()));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    let peer = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    tracker.set([first.clone()]);
    let State::Ready(original) = tracker.wait(&first).await else {
        panic!("first detail must be ready");
    };
    peer.set([held.clone()]);
    let State::Ready(pinned) = peer.wait(&held).await else {
        panic!("peer detail must be ready");
    };
    let candidate_size = allocation(&serde_json::from_str(&backend.text).unwrap(), &candidate);
    assert!(pinned.bytes + candidate_size > MAX_BYTES);
    tracker.include(candidate.clone());
    wait_contended(&tracker, &candidate).await;
    let mut waiting = Box::pin(tracker.wait(&candidate));
    assert!(futures::poll!(waiting.as_mut()).is_pending());
    let State::Ready(preserved) = tracker.state(&first) else {
        panic!("blocked admission must preserve the visible detail");
    };
    assert!(Arc::ptr_eq(&original.detail, &preserved.detail));
    assert!(matches!(peer.state(&held), State::Ready(_)));
    let failures = tracker.failures();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].0, candidate);
    assert!(tracker.failures().is_empty());
    assert!(peer.failures().is_empty());
    {
        let values = client.details.values.lock().unwrap();
        assert_eq!(values.entries.len(), 2);
        assert!(values.entries.iter().all(|entry| entry.pins == 1));
        assert!(
            values
                .entries
                .iter()
                .map(|entry| entry.value.bytes)
                .sum::<usize>()
                <= MAX_BYTES
        );
    }
    drop((original, pinned, preserved));
    peer.set([]);
    assert!(matches!(
        tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
            .await
            .expect("released peer budget restores the selected detail"),
        State::Ready(_)
    ));
    assert!(matches!(tracker.state(&first), State::Ready(_)));
    assert!(tracker.failures().is_empty());
    tracker.set([]);
    assert!(
        client
            .details
            .values
            .lock()
            .unwrap()
            .entries
            .iter()
            .all(|entry| entry.pins == 0)
    );
}

#[tokio::test(start_paused = true)]
async fn missing_details_retry_and_report_the_failure() {
    let mut backend = Arc::try_unwrap(Backend::new()).ok().unwrap();
    backend.failure_code = ErrorCode::NotFound;
    backend.failures.store(4, Ordering::SeqCst);
    let backend = Arc::new(backend);
    let tracker = Tracker::new(
        Arc::new(Client::new(backend.clone())),
        tokio::runtime::Handle::current(),
    );
    tracker.set([backend.reference.clone()]);
    let State::Failed {
        fault,
        retry_at: None,
    } = tracker.wait(&backend.reference).await
    else {
        panic!("missing detail must remain retryable");
    };
    assert_eq!(fault.code, ErrorCode::NotFound);
    assert_eq!(backend.calls.load(Ordering::SeqCst), 4);
    assert_eq!(tracker.failures()[0].1.code, ErrorCode::NotFound);
    assert!(tracker.failures().is_empty());
    tracker.retry(&backend.reference);
    assert!(matches!(
        tracker.wait(&backend.reference).await,
        State::Ready(_)
    ));
}

#[tokio::test]
async fn releases_unobserved_change_metadata() {
    let backend = Backend::new();
    let client = Arc::new(Client::new(backend.clone()));
    let tracker = Tracker::new(client.clone(), tokio::runtime::Handle::current());
    for index in 0..1000 {
        let mut reference = backend.reference.clone();
        reference.entry = format!("entry-{index}");
        tracker.include(reference.clone());
        tracker.remove(&reference);
    }
    tracker.retain(|_| panic!("removed references must not remain selected"));
    assert!(tracker.changes().is_empty());
    assert!(client.details.values.lock().unwrap().tracked.is_empty());
}
