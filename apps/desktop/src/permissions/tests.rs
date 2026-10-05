use super::*;
use core::prelude::v1::test;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};

struct Surface;
impl Render for Surface {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}
struct Mock {
    status: Arc<Mutex<Status>>,
    calls: Arc<AtomicUsize>,
}
impl Mock {
    fn new(status: Status) -> Self {
        Self {
            status: Arc::new(Mutex::new(status)),
            calls: Default::default(),
        }
    }
    fn card(&self, resource: Resource) -> Card {
        let status = self.status.clone();
        let check: Action = Rc::new(move |cx, _| {
            let status = *status.lock().unwrap();
            cx.background_executor()
                .spawn(async move { Ok(vec![(resource, status)]) })
        });
        let status = self.status.clone();
        let calls = self.calls.clone();
        let request: Action = Rc::new(move |cx, _| {
            calls.fetch_add(1, Ordering::SeqCst);
            let status = *status.lock().unwrap();
            cx.background_executor()
                .spawn(async move { Ok(vec![(resource, status)]) })
        });
        Card {
            resource,
            status: Status::Unknown,
            settings: Some(
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone",
            ),
            check: Some(check),
            request: Some(request),
            requires: None,
        }
    }
}
fn mount(cx: &mut TestAppContext) -> &mut VisualTestContext {
    cx.update(|cx| {
        gpui_kit::init(cx);
        crate::preferences::init(cx);
        crate::theme::init(cx);
    });
    let (_, visual) = cx.add_window_view(|window, cx| Root::new(cx.new(|_| Surface), window, cx));
    visual
}
fn draw(visual: &mut VisualTestContext) {
    visual.run_until_parked();
    visual.update(|window, cx| window.draw(cx).clear(cx));
}
fn tap(visual: &mut VisualTestContext, id: &'static str) {
    draw(visual);
    let bounds = visual
        .debug_bounds(id)
        .unwrap_or_else(|| panic!("missing {id}"));
    visual.simulate_click(bounds.center(), Modifiers::default());
    draw(visual);
}
fn present(
    visual: &mut VisualTestContext,
    cards: Vec<Card>,
    stop: CancellationToken,
) -> Rc<RefCell<Vec<bool>>> {
    let completed = Rc::new(RefCell::new(Vec::new()));
    let result = completed.clone();
    visual.update(|window, cx| {
        open(
            cards,
            stop,
            Box::new(move |granted, _, _| result.borrow_mut().push(granted)),
            window,
            cx,
        )
    });
    draw(visual);
    completed
}

#[gpui::test]
fn checks_without_prompting_and_rechecks_after_settings(cx: &mut TestAppContext) {
    let visual = mount(cx);
    let mock = Mock::new(Status::Required);
    let completed = present(
        visual,
        vec![mock.card(Resource::Microphone)],
        CancellationToken::new(),
    );
    assert!(visual.debug_bounds("permissions-modal").is_some());
    assert!(
        visual
            .debug_bounds("permission_microphone-permission_required")
            .is_some()
    );
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    tap(visual, "permission_microphone-request");
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
    assert!(completed.borrow().is_empty());
    *mock.status.lock().unwrap() = Status::Denied;
    tap(visual, "permission_microphone-check");
    tap(visual, "permission_microphone-request");
    assert_eq!(mock.calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        visual.opened_url().as_deref(),
        Some("x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone")
    );
    visual.deactivate_window();
    visual.update(|window, _| window.activate_window());
    draw(visual);
    assert!(
        visual
            .debug_bounds("permission_microphone-permission_denied")
            .is_some()
    );
    assert!(completed.borrow().is_empty());
    *mock.status.lock().unwrap() = Status::Granted;
    visual.deactivate_window();
    visual.update(|window, _| window.activate_window());
    draw(visual);
    assert_eq!(&*completed.borrow(), &[true]);
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn close_and_external_cancel_do_not_resume(cx: &mut TestAppContext) {
    let visual = mount(cx);
    for external in [false, true] {
        let mock = Mock::new(Status::Required);
        let stop = CancellationToken::new();
        let completed = present(visual, vec![mock.card(Resource::Microphone)], stop.clone());
        if external {
            stop.cancel();
            draw(visual);
        } else {
            tap(visual, "permissions-cancel");
        }
        assert_eq!(&*completed.borrow(), &[false]);
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }
}

#[gpui::test]
fn cancelled_native_completion_is_not_replayed(cx: &mut TestAppContext) {
    let visual = mount(cx);
    let calls = Arc::new(AtomicUsize::new(0));
    let native = calls.clone();
    let (send, receive) = tokio::sync::oneshot::channel();
    let receive = Arc::new(Mutex::new(Some(receive)));
    let action: Action = Rc::new(move |cx, _| {
        native.fetch_add(1, Ordering::SeqCst);
        let receive = receive.lock().unwrap().take().unwrap();
        cx.background_executor().spawn(async move {
            let _ = receive.await;
            Ok(vec![(Resource::Chrome, Status::Granted)])
        })
    });
    let completed = present(
        visual,
        vec![Card {
            resource: Resource::Chrome,
            status: Status::Required,
            settings: None,
            check: None,
            request: Some(action),
            requires: None,
        }],
        CancellationToken::new(),
    );
    tap(visual, "permission_chrome-request");
    tap(visual, "permission_chrome-request");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(completed.borrow().is_empty());
    tap(visual, "permissions-cancel");
    let _ = send.send(());
    draw(visual);
    assert_eq!(&*completed.borrow(), &[false]);
    assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
}

#[gpui::test]
fn remote_and_unavailable_access_cannot_be_requested_locally(cx: &mut TestAppContext) {
    let visual = mount(cx);
    for status in [Status::Remote, Status::Unavailable, Status::Unknown] {
        let mock = Mock::new(status);
        let mut card = mock.card(Resource::Screen);
        if status == Status::Unknown {
            card.request = None;
        }
        let completed = present(visual, vec![card], CancellationToken::new());
        tap(visual, "permission_screen-request");
        assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
        assert!(completed.borrow().is_empty());
        tap(visual, "permissions-cancel");
    }
}

#[gpui::test]
fn dependent_keychain_request_waits_for_data(cx: &mut TestAppContext) {
    let visual = mount(cx);
    let data = Mock::new(Status::Granted);
    let key = Mock::new(Status::Required);
    let mut data_card = data.card(Resource::Chrome);
    data_card.check = None;
    data_card.status = Status::Required;
    let mut key_card = key.card(Resource::Keychain);
    key_card.check = None;
    key_card.status = Status::Required;
    key_card.requires = Some(Resource::Chrome);
    let completed = present(visual, vec![data_card, key_card], CancellationToken::new());
    tap(visual, "permission_keychain-request");
    assert_eq!(key.calls.load(Ordering::SeqCst), 0);
    tap(visual, "permission_chrome-request");
    assert_eq!(data.calls.load(Ordering::SeqCst), 1);
    assert!(completed.borrow().is_empty());
    *key.status.lock().unwrap() = Status::Granted;
    tap(visual, "permission_keychain-request");
    assert_eq!(key.calls.load(Ordering::SeqCst), 1);
    assert_eq!(&*completed.borrow(), &[true]);
}

#[gpui::test]
fn failed_checks_keep_the_card_and_show_feedback(cx: &mut TestAppContext) {
    let visual = mount(cx);
    let mock = Mock::new(Status::Required);
    let mut card = mock.card(Resource::Screen);
    card.check = Some(Rc::new(|cx, _| {
        cx.background_executor().spawn(async {
            Err(Failure {
                key: "computer_permissions_failed".into(),
                status: Status::Unknown,
            })
        })
    }));
    let completed = present(visual, vec![card], CancellationToken::new());
    assert!(
        visual
            .debug_bounds("permission_screen-permission_unknown")
            .is_some()
    );
    assert!(visual.update(|window, cx| !window.notifications(cx).is_empty()));
    assert!(completed.borrow().is_empty());
    assert_eq!(mock.calls.load(Ordering::SeqCst), 0);
    tap(visual, "permission_screen-check");
    assert!(visual.debug_bounds("permissions-modal").is_some());
    assert!(completed.borrow().is_empty());
    tap(visual, "permissions-cancel");
}

#[gpui::test]
fn fast_completion_releases_the_requesting_entity(cx: &mut TestAppContext) {
    let visual = mount(cx);
    for granted in [false, true] {
        let owner = visual.update(|_, cx| cx.new(|_| 0_u8));
        let callback = owner.clone();
        visual.update(|window, cx| {
            owner.update(cx, |value, cx| {
                let done: Completion = Box::new(move |result, _, cx| {
                    assert_eq!(result, granted);
                    callback.update(cx, |value, _| *value += 1);
                });
                if granted {
                    microphone::open_with_status(
                        Status::Granted,
                        CancellationToken::new(),
                        done,
                        window,
                        cx,
                    );
                } else {
                    let stop = CancellationToken::new();
                    stop.cancel();
                    open(Vec::new(), stop, done, window, cx);
                }
                assert_eq!(*value, 0);
            })
        });
        draw(visual);
        assert_eq!(owner.read_with(visual, |value, _| *value), 1);
        assert!(!visual.update(|window, cx| window.has_active_dialog(cx)));
    }
}
