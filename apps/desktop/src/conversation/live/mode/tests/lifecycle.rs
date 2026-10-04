use super::*;
use sailry_link::{Admission, Pending, Subscription, Transport};
use sailry_protocol::{ErrorCode, Fault, Topic};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};

struct Observed {
    inner: Arc<dyn Transport>,
    mode: AtomicU8,
    entered: AtomicUsize,
    release: tokio::sync::Semaphore,
    requests: Mutex<Vec<Request>>,
}

impl Transport for Observed {
    fn target(&self) -> NodeId {
        self.inner.target()
    }
    fn subscribe(&self, topic: Topic) -> Pending<'_, Result<Box<dyn Subscription>, Fault>> {
        self.inner.subscribe(topic)
    }
    fn dispatch(&self, request: Request) -> Pending<'_, Result<Admission, Fault>> {
        Box::pin(async move {
            if !matches!(request.command, Command::SetSessionConfig { .. }) {
                return self.inner.dispatch(request).await;
            }
            self.requests.lock().unwrap().push(request.clone());
            let mode = self.mode.swap(0, Ordering::SeqCst);
            if mode == 2 {
                self.entered.fetch_add(1, Ordering::SeqCst);
                self.release.acquire().await.unwrap().forget();
            }
            let mut admission = self.inner.dispatch(request).await?;
            if mode == 0 || mode == 2 {
                return Ok(admission);
            }
            let result = admission.completion.await.unwrap();
            self.entered.fetch_add(1, Ordering::SeqCst);
            self.release.acquire().await.unwrap().forget();
            if mode == 1 {
                return Err(Fault::new(
                    ErrorCode::OutcomeUnknown,
                    "injected missing mode response",
                ));
            }
            let (sender, completion) = tokio::sync::oneshot::channel();
            sender.send(result).unwrap();
            admission.completion = completion;
            Ok(admission)
        })
    }
}

#[gpui::test]
fn recovery_preserves_newer_config(cx: &mut TestAppContext) {
    init(cx);
    for remote in [false, true] {
        let fixture = Fixture::with_tools(remote, vec![]);
        let observed = Arc::new(Observed {
            inner: fixture.transport.clone(),
            mode: AtomicU8::new(1),
            entered: AtomicUsize::new(0),
            release: tokio::sync::Semaphore::new(0),
            requests: Mutex::new(vec![]),
        });
        let mut binding = fixture.binding.clone();
        binding.client = Arc::new(Client::new(observed.clone()));
        let (view, visual) = open(cx, binding.clone(), fixture.session.clone());
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("Preserved draft 中文 🙂");
        tap(visual, "live-chat-mode");
        tap(visual, "composer_mode_plan-option");
        wait(visual, |cx| {
            observed.entered.load(Ordering::SeqCst) == 1
                && view.read(cx).session.as_ref().unwrap().revision == 2
        });
        tap(visual, "live-chat-mode");
        assert!(visual.debug_bounds("composer_mode_code-option").is_none());
        let mut current = session(&view, visual);
        current.config.mode = WorkMode::Code;
        current.config.effort = Effort::Low;
        current.config.permission = Permission::Project;
        fixture.execute(Command::SetSessionConfig {
            session: current.id,
            expected_revision: current.revision,
            config: current.config,
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == 3
        });
        tap(visual, "live-chat-input");
        visual.simulate_input(" while saving");
        observed.release.add_permits(1);
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).error.is_some()
        });
        tap(visual, "live-chat-mode");
        assert!(visual.debug_bounds("composer_mode_plan-option").is_none());
        tap(visual, "chat-retry");
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).retry.is_none()
        });
        let current = session(&view, visual);
        assert_eq!(current.revision, 3);
        assert_eq!(current.config.mode, WorkMode::Code);
        assert_eq!(current.config.effort, Effort::Low);
        assert_eq!(current.config.permission, Permission::Project);
        {
            let requests = observed.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0], requests[1]);
        }

        observed.mode.store(2, Ordering::SeqCst);
        tap(visual, "live-chat-mode");
        tap(visual, "composer_mode_plan-option");
        wait(visual, |_| observed.entered.load(Ordering::SeqCst) == 2);
        let mut config = current.config;
        config.effort = Effort::High;
        fixture.execute(Command::SetSessionConfig {
            session: current.id,
            expected_revision: current.revision,
            config,
        });
        wait(visual, |cx| {
            view.read(cx).session.as_ref().unwrap().revision == 4
        });
        observed.release.add_permits(1);
        wait(visual, |cx| {
            !view.read(cx).pending && view.read(cx).error.is_some() && view.read(cx).retry.is_none()
        });
        assert_eq!(session(&view, visual).config.mode, WorkMode::Code);
        assert_eq!(session(&view, visual).config.effort, Effort::High);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "Preserved draft 中文 🙂 while saving"
        );
        select(&view, visual, WorkMode::Plan);
        assert_eq!(session(&view, visual).revision, 5);
        let requests = observed.requests.lock().unwrap().clone();
        assert_ne!(requests[2].id, requests[3].id);

        observed.mode.store(3, Ordering::SeqCst);
        tap(visual, "live-chat-mode");
        tap(visual, "composer_mode_code-option");
        wait(visual, |cx| {
            observed.entered.load(Ordering::SeqCst) == 3
                && view.read(cx).session.as_ref().unwrap().revision == 6
        });
        let current = session(&view, visual);
        visual.update(|window, _| window.remove_window());
        drop(view);
        let (view, visual) = open(cx, binding, current);
        wait(visual, |cx| view.read(cx).connected());
        tap(visual, "live-chat-input");
        visual.simulate_input("New window draft");
        select(&view, visual, WorkMode::Plan);
        observed.release.add_permits(1);
        visual.run_until_parked();
        assert_eq!(session(&view, visual).revision, 7);
        assert_eq!(session(&view, visual).config.mode, WorkMode::Plan);
        assert_eq!(
            view.read_with(visual, |view, cx| view.input.read(cx).value()),
            "New window draft"
        );
        assert!(fixture.server.requests.lock().unwrap().is_empty());
        visual.update(|window, _| window.remove_window());
        fixture.close();
    }
}
