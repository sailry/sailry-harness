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
    fail_at: Mutex<Option<usize>>,
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
            fail_at: Mutex::new(None),
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
                cursor,
                limit,
                expected_revision,
                ..
            } = &request.command
            else {
                panic!("detail read expected");
            };
            let count = self.calls.fetch_add(1, Ordering::SeqCst);
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
            let mut reference = self.reference.clone();
            reference.revision = expected_revision.clone();
            let output = Output::ToolDetail(DetailPage {
                reference,
                offset,
                total_bytes: self.text.len(),
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
    let revision = cache.begin(&backend.reference);
    let mut page = backend.page(1);
    let Part::ToolResult { summary, .. } = &mut page.entries[0].parts[0] else {
        unreachable!()
    };
    summary.revision = "b".repeat(64);
    cache.reconcile(&page);
    let detail = Arc::new(serde_json::from_str(&backend.text).unwrap());
    assert!(!cache.finish(
        &backend.reference,
        revision,
        Some((detail, backend.text.len()))
    ));
    assert!(cache.get(&backend.reference).is_none());
}
