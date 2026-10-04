use super::*;
use sailry_link::{Admission, Pending, Stream, Subscription, Transport};
use sailry_protocol::{ErrorCode, Fault, StreamId, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Observed {
    inner: Arc<dyn Transport>,
    requests: Mutex<Vec<Request>>,
    mode: AtomicUsize,
    opened: AtomicUsize,
    completed: AtomicUsize,
    release: tokio::sync::Semaphore,
}

impl Observed {
    fn new(inner: Arc<dyn Transport>) -> Self {
        Self {
            inner,
            requests: Mutex::new(vec![]),
            mode: AtomicUsize::new(0),
            opened: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn open(&self, stream: StreamId) -> Pending<'_, Result<Stream, Fault>> {
        Box::pin(async move {
            self.opened.fetch_add(1, Ordering::SeqCst);
            if self.mode.load(Ordering::SeqCst) == 3 {
                self.release.acquire().await.unwrap().forget();
            }
            self.inner.open(stream).await
        })
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            let mode = self.mode.load(Ordering::SeqCst);
            let lose = match mode {
                1 => matches!(request.command, Command::FinishAttachmentUpload { .. }),
                2 => matches!(request.command, Command::SubmitTurn { .. }),
                4 => matches!(request.command, Command::DiscardAttachment { .. }),
                _ => false,
            };
            self.requests.lock().unwrap().push(request.clone());
            let admission = self.inner.dispatch(request).await?;
            if !lose {
                return Ok(admission);
            }
            self.mode.store(0, Ordering::SeqCst);
            admission.completion.await.unwrap().unwrap();
            self.completed.fetch_add(1, Ordering::SeqCst);
            if mode == 2 {
                self.release.acquire().await.unwrap().forget();
            }
            Err(Fault::new(
                ErrorCode::OutcomeUnknown,
                "injected missing response",
            ))
        })
    }
}

#[gpui::test]
fn receipt_recovery(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("receipt.txt");
        std::fs::write(&path, "complete attachment").unwrap();
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        observed.mode.store(1, Ordering::SeqCst);
        choose(visual, std::slice::from_ref(&path));
        wait(visual, |cx| {
            matches!(
                view.read(cx).attachments.items[0].status,
                Status::Uncertain(_)
            )
        });
        tap(visual, "live-chat-send");
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        control(visual, 0, Control::Retry);
        view.read_with(visual, |view, _| {
            let item = &view.attachments.items[0];
            assert!(
                item.pending || !matches!(item.status, Status::Uncertain(_)),
                "attachment retry remains unconfirmed: remote={remote}, status={:?}, finishes={:?}",
                std::mem::discriminant(&item.status),
                observed
                    .requests
                    .lock()
                    .unwrap()
                    .iter()
                    .filter(|request| matches!(
                        request.command,
                        Command::FinishAttachmentUpload { .. }
                    ))
                    .map(|request| request.id)
                    .collect::<Vec<_>>()
            );
        });
        let attachment = ready(visual, &view, 1).remove(0);
        {
            let requests = observed.requests.lock().unwrap();
            assert_eq!(
                requests
                    .iter()
                    .filter(|r| matches!(r.command, Command::UploadAttachment(_)))
                    .count(),
                1
            );
            let finishes: Vec<_> = requests
                .iter()
                .filter(|r| matches!(r.command, Command::FinishAttachmentUpload { .. }))
                .collect();
            assert_eq!(finishes.len(), 2);
            assert_eq!(finishes[0], finishes[1]);
        }
        observed.mode.store(2, Ordering::SeqCst);
        tap(visual, "live-chat-input");
        visual.simulate_input("original input");
        tap(visual, "live-chat-send");
        wait(visual, |_| observed.completed.load(Ordering::SeqCst) == 2);
        tap(visual, "live-chat-input");
        visual.simulate_input(" plus new typing");
        control(visual, 0, Control::Remove);
        assert_eq!(
            view.read_with(visual, |view, _| view.attachments.ids()),
            [attachment.id]
        );
        observed.release.add_permits(1);
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).retry.is_some()
        });
        tap(visual, "chat-retry");
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).retry.is_none()
        });
        view.read_with(visual, |view, cx| {
            assert_eq!(
                view.input.read(cx).value(),
                "original input plus new typing"
            );
            assert!(view.attachments.items.is_empty());
        });
        wait(visual, |cx| {
            view.read(cx)
                .history
                .snapshot
                .as_ref()
                .unwrap()
                .page
                .runs
                .iter()
                .any(|run| run.status == sailry_protocol::conversation::Status::Completed)
        });
        {
            let requests = observed.requests.lock().unwrap();
            let submissions: Vec<_> = requests
                .iter()
                .filter(|r| matches!(r.command, Command::SubmitTurn { .. }))
                .collect();
            assert_eq!(submissions.len(), 2);
            assert_eq!(submissions[0], submissions[1]);
        }
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        choose(visual, &[path]);
        ready(visual, &view, 1);
        observed.mode.store(4, Ordering::SeqCst);
        control(visual, 1, Control::Remove);
        wait(visual, |cx| {
            matches!(
                view.read(cx).attachments.items[0].status,
                Status::Uncertain(_)
            )
        });
        control(visual, 1, Control::Retry);
        ready(visual, &view, 0);
        {
            let requests = observed.requests.lock().unwrap();
            let removals: Vec<_> = requests
                .iter()
                .filter(|r| matches!(r.command, Command::DiscardAttachment { .. }))
                .collect();
            assert_eq!(removals.len(), 2);
            assert_eq!(removals[0], removals[1]);
        }
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn cancels_before_publication(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let path = fixture.directory.path().join("cancel.txt");
        std::fs::write(&path, "not published").unwrap();
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        observed.mode.store(3, Ordering::SeqCst);
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        choose(visual, &[path]);
        wait(visual, |_| observed.opened.load(Ordering::SeqCst) == 1);
        control(visual, 0, Control::Remove);
        ready(visual, &view, 0);
        assert!(
            !observed
                .requests
                .lock()
                .unwrap()
                .iter()
                .any(|r| matches!(r.command, Command::FinishAttachmentUpload { .. }))
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}

#[gpui::test]
fn cancels_waiting_uploads(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let paths: Vec<_> = (0..3)
            .map(|index| {
                let path = fixture
                    .directory
                    .path()
                    .join(format!("waiting-{index}.txt"));
                std::fs::write(&path, "not published").unwrap();
                path
            })
            .collect();
        let observed = Arc::new(Observed::new(fixture.transport.clone()));
        observed.mode.store(3, Ordering::SeqCst);
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = fixture::open(cx, binding, fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        choose(visual, &paths);
        wait(visual, |_| observed.opened.load(Ordering::SeqCst) == 2);
        let waiting = view.read_with(visual, |view, _| {
            view.attachments.items.iter().find(|item| {
                !observed.requests.lock().unwrap().iter().any(|request| {
                    matches!(&request.command, Command::UploadAttachment(spec) if spec.name == item.name)
                })
            }).unwrap().key
        });
        visual.update(|window, cx| {
            view.update(cx, |view, cx| view.remove_attachment(waiting, window, cx))
        });
        wait(visual, |cx| view.read(cx).attachments.items.len() == 2);
        assert_eq!(observed.opened.load(Ordering::SeqCst), 2);
        visual.update(|window, cx| {
            view.update(cx, |view, cx| {
                let keys: Vec<_> = view.attachments.items.iter().map(|item| item.key).collect();
                for key in keys {
                    view.remove_attachment(key, window, cx);
                }
            })
        });
        ready(visual, &view, 0);
        let requests = observed.requests.lock().unwrap();
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(request.command, Command::UploadAttachment(_)))
                .count(),
            2
        );
        assert!(
            !requests
                .iter()
                .any(|request| matches!(request.command, Command::FinishAttachmentUpload { .. }))
        );
        drop(requests);
        visual.update(|window, _| window.remove_window());
        drop(view);
        fixture.close();
    }
}
