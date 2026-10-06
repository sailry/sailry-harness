use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{ErrorCode, Fault, NodeId, Request, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};

struct Loss {
    inner: Arc<dyn Transport>,
    first: AtomicBool,
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
            if matches!(request.command, Command::CompleteProviderLogin { .. })
                && self.first.swap(false, Ordering::SeqCst)
            {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "isolated uncertain callback admission",
                ));
            }
            self.inner.dispatch(request).await
        })
    }
}

#[gpui::test]
fn retries_the_original_callback(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let server = runtime.block_on(support::server(Arc::new(AtomicU8::new(1))));
        let node = runtime
            .block_on(Node::start_with_authorization(
                directory.path().join("node"),
                &server.endpoint,
            ))
            .unwrap();
        let mut fixture = Fixture::with_node(remote, runtime, directory, node);
        let loss = Arc::new(Loss {
            inner: fixture.transport.clone(),
            first: AtomicBool::new(true),
            requests: Mutex::new(vec![]),
        });
        fixture.transport = loss.clone();
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = fixture.owner(window, cx);
            owner = Some(workspace.clone());
            Root::new(cx.new(|_| Surface(workspace)), window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner.read(cx).provider_link.as_ref().unwrap().connected
        });
        let editor = visual.update(|window, cx| {
            let editor = crate::settings::providers::editor::open(owner.clone(), None, window, cx);
            editor.update(cx, |editor, cx| {
                editor.select_preset(Preset::ChatGpt, window, cx);
                editor.name.update(cx, |input, cx| {
                    input.set_value("Account registration", window, cx)
                });
            });
            editor
        });
        settle(visual);
        tap(visual, "provider-connect");
        wait(visual, |cx| {
            editor.read(cx).authorization.as_ref().is_some_and(|login| {
                matches!(login.read(cx).state(), Some(login::State::Browser { .. }))
            })
        });
        let authorization = editor.read_with(visual, |editor, cx| {
            let Some(login::State::Browser {
                authorization_url, ..
            }) = editor.authorization.as_ref().unwrap().read(cx).state()
            else {
                panic!("browser authorization expected")
            };
            authorization_url.clone()
        });
        fixture.runtime.block_on(async {
            reqwest::get(support::callback(&authorization).expose())
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        });
        wait(visual, |cx| {
            !loss.first.load(Ordering::SeqCst)
                && !editor
                    .read(cx)
                    .authorization
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .pending()
        });
        settle(visual);
        assert!(visual.debug_bounds("provider-login-retry").is_some());
        assert!(editor.read_with(visual, |editor, _| !editor.closed && !editor.configured));
        tap(visual, "provider-login-retry");
        wait(visual, |cx| {
            editor.read(cx).configured && !editor.read(cx).pending
        });
        let requests = loss.requests.lock().unwrap();
        let complete = requests
            .iter()
            .filter(|request| matches!(request.command, Command::CompleteProviderLogin { .. }))
            .collect::<Vec<_>>();
        assert_eq!(complete.len(), 2);
        assert_eq!(complete[0], complete[1]);
        assert_eq!(
            requests
                .iter()
                .filter(|request| matches!(request.command, Command::BeginProviderLogin { .. }))
                .count(),
            1
        );
        drop(requests);
        visual.update(|window, cx| assert!(window.has_active_dialog(cx)));
        tap(visual, "provider-cancel");
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

struct Delayed {
    inner: Arc<dyn Transport>,
    first: AtomicBool,
    release: tokio::sync::Notify,
    requests: Mutex<Vec<Request>>,
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
            self.requests.lock().unwrap().push(request.clone());
            if matches!(request.command, Command::CompleteProviderLogin { .. })
                && self.first.swap(false, Ordering::SeqCst)
            {
                self.release.notified().await;
            }
            self.inner.dispatch(request).await
        })
    }
}

#[gpui::test]
fn closes_while_callback_admission_is_pending(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let directory = tempfile::tempdir().unwrap();
        let runtime = Arc::new(tokio::runtime::Runtime::new().unwrap());
        let server = runtime.block_on(support::server(Arc::new(AtomicU8::new(0))));
        let node = runtime
            .block_on(Node::start_with_authorization(
                directory.path().join("node"),
                &server.endpoint,
            ))
            .unwrap();
        let mut fixture = Fixture::with_node(remote, runtime, directory, node);
        let delayed = Arc::new(Delayed {
            inner: fixture.transport.clone(),
            first: AtomicBool::new(true),
            release: tokio::sync::Notify::new(),
            requests: Mutex::new(vec![]),
        });
        fixture.transport = delayed.clone();
        let mut owner = None;
        let (_, visual) = cx.add_window_view(|window, cx| {
            let workspace = fixture.owner(window, cx);
            owner = Some(workspace.clone());
            Root::new(cx.new(|_| Surface(workspace)), window, cx)
        });
        let owner = owner.unwrap();
        wait(visual, |cx| {
            owner.read(cx).provider_link.as_ref().unwrap().connected
        });
        let editor = visual.update(|window, cx| {
            let editor = crate::settings::providers::editor::open(owner.clone(), None, window, cx);
            editor.update(cx, |editor, cx| {
                editor.select_preset(Preset::ChatGpt, window, cx);
                editor.name.update(cx, |input, cx| {
                    input.set_value("Account registration", window, cx)
                });
            });
            editor
        });
        settle(visual);
        tap(visual, "provider-connect");
        wait(visual, |cx| {
            editor.read(cx).authorization.as_ref().is_some_and(|login| {
                matches!(login.read(cx).state(), Some(login::State::Browser { .. }))
            })
        });
        let (attempt, authorization) = editor.read_with(visual, |editor, cx| {
            let login = editor.authorization.as_ref().unwrap().read(cx);
            let Some(login::State::Browser {
                authorization_url, ..
            }) = login.state()
            else {
                panic!("browser authorization expected")
            };
            (login.attempt_id(), authorization_url.clone())
        });
        fixture.runtime.block_on(async {
            reqwest::get(support::callback(&authorization).expose())
                .await
                .unwrap()
                .error_for_status()
                .unwrap();
        });
        wait(visual, |cx| {
            !delayed.first.load(Ordering::SeqCst)
                && editor
                    .read(cx)
                    .authorization
                    .as_ref()
                    .unwrap()
                    .read(cx)
                    .pending()
        });
        tap(visual, "provider-cancel");
        assert!(editor.read_with(visual, |editor, _| editor.closed
            && editor.authorization.is_none()));
        settle(visual);
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        delayed.release.notify_one();
        wait(visual, |_| {
            delayed.requests.lock().unwrap().iter().any(|request| matches!(request.command, Command::CancelProviderLogin { attempt: id } if id == attempt))
        });
        fixture.runtime.block_on(async {
            let client = Client::new(fixture.transport.clone());
            let mut stream = client.subscribe_login(attempt).await.unwrap();
            tokio::time::timeout(Duration::from_secs(5), async {
                loop {
                    let sailry_protocol::Update::ProviderLogin(update) =
                        stream.next().await.unwrap()
                    else {
                        panic!("login update expected")
                    };
                    if !update.state.active() {
                        assert_eq!(update.state, login::State::Cancelled);
                        break;
                    }
                }
            })
            .await
            .unwrap();
        });
        let Output::Providers(providers) = fixture.execute(Command::ListProviders) else {
            panic!("providers expected")
        };
        assert!(providers[0].credential.is_none());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
