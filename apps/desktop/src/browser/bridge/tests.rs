use super::*;
use core::prelude::v1::test;

fn entry(browser: &Entity<Browser>) -> Entry {
    Entry {
        view: Some(browser.clone()),
        serial: 0,
    }
}

#[gpui::test]
fn release_preserves_other_owners(cx: &mut TestAppContext) {
    let released = cx.update(|cx| {
        let browser = cx.new(|_| super::super::tests::fixture());
        browser.update(cx, |browser, _| browser.serial = 7);
        let other = cx.new(|_| super::super::tests::fixture());
        let session = SessionId::new();
        let owner = Owner::Session(NodeId([1; 32]), session);
        let remaining = Owner::Session(NodeId([2; 32]), session);
        let draft = Owner::Draft(NodeId([1; 32]), None);
        let mut state = State::default();
        state.views.insert(owner, entry(&browser));
        state.views.insert(remaining, entry(&other));
        state.views.insert(draft, entry(&browser));
        state.release(&browser, cx);
        for released in [owner, draft] {
            let entry = state.views.get(&released).unwrap();
            assert!(entry.view.is_none());
            assert_eq!(entry.serial, 7);
        }
        assert!(state.views.get(&remaining).unwrap().view.as_ref() == Some(&other));
        let released = browser.downgrade();
        drop(browser);
        released
    });
    assert!(released.upgrade().is_none());
}

#[gpui::test]
fn stale_release_preserves_replacement(cx: &mut TestAppContext) {
    cx.update(|cx| {
        let closed = cx.new(|_| super::super::tests::fixture());
        let reopened = cx.new(|_| super::super::tests::fixture());
        let owner = Owner::Session(NodeId([1; 32]), SessionId::new());
        let mut state = State::default();
        state.views.insert(owner, entry(&reopened));
        state.release(&closed, cx);
        let current = state.views.get(&owner).unwrap();
        assert!(current.view.as_ref() == Some(&reopened));
        assert_eq!(current.serial, 0);
        assert!(!reopened.read(cx).stop.is_cancelled());
    });
}
