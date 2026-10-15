use super::*;
use core::prelude::v1::test;

#[test]
fn preserves_promotion_order() {
    let mut state = State::default();
    let session = SessionId::new();
    let a = Key::Session(NodeId([1; 32]), session);
    let b = Key::Session(NodeId([2; 32]), session);
    state.open(a);
    state.open(Key::Draft);
    state.open(b);
    state.open(a);
    assert_eq!(state.open, [a, b]);
    let created = Key::Session(NodeId([1; 32]), SessionId::new());
    state.promote(created);
    assert_eq!(state.open, [a, b, created]);
    assert_eq!(state.active, a);
    state.promote(created);
    assert_eq!(state.open, [a, b, created]);
}

#[test]
fn promotes_without_a_draft_tab() {
    let mut state = State::default();
    state.open(Key::Draft);
    state.open(Key::Draft);
    state.mounted = Some(Key::Draft);
    assert!(state.open.is_empty());
    assert_eq!(state.active, Key::Draft);
    let created = Key::Session(NodeId([1; 32]), SessionId::new());
    state.promote(created);
    assert_eq!(state.open, [created]);
    assert_eq!(state.active, created);
    assert_eq!(state.mounted, Some(created));
}
