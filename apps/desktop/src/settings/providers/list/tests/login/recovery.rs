use super::*;
use sailry_protocol::ErrorCode;
use std::sync::atomic::{AtomicBool, Ordering};

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
        let mut fixture = Fixture::new(remote);
        fixture.mode.store(1, Ordering::SeqCst);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ChatGpt, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let loss = Arc::new(Loss {
            inner: fixture.transport.clone(),
            first: AtomicBool::new(true),
            requests: Mutex::new(vec![]),
        });
        fixture.transport = loss.clone();
        let (owner, visual) = fixture.mount(cx);
        let (attempt, authorization) = fixture.begin(visual);
        let response = fixture.callback(&authorization);
        let deadline = Instant::now() + Duration::from_secs(10);
        while visual.debug_bounds("provider-login-retry").is_none() {
            draw(visual);
            assert!(Instant::now() < deadline, "callback recovery deadline");
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(!loss.first.load(Ordering::SeqCst));
        assert!(fixture.provider().credential.is_none());
        assert!(!response.is_finished());
        assert!(visual.update(|window, cx| window.has_active_dialog(cx)));
        tap(visual, "provider-login-retry");
        wait(visual, |_| fixture.state(attempt) == State::Connected);
        wait(visual, |cx| {
            owner.read(cx).providers.channels[0].credential_configured
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
        draw(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        assert!(fixture.provider().credential.is_some());
        fixture.page_result(response, visual, "connected");
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
        drop(owner);
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
        let mut fixture = Fixture::new(remote);
        fixture.execute(Command::SaveProvider {
            provider: support::provider(Authentication::ChatGpt, ModelApi::Responses),
            expected_revision: 0,
            secret: None,
        });
        let saved = fixture.provider();
        let delayed = Arc::new(Delayed {
            inner: fixture.transport.clone(),
            first: AtomicBool::new(true),
            release: tokio::sync::Notify::new(),
            requests: Mutex::new(vec![]),
        });
        fixture.transport = delayed.clone();
        let (owner, visual) = fixture.mount(cx);
        let (attempt, authorization) = fixture.begin(visual);
        let response = fixture.callback(&authorization);
        wait(visual, |_| !delayed.first.load(Ordering::SeqCst));
        assert!(!response.is_finished());
        close_dialog(visual);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
        delayed.release.notify_one();
        wait(visual, |_| {
            delayed.requests.lock().unwrap().iter().any(|request| matches!(request.command, Command::CancelProviderLogin { attempt: id } if id == attempt))
        });
        wait(visual, |_| fixture.state(attempt) == State::Cancelled);
        assert_eq!(fixture.provider(), saved);
        assert!(fixture.provider().credential.is_none());
        fixture.page_result(response, visual, "cancelled");
        visual.update(|window, _| window.remove_window());
        visual.run_until_parked();
        drop(owner);
        fixture.close();
    }
}
