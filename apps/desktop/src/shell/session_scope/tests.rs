use super::*;
use core::prelude::v1::test;

#[test]
fn draft_promotion() {
    for (scenario, check) in [
        (
            "preserves_promotion_order",
            (|_scenario: &str| {
                let mut state = State::default();
                let session = SessionId::new();
                let a = Key::Session(NodeId([1; 32]), session);
                let b = Key::Session(NodeId([2; 32]), session);
                state.open(a);
                state.open(Key::Draft);
                state.open(b);
                state.open(a);
                assert_eq!(state.open, [a, b], "preserves promotion order");
                let created = Key::Session(NodeId([1; 32]), SessionId::new());
                state.promote(created);
                assert_eq!(state.open, [a, b, created], "preserves promotion order");
                assert_eq!(state.active, a, "preserves promotion order");
                state.promote(created);
                assert_eq!(state.open, [a, b, created], "preserves promotion order");
            }) as fn(&str),
        ),
        (
            "promotes_without_a_draft_tab",
            (|_scenario: &str| {
                let mut state = State::default();
                state.open(Key::Draft);
                state.open(Key::Draft);
                state.mounted = Some(Key::Draft);
                assert!(state.open.is_empty(), "promotes without a draft tab");
                assert_eq!(state.active, Key::Draft, "promotes without a draft tab");
                let created = Key::Session(NodeId([1; 32]), SessionId::new());
                state.promote(created);
                assert_eq!(state.open, [created], "promotes without a draft tab");
                assert_eq!(state.active, created, "promotes without a draft tab");
                assert_eq!(state.mounted, Some(created), "promotes without a draft tab");
            }),
        ),
    ] {
        eprintln!("case: {scenario}");
        check(scenario);
    }
}
