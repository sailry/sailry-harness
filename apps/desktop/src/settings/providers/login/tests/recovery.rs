use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{NodeId, Topic};
use std::sync::{Mutex, atomic::AtomicBool};

struct Delayed {
    inner: Arc<dyn Transport>,
    first: AtomicBool,
    release: Arc<tokio::sync::Notify>,
    finished: Arc<tokio::sync::Notify>,
}

impl Transport for Delayed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::BeginProviderLogin { .. })
                && self.first.swap(false, Ordering::SeqCst)
            {
                let inner = self.inner.clone();
                let release = self.release.clone();
                let finished = self.finished.clone();
                tokio::spawn(async move {
                    release.notified().await;
                    inner
                        .dispatch(request)
                        .await
                        .unwrap()
                        .completion
                        .await
                        .unwrap()
                        .unwrap();
                    finished.notify_one();
                });
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "isolated delayed admission",
                ));
            }
            self.inner.dispatch(request).await
        })
    }
}

#[gpui::test]
fn cancels_pending_admission(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::Copilot, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let delayed = Arc::new(Delayed {
            inner: fixture.transport.clone(),
            first: AtomicBool::new(true),
            release: Arc::new(tokio::sync::Notify::new()),
            finished: Arc::new(tokio::sync::Notify::new()),
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Workspace::new(window, cx));
            view.update(cx, |view, cx| {
                view.bind_providers(
                    delayed.clone(),
                    fixture.runtime.clone(),
                    "Authorization Node".into(),
                    cx,
                )
            });
            owner = Some(view.clone());
            let surface = cx.new(|_| Surface(view));
            Root::new(surface, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
        let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
        wait(visual, |cx| {
            login.read(cx).error == Some("provider_login_unknown")
        });
        let id = login.read_with(visual, |login, _| login.request.id);
        settle_dialog(visual);
        tap(visual, "provider-login-cancel");
        wait(visual, |cx| {
            !login.read(cx).pending
                && (login.read(cx).closed
                    || matches!(login.read(cx).state(), Some(State::Cancelled)))
        });
        delayed.release.notify_one();
        fixture.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), delayed.finished.notified())
                .await
                .unwrap();
        });
        let client = Client::new(fixture.transport.clone());
        let state = fixture.runtime.block_on(async {
            client
                .subscribe_login(id)
                .await
                .unwrap()
                .next()
                .await
                .unwrap()
        });
        assert!(
            matches!(state, sailry_protocol::Update::ProviderLogin(update) if update.state == State::Cancelled)
        );
        assert_eq!(fixture.provider().credential, None);
        tap(visual, "provider-login-cancel");
        visual.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            window.remove_window();
        });
        fixture.close();
    }
}

struct Loss {
    inner: Arc<dyn Transport>,
    begin: AtomicBool,
    cancel: AtomicBool,
    requests: Mutex<Vec<Request>>,
}
impl Transport for Loss {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            self.requests.lock().unwrap().push(request.clone());
            let lose = match request.command {
                Command::BeginProviderLogin { .. } => self.begin.swap(false, Ordering::SeqCst),
                Command::CancelProviderLogin { .. } => self.cancel.swap(false, Ordering::SeqCst),
                _ => false,
            };
            let admission = self.inner.dispatch(request).await?;
            if !lose {
                return Ok(admission);
            }
            let _ = admission.completion.await;
            let (sender, completion) = tokio::sync::oneshot::channel();
            drop(sender);
            Ok(Admission {
                receipt: admission.receipt,
                completion,
            })
        })
    }
}

#[gpui::test]
fn retries_original_attempt(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::Copilot, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let other = fixture
            .runtime
            .block_on(Node::start(fixture._directory.path().join("other")))
            .unwrap();
        let loss = Arc::new(Loss {
            inner: fixture.transport.clone(),
            begin: AtomicBool::new(true),
            cancel: AtomicBool::new(true),
            requests: Mutex::new(Vec::new()),
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Workspace::new(window, cx));
            view.update(cx, |view, cx| {
                view.bind_providers(
                    loss.clone(),
                    fixture.runtime.clone(),
                    "Original Node".into(),
                    cx,
                )
            });
            owner = Some(view.clone());
            let surface = cx.new(|_| Surface(view));
            Root::new(surface, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
        let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
        wait(visual, |cx| {
            login.read(cx).error == Some("provider_login_unknown")
        });
        let request = login.read_with(visual, |login, _| login.request.clone());
        visual.update(|_, cx| {
            owner.update(cx, |owner, cx| {
                owner.bind_providers(
                    other.local(),
                    fixture.runtime.clone(),
                    "Other Node".into(),
                    cx,
                )
            })
        });
        settle_dialog(visual);
        tap(visual, "provider-login-retry");
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Pending { .. }))
        });
        login.read_with(visual, |login, _| {
            assert_eq!(login.request, request);
            assert_eq!(login.binding.client.target(), fixture.node.id());
            assert_eq!(login.error, None);
        });
        settle_dialog(visual);
        visual.simulate_keystrokes("escape");
        wait(visual, |cx| {
            !login.read(cx).pending && matches!(login.read(cx).state(), Some(State::Cancelled))
        });
        assert_eq!(login.read_with(visual, |login, _| login.error), None);
        tap(visual, "provider-login-cancel");
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        let requests = loss.requests.lock().unwrap();
        let beginnings = requests
            .iter()
            .filter(|request| matches!(request.command, Command::BeginProviderLogin { .. }))
            .collect::<Vec<_>>();
        assert_eq!(beginnings.len(), 2);
        assert_eq!(beginnings[0], beginnings[1]);
        assert_eq!(
            fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.path == "/login/device/code")
                .count(),
            1
        );
        let client = Client::new(other.local());
        assert!(
            matches!(fixture.runtime.block_on(client.execute(client.prepare(Command::ListCredentials))).unwrap(), Output::Credentials(credentials) if credentials.is_empty())
        );
        drop(requests);
        visual.update(|window, _| window.remove_window());
        fixture.runtime.block_on(other.shutdown()).unwrap();
        fixture.close();
    }
}

#[gpui::test]
fn retries_finished_attempts(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.mode.store(2, Ordering::SeqCst);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::Copilot, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let view = cx.new(|cx| Workspace::new(window, cx));
            view.update(cx, |view, cx| {
                view.bind_providers(
                    fixture.transport.clone(),
                    fixture.runtime.clone(),
                    "Authorization Node".into(),
                    cx,
                )
            });
            owner = Some(view.clone());
            let surface = cx.new(|_| Surface(view));
            Root::new(surface, window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
        let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Failed(_)))
        });
        let first = login.read_with(visual, |login, _| login.request.id);
        fixture.mode.store(1, Ordering::SeqCst);
        settle_dialog(visual);
        tap(visual, "provider-login-retry");
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Connected))
        });
        login.read_with(visual, |login, _| {
            assert_ne!(login.request.id, first);
            assert_eq!(login.error, None);
        });
        tap(visual, "provider-login-cancel");
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        assert_eq!(
            fixture
                .server
                .requests
                .lock()
                .unwrap()
                .iter()
                .filter(|request| request.path == "/login/device/code")
                .count(),
            2
        );
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
