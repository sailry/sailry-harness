use super::super::compaction as context;
use super::{fixture::Fixture, *};

fn event(text: &str) -> AdkEvent {
    let mut event = AdkEvent::new("context-fixture");
    event.author = "assistant".into();
    event.set_content(adk_core::Content::new("model").with_text(text));
    event
}

fn summary(end: &AdkEvent) -> AdkEvent {
    let mut summary = AdkEvent::new("summary-fixture");
    summary.author = "system".into();
    summary
        .provider_metadata
        .insert(context::END_EVENT.into(), end.id.clone());
    summary.actions.compaction = Some(adk_core::EventCompaction {
        start_timestamp: end.timestamp,
        end_timestamp: end.timestamp,
        compacted_content: adk_core::Content::new("model").with_text("Summary 中文 🙂"),
    });
    summary
}

fn load(fixture: &Fixture, turn: TurnId) -> Vec<AdkEvent> {
    sessions::get(
        &fixture.database.connection,
        turn,
        GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: fixture.session.to_string(),
            num_recent_events: None,
            after: None,
        },
    )
    .unwrap()
    .events()
    .all()
}

#[test]
fn retains_summary_only_update_time() {
    let mut fixture = Fixture::new();
    let original = event("Original evidence");
    fixture.append([original.clone()]);
    let compacted = summary(&original);
    let turn = fixture.append([compacted.clone()]);
    let session = sessions::get(
        &fixture.database.connection,
        turn,
        GetRequest {
            app_name: APP.into(),
            user_id: USER.into(),
            session_id: fixture.session.to_string(),
            num_recent_events: None,
            after: None,
        },
    )
    .unwrap();
    assert_eq!(session.events().len(), 1);
    assert_eq!(session.last_update_time(), compacted.timestamp);
}

#[test]
fn ignores_clock_rewinds() {
    let mut fixture = Fixture::new();
    let original = event("Original evidence");
    fixture.append([original.clone()]);
    let mut recent = event("Recent must survive");
    recent.timestamp = original.timestamp - chrono::Duration::seconds(60);
    fixture.append([recent.clone()]);
    let compacted = summary(&original);
    let turn = fixture.append([event("Current"), compacted.clone()]);
    let full = fixture.read(None, 100).page;
    let context = load(&fixture, turn);
    assert_eq!(context.len(), 3);
    assert_eq!(context[0].id, compacted.id);
    assert!(context[0].actions.compaction.is_none());
    assert_eq!(context[1].id, recent.id);
    assert!(!context.iter().any(|event| event.id == original.id));
    assert_eq!(full.entries.len(), 4);
    assert_eq!(
        full.entries.last().unwrap().parts,
        vec![Part::Compaction("Summary 中文 🙂".into())]
    );
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(fixture.read(None, 100).page, full);
    assert_eq!(
        load(&fixture, turn)
            .iter()
            .map(|event| &event.id)
            .collect::<Vec<_>>(),
        context.iter().map(|event| &event.id).collect::<Vec<_>>()
    );
}

#[test]
fn rejects_invalid_boundaries() {
    let mut fixture = Fixture::new();
    let first = event("First");
    let last = event("Last");
    fixture.append([first.clone(), last.clone()]);
    let turn = fixture.queued();
    for boundary in [first, event("Not a member")] {
        assert!(
            history::append(
                &mut fixture.database,
                fixture.session,
                turn,
                summary(&boundary),
                &fixture.events
            )
            .is_err()
        );
    }
    let current = event("Current");
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        current.clone(),
        &fixture.events,
    )
    .unwrap();
    assert!(
        history::append(
            &mut fixture.database,
            fixture.session,
            turn,
            summary(&current),
            &fixture.events
        )
        .is_err()
    );
    assert_eq!(fixture.read(None, 100).page.entries.len(), 3);
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        summary(&last),
        &fixture.events,
    )
    .unwrap();
}

#[test]
fn restores_branched_context() {
    let mut fixture = Fixture::new();
    let original = event("Original");
    let first = fixture.append([original.clone()]);
    let compacted = summary(&original);
    let head = fixture.append([event("Recent"), compacted.clone()]);
    let Output::Session(branch) = command(
        &mut fixture.database,
        &fixture.events,
        Command::ForkConversation {
            session: fixture.session,
            through: head,
            expected_revision: 1,
        },
    ) else {
        panic!("fork expected")
    };
    let source = fixture.session;
    fixture.session = branch.id;
    let branch_turn = fixture.queued();
    assert_eq!(load(&fixture, branch_turn)[0].id, compacted.id);
    fixture.session = source;
    let Output::Rewound(rewound) = command(
        &mut fixture.database,
        &fixture.events,
        Command::RewindConversation {
            session: source,
            through: Some(first),
            expected_head: head,
            expected_revision: 1,
        },
    ) else {
        panic!("rewind expected")
    };
    let context = load(&fixture, first);
    assert_eq!(context.len(), 1);
    assert_eq!(context[0].id, original.id);
    fixture.session = rewound.backup.id;
    let backup_turn = fixture.queued();
    assert_eq!(load(&fixture, backup_turn)[0].id, compacted.id);
    assert_eq!(fixture.read(None, 100).page.entries.len(), 3);
}

#[test]
fn loads_complete_history() {
    let mut fixture = Fixture::new();
    let turn = fixture.completed(&["Preserved"; 300]);
    assert_eq!(load(&fixture, turn).len(), 300);
}

#[test]
fn accepts_complete_batches() {
    use adk_core::{Content, FunctionResponseData, Part as AdkPart};
    let mut fixture = Fixture::new();
    let turn = fixture.queued();
    runs::start(&fixture.database.connection, turn).unwrap();
    runs::claim(&mut fixture.database, &fixture.events)
        .unwrap()
        .unwrap();
    let mut call = event("");
    call.set_content(Content {
        role: "model".into(),
        parts: ["first", "second"]
            .map(|id| AdkPart::FunctionCall {
                id: Some(id.into()),
                name: "read_file".into(),
                args: serde_json::json!({"path":id}),
                thought_signature: None,
            })
            .into(),
    });
    let results = ["first", "second"].map(|id| {
        let mut result = event("");
        result.set_content(Content {
            role: "user".into(),
            parts: vec![AdkPart::FunctionResponse {
                id: Some(id.into()),
                function_response: FunctionResponseData::new(
                    "read_file",
                    serde_json::json!({"content":id}),
                ),
                annotations: None,
            }],
        });
        result
    });
    for boundary in [&call, &results[0]] {
        history::append(
            &mut fixture.database,
            fixture.session,
            turn,
            boundary.clone(),
            &fixture.events,
        )
        .unwrap();
        assert!(
            history::append(
                &mut fixture.database,
                fixture.session,
                turn,
                summary(boundary),
                &fixture.events
            )
            .is_err()
        );
    }
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        results[1].clone(),
        &fixture.events,
    )
    .unwrap();
    let compacted = summary(&results[1]);
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        compacted.clone(),
        &fixture.events,
    )
    .unwrap();
    let recent = event("Recent evidence");
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        recent.clone(),
        &fixture.events,
    )
    .unwrap();
    let current = load(&fixture, turn);
    assert_eq!(
        current.iter().map(|event| &event.id).collect::<Vec<_>>(),
        [&compacted.id, &recent.id]
    );
    runs::finish(
        &mut fixture.database,
        turn,
        Status::Completed,
        None,
        &fixture.events,
    )
    .unwrap();
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        compacted,
        &fixture.events,
    )
    .unwrap();
    let full = fixture.read(None, 100).page;
    assert_eq!(full.entries.len(), 5);
    history::rebuild(&mut fixture.database.connection).unwrap();
    assert_eq!(fixture.read(None, 100).page, full);
}

#[test]
fn rejects_orphans_and_mismatches() {
    use adk_core::{Content, FunctionResponseData, Part as AdkPart};
    let mut call = event("");
    call.set_content(Content {
        role: "model".into(),
        parts: vec![AdkPart::FunctionCall {
            id: Some("call".into()),
            name: "read_file".into(),
            args: serde_json::json!({}),
            thought_signature: None,
        }],
    });
    for (id, name) in [
        ("wrong", "read_file"),
        ("call", "wrong"),
        ("call", "read_file"),
    ] {
        let mut result = event("");
        result.set_content(Content {
            role: "user".into(),
            parts: vec![AdkPart::FunctionResponse {
                id: Some(id.into()),
                function_response: FunctionResponseData::new(name, serde_json::json!({})),
                annotations: None,
            }],
        });
        assert!(!context::complete_exchange(&[result.clone()]));
        assert_eq!(
            context::complete_exchange(&[call.clone(), result]),
            id == "call" && name == "read_file"
        );
    }
}
