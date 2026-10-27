use super::*;
use sailry_client::conversation::tools::Reference;

fn call(index: usize) -> Call {
    Call {
        turn: TurnId::new(),
        id: Some(format!("call-{index}")),
        name: "fixture".into(),
        terminal_interaction: false,
        presentation: Default::default(),
        grouping: Default::default(),
        source: Reference {
            entry: format!("entry-{index}"),
            index: 0,
        },
        response: None,
        state: State::Running,
        failure: None,
        approval: None,
        question: None,
        progress: None,
        content: None,
        resolved: None,
        detail: None,
    }
}

#[test]
fn skips_unchanged_calls_beyond_client_cache() {
    let session = SessionId::new();
    let mut presentation = Presentation::default();
    let calls = (0..65).map(call).collect::<Vec<_>>();
    let stamp = |call: &Call| {
        Stamp::new(
            call,
            sailry_protocol::tool::detail::Reference {
                session,
                turn: call.turn,
                entry: call.source.entry.clone(),
                index: 0,
                call: call.id.clone(),
                revision: "a".repeat(64),
            },
        )
    };
    for call in &calls {
        let id = call_id(session, call);
        let current = stamp(call);
        assert!(presentation.changed(&id, &current));
        presentation.stamps.insert(id, current);
    }
    assert!(
        calls
            .iter()
            .all(|call| !presentation.changed(&call_id(session, call), &stamp(call)))
    );
    let mut returned = calls[0].clone();
    returned.state = State::Returned;
    returned.response = Some(Reference {
        entry: "result".into(),
        index: 0,
    });
    let mut current = stamp(&returned);
    current.reference.entry = "result".into();
    assert!(presentation.changed(&call_id(session, &returned), &current));
    let mut revised = stamp(&calls[1]);
    revised.reference.revision = "b".repeat(64);
    assert!(presentation.changed(&call_id(session, &calls[1]), &revised));
}

#[test]
fn releases_stamps_outside_current_calls() {
    let source = SessionId::new();
    let other = SessionId::new();
    let current = call(0);
    let removed = call(1);
    let reference = |session, call: &Call| sailry_protocol::tool::detail::Reference {
        session,
        turn: call.turn,
        entry: call.source.entry.clone(),
        index: 0,
        call: call.id.clone(),
        revision: "a".repeat(64),
    };
    let mut presentation = Presentation::default();
    for (session, call) in [(source, &current), (source, &removed), (other, &removed)] {
        presentation.stamps.insert(
            call_id(session, call),
            Stamp::new(call, reference(session, call)),
        );
    }
    presentation.retain(source, &[call_id(source, &current)].into());
    assert_eq!(presentation.stamps.len(), 2);
    assert!(!presentation.stamps.contains_key(&call_id(source, &removed)));
    presentation.retain(source, &BTreeSet::new());
    assert_eq!(presentation.stamps.len(), 1);
    assert!(presentation.stamps.contains_key(&call_id(other, &removed)));
}
