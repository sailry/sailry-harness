use super::*;
use sailry_link::{Admission, Pending, Subscription};
use sailry_protocol::{NodeId, Topic};
use std::sync::{Mutex, atomic::AtomicBool};

struct Delayed {
    inner: Arc<dyn Transport>,
    waiting: AtomicBool,
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
            if matches!(
                request.command,
                Command::BeginProviderLogin { .. } | Command::CancelProviderLogin { .. }
            ) {
                self.requests.lock().unwrap().push(request.clone());
            }
            if matches!(request.command, Command::BeginProviderLogin { .. }) {
                self.waiting.store(true, Ordering::SeqCst);
                self.release.notified().await;
            }
            self.inner.dispatch(request).await
        })
    }
}

fn mount<'a>(
    cx: &'a mut TestAppContext,
    fixture: &Fixture,
    transport: Arc<dyn Transport>,
) -> (Entity<Workspace>, &'a mut VisualTestContext) {
    let mut owner = None;
    let (_, visual) = cx.add_window_view(|window, cx| {
        let workspace = cx.new(|cx| Workspace::new(window, cx));
        workspace.update(cx, |workspace, cx| {
            workspace.bind_providers(
                transport,
                fixture.runtime.clone(),
                "Authorization Node".into(),
                cx,
            );
            workspace.select(crate::settings::Section::Providers, cx);
        });
        owner = Some(workspace.clone());
        Root::new(cx.new(|_| Surface(workspace)), window, cx)
    });
    let owner = owner.unwrap();
    wait(visual, |cx| owner.read(cx).providers.channels.len() == 1);
    (owner, visual)
}

#[gpui::test]
fn preserves_delayed_cleanup(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        for rejected in [false, true] {
            let fixture = Fixture::new(remote);
            fixture.execute(Command::SaveProvider {
                provider: support::provider(Authentication::Copilot, ModelApi::Responses),
                expected_revision: 0,
                secret: None,
            });
            let delayed = Arc::new(Delayed {
                inner: fixture.transport.clone(),
                waiting: AtomicBool::new(false),
                release: Default::default(),
                requests: Default::default(),
            });
            let (owner, visual) = mount(cx, &fixture, delayed.clone());
            let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
            wait(visual, |_| delayed.waiting.load(Ordering::SeqCst));
            let (begin, cancellation) = login.read_with(visual, |login, _| {
                assert!(login.pending && login.attempt.is_none());
                (login.request.clone(), login.cancellation.clone())
            });
            let retained = login.downgrade();
            drop(login);
            settle_dialog(visual);
            tap(visual, "provider-login-cancel");
            visual.update(|window, cx| {
                assert!(!window.has_active_dialog(cx));
                assert!(window.notifications(cx).is_empty());
            });
            assert_eq!(
                delayed.requests.lock().unwrap().as_slice(),
                std::slice::from_ref(&begin)
            );
            assert!(retained.upgrade().is_some());
            visual.update(|window, cx| {
                window.open_dialog(cx, |dialog, _, _| {
                    dialog.child(div().debug_selector(|| "replacement-dialog".into()))
                })
            });
            if rejected {
                let mut provider = fixture.provider();
                let revision = provider.revision;
                provider.name = "Changed provider".into();
                fixture.execute(Command::SaveProvider {
                    provider,
                    expected_revision: revision,
                    secret: None,
                });
            }
            delayed.release.notify_one();
            wait(visual, |_| retained.upgrade().is_none());
            if rejected {
                assert_eq!(
                    delayed.requests.lock().unwrap().as_slice(),
                    std::slice::from_ref(&begin)
                );
                let client = Client::new(fixture.transport.clone());
                let state = fixture.runtime.block_on(async {
                    let mut updates = client.subscribe_login(begin.id).await?;
                    updates.next().await
                });
                assert!(matches!(
                    state,
                    Err(Fault {
                        code: ErrorCode::NotFound,
                        ..
                    })
                ));
            } else {
                assert_eq!(fixture.state(begin.id), State::Cancelled);
                assert_eq!(*delayed.requests.lock().unwrap(), [begin, cancellation]);
            }
            assert!(fixture.provider().credential.is_none());
            draw(visual);
            assert!(visual.debug_bounds("replacement-dialog").is_some());
            visual.update(|window, cx| {
                assert!(window.has_active_dialog(cx));
                assert!(window.notifications(cx).is_empty());
                assert_eq!(
                    crate::feedback::tests::count(window, &tr("provider_login_cancelled"), cx),
                    0
                );
                window.close_dialog(cx);
                window.remove_window();
            });
            fixture.close();
        }
    }
}

struct Uncertain(Arc<dyn Transport>);
impl Transport for Uncertain {
    fn target(&self) -> NodeId {
        self.0.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.0.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if matches!(request.command, Command::CancelProviderLogin { .. }) {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "Isolated cancellation uncertainty",
                ));
            }
            self.0.dispatch(request).await
        })
    }
}

#[gpui::test]
fn reports_uncertain_cleanup(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::Copilot, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let (owner, visual) = mount(cx, &fixture, Arc::new(Uncertain(fixture.transport.clone())));
        let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Pending { .. }))
        });
        let id = login.read_with(visual, |login, _| login.request.id);
        drop(login);
        settle_dialog(visual);
        tap(visual, "provider-login-cancel");
        visual.update(|window, cx| assert!(!window.has_active_dialog(cx)));
        crate::feedback::tests::shown(visual);
        assert_eq!(
            visual.update(crate::feedback::tests::summary),
            tr("provider_login_cancel_unknown")
        );
        assert!(matches!(fixture.state(id), State::Pending { .. }));
        assert!(fixture.provider().credential.is_none());
        fixture.execute(Command::CancelProviderLogin { attempt: id });
        assert_eq!(fixture.state(id), State::Cancelled);
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}

#[gpui::test]
fn closes_on_cancelled_update(cx: &mut TestAppContext) {
    cx.executor().allow_parking();
    cx.update(gpui_kit::init);
    for remote in [false, true] {
        let fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::Copilot, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let (owner, visual) = mount(cx, &fixture, fixture.transport.clone());
        let login = visual.update(|window, cx| open(owner.clone(), 0, window, cx).unwrap());
        wait(visual, |cx| {
            matches!(login.read(cx).state(), Some(State::Pending { .. }))
        });
        fixture.execute(Command::CancelProviderLogin {
            attempt: login.read_with(visual, |login, _| login.request.id),
        });
        wait(visual, |cx| login.read(cx).closed);
        draw(visual);
        visual.update(|window, cx| {
            assert!(!window.has_active_dialog(cx));
            assert!(window.notifications(cx).is_empty());
            assert_eq!(
                crate::feedback::tests::count(window, &tr("provider_login_cancelled"), cx),
                0
            );
            window.remove_window();
        });
        fixture.close();
    }
}
