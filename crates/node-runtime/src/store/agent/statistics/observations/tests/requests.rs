use super::*;
use sailry_protocol::conversation::observation::{Attempt, CategoryKind, Outcome};

fn attempt(index: u64, outcome: Outcome) -> Attempt {
    Attempt {
        index,
        transport: "http".into(),
        reused: Some(false),
        headers_us: Some(2),
        response_us: Some(2),
        first_text_us: Some(4),
        elapsed_us: Some(10),
        status: Some(if outcome == Outcome::Failed { 503 } else { 200 }),
        outcome,
        categories: vec![Category {
            kind: CategoryKind::User,
            items: Some(1),
            tokens: Some(3),
            method: Some("estimate".into()),
        }],
        complete_context: true,
        usage: Some(Usage {
            input: 10,
            output: 4,
            cached_input: 2,
            reasoning: 1,
        }),
        cache_reported: true,
        reasoning_reported: true,
    }
}

fn observed(request: &Request) -> AdkEvent {
    let mut event = filler();
    event.provider_metadata.insert(
        "sailry_model_observation".into(),
        serde_json::to_string(request).unwrap(),
    );
    event
}

#[test]
fn merges_recovered_attempts_across_rebuilds() {
    let mut fixture = Fixture::new();
    let turn = fixture.queued();
    let request = Request {
        id: RequestId::new().to_string(),
        turn,
        model: "fixture".into(),
        context_limit: None,
        timestamp_ms: 1,
        boundary: None,
        attempts: Vec::new(),
    };
    let mut active = request.clone();
    active.attempts = vec![Attempt {
        elapsed_us: None,
        usage: None,
        ..attempt(1, Outcome::Cancelled)
    }];
    let mut failed = request.clone();
    failed.attempts = vec![attempt(1, Outcome::Failed)];
    let failed_event = observed(&failed);
    let mut cancelled = request.clone();
    cancelled.attempts = vec![attempt(2, Outcome::Cancelled)];
    let mut succeeded = request.clone();
    succeeded.attempts = vec![attempt(3, Outcome::Succeeded)];
    let mut snapshot = request.clone();
    snapshot.attempts = vec![attempt(1, Outcome::Failed), attempt(2, Outcome::Cancelled)];
    let phases = Phases {
        active_us: Some(30),
        waiting_us: Some(12),
        output_us: Some(18),
        tools_us: Some(0),
        other_us: Some(0),
    };
    complete(
        &mut fixture,
        turn,
        [
            observed(&request),
            observed(&active),
            failed_event.clone(),
            observed(&cancelled),
            observed(&snapshot),
            observed(&succeeded),
            observed(&active),
            response(),
            phase(phases.clone()),
        ],
    );
    let before: i64 = fixture
        .database
        .connection
        .query_row(
            "SELECT count(*) FROM agent_events WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    history::append(
        &mut fixture.database,
        fixture.session,
        turn,
        failed_event,
        &fixture.events,
    )
    .unwrap();
    let after: i64 = fixture
        .database
        .connection
        .query_row(
            "SELECT count(*) FROM agent_events WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(
        before, after,
        "an acknowledged terminal event is not appended twice"
    );
    let expected =
        super::super::super::read(&fixture.database.connection, fixture.session).unwrap();
    let capture: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT input FROM agent_runs WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(expected.responses, 1);
    assert_eq!(
        expected.usage,
        Some(Usage {
            input: 10,
            output: 4,
            cached_input: 2,
            reasoning: 1
        })
    );
    let summary = &expected.observations;
    assert_eq!(summary.calls, 1);
    assert_eq!(summary.replies, 1);
    assert_eq!(summary.phases, phases);
    assert_eq!(summary.model_us, 30);
    assert_eq!(summary.output_us, 18);
    assert_eq!(summary.output_tokens, 9);
    assert_eq!(summary.output_samples, 3);
    assert_eq!(summary.composition[0].items, Some(3));
    assert_eq!(summary.composition[0].tokens, Some(9));
    let call = &summary.recent[0];
    assert_eq!(call.id, request.id);
    assert_eq!(
        call.attempts,
        vec![
            attempt(1, Outcome::Failed),
            attempt(2, Outcome::Cancelled),
            attempt(3, Outcome::Succeeded)
        ]
    );
    let page = fixture.read(None, 20).page;
    assert!(page.entries.iter().any(|entry| {
        entry
            .parts
            .iter()
            .any(|part| matches!(part, Part::Text(text) if text == "Response"))
    }));
    assert!(page.entries.iter().all(|entry| !entry.parts.is_empty()));
    let source = fixture.session;
    let fork = fixture.fork(source, turn);
    let inherited = super::super::super::read(&fixture.database.connection, fork.id).unwrap();
    assert_eq!(inherited, expected);
    history::rebuild(&mut fixture.database.connection).unwrap();
    let rebuilt: Vec<u8> = fixture
        .database
        .connection
        .query_row(
            "SELECT input FROM agent_runs WHERE turn=?1",
            [turn.to_string()],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rebuilt, capture);
    for session in [source, fork.id] {
        assert_eq!(
            super::super::super::read(&fixture.database.connection, session).unwrap(),
            expected
        );
    }
    assert_eq!(fixture.read(None, 20).page, page);
}
