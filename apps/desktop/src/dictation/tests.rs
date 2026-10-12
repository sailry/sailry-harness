use super::*;
use gpui_kit::TestAppContext;
use gpui_kit::component::speech::SpeechEvent;
use std::{cell::RefCell, rc::Rc};
mod runtime;

#[gpui_kit::test]
fn concurrent_recording(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("model");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let owner = cx.update(Service::shared);
    let guard = owner.try_lock_owned().unwrap();
    let state = cx.update(|cx| {
        session(
            models::Capture::default_at(path.clone()),
            Default::default(),
            runtime.handle().clone(),
            CancellationToken::new(),
            cx,
        )
    });
    let errors = Rc::new(RefCell::new(Vec::new()));
    let observed = errors.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&state, move |_, event: &SpeechEvent, _| {
            if let SpeechEvent::Error(error) = event {
                observed.borrow_mut().push(speech_error_key(error));
            }
        })
    });
    state.update(cx, |state, cx| state.start(cx));
    assert_eq!(*errors.borrow(), ["dictation_busy"]);
    assert!(!path.exists());
    drop(guard);
}

#[gpui_kit::test]
fn cancelled_input(cx: &mut TestAppContext) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("model");
    let runtime = tokio::runtime::Runtime::new().unwrap();
    let cancel = CancellationToken::new();
    cancel.cancel();
    let state = cx.update(|cx| {
        session(
            models::Capture::default_at(path.clone()),
            Default::default(),
            runtime.handle().clone(),
            cancel,
            cx,
        )
    });
    let errors = Rc::new(RefCell::new(Vec::new()));
    let observed = errors.clone();
    let _subscription = cx.update(|cx| {
        cx.subscribe(&state, move |_, event: &SpeechEvent, _| {
            if let SpeechEvent::Error(error) = event {
                observed.borrow_mut().push(speech_error_key(error));
            }
        })
    });
    state.update(cx, |state, cx| state.start(cx));
    assert_eq!(*errors.borrow(), ["dictation_cancelled"]);
    assert!(!path.exists());
}
